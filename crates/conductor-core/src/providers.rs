use crate::{domain::*, Error, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use url::Url;

pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub instructions: String,
    pub effort: Option<String>,
    pub allow_highest_effort: bool,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    Delta {
        text: String,
    },
    /// Cumulative totals for this response, including counts reported in
    /// earlier frames. Callers must not add consecutive usage events.
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Done,
}

pub fn validate(config: &ProviderConfig) -> Result<Url> {
    if config.id.is_empty() || config.name.trim().is_empty() {
        return Err(Error::Invalid("Provider name is required".into()));
    }
    let url =
        Url::parse(&config.base_url).map_err(|_| Error::Invalid("Invalid provider URL".into()))?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(Error::Invalid(
            "Use HTTPS, or HTTP on localhost for local providers".into(),
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Invalid(
            "Provider URLs cannot contain credentials, query strings or fragments".into(),
        ));
    }
    let required = match config.kind {
        ProviderKind::Openai => Some("api.openai.com"),
        ProviderKind::Anthropic => Some("api.anthropic.com"),
        ProviderKind::Gemini => Some("generativelanguage.googleapis.com"),
        ProviderKind::OpenaiCompatible => None,
    };
    if required.is_some_and(|host| url.host_str() != Some(host)) {
        return Err(Error::Invalid(
            "Built-in provider URL must use its official host. Use Custom for another endpoint."
                .into(),
        ));
    }
    Ok(url)
}
fn endpoint(config: &ProviderConfig, path: &str) -> Result<String> {
    validate(config)?;
    Ok(format!(
        "{}/{}",
        config.base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    ))
}
fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn auth(
    request: reqwest::RequestBuilder,
    kind: &ProviderKind,
    key: &str,
) -> reqwest::RequestBuilder {
    match kind {
        ProviderKind::Anthropic => request
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        ProviderKind::Gemini => request.header("x-goog-api-key", key),
        _ if !key.is_empty() => request.bearer_auth(key),
        _ => request,
    }
}
pub fn status_error(status: u16) -> Error {
    Error::Provider {
        status,
        message: match status {
            401 | 403 => "Provider signed out or key rejected. Reconnect to continue.",
            429 => "Provider usage or rate limit reached. Work has been preserved.",
            404 => "Model or endpoint unavailable. Refresh the model catalog.",
            500..=599 => "Provider unavailable. Work has been preserved. Try another provider.",
            _ => "Provider rejected the request. Check model and connection settings.",
        }
        .into(),
    }
}

pub async fn list_models(
    config: &ProviderConfig,
    key: &str,
    cancel: CancellationToken,
) -> Result<Vec<Model>> {
    let base = Url::parse(&endpoint(config, "models")?)
        .map_err(|_| Error::Invalid("Invalid model catalog URL".into()))?;
    catalog_pages(&config.kind, base, key, cancel).await
}

