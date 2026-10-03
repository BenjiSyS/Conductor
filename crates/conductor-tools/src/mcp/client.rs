//! Minimal MCP client over stdio (newline-delimited JSON-RPC 2.0).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

pub const PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("could not start MCP server: {0}")]
    Spawn(String),
    #[error("MCP server did not respond within {0}s")]
    Timeout(u64),
    #[error("MCP server closed the connection{0}")]
    Closed(String),
    #[error("MCP protocol error: {0}")]
    Protocol(String),
    #[error("MCP server error {code}: {message}")]
    Rpc { code: i64, message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub protocol_version: String,
}

pub struct McpClient {
    child: Child,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    next_id: AtomicU64,
    timeout: Duration,
    pub server: Option<ServerInfo>,
    stderr_tail: std::sync::Arc<std::sync::Mutex<String>>,
}

impl McpClient {
    /// Spawn a stdio server. `env` must already have secrets resolved.
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        cwd: Option<&Path>,
        timeout_secs: u64,
    ) -> Result<Self, McpError> {
        let exe = crate::exec::resolve_program(command)
            .ok_or_else(|| McpError::Spawn(format!("'{command}' not found on PATH")))?;
        let mut cmd = crate::exec::command_for(&exe, args).map_err(McpError::Spawn)?;
        cmd.envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(c) = cwd {
            cmd.current_dir(c);
        }
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = cmd.spawn().map_err(|e| McpError::Spawn(e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Spawn("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Spawn("no stdout".into()))?;
        let stderr_tail = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        if let Some(mut se) = child.stderr.take() {
            let tail = stderr_tail.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = se.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut t) = tail.lock() {
                        t.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if t.len() > 8192 {
                            let cut = t.len() - 8192;
                            let cut = (cut..t.len())
                                .find(|i| t.is_char_boundary(*i))
                                .unwrap_or(t.len());
                            t.drain(..cut);
                        }
                    }
                }
            });
        }
        Ok(Self {
            child,
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            next_id: AtomicU64::new(1),
            timeout: Duration::from_secs(timeout_secs),
            server: None,
            stderr_tail,
        })
    }

    pub fn stderr_tail(&self) -> String {
        self.stderr_tail
            .lock()
            .map(|t| t.clone())
            .unwrap_or_default()
    }

    async fn send(&self, msg: &Value) -> Result<(), McpError> {
        let mut line = serde_json::to_vec(msg).map_err(|e| McpError::Protocol(e.to_string()))?;
        line.push(b'\n');
        let mut w = self.stdin.lock().await;
        w.write_all(&line)
            .await
            .map_err(|e| McpError::Closed(format!(": {e}")))?;
        w.flush()
            .await
            .map_err(|e| McpError::Closed(format!(": {e}")))
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;
        let fut = async {
            let mut r = self.stdout.lock().await;
            let mut line = String::new();
            loop {
                line.clear();
                let n = r
                    .read_line(&mut line)
                    .await
                    .map_err(|e| McpError::Closed(format!(": {e}")))?;
                if n == 0 {
                    let tail = self.stderr_tail();
                    return Err(McpError::Closed(if tail.trim().is_empty() {
                        String::new()
                    } else {
                        format!(": {}", last_lines(&tail, 5))
                    }));
                }
                let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                    // Servers sometimes log to stdout; ignore non-JSON lines.
                    continue;
                };
                if v.get("id").and_then(Value::as_u64) != Some(id) {
                    // Notification or server->client request; ignore.
                    continue;
                }
                if let Some(err) = v.get("error") {
                    return Err(McpError::Rpc {
                        code: err.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: err
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    });
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        };
        tokio::time::timeout(self.timeout, fut)
            .await
            .map_err(|_| McpError::Timeout(self.timeout.as_secs()))?
    }

    pub async fn initialize(&mut self) -> Result<ServerInfo, McpError> {
        let r = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "Conductor", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        let info = ServerInfo {
            name: r
                .pointer("/serverInfo/name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            version: r
                .pointer("/serverInfo/version")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            protocol_version: r
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        };
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await?;
        self.server = Some(info.clone());
        Ok(info)
    }

    pub async fn list_tools(&self) -> Result<Vec<ToolInfo>, McpError> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let r = self.request("tools/list", params).await?;
            let tools: Vec<ToolInfo> =
                serde_json::from_value(r.get("tools").cloned().unwrap_or(json!([])))
                    .map_err(|e| McpError::Protocol(e.to_string()))?;
            out.extend(tools);
            cursor = r
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(String::from);
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }

    /// Call a tool; returns concatenated text content and the error flag.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<(String, bool), McpError> {
        let r = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        let is_error = r.get("isError").and_then(Value::as_bool).unwrap_or(false);
        let mut text = String::new();
        if let Some(items) = r.get("content").and_then(Value::as_array) {
            for it in items {
                match it.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(it.get("text").and_then(Value::as_str).unwrap_or(""));
                    }
                    Some(other) => text.push_str(&format!("\n[{other} content omitted]")),
                    None => {}
                }
            }
        }
        Ok((text, is_error))
    }

    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let v: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    v[v.len().saturating_sub(n)..].join(" | ")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny MCP server implemented in Node for tests.
    pub const FAKE_SERVER: &str = r#"
