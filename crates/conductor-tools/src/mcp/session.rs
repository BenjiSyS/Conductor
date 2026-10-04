//! Permission-neutral transport session. Callers must authorize before connect.
//! Credentials resolve once from the OS vault; outbound tool results are untrusted.

use super::{
    client::{
        page_cursor, rpc_result, tool_result, validate_protocol, McpClient, McpError, ServerInfo,
        ToolInfo, MAX_FRAME, MAX_TOOLS, PROTOCOL_VERSION,
    },
    config::{EnvValue, McpServer, Transport},
};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Url,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const TIMEOUT: Duration = Duration::from_secs(30);

enum Client {
    Stdio(Box<McpClient>),
    Http(Box<HttpClient>),
}

pub struct McpSession {
    client: Option<Client>,
    secrets: Vec<String>,
}

impl McpSession {
    pub fn is_open(&self) -> bool {
        self.client.is_some()
    }
    pub fn server_info(&self) -> Option<ServerInfo> {
        let mut info = match self.client.as_ref()? {
            Client::Stdio(c) => c.server.clone(),
            Client::Http(c) => c.server.clone(),
        }?;
        info.name = sanitize(&info.name, &self.secrets);
        info.version = sanitize(&info.version, &self.secrets);
        Some(info)
    }
    pub async fn connect(
        server: &McpServer,
        lookup: &(dyn Fn(&str) -> Option<String> + Sync),
        cwd: &Path,
        cancel: &CancellationToken,
    ) -> Result<Self, McpError> {
        if !server.enabled {
            return Err(McpError::Protocol("server is disabled".into()));
        }
        if cancel.is_cancelled() {
            return Err(McpError::Cancelled);
        }
        let mut secrets = Vec::new();
        let client = match &server.transport {
            Transport::Stdio { command, args, env } => {
                let env = resolve(env, lookup, &mut secrets)?;
                let c = McpClient::spawn(command, args, &env, Some(cwd), TIMEOUT.as_secs())
                    .await
                    .map_err(|e| scrub_error(e, &secrets))?;
                Client::Stdio(Box::new(c))
            }
            Transport::Http { url, headers } => {
                let headers = resolve(headers, lookup, &mut secrets)?;
                Client::Http(Box::new(HttpClient::new(url, headers)?))
            }
        };
        let mut session = Self {
            client: Some(client),
            secrets,
        };
        let result = {
            let client = session.client.as_mut().unwrap();
            tokio::select! { biased;
                _ = cancel.cancelled() => Err(McpError::Cancelled),
                r = async { match client {
                    Client::Stdio(c) => c.initialize().await.map(|_| ()),
                    Client::Http(c) => c.initialize().await,
                }} => r,
            }
        };
        if let Err(e) = result {
            session.close().await;
            return Err(scrub_error(e, &session.secrets));
        }
        Ok(session)
    }

    pub async fn list_tools(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<Vec<ToolInfo>, McpError> {
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| McpError::Closed(String::new()))?;
        let result = tokio::select! { biased;
            _ = cancel.cancelled() => Err(McpError::Cancelled),
            r = async { match client { Client::Stdio(c) => c.list_tools().await, Client::Http(c) => c.list_tools().await }} => r,
        };
        match result {
            Ok(mut tools) => {
                let mut names = BTreeSet::new();
                for tool in &mut tools {
                    if tool.name.is_empty()
                        || tool.name.len() > 128
                        || tool.name.chars().any(char::is_control)
                        || !names.insert(tool.name.clone())
                    {
                        self.close().await;
                        return Err(McpError::Protocol("invalid or duplicate tool name".into()));
                    }
                    // Preserve tool identity. A credential-shaped identity must never reach a model.
                    if sanitize(&tool.name, &self.secrets) != tool.name {
                        self.close().await;
                        return Err(McpError::Protocol(
                            "tool identity contains sensitive data".into(),
                        ));
                    }
                    tool.description = sanitize(&tool.description, &self.secrets);
                    sanitize_value(&mut tool.input_schema, &self.secrets);
                }
                Ok(tools)
            }
            Err(e) => {
                self.cancel_if_needed(&e).await;
                self.close().await;
                Err(scrub_error(e, &self.secrets))
            }
        }
    }

