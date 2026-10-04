use conductor_tools::mcp::{
    client::McpError,
    config::{EnvValue, McpServer, Transport},
    session::{validate_http_url, McpSession},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

struct Fixture {
    url: String,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(mode: &'static str) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    let task = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let log = log.clone();
            connections.spawn(async move {
                let mut bytes = Vec::new();
                let mut buf = [0; 4096];
                let head_end = loop {
                    let n = stream.read(&mut buf).await.unwrap();
                    if n == 0 { return; }
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(i) = bytes.windows(4).position(|p| p == b"\r\n\r\n") { break i + 4; }
                    assert!(bytes.len() < 16384);
                };
                let head = String::from_utf8(bytes[..head_end].to_vec()).unwrap();
                let len = head.lines().find_map(|s| s.to_ascii_lowercase().strip_prefix("content-length:").map(|s| s.trim().parse::<usize>().unwrap())).unwrap_or(0);
                while bytes.len() < head_end + len {
                    let n = stream.read(&mut buf).await.unwrap(); if n == 0 { return; } bytes.extend_from_slice(&buf[..n]);
                }
                let value: Value = serde_json::from_slice(&bytes[head_end..head_end+len]).unwrap_or(Value::Null);
                log.lock().unwrap().push((head.clone(), value.clone()));
                if head.starts_with("DELETE") { let _ = stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await; return; }
                let method = value.get("method").and_then(Value::as_str).unwrap_or("");
                let id = value["id"].clone();
                if method.starts_with("notifications/") { let _ = stream.write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await; return; }
                if mode == "stall" && method == "tools/call" { std::future::pending::<()>().await; }
                if mode == "redirect" { let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await; return; }
                let result = match method {
                    "initialize" => json!({"protocolVersion":if mode == "version" {"future-unsupported"} else {"2025-06-18"},"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
                    "tools/list" => if mode == "cursor" { json!({"tools":[],"nextCursor":"same"}) } else if value["params"]["cursor"] == "second" { json!({"tools":[{"name":"second","description":"opaque-vault-value","inputSchema":{"type":"object","properties":{"key":{"description":"opaque-vault-value"}}}}]}) } else { json!({"tools":[{"name":"echo","description":"Echo","inputSchema":{"type":"object"}}],"nextCursor":"second"}) },
                    "tools/call" => if mode == "error" { Value::Null } else { json!({"content":[{"type":"text","text":"héllo opaque-vault-value"}],"structuredContent":{"value":"opaque-vault-value"}}) },
                    _ => panic!("unknown method {method}"),
                };
                let response = if mode == "error" && method == "tools/call" { json!({"jsonrpc":"2.0","id":id,"error":{"code":-1,"message":"opaque-vault-value"}}) } else { json!({"jsonrpc":"2.0","id":id,"result":result}) };
                let body = if mode == "oversize" && method == "tools/list" { vec![b'x'; 2 * 1024 * 1024 + 1] } else if mode == "sse" { format!(": ping\r\ndata: {}\r\n\r\ndata: {}\r\n\r\n",json!({"jsonrpc":"2.0","method":"notifications/progress","params":{}}),response).into_bytes() } else { response.to_string().into_bytes() };
                let kind = if mode == "sse" {"text/event-stream"} else {"application/json"};
                let header = format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nMcp-Session-Id: fixture-session\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                if stream.write_all(header.as_bytes()).await.is_err() { return; }
                // Small pieces exercise a UTF-8 character and CRLF split across network writes.
                for chunk in body.chunks(7) { if stream.write_all(chunk).await.is_err() { return; } tokio::task::yield_now().await; }
            });
        }
    });
    Fixture {
        url,
        requests,
        task,
    }
}

fn server(transport: Transport) -> McpServer {
    McpServer {
        name: "fixture".into(),
        transport,
        enabled: true,
        description: String::new(),
        source: "manual".into(),
        version: None,
        project: None,
        providers: vec![],
    }
}
fn http_server(f: &Fixture) -> McpServer {
    server(Transport::Http {
        url: f.url.clone(),
        headers: BTreeMap::from([(
            "Authorization".into(),
            EnvValue::Secret {
                secret: "fixture".into(),
            },
        )]),
    })
}
fn lookup(_: &str) -> Option<String> {
    Some("opaque-vault-value".into())
}

#[tokio::test]
async fn http_json_and_sse_sessions_paginate_redact_and_delete() {
    for mode in ["json", "sse"] {
        let fixture = fixture(mode).await;
        let d = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let mut session = McpSession::connect(&http_server(&fixture), &lookup, d.path(), &token)
            .await
            .unwrap();
        let tools = session.list_tools(&token).await.unwrap();
        assert_eq!(
            tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["echo", "second"]
        );
        assert!(!serde_json::to_string(&tools)
            .unwrap()
            .contains("opaque-vault-value"));
        let (text, error) = session
            .call_tool("echo", json!({"message":"hello"}), &token)
            .await
            .unwrap();
        assert!(!error);
        assert!(text.contains("héllo"));
        assert!(!text.contains("opaque-vault-value"));
        session.shutdown().await;
        let requests = fixture.requests.lock().unwrap();
        assert_eq!(requests.len(), 6);
        assert!(requests[0]
            .0
            .to_lowercase()
            .contains("authorization: opaque-vault-value"));
        assert!(!requests[0].0.to_lowercase().contains("mcp-session-id:"));
        for (head, _) in &requests[1..] {
            assert!(head
                .to_lowercase()
                .contains("mcp-session-id: fixture-session"));
            assert!(head
                .to_lowercase()
                .contains("mcp-protocol-version: 2025-06-18"));
        }
    }
}

#[tokio::test]
async fn http_cancel_sends_notification_closes_and_cannot_reuse_session() {
    let fixture = fixture("stall").await;
    let d = tempfile::tempdir().unwrap();
    let token = CancellationToken::new();
    let mut session = McpSession::connect(&http_server(&fixture), &lookup, d.path(), &token)
        .await
        .unwrap();
    let child = token.clone();
    let stop = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        child.cancel();
    });
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        session.call_tool("echo", json!({}), &token),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(McpError::Cancelled)));
    stop.await.unwrap();
    assert!(matches!(
        session.list_tools(&CancellationToken::new()).await,
        Err(McpError::Closed(_))
    ));
    let requests = fixture.requests.lock().unwrap();
    assert!(requests
        .iter()
        .any(|(_, v)| v["method"] == "notifications/cancelled" && v["params"]["requestId"] == 2));
    assert!(requests.iter().any(|(head, _)| head.starts_with("DELETE")));
}