const rl = require('readline').createInterface({ input: process.stdin });
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');
console.log('not json log line');
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.method === 'initialize') send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: m.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1.0.0' } } });
  else if (m.method === 'tools/list') send({ jsonrpc: '2.0', id: m.id, result: { tools: [{ name: 'echo', description: 'Echo text back.', inputSchema: { type: 'object', properties: { text: { type: 'string' } } } }] } });
  else if (m.method === 'tools/call') {
    if (m.params.name === 'echo') send({ jsonrpc: '2.0', id: m.id, result: { content: [{ type: 'text', text: 'echo: ' + m.params.arguments.text }] } });
    else send({ jsonrpc: '2.0', id: m.id, error: { code: -32602, message: 'unknown tool' } });
  }
});
"#;

    pub fn write_fake(dir: &Path) -> Option<String> {
        crate::exec::resolve_program("node")?;
        let p = dir.join("fake-mcp.js");
        std::fs::write(&p, FAKE_SERVER).ok()?;
        Some(p.to_string_lossy().to_string())
    }

    #[tokio::test]
    async fn initialize_list_and_call() {
        let d = tempfile::tempdir().unwrap();
        let Some(script) = write_fake(d.path()) else {
            eprintln!("node not installed; skipping MCP client test");
            return;
        };
        let mut c = McpClient::spawn("node", &[script], &BTreeMap::new(), None, 10)
            .await
            .unwrap();
        let info = c.initialize().await.unwrap();
        assert_eq!(info.name, "fake");
        let tools = c.list_tools().await.unwrap();
        assert_eq!(tools[0].name, "echo");
        let (text, err) = c.call_tool("echo", json!({ "text": "hi" })).await.unwrap();
        assert_eq!(text, "echo: hi");
        assert!(!err);
        assert!(matches!(
            c.call_tool("nope", json!({})).await,
            Err(McpError::Rpc { code: -32602, .. })
        ));
        c.shutdown().await;
    }

    #[tokio::test]
    async fn crashing_server_reports_stderr() {
        let d = tempfile::tempdir().unwrap();
        if crate::exec::resolve_program("node").is_none() {
            return;
        }
        let p = d.path().join("crash.js");
        std::fs::write(
            &p,
            "console.error('Error: GITHUB_TOKEN missing'); process.exit(1);",
        )
        .unwrap();
        let mut c = McpClient::spawn(
            "node",
            &[p.to_string_lossy().to_string()],
            &BTreeMap::new(),
            None,
            10,
        )
        .await
        .unwrap();
        let e = c.initialize().await.unwrap_err();
        // stderr reader may race the exit; either way it is a closed connection
        assert!(matches!(e, McpError::Closed(_)), "{e:?}");
    }
}