    pub async fn call_tool(
        &mut self,
        name: &str,
        args: Value,
        cancel: &CancellationToken,
    ) -> Result<(String, bool), McpError> {
        if name.is_empty()
            || name.len() > 128
            || name.chars().any(char::is_control)
            || !args.is_object()
        {
            return Err(McpError::Protocol(
                "tool requires a valid name and object arguments".into(),
            ));
        }
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| McpError::Closed(String::new()))?;
        let result = tokio::select! { biased;
            _ = cancel.cancelled() => Err(McpError::Cancelled),
            r = async { match client { Client::Stdio(c) => c.call_tool(name, args).await, Client::Http(c) => c.call_tool(name, args).await }} => r,
        };
        match result {
            Ok((text, error)) => Ok((sanitize(&text, &self.secrets), error)),
            Err(e) => {
                self.cancel_if_needed(&e).await;
                self.close().await;
                Err(scrub_error(e, &self.secrets))
            }
        }
    }

    async fn cancel_if_needed(&mut self, error: &McpError) {
        if matches!(error, McpError::Cancelled) {
            if let Some(Client::Http(client)) = &mut self.client {
                if client.version.is_some() {
                    let id = client.next_id.saturating_sub(1);
                    let _ = tokio::time::timeout(Duration::from_millis(500), client.post(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"User stopped task"}}))).await;
                }
            }
        }
    }

    async fn close(&mut self) {
        match self.client.take() {
            Some(Client::Stdio(c)) => (*c).shutdown().await,
            Some(Client::Http(c)) => (*c).shutdown().await,
            None => (),
        }
    }

    pub async fn shutdown(mut self) {
        self.close().await;
    }
}

fn resolve(
    values: &BTreeMap<String, EnvValue>,
    lookup: &(dyn Fn(&str) -> Option<String> + Sync),
    secrets: &mut Vec<String>,
) -> Result<BTreeMap<String, String>, McpError> {
    values
        .iter()
        .map(|(key, value)| {
            let text = match value {
                EnvValue::Literal(s) => s.clone(),
                EnvValue::Secret { secret } => {
                    let value = lookup(secret).filter(|s| !s.is_empty()).ok_or_else(|| {
                        McpError::Protocol("required credential is unavailable".into())
                    })?;
                    secrets.push(value.clone());
                    value
                }
            };
            Ok((key.clone(), text))
        })
        .collect()
}

fn sanitize(text: &str, secrets: &[String]) -> String {
    let mut clean = text.to_owned();
    // Replace longest first so overlapping credential values cannot expose a suffix.
    let mut sorted = secrets.iter().collect::<Vec<_>>();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for secret in sorted {
        clean = clean.replace(secret.as_str(), "[REDACTED]");
    }
    conductor_security::secrets::redact(&clean).text
}

fn sanitize_value(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(s) => *s = sanitize(s, secrets),
        Value::Array(items) => items.iter_mut().for_each(|v| sanitize_value(v, secrets)),
        Value::Object(items) => {
            let old = std::mem::take(items);
            for (key, mut value) in old {
                sanitize_value(&mut value, secrets);
                items.insert(sanitize(&key, secrets), value);
            }
        }
        _ => (),
    }
}

fn scrub_error(error: McpError, secrets: &[String]) -> McpError {
    match error {
        McpError::Spawn(s) => McpError::Spawn(sanitize(&s, secrets)),
        McpError::Closed(s) => McpError::Closed(sanitize(&s, secrets)),
        McpError::Protocol(s) => McpError::Protocol(sanitize(&s, secrets)),
        McpError::Rpc { code, message } => McpError::Rpc {
            code,
            message: sanitize(&message, secrets),
        },
        e => e,
    }
}