async fn catalog_page(
    client: &reqwest::Client,
    kind: &ProviderKind,
    url: Url,
    key: &str,
    cancel: &CancellationToken,
    downloaded: &mut usize,
) -> Result<Value> {
    let request = auth(client.get(url), kind, key);
    let response = tokio::select! {result=request.send()=>result?,_ = cancel.cancelled()=>return Err(Error::Cancelled)};
    if !response.status().is_success() {
        return Err(status_error(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|size| size > 2_000_000usize.saturating_sub(*downloaded) as u64)
    {
        return Err(Error::Invalid("Provider catalog exceeds size limit".into()));
    }
    let mut body = response.bytes_stream();
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! { biased; _ = cancel.cancelled()=>return Err(Error::Cancelled),chunk=body.next()=>chunk };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk?;
        *downloaded = downloaded.saturating_add(chunk.len());
        if *downloaded > 2_000_000 {
            return Err(Error::Invalid("Provider catalog exceeds size limit".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn catalog_model(kind: &ProviderKind, item: &Value) -> Option<Model> {
    if *kind == ProviderKind::Gemini
        && !item["supportedGenerationMethods"]
            .as_array()
            .is_some_and(|methods| methods.iter().any(|method| method == "generateContent"))
    {
        return None;
    }
    let id = item
        .get(if *kind == ProviderKind::Gemini {
            "name"
        } else {
            "id"
        })?
        .as_str()?;
    let id = id.strip_prefix("models/").unwrap_or(id);
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return None;
    }
    let name_field = if *kind == ProviderKind::Anthropic {
        "display_name"
    } else {
        "displayName"
    };
    let name = item[name_field]
        .as_str()
        .filter(|name| !name.is_empty() && name.len() <= 512)
        .unwrap_or(id);
    let context_field = if *kind == ProviderKind::Anthropic {
        "max_input_tokens"
    } else {
        "inputTokenLimit"
    };
    let efforts = if *kind == ProviderKind::Anthropic
        && item["capabilities"]["effort"]["supported"] == true
    {
        ["low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .filter(|level| item["capabilities"]["effort"][*level]["supported"] == true)
            .map(str::to_string)
            .collect()
    } else {
        vec![]
    };
    Some(Model {
        id: id.into(),
        name: name.into(),
        efforts,
        context_window: item[context_field]
            .as_u64()
            .filter(|tokens| *tokens > 0)
            .and_then(|tokens| tokens.try_into().ok()),
        tools: false,
        vision: *kind == ProviderKind::Anthropic
            && item["capabilities"]["image_input"]["supported"] == true,
    })
}

fn next_catalog_cursor(kind: &ProviderKind, value: &Value) -> Result<Option<String>> {
    let next = if *kind == ProviderKind::Gemini {
        match value.get("nextPageToken") {
            None | Some(Value::Null) => None,
            Some(Value::String(token)) if token.is_empty() => None,
            Some(Value::String(token)) => Some(token.as_str()),
            _ => {
                return Err(Error::Invalid(
                    "Provider returned an invalid page token".into(),
                ))
            }
        }
    } else if match value.get("has_more") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(more)) => *more,
        _ => {
            return Err(Error::Invalid(
                "Provider returned an invalid pagination flag".into(),
            ))
        }
    } {
        Some(
            value["last_id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| {
                    Error::Invalid("Provider catalog has more pages but no cursor".into())
                })?,
        )
    } else {
        None
    };
    match next {
        Some(cursor) if cursor.len() > 4096 || cursor.chars().any(char::is_control) => Err(
            Error::Invalid("Provider returned an invalid page cursor".into()),
        ),
        cursor => Ok(cursor.map(str::to_string)),
    }
}

async fn catalog_pages(
    kind: &ProviderKind,
    base: Url,
    key: &str,
    cancel: CancellationToken,
) -> Result<Vec<Model>> {
    let client = client()?;
    let mut cursor: Option<String> = None;
    let mut seen_cursors = std::collections::HashSet::new();
    let mut downloaded = 0;
    let mut models = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for _ in 0..64 {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut url = base.clone();
        if *kind == ProviderKind::Gemini {
            url.query_pairs_mut().append_pair("pageSize", "1000");
        } else if *kind == ProviderKind::Anthropic {
            url.query_pairs_mut().append_pair("limit", "1000");
        }
        if let Some(token) = &cursor {
            url.query_pairs_mut().append_pair(
                if *kind == ProviderKind::Gemini {
                    "pageToken"
                } else {
                    "after_id"
                },
                token,
            );
        }
        let value = catalog_page(&client, kind, url, key, &cancel, &mut downloaded).await?;
        let items = value
            .get(if *kind == ProviderKind::Gemini {
                "models"
            } else {
                "data"
            })
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Invalid("Provider returned an invalid model catalog".into()))?;
        for item in items {
            if let Some(model) = catalog_model(kind, item) {
                if ids.insert(model.id.clone()) {
                    models.push(model);
                    if models.len() > 1000 {
                        return Err(Error::Invalid(
                            "Provider catalog exceeds 1000 models".into(),
                        ));
                    }
                }
            }
        }
        cursor = next_catalog_cursor(kind, &value)?;
        if let Some(token) = &cursor {
            if !seen_cursors.insert(token.clone()) {
                return Err(Error::Invalid("Provider repeated a page cursor".into()));
            }
        } else {
            models.sort_by(|a, b| a.name.cmp(&b.name));
            return Ok(models);
        }
    }
    Err(Error::Invalid("Provider catalog exceeds 64 pages".into()))
}

pub fn request_body(config: &ProviderConfig, request: &ProviderRequest) -> Result<Value> {
    let model = config
        .models
        .iter()
        .find(|m| m.id == request.model)
        .ok_or_else(|| Error::Invalid("Choose a model from the provider catalog".into()))?;
    if let Some(effort) = &request.effort {
        if !model.efforts.contains(effort) {
            return Err(Error::Invalid(
                "This model does not declare that effort level".into(),
            ));
        }
        if ["max", "ultra", "xhigh"].contains(&effort.as_str()) && !request.allow_highest_effort {
            return Err(Error::Approval(
                "Highest effort requires explicit allowance in Settings".into(),
            ));
        }
        if config.kind == ProviderKind::Gemini {
            return Err(Error::Invalid(
                "Provider-specific effort mapping is not configured".into(),
            ));
        }
    }
    let messages:Vec<_> = request.messages.iter().filter(|m|m.role!=Role::System).map(|m|json!({"role":if m.role==Role::User {"user"}else{"assistant"},"content":crate::context::redact(&m.text)})).collect();
    let instructions = crate::context::redact(&request.instructions);
    Ok(match config.kind {
        ProviderKind::Anthropic => {
            let mut body = json!({"model":request.model,"messages":messages,"system":instructions,"max_tokens":4096,"stream":true});
            if let Some(effort) = &request.effort {
                if !["low", "medium", "high", "xhigh", "max"].contains(&effort.as_str()) {
                    return Err(Error::Invalid("Unsupported Anthropic effort level".into()));
                }
                body["output_config"] = json!({"effort":effort});
            }
            body
        }
        ProviderKind::Gemini => {
            json!({"systemInstruction":{"parts":[{"text":instructions}]},"contents":request.messages.iter().filter(|m|m.role!=Role::System).map(|m|json!({"role":if m.role==Role::User {"user"}else{"model"},"parts":[{"text":crate::context::redact(&m.text)}]})).collect::<Vec<_>>()})
        }
        ProviderKind::Openai => {
            let mut body = json!({"model":request.model,"instructions":instructions,"input":messages,"stream":true,"store":false});
            if let Some(effort) = &request.effort {
                body["reasoning"] = json!({"effort":effort});
            }
            body
        }
        ProviderKind::OpenaiCompatible => {
            let mut all = vec![json!({"role":"system","content":instructions})];
            all.extend(messages);
            let mut body = json!({"model":request.model,"messages":all,"stream":true});
            if let Some(effort) = &request.effort {
                body["reasoning_effort"] = json!(effort);
            }
            body
        }
    })
}

struct DecodedFrame {
    events: Vec<StreamEvent>,
    error: Option<Error>,
}

fn decode(kind: &ProviderKind, data: &str) -> Result<DecodedFrame> {
    if data == "[DONE]" {
        return Ok(DecodedFrame {
            events: vec![StreamEvent::Done],
            error: None,
        });
    }
    let value: Value = serde_json::from_str(data)?;
    let mut events = vec![];
    let mut error = None;
    let incomplete = || {
        Error::Invalid(
            "Provider stopped before finishing the response. Partial response preserved.".into(),
        )
    };
    match kind {
        ProviderKind::Openai => match value["type"].as_str().unwrap_or("") {
            "response.output_text.delta" | "response.refusal.delta" => {
                if let Some(text) = value["delta"].as_str() {
                    events.push(StreamEvent::Delta { text: text.into() });
                }
            }
            "response.completed" => {
                let usage = &value["response"]["usage"];
                events.push(StreamEvent::Usage {
                    input_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
                });
                events.push(StreamEvent::Done);
            }
            "error" => return Err(stream_error(&value["error"])),
            "response.failed" => return Err(stream_error(&value["response"]["error"])),
            "response.incomplete" => {
                return Err(Error::Invalid(
                    "Provider could not finish the response. Partial response preserved.".into(),
                ))
            }
            _ => {}
        },
        ProviderKind::Anthropic => match value["type"].as_str().unwrap_or("") {
            "content_block_delta" => {
                if let Some(text) = value["delta"]["text"].as_str() {
                    events.push(StreamEvent::Delta { text: text.into() });
                }
            }
            "message_start" => events.push(StreamEvent::Usage {
                input_tokens: value["message"]["usage"]["input_tokens"]
                    .as_u64()
                    .unwrap_or(0),
                output_tokens: 0,
            }),
            "message_delta" => {
                events.push(StreamEvent::Usage {
                    input_tokens: 0,
                    output_tokens: value["usage"]["output_tokens"].as_u64().unwrap_or(0),
                });
                if matches!(
                    value["delta"]["stop_reason"].as_str(),
                    Some("max_tokens" | "model_context_window_exceeded" | "pause_turn")
                ) {
                    error = Some(incomplete());
                }
            }
            "message_stop" => events.push(StreamEvent::Done),
            "error" => return Err(stream_error(&value["error"])),
            _ => {}
        },
        ProviderKind::Gemini => {
            if value.get("error").is_some() {
                return Err(stream_error(&value["error"]));
            }
            if let Some(parts) = value["candidates"][0]["content"]["parts"].as_array() {
                for part in parts {
                    if part["thought"] != true {
                        if let Some(text) = part["text"].as_str() {
                            events.push(StreamEvent::Delta { text: text.into() });
                        }
                    }
                }
            }
            if let Some(usage) = value.get("usageMetadata") {
                events.push(StreamEvent::Usage {
                    input_tokens: usage["promptTokenCount"].as_u64().unwrap_or(0),
                    output_tokens: usage["candidatesTokenCount"].as_u64().unwrap_or(0),
                });
            }
            if let Some(reason) = value["candidates"][0]["finishReason"].as_str() {
                if reason == "STOP" {
                    events.push(StreamEvent::Done);
                } else {
                    error = Some(incomplete());
                }
            }
            if value["promptFeedback"]["blockReason"].as_str().is_some() {
                return Err(status_error(400));
            }
        }
        ProviderKind::OpenaiCompatible => {
            if value.get("error").is_some() {
                return Err(stream_error(&value["error"]));
            }
            if let Some(text) = value["choices"][0]["delta"]["content"].as_str() {
                events.push(StreamEvent::Delta { text: text.into() });
            }
            if let Some(usage) = value.get("usage") {
                events.push(StreamEvent::Usage {
                    input_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0),
                    output_tokens: usage["completion_tokens"].as_u64().unwrap_or(0),
                });
            }
            if matches!(
                value["choices"][0]["finish_reason"].as_str(),
                Some("length" | "content_filter")
            ) {
                error = Some(incomplete());
            }
        }
    }
    Ok(DecodedFrame { events, error })
}

fn stream_error(value: &Value) -> Error {
    let status = value["status"].as_u64().or_else(|| value["code"].as_u64());
    let kind = value["type"]
        .as_str()
        .or_else(|| value["code"].as_str())
        .unwrap_or("");
    let status = status
        .and_then(|n| u16::try_from(n).ok())
        .unwrap_or(match kind {
            "overloaded_error" => 529,
            "server_error" | "internal_error" | "api_error" => 500,
            "rate_limit_error" | "rate_limit_exceeded" | "insufficient_quota" => 429,
            "authentication_error" | "invalid_api_key" => 401,
            "permission_error" => 403,
            "not_found_error" | "model_not_found" => 404,
            _ => 400,
        });
    status_error(status)
}

#[derive(Default)]
struct SseDecoder {
    buffer: Vec<u8>,
    data: String,
    usage_input: u64,
    usage_output: u64,
    output_bytes: usize,
}
impl SseDecoder {
    fn push(
        &mut self,
        kind: &ProviderKind,
        chunk: &[u8],
        emit: &mut impl FnMut(StreamEvent) -> Result<()>,
    ) -> Result<bool> {
        // Process bytes as they arrive. A chunk may contain many valid frames
        // and exceed a frame's cap without being one oversized frame.
        for byte in chunk {
            if *byte != b'\n' {
                self.buffer.push(*byte);
                if self.buffer.len() > 1_000_000 {
                    return Err(Error::Invalid("Provider stream frame exceeds limit".into()));
                }
                continue;
            }
            let line = std::str::from_utf8(&self.buffer)
                .map_err(|_| Error::Invalid("Provider sent invalid UTF-8".into()))?
                .trim_end_matches('\r');
            if line.is_empty() && !self.data.is_empty() {
                let frame = decode(kind, self.data.trim_end_matches('\n'))?;
                self.data.clear();
                self.buffer.clear();
                for event in frame.events {
                    // Some providers report input/output in separate frames.
                    // Expose cumulative totals so usage budgets do not lose
                    // the input count when the output count arrives later.
                    let event = match event {
                        StreamEvent::Delta { text } => {
                            self.output_bytes = self.output_bytes.saturating_add(text.len());
                            if self.output_bytes > 2_000_000 {
                                return Err(Error::Invalid(
                                    "Response exceeded 2 MB. Partial response preserved.".into(),
                                ));
                            }
                            StreamEvent::Delta { text }
                        }
                        StreamEvent::Usage {
                            input_tokens,
                            output_tokens,
                        } => {
                            self.usage_input = self.usage_input.max(input_tokens);
                            self.usage_output = self.usage_output.max(output_tokens);
                            if self.usage_input.checked_add(self.usage_output).is_none() {
                                return Err(Error::Invalid(
                                    "Provider returned invalid usage totals".into(),
                                ));
                            }
                            StreamEvent::Usage {
                                input_tokens: self.usage_input,
                                output_tokens: self.usage_output,
                            }
                        }
                        event => event,
                    };
                    let complete = matches!(event, StreamEvent::Done);
                    emit(event)?;
                    if complete {
                        return Ok(true);
                    }
                }
                if let Some(error) = frame.error {
                    return Err(error);
                }
            } else if let Some(value) = line.strip_prefix("data:") {
                self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
                self.data.push('\n');
                if self.data.len() > 1_000_000 {
                    return Err(Error::Invalid("Provider event exceeds limit".into()));
                }
            }
            self.buffer.clear();
        }
        Ok(false)
    }
}

pub async fn stream(
    config: &ProviderConfig,
    key: &str,
    request: &ProviderRequest,
    cancel: CancellationToken,
    mut emit: impl FnMut(StreamEvent) -> Result<()> + Send,
) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let path = match config.kind {
        ProviderKind::Openai => "responses".into(),
        ProviderKind::Anthropic => "messages".into(),
        ProviderKind::OpenaiCompatible => "chat/completions".into(),
        ProviderKind::Gemini => format!(
            "models/{}:streamGenerateContent?alt=sse",
            url::form_urlencoded::byte_serialize(request.model.as_bytes()).collect::<String>()
        ),
    };
    let body = request_body(config, request)?;
    let request = auth(
        client()?.post(endpoint(config, &path)?).json(&body),
        &config.kind,
        key,
    );
    let response = tokio::select! {result=request.send()=>result?,_ = cancel.cancelled()=>return Err(Error::Cancelled)};
    if !response.status().is_success() {
        return Err(status_error(response.status().as_u16()));
    }
    let mut stream = response.bytes_stream();
    let mut decoder = SseDecoder::default();
    loop {
        let chunk = tokio::select! {biased;_ = cancel.cancelled()=>return Err(Error::Cancelled),chunk=stream.next()=>chunk};
        let Some(chunk) = chunk else {
            break;
        };
        if decoder.push(&config.kind, &chunk?, &mut emit)? {
            return Ok(());
        }
    }
    // SSE dispatch requires a blank-line delimiter. Undelimited EOF frames
    // must not masquerade as verified completion.
    Err(Error::Invalid(
        "Provider stream ended before completion. Partial response preserved.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };
    fn custom(url: String) -> ProviderConfig {
        ProviderConfig {
            id: "custom".into(),
            name: "Test".into(),
            kind: ProviderKind::OpenaiCompatible,
            base_url: url,
            models: vec![Model {
                id: "test".into(),
                name: "Test".into(),
                efforts: vec!["low".into(), "ultra".into()],
                context_window: None,
                tools: false,
                vision: false,
            }],
            enabled: true,
        }
    }
    fn request() -> ProviderRequest {
        ProviderRequest {
            model: "test".into(),
            messages: vec![Message::new(Role::User, "hello".into())],
            instructions: "rules".into(),
            effort: None,
            allow_highest_effort: false,
        }
    }
    #[test]
    fn sse_handles_every_byte_boundary_multiline_and_crlf() -> Result<()> {
        let bytes = ": heartbeat\r\nevent: delta\r\ndata: {\"choices\":\r\ndata: [{\"delta\":{\"content\":\"å🌊\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n".as_bytes();
        for boundary in 0..=bytes.len() {
            let mut decoder = SseDecoder::default();
            let mut events = Vec::new();
            let mut emit = |event| {
                events.push(event);
                Ok(())
            };
            let complete = decoder.push(
                &ProviderKind::OpenaiCompatible,
                &bytes[..boundary],
                &mut emit,
            )?;
            let remaining_complete = decoder.push(
                &ProviderKind::OpenaiCompatible,
                &bytes[boundary..],
                &mut emit,
            )?;
            assert!(complete || remaining_complete, "boundary {boundary}");
            assert!(matches!(&events[0],StreamEvent::Delta{text} if text=="å🌊"));
            assert!(matches!(events[1], StreamEvent::Done));
            assert_eq!(events.len(), 2);
        }
        Ok(())
    }
    #[test]
    fn sse_emits_partial_text_before_later_error_in_same_chunk() {
        let bytes = b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\ndata: {\"error\":{\"type\":\"rate_limit_error\"}}\n\n";
        let mut events = Vec::new();
        let result =
            SseDecoder::default().push(&ProviderKind::OpenaiCompatible, bytes, &mut |event| {
                events.push(event);
                Ok(())
            });
        assert!(matches!(result, Err(Error::Provider { status: 429, .. })));
        assert!(matches!(&events[0],StreamEvent::Delta{text} if text=="partial"));
    }
    #[test]
    fn sse_rejects_invalid_utf8_and_oversized_frames() {
        assert!(SseDecoder::default()
            .push(
                &ProviderKind::OpenaiCompatible,
                b"data: \xff\n",
                &mut |_| Ok(())
            )
            .is_err());
        assert!(SseDecoder::default()
            .push(
                &ProviderKind::OpenaiCompatible,
                &vec![b'x'; 1_000_001],
                &mut |_| Ok(())
            )
            .is_err());
    }
    #[test]
    fn sse_does_not_complete_undelimited_eof_or_gemini_token_limit() -> Result<()> {
        assert!(!SseDecoder::default().push(
            &ProviderKind::OpenaiCompatible,
            b"data: [DONE]\n",
            &mut |_| Ok(())
        )?);
        let mut events = Vec::new();
        let bytes = b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"partial\"}]},\"finishReason\":\"MAX_TOKENS\"}]}\n\n";
        assert!(matches!(
            SseDecoder::default().push(&ProviderKind::Gemini, bytes, &mut |event| {
                events.push(event);
                Ok(())
            }),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(&events[0],StreamEvent::Delta{text} if text=="partial"));
        Ok(())
    }
    #[test]
    fn token_limit_and_filtered_streams_preserve_output_without_completion() {
        for (kind, bytes) in [
            (ProviderKind::Anthropic, "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"partial\"}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":10}}\n\ndata: {\"type\":\"message_stop\"}\n\n"),
            (ProviderKind::OpenaiCompatible, "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n"),
            (ProviderKind::OpenaiCompatible, "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"content_filter\"}]}\n\ndata: [DONE]\n\n"),
        ] {
            let mut events = Vec::new();
            assert!(matches!(SseDecoder::default().push(&kind, bytes.as_bytes(), &mut |event| { events.push(event); Ok(()) }), Err(Error::Invalid(_))));
            assert!(matches!(&events[0], StreamEvent::Delta {text} if text == "partial"));
            assert!(!events.iter().any(|event| matches!(event, StreamEvent::Done)));
        }
    }
    #[test]
    fn partial_usage_frames_become_cumulative_and_cannot_overflow() -> Result<()> {
        let mut events = Vec::new();
        let bytes = b"data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10}}}\n\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":5}}\n\ndata: {\"type\":\"message_stop\"}\n\n";
        assert!(
            SseDecoder::default().push(&ProviderKind::Anthropic, bytes, &mut |event| {
                events.push(event);
                Ok(())
            })?
        );
        assert!(matches!(
            events[1],
            StreamEvent::Usage {
                input_tokens: 10,
                output_tokens: 5
            }
        ));
        let bytes = format!(
            "data: {{\"usage\":{{\"prompt_tokens\":{},\"completion_tokens\":1}}}}\n\n",
            u64::MAX
        );
        assert!(matches!(
            SseDecoder::default().push(
                &ProviderKind::OpenaiCompatible,
                bytes.as_bytes(),
                &mut |_| Ok(())
            ),
            Err(Error::Invalid(_))
        ));
        Ok(())
    }
    #[test]
    fn bounded_frames_cannot_accumulate_unbounded_response_text() -> Result<()> {
        let chunk = format!(
            "data: {}\n\n",
            json!({"choices":[{"delta":{"content":"a".repeat(500_001)}}]})
        );
        let mut decoder = SseDecoder::default();
        let mut delivered = 0;
        let mut emit = |event| {
            if let StreamEvent::Delta { text } = event {
                delivered += text.len();
            }
            Ok(())
        };
        for _ in 0..3 {
            assert!(!decoder.push(&ProviderKind::OpenaiCompatible, chunk.as_bytes(), &mut emit)?);
        }
        assert!(matches!(
            decoder.push(&ProviderKind::OpenaiCompatible, chunk.as_bytes(), &mut emit),
            Err(Error::Invalid(_))
        ));
        assert_eq!(delivered, 1_500_003);
        Ok(())
    }
    #[test]
    fn request_payloads_follow_provider_schemas_and_redact_context() -> Result<()> {
        for kind in [
            ProviderKind::Openai,
            ProviderKind::Anthropic,
            ProviderKind::Gemini,
            ProviderKind::OpenaiCompatible,
        ] {
            let mut config = custom("https://example.com/v1".into());
            config.kind = kind.clone();
            let mut req = request();
            req.instructions = "password=some-private-value".into();
            req.messages[0].text = "sk-abcdefghijklmnopqrstuvwxyz123456".into();
            let body = request_body(&config, &req)?;
            let serialized = body.to_string();
            assert!(!serialized.contains("some-private-value"));
            assert!(!serialized.contains("abcdefghijklmnopqrstuvwxyz"));
            match kind {
                ProviderKind::Openai => {
                    assert_eq!(body["store"], false);
                    assert!(body.get("input").is_some());
                }
                ProviderKind::Anthropic => {
                    assert_eq!(body["max_tokens"], 4096);
                    assert!(body.get("system").is_some());
                }
                ProviderKind::Gemini => {
                    assert_eq!(body["contents"][0]["role"], "user");
                    assert!(body.get("systemInstruction").is_some());
                }
                ProviderKind::OpenaiCompatible => assert_eq!(body["messages"][0]["role"], "system"),
            }
        }
        Ok(())
    }
    #[tokio::test]
    async fn catalogs_deduplicate_models_bound_sizes_and_reject_auth_errors() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("authorization", "Bearer fixture-key"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"data":[{"id":"test"},{"id":"test"}]})),
            )
            .mount(&server)
            .await;
        let config = custom(server.uri());
        let models = list_models(&config, "fixture-key", CancellationToken::new()).await?;
        assert_eq!(models.len(), 1);
        assert!(
            models[0].efforts.is_empty(),
            "Unknown capability must remain unknown"
        );
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(2_000_001)))
            .mount(&server)
            .await;
        assert!(matches!(
            list_models(&config, "fixture-key", CancellationToken::new()).await,
            Err(Error::Invalid(_))
        ));
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        assert!(matches!(
            list_models(&config, "bad-key", CancellationToken::new()).await,
            Err(Error::Provider { status: 401, .. })
        ));
        Ok(())
    }
    #[tokio::test]
    async fn anthropic_catalog_pages_keep_auth_encode_cursors_and_map_declared_capabilities(
    ) -> Result<()> {
        let server = MockServer::start().await;
        let cursor = "claude/page?next=2&safe=yes";
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(query_param("limit", "1000"))
            .and(header("x-api-key", "fixture-key"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data":[{"id":"claude-test","display_name":"Claude Test","max_input_tokens":200000,
                    "capabilities":{"effort":{"supported":true,"low":{"supported":true},"high":{"supported":true},"max":{"supported":true}},"image_input":{"supported":true}}}],
                "has_more":true,"last_id":cursor
            })))
            .with_priority(2)
            .expect(1)
            .mount(&server).await;
        Mock::given(method("GET"))
            .and(query_param("after_id", cursor))
            .and(query_param("limit", "1000"))
            .and(header("x-api-key", "fixture-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data":[{"id":"claude-test"},{"id":"unknown-model"}],"has_more":false
            })))
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        let models = catalog_pages(
            &ProviderKind::Anthropic,
            Url::parse(&format!("{}/models", server.uri())).unwrap(),
            "fixture-key",
            CancellationToken::new(),
        )
        .await?;
        assert_eq!(models.len(), 2);
        let model = models
            .iter()
            .find(|model| model.id == "claude-test")
            .unwrap();
        assert_eq!(model.name, "Claude Test");
        assert_eq!(model.context_window, Some(200000));
        assert_eq!(model.efforts, vec!["low", "high", "max"]);
        assert!(model.vision);
        let unknown = models
            .iter()
            .find(|model| model.id == "unknown-model")
            .unwrap();
        assert!(unknown.efforts.is_empty());
        assert_eq!(unknown.context_window, None);
        assert!(!unknown.tools);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].url.query_pairs().count(),
            2,
            "Cursor must remain one encoded query value"
        );
        assert!(!requests[0].headers.contains_key("authorization"));
        Ok(())
    }
    #[tokio::test]
    async fn gemini_catalog_pages_filter_generation_methods_and_preserve_page_size() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(query_param("pageSize","1000"))
            .and(header("x-goog-api-key","fixture-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models":[
                {"name":"models/gemini-test","displayName":"Gemini Test","inputTokenLimit":1000000,"supportedGenerationMethods":["generateContent"]},
                {"name":"models/embedding-test","supportedGenerationMethods":["embedContent"]}
            ],"nextPageToken":"page / 2"})))
            .with_priority(2).expect(1).mount(&server).await;
        Mock::given(method("GET"))
            .and(query_param("pageToken", "page / 2"))
            .and(query_param("pageSize", "1000"))
            .and(header("x-goog-api-key", "fixture-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models":[
                {"name":"models/gemini-second","supportedGenerationMethods":["generateContent"]}
            ]})))
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        let models = catalog_pages(
            &ProviderKind::Gemini,
            Url::parse(&format!("{}/models", server.uri())).unwrap(),
            "fixture-key",
            CancellationToken::new(),
        )
        .await?;
        assert_eq!(models.len(), 2);
        assert_eq!(
            models
                .iter()
                .find(|model| model.id == "gemini-test")
                .unwrap()
                .context_window,
            Some(1000000)
        );
        assert!(models.iter().all(|model| model.efforts.is_empty()));
        Ok(())
    }
    #[test]
    fn catalogs_reject_invalid_pagination_metadata() {
        for value in [
            json!({"has_more":true}),
            json!({"has_more":"true"}),
            json!({"has_more":true,"last_id":"line\nbreak"}),
            json!({"has_more":true,"last_id":"x".repeat(4097)}),
        ] {
            assert!(next_catalog_cursor(&ProviderKind::Anthropic, &value).is_err());
        }
        assert!(next_catalog_cursor(&ProviderKind::Gemini, &json!({"nextPageToken":23})).is_err());
        assert_eq!(
            next_catalog_cursor(&ProviderKind::Gemini, &json!({"nextPageToken":""})).unwrap(),
            None
        );
    }
    #[tokio::test]
    async fn pagination_rejects_repeated_cursors_and_aggregate_oversize_and_cancels() -> Result<()>
    {
        let server = MockServer::start().await;
        let base = Url::parse(&format!("{}/models", server.uri())).unwrap();
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"data":[],"has_more":true,"last_id":"same"})),
            )
            .expect(2)
            .mount(&server)
            .await;
        assert!(
            matches!(catalog_pages(&ProviderKind::OpenaiCompatible,base.clone(),"",CancellationToken::new()).await,Err(Error::Invalid(message)) if message.contains("repeated"))
        );
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data":[],"padding":"x".repeat(1_100_000),"has_more":true,"last_id":"next"}),
            ))
            .expect(2)
            .mount(&server)
            .await;
        assert!(
            matches!(catalog_pages(&ProviderKind::OpenaiCompatible,base.clone(),"",CancellationToken::new()).await,Err(Error::Invalid(message)) if message.contains("size limit"))
        );
        server.reset().await;
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            catalog_pages(&ProviderKind::OpenaiCompatible, base, "", cancel).await,
            Err(Error::Cancelled)
        ));
        assert!(server.received_requests().await.unwrap().is_empty());
        Ok(())
    }
    #[tokio::test]
    async fn oversized_model_catalog_is_rejected_without_silent_truncation() -> Result<()> {
        let server = MockServer::start().await;
        let models: Vec<_> = (0..1001)
            .map(|i| json!({"id":format!("model-{i}")}))
            .collect();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":models})))
            .mount(&server)
            .await;
        assert!(
            matches!(list_models(&custom(server.uri()),"",CancellationToken::new()).await,Err(Error::Invalid(message)) if message.contains("1000 models"))
        );
        Ok(())
    }
    #[tokio::test]
    async fn pagination_cannot_run_forever_with_unique_cursors() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(|request: &wiremock::Request| {
                let page = request
                    .url
                    .query_pairs()
                    .find(|(key, _)| key == "after_id")
                    .and_then(|(_, value)| value.parse::<usize>().ok())
                    .unwrap_or(0);
                ResponseTemplate::new(200).set_body_json(
                    json!({"data":[],"has_more":true,"last_id":(page+1).to_string()}),
                )
            })
            .expect(64)
            .mount(&server)
            .await;
        assert!(
            matches!(list_models(&custom(server.uri()),"",CancellationToken::new()).await,Err(Error::Invalid(message)) if message.contains("64 pages"))
        );
        Ok(())
    }
    #[test]
    fn anthropic_effort_uses_output_config_and_requires_declared_explicit_max() -> Result<()> {
        let mut config = custom("https://api.anthropic.com/v1".into());
        config.kind = ProviderKind::Anthropic;
        config.models[0].efforts = vec!["low".into(), "max".into()];
        let mut request = request();
        request.effort = Some("low".into());
        assert_eq!(
            request_body(&config, &request)?["output_config"]["effort"],
            "low"
        );
        request.effort = Some("high".into());
        assert!(matches!(
            request_body(&config, &request),
            Err(Error::Invalid(_))
        ));
        request.effort = Some("max".into());
        assert!(matches!(
            request_body(&config, &request),
            Err(Error::Approval(_))
        ));
        request.allow_highest_effort = true;
        let body = request_body(&config, &request)?;
        assert_eq!(body["output_config"]["effort"], "max");
        assert!(body.get("reasoning").is_none());
        Ok(())
    }
    #[tokio::test]
    async fn redirects_never_forward_provider_credentials() -> Result<()> {
        let source = MockServer::start().await;
        let target = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/models", target.uri())),
            )
            .mount(&source)
            .await;
        let config = custom(source.uri());
        assert!(matches!(
            list_models(&config, "fixture-secret", CancellationToken::new()).await,
            Err(Error::Provider { status: 307, .. })
        ));
        assert!(target
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty());
        Ok(())
    }
    #[tokio::test]
    async fn partial_http_stream_without_completion_returns_error() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
                ),
            )
            .mount(&server)
            .await;
        let mut events = Vec::new();
        let result = stream(
            &custom(server.uri()),
            "",
            &request(),
            CancellationToken::new(),
            |event| {
                events.push(event);
                Ok(())
            },
        )
        .await;
        assert!(matches!(result, Err(Error::Invalid(_))));
        assert!(matches!(&events[0],StreamEvent::Delta{text} if text=="partial"));
        Ok(())
    }
    #[test]
    fn refuses_insecure_endpoints_invalid_effort_and_premium_escalation() {
        assert!(validate(&custom("http://example.com".into())).is_err());
        assert!(validate(&custom("https://key@example.com".into())).is_err());
        let config = custom("http://localhost:1234/v1".into());
        let mut request = request();
        request.effort = Some("invented".into());
        assert!(request_body(&config, &request).is_err());
        request.effort = Some("ultra".into());
        assert!(matches!(
            request_body(&config, &request),
            Err(Error::Approval(_))
        ));
    }
    #[tokio::test]
    async fn streams_real_http_fixture_with_auth_and_usage() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/chat/completions")).and(header("authorization","Bearer fixture-key")).respond_with(ResponseTemplate::new(200).insert_header("content-type","text/event-stream").set_body_string("data: {\"choices\":[{\"delta\":{\"content\":\"Hello å\"}}]}\n\ndata: {\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n")).mount(&server).await;
        let config = custom(format!("{}/v1", server.uri()));
        let mut events = Vec::new();
        stream(
            &config,
            "fixture-key",
            &request(),
            CancellationToken::new(),
            |event| {
                events.push(event);
                Ok(())
            },
        )
        .await?;
        assert!(matches!(&events[0],StreamEvent::Delta{text} if text=="Hello å"));
        assert!(matches!(
            events[1],
            StreamEvent::Usage {
                input_tokens: 8,
                output_tokens: 2
            }
        ));
        assert!(matches!(events[2], StreamEvent::Done));
        Ok(())
    }
    #[tokio::test]
    async fn preserves_http_error_classification_and_cancellation() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        let config = custom(server.uri());
        assert!(matches!(
            stream(
                &config,
                "",
                &request(),
                CancellationToken::new(),
                |_| Ok(())
            )
            .await,
            Err(Error::Provider { status: 429, .. })
        ));
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            stream(&config, "", &request(), cancel, |_| Ok(())).await,
            Err(Error::Cancelled)
        ));
        Ok(())
    }
    #[test]
    fn decodes_each_provider_protocol() -> Result<()> {
        assert!(
            matches!(&decode(&ProviderKind::Openai,r#"{"type":"response.output_text.delta","delta":"ok"}"#)?.events[0],StreamEvent::Delta{text} if text=="ok")
        );
        assert!(
            matches!(&decode(&ProviderKind::Anthropic,r#"{"type":"content_block_delta","delta":{"text":"ok"}}"#)?.events[0],StreamEvent::Delta{text} if text=="ok")
        );
        assert!(
            matches!(&decode(&ProviderKind::Gemini,r#"{"candidates":[{"content":{"parts":[{"text":"ok"}]},"finishReason":"STOP"}]}"#)?.events[0],StreamEvent::Delta{text} if text=="ok")
        );
        Ok(())
    }
}