#[tokio::test]
async fn http_oversize_duplicate_cursor_unsupported_version_and_redirect_fail_closed() {
    for mode in ["oversize", "cursor", "version", "redirect"] {
        let fixture = fixture(mode).await;
        let d = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let connected =
            McpSession::connect(&http_server(&fixture), &lookup, d.path(), &token).await;
        if matches!(mode, "version" | "redirect") {
            assert!(connected.is_err());
        } else {
            let mut session = connected.unwrap();
            assert!(session.list_tools(&token).await.is_err());
            assert!(session.list_tools(&token).await.is_err());
        }
    }
}

#[tokio::test]
async fn rpc_error_never_exposes_resolved_credential() {
    let fixture = fixture("error").await;
    let d = tempfile::tempdir().unwrap();
    let token = CancellationToken::new();
    let mut session = McpSession::connect(&http_server(&fixture), &lookup, d.path(), &token)
        .await
        .unwrap();
    let error = session
        .call_tool("echo", json!({}), &token)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("[REDACTED]"));
    assert!(!error.contains("opaque-vault-value"));
}

#[test]
fn endpoint_validation_rejects_prefix_spoofing_and_credentials() {
    for url in [
        "http://localhost.evil/mcp",
        "http://127.0.0.1.evil/mcp",
        "http://192.168.1.3/mcp",
        "https://user:pass@example.com/mcp",
        "https://example.com/mcp?token=secret",
        "https://example.com/mcp#secret",
        "file:///tmp/mcp",
    ] {
        assert!(validate_http_url(url).is_err(), "{url}");
    }
    for url in [
        "http://127.0.0.1:3456/mcp",
        "http://[::1]:3456/mcp",
        "http://localhost:3456/mcp",
        "https://example.com/mcp",
    ] {
        assert!(validate_http_url(url).is_ok(), "{url}");
    }
}