/// Remote HTTP requires TLS. Cleartext is accepted only for parsed loopback hosts.
pub fn validate_http_url(text: &str) -> Result<Url, McpError> {
    let url =
        Url::parse(text).map_err(|_| McpError::Protocol("invalid MCP endpoint URL".into()))?;
    let host = url.host_str().unwrap_or("");
    let local = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
        || (url.scheme() != "https" && !(url.scheme() == "http" && local))
    {
        return Err(McpError::Protocol(
            "MCP endpoint requires HTTPS or loopback HTTP, with credentials in headers".into(),
        ));
    }
    Ok(url)
}

struct HttpClient {
    client: reqwest::Client,
    url: Url,
    headers: HeaderMap,
    session: Option<HeaderValue>,
    version: Option<String>,
    server: Option<ServerInfo>,
    next_id: u64,
}

impl HttpClient {
    fn new(url: &str, headers: BTreeMap<String, String>) -> Result<Self, McpError> {
        let url = validate_http_url(url)?;
        let mut safe = HeaderMap::new();
        for (name, value) in headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| McpError::Protocol("invalid header name".into()))?;
            if matches!(
                name.as_str(),
                "host"
                    | "content-length"
                    | "content-type"
                    | "accept"
                    | "connection"
                    | "transfer-encoding"
                    | "mcp-session-id"
                    | "mcp-protocol-version"
            ) {
                return Err(McpError::Protocol("reserved MCP transport header".into()));
            }
            safe.insert(
                name,
                HeaderValue::from_str(&value)
                    .map_err(|_| McpError::Protocol("invalid header value".into()))?,
            );
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .timeout(TIMEOUT)
            .build()
            .map_err(|_| McpError::Protocol("could not create HTTP client".into()))?;
        Ok(Self {
            client,
            url,
            headers: safe,
            session: None,
            version: None,
            server: None,
            next_id: 1,
        })
    }

    fn builder(&self, method: reqwest::Method) -> reqwest::RequestBuilder {
        let mut r = self
            .client
            .request(method, self.url.clone())
            .headers(self.headers.clone())
            .header("Accept", "application/json, text/event-stream");
        if let Some(session) = &self.session {
            r = r.header("Mcp-Session-Id", session);
        }
        if let Some(version) = &self.version {
            r = r.header("MCP-Protocol-Version", version);
        }
        r
    }

    async fn post(&mut self, message: Value) -> Result<Option<Value>, McpError> {
        let body = serde_json::to_vec(&message).map_err(|e| McpError::Protocol(e.to_string()))?;
        if body.len() > MAX_FRAME {
            return Err(McpError::Protocol("request exceeds 2 MiB".into()));
        }
        let id = message.get("id").and_then(Value::as_u64);
        let mut response = self
            .builder(reqwest::Method::POST)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| McpError::Closed(": HTTP request failed".into()))?;
        if !response.status().is_success() {
            // Never include URL, credential headers or response body in transport errors.
            return Err(McpError::Protocol(format!(
                "HTTP status {}",
                response.status().as_u16()
            )));
        }
        if id.is_none() {
            if response.status() != reqwest::StatusCode::ACCEPTED {
                return Err(McpError::Protocol("notification requires HTTP 202".into()));
            }
            return Ok(None);
        }
        if message.get("method").and_then(Value::as_str) == Some("initialize") {
            if let Some(header) = response.headers().get("Mcp-Session-Id") {
                if header.as_bytes().is_empty()
                    || header.as_bytes().len() > 1024
                    || header.as_bytes().iter().any(|b| !(0x21..=0x7e).contains(b))
                {
                    return Err(McpError::Protocol("invalid session header".into()));
                }
                self.session = Some(header.clone());
            }
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if content_type != "application/json" && content_type != "text/event-stream" {
            return Err(McpError::Protocol(
                "unsupported response content type".into(),
            ));
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_FRAME as u64)
        {
            return Err(McpError::Protocol("response exceeds 2 MiB".into()));
        }
        let mut buffer = Vec::new();
        let mut total = 0usize;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| McpError::Closed(": HTTP body failed".into()))?
        {
            total += chunk.len();
            if total > MAX_FRAME {
                return Err(McpError::Protocol("response exceeds 2 MiB".into()));
            }
            buffer.extend_from_slice(&chunk);
            if content_type == "text/event-stream" {
                while let Some((end, skip)) = event_boundary(&buffer) {
                    let event = buffer.drain(..end + skip).collect::<Vec<_>>();
                    if let Some(value) = event_data(&event)? {
                        if value.get("id").and_then(Value::as_u64) == id {
                            return rpc_result(value, id.unwrap()).map(Some);
                        }
                        if value.get("method").is_some() && value.get("id").is_some() {
                            return Err(McpError::Protocol(
                                "server-to-client requests are unsupported".into(),
                            ));
                        }
                    }
                }
            }
        }
        if content_type == "text/event-stream" {
            return Err(McpError::Protocol(
                "SSE ended before matching response".into(),
            ));
        }
        let value = serde_json::from_slice(&buffer)
            .map_err(|_| McpError::Protocol("invalid JSON response".into()))?;
        rpc_result(value, id.unwrap()).map(Some)
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;
        self.post(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?
            .ok_or_else(|| McpError::Protocol("missing response".into()))
    }

    async fn initialize(&mut self) -> Result<(), McpError> {
        let result = self.request("initialize", json!({"protocolVersion":PROTOCOL_VERSION,"capabilities":{},"clientInfo":{"name":"Conductor","version":env!("CARGO_PKG_VERSION")}})).await?;
        let version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or("");
        validate_protocol(version)?;
        self.version = Some(version.into());
        self.server = Some(ServerInfo {
            name: result
                .pointer("/serverInfo/name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .into(),
            version: result
                .pointer("/serverInfo/version")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            protocol_version: version.into(),
        });
        self.post(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await?;
        Ok(())
    }

    async fn list_tools(&mut self) -> Result<Vec<ToolInfo>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = BTreeSet::new();
        let mut total = 0;
        for _ in 0..50 {
            let params = cursor
                .as_ref()
                .map(|s| json!({"cursor":s}))
                .unwrap_or(json!({}));
            let result = self.request("tools/list", params).await?;
            total += serde_json::to_vec(&result)
                .map_err(|e| McpError::Protocol(e.to_string()))?
                .len();
            if total > MAX_FRAME {
                return Err(McpError::Protocol("tool catalog exceeds 2 MiB".into()));
            }
            let page: Vec<ToolInfo> = serde_json::from_value(
                result
                    .get("tools")
                    .cloned()
                    .ok_or_else(|| McpError::Protocol("missing tool catalog".into()))?,
            )
            .map_err(|_| McpError::Protocol("invalid tool catalog".into()))?;
            tools.extend(page);
            if tools.len() > MAX_TOOLS {
                return Err(McpError::Protocol("tool catalog exceeds 1000 tools".into()));
            }
            cursor = page_cursor(&result)?;
            if cursor.is_none() {
                return Ok(tools);
            }
            if !seen.insert(cursor.clone().unwrap()) {
                return Err(McpError::Protocol("repeated tools/list cursor".into()));
            }
        }
        Err(McpError::Protocol("tool catalog exceeds 50 pages".into()))
    }

    async fn call_tool(&mut self, name: &str, args: Value) -> Result<(String, bool), McpError> {
        let result = self
            .request("tools/call", json!({"name":name,"arguments":args}))
            .await?;
        tool_result(&result)
    }

    async fn shutdown(self) {
        if self.session.is_some() {
            let _ = tokio::time::timeout(
                Duration::from_millis(500),
                self.builder(reqwest::Method::DELETE).send(),
            )
            .await;
        }
    }
}

fn event_boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            if bytes.get(i + 1) == Some(&b'\n') {
                return Some((i, 2));
            }
            if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                return Some((i, 3));
            }
        }
    }
    None
}

fn event_data(bytes: &[u8]) -> Result<Option<Value>, McpError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| McpError::Protocol("invalid SSE UTF-8".into()))?;
    let data = text
        .lines()
        .filter_map(|s| {
            s.strip_prefix("data:")
                .map(|s| s.strip_prefix(' ').unwrap_or(s))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&data)
        .map(Some)
        .map_err(|_| McpError::Protocol("invalid SSE JSON".into()))
}
