//! Remote client: pairs with a host and talks to it over pinned TLS.
//! Used by the desktop app's "Connect to host" and by integration tests.

use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::Message;

use crate::tls::pinned_client_config;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("host refused: {status} {message}")]
    Http { status: u16, message: String },
    #[error("protocol error: {0}")]
    Protocol(String),
}

#[derive(Clone)]
pub struct RemoteClient {
    pub base: String,
    pub fingerprint: String,
    pub token: Option<String>,
    http: reqwest::Client,
}

impl RemoteClient {
    /// `addr` is `host:port`; `fingerprint` comes from the host's pairing
    /// screen.
    pub fn new(addr: &str, fingerprint: &str) -> Result<Self, ClientError> {
        let cfg = pinned_client_config(fingerprint);
        let http = reqwest::Client::builder()
            .use_preconfigured_tls(cfg)
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        Ok(Self {
            base: format!("https://{addr}"),
            fingerprint: fingerprint.to_string(),
            token: None,
            http,
        })
    }

    async fn check(resp: reqwest::Response) -> Result<Value, ClientError> {
        let status = resp.status().as_u16();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            Ok(v)
        } else {
            Err(ClientError::Http {
                status,
                message: v
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        }
    }

    pub async fn pair(&mut self, code: &str, device_name: &str) -> Result<Value, ClientError> {
        let r = self
            .http
            .post(format!("{}/api/pair", self.base))
            .json(&json!({ "code": code, "name": device_name }))
            .send()
            .await
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        let v = Self::check(r).await?;
        self.token = v.get("token").and_then(Value::as_str).map(String::from);
        Ok(v)
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(t) => rb.bearer_auth(t),
            None => rb,
        }
    }

    pub async fn status(&self) -> Result<Value, ClientError> {
        let r = self
            .auth(self.http.get(format!("{}/api/status", self.base)))
            .send()
            .await
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        Self::check(r).await
    }

    pub async fn read_file(&self, project: &str, path: &str) -> Result<Value, ClientError> {
        let r = self
            .auth(
                self.http
                    .get(format!("{}/api/projects/{project}/file", self.base))
                    .query(&[("path", path)]),
            )
            .send()
            .await
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        Self::check(r).await
    }

    pub async fn list(&self, project: &str, path: &str) -> Result<Value, ClientError> {
        let r = self
            .auth(
                self.http
                    .get(format!("{}/api/projects/{project}/list", self.base))
                    .query(&[("path", path)]),
            )
            .send()
            .await
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        Self::check(r).await
    }

    pub async fn write_file(
        &self,
        project: &str,
        path: &str,
        content: &str,
        base_hash: Option<&str>,
    ) -> Result<Value, ClientError> {
        let r = self
            .auth(
                self.http
                    .put(format!("{}/api/projects/{project}/file", self.base)),
            )
            .json(&json!({ "path": path, "content": content, "base_hash": base_hash }))
            .send()
            .await
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        Self::check(r).await
    }

    /// Open the real-time state channel. Returns a sender for client
    /// messages and a receiver of host events.
    pub async fn connect(
        &self,
    ) -> Result<
        (
            tokio::sync::mpsc::Sender<Value>,
            tokio::sync::mpsc::Receiver<Value>,
        ),
        ClientError,
    > {
        let hostport = self.base.trim_start_matches("https://").to_string();
        let tcp = TcpStream::connect(&hostport)
            .await
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        let connector = TlsConnector::from(Arc::new(pinned_client_config(&self.fingerprint)));
        let sni = rustls::pki_types::ServerName::try_from("conductor-host")
            .map_err(|e| ClientError::Protocol(e.to_string()))?;
        let tls = connector
            .connect(sni, tcp)
            .await
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        let url = format!("wss://{hostport}/ws");
        let (ws, _) = tokio_tungstenite::client_async(url, tls)
            .await
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        let (mut sink, mut stream) = ws.split();
        sink.send(Message::Text(
            json!({ "token": self.token.clone().unwrap_or_default() })
                .to_string()
                .into(),
        ))
        .await
        .map_err(|e| ClientError::Protocol(e.to_string()))?;
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Value>(64);
        let (in_tx, in_rx) = tokio::sync::mpsc::channel::<Value>(256);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    m = out_rx.recv() => match m {
                        Some(v) => { if sink.send(Message::Text(v.to_string().into())).await.is_err() { break; } }
                        None => { let _ = sink.close().await; break; }
                    },
                    m = stream.next() => match m {
                        Some(Ok(Message::Text(t))) => {
                            if let Ok(v) = serde_json::from_str::<Value>(&t) {
                                if in_tx.send(v).await.is_err() { break; }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        _ => {}
                    }
                }
            }
        });
        Ok((out_tx, in_rx))
    }
}