#[tokio::test]
async fn stdio_redacts_vault_output_and_rejects_large_newline_free_frame() {
    assert!(
        conductor_tools::exec::resolve_program("node").is_some(),
        "node required for integration fixture"
    );
    for large in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let script = d.path().join("mcp.cjs");
        std::fs::write(&script,format!(r#"
const rl=require('readline').createInterface({{input:process.stdin}});
const send=(m,r)=>process.stdout.write(JSON.stringify({{jsonrpc:'2.0',id:m.id,result:r}})+'\n');
rl.on('line',line=>{{ const m=JSON.parse(line);
if(m.method==='initialize') send(m,{{protocolVersion:m.params.protocolVersion,serverInfo:{{name:'fixture',version:'1'}}}});
if(m.method==='tools/call') {{ if({large}) process.stdout.write('x'.repeat(2*1024*1024+1)); else send(m,{{content:[{{type:'text',text:process.env.FIXTURE_SECRET}}]}}); }}
}});
"#)).unwrap();
        let server = server(Transport::Stdio {
            command: "node".into(),
            args: vec![script.to_string_lossy().into()],
            env: BTreeMap::from([(
                "FIXTURE_SECRET".into(),
                EnvValue::Secret {
                    secret: "fixture".into(),
                },
            )]),
        });
        let token = CancellationToken::new();
        let mut session = McpSession::connect(&server, &lookup, d.path(), &token)
            .await
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            session.call_tool("echo", json!({}), &token),
        )
        .await
        .unwrap();
        if large {
            assert!(matches!(result, Err(McpError::Protocol(_))));
        } else {
            assert_eq!(result.unwrap().0, "[REDACTED]");
            session.shutdown().await;
        }
    }
}

#[tokio::test]
async fn stdio_timeout_includes_blocked_stdin_write() {
    let client = conductor_tools::mcp::client::McpClient::spawn(
        "node",
        &["-e".into(), "setInterval(()=>{},1000)".into()],
        &BTreeMap::new(),
        None,
        1,
    )
    .await
    .unwrap();
    // The server never reads stdin. A large request fills its pipe before any
    // response read begins; the timeout must cover send and lock waits too.
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        client.call_tool("fixture", json!({"payload":"x".repeat(1024*1024)})),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(McpError::Timeout(1))));
    client.shutdown().await;
}

#[test]
fn config_load_revalidates_untrusted_disk_and_size() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("mcp.json");
    let mut server = server(Transport::Http {
        url: "http://localhost.evil/mcp".into(),
        headers: BTreeMap::new(),
    });
    std::fs::write(&path, json!({"servers":[server.clone()]}).to_string()).unwrap();
    assert!(conductor_tools::mcp::McpConfig::load(&path).is_err());
    server.name = "valid".into();
    server.transport = Transport::Http {
        url: "https://example.com/mcp".into(),
        headers: BTreeMap::new(),
    };
    std::fs::write(
        &path,
        json!({"servers":[server.clone(),server]}).to_string(),
    )
    .unwrap();
    assert!(conductor_tools::mcp::McpConfig::load(&path).is_err());
    std::fs::write(&path, vec![b' '; 2 * 1024 * 1024 + 1]).unwrap();
    assert!(conductor_tools::mcp::McpConfig::load(&path).is_err());
}

#[tokio::test]
async fn doctor_checks_http_and_diagnoses_missing_secret_without_connecting() {
    let fixture = fixture("json").await;
    let server = http_server(&fixture);
    let missing = conductor_tools::mcp::doctor::diagnose(&server, &|_| None, 2).await;
    assert_eq!(
        missing.status,
        conductor_tools::mcp::doctor::McpStatus::AuthenticationRequired
    );
    assert!(fixture.requests.lock().unwrap().is_empty());
    let diagnosis = conductor_tools::mcp::doctor::diagnose(&server, &lookup, 2).await;
    assert_eq!(
        diagnosis.status,
        conductor_tools::mcp::doctor::McpStatus::Connected
    );
    assert_eq!(diagnosis.tools, ["echo", "second"]);
    assert!(diagnosis.detail.contains("2025-06-18"));
    assert!(!serde_json::to_string(&diagnosis)
        .unwrap()
        .contains("opaque-vault-value"));
}
