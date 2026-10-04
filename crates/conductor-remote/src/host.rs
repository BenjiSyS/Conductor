//! The Conductor Host server.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path as AxPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use conductor_security::paths::PathGuard;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, watch};

use crate::audit::Audit;
use crate::devices::{Device, DeviceStore, PairError, PairingCode};
use crate::sync::{self, SyncError};
use crate::tls::HostIdentity;

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub bind: String,
    /// 0 = pick a free port.
    pub port: u16,
    pub data_dir: PathBuf,
    /// project id -> (display name, root folder)
    pub projects: BTreeMap<String, (String, PathBuf)>,
}

/// A message from a remote client for the integrator to act on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inbound {
    pub device_id: String,
    pub device_name: String,
    pub kind: String,
    pub payload: Value,
}

struct Shared {
    devices: Mutex<DeviceStore>,
    projects: BTreeMap<String, (String, PathGuard)>,
    audit: Audit,
    events: broadcast::Sender<Value>,
    inbound: mpsc::Sender<Inbound>,
    /// session id -> (device id, kill switch)
    sessions: Mutex<HashMap<String, (String, watch::Sender<bool>)>>,
    watchers: Mutex<HashMap<String, notify::RecommendedWatcher>>,
}

pub struct HostHandle {
    pub addr: SocketAddr,
    pub fingerprint: String,
    shared: Arc<Shared>,
    server: axum_server::Handle,
    pub inbound: mpsc::Receiver<Inbound>,
}

impl HostHandle {
    pub fn new_pairing_code(&self, projects: Vec<String>, can_control: bool) -> PairingCode {
        let c = self.shared.devices.lock().expect("devices lock").new_code(
            projects,
            can_control,
            crate::unix_now(),
        );
        self.shared
            .audit
            .log("pairing_code_created", None, "", None);
        c
    }

    pub fn devices(&self) -> Vec<Device> {
        self.shared.devices.lock().expect("devices lock").list()
    }

    /// Revoke a device and immediately close its live sessions.
    pub fn revoke(&self, device_id: &str) -> bool {
        let ok = self
            .shared
            .devices
            .lock()
            .expect("devices lock")
            .revoke(device_id);
        if ok {
            let sessions = self.shared.sessions.lock().expect("sessions lock");
            for (dev, kill) in sessions.values() {
                if dev == device_id {
                    let _ = kill.send(true);
                }
            }
            self.shared
                .audit
                .log("device_revoked", Some(device_id), "", None);
        }
        ok
    }

    /// Publish a state event to connected clients (filtered by project scope
    /// when the event has a `project` field).
    pub fn publish(&self, event: Value) {
        let _ = self.shared.events.send(event);
    }

    pub fn connected_clients(&self) -> usize {
        self.shared.sessions.lock().expect("sessions lock").len()
    }

    pub fn audit_tail(&self, n: usize) -> Vec<Value> {
        self.shared.audit.tail(n)
    }

    pub fn shutdown(&self) {
        self.server.graceful_shutdown(Some(Duration::from_secs(2)));
    }
}

pub struct Host;

impl Host {
    pub async fn start(cfg: HostConfig) -> Result<HostHandle, String> {
        let identity = HostIdentity::load_or_create(&cfg.data_dir).map_err(|e| e.to_string())?;
        let fingerprint = identity.fingerprint();
        let tls = identity.server_config().map_err(|e| e.to_string())?;
        let mut projects = BTreeMap::new();
        for (id, (name, root)) in &cfg.projects {
            let guard = PathGuard::new(root).map_err(|e| format!("project {name}: {e}"))?;
            projects.insert(id.clone(), (name.clone(), guard));
        }
        let (events, _) = broadcast::channel(1024);
        let (in_tx, in_rx) = mpsc::channel(256);
        let shared = Arc::new(Shared {
            devices: Mutex::new(
                DeviceStore::open(cfg.data_dir.join("devices.json")).map_err(|e| e.to_string())?,
            ),
            projects,
            audit: Audit::new(cfg.data_dir.join("audit.jsonl")),
            events,
            inbound: in_tx,
            sessions: Mutex::new(HashMap::new()),
            watchers: Mutex::new(HashMap::new()),
        });
        let app = Router::new()
            .route("/", get(index))
            .route("/api/pair", post(pair))
            .route("/api/status", get(status))
            .route("/api/projects/{id}/list", get(list_dir))
            .route("/api/projects/{id}/file", get(read_file).put(write_file))
            .route("/ws", get(ws))
            .with_state(shared.clone());
        let addr: SocketAddr = format!("{}:{}", cfg.bind, cfg.port)
            .parse()
            .map_err(|e| format!("bad bind address: {e}"))?;
        let std_listener = std::net::TcpListener::bind(addr)
            .map_err(|e| format!("cannot listen on {addr}: {e}"))?;
        std_listener
            .set_nonblocking(true)
            .map_err(|e| e.to_string())?;
        let local = std_listener.local_addr().map_err(|e| e.to_string())?;
        let rustls_cfg = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(tls));
        let handle = axum_server::Handle::new();
        let server = axum_server::from_tcp_rustls(std_listener, rustls_cfg).handle(handle.clone());
        tokio::spawn(async move {
            if let Err(e) = server
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await
            {
                tracing::error!(error = %e, "conductor host stopped");
            }
        });
        shared
            .audit
            .log("host_started", None, &local.to_string(), None);
        Ok(HostHandle {
            addr: local,
            fingerprint,
            shared,
            server: handle,
            inbound: in_rx,
        })
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
}

#[allow(clippy::result_large_err)] // axum Response as the error keeps handlers simple
fn auth(shared: &Shared, headers: &HeaderMap) -> Result<Device, Response> {
    let Some(tok) = bearer(headers) else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "sign in required" })),
        )
            .into_response());
    };
    shared
        .devices
        .lock()
        .expect("devices lock")
        .authenticate(&tok, crate::unix_now())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "device not trusted or revoked" })),
            )
                .into_response()
        })
}

#[allow(clippy::result_large_err)]
fn project_guard<'a>(
    shared: &'a Shared,
    dev: &Device,
    id: &str,
) -> Result<&'a PathGuard, Response> {
    if !dev.projects.iter().any(|p| p == id) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "this device has no access to that project" })),
        )
            .into_response());
    }
    shared.projects.get(id).map(|(_, g)| g).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "unknown project" })),
        )
            .into_response()
    })
}

#[derive(Deserialize)]
struct PairReq {
    code: String,
    name: String,
}

async fn pair(
    State(s): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<PairReq>,
) -> Response {
    let r = s
        .devices
        .lock()
        .expect("devices lock")
        .pair(&req.code, &req.name, crate::unix_now());
    match r {
        Ok((dev, token)) => {
            s.audit.log(
                "device_paired",
                Some(&dev.id),
                &dev.name,
                Some(&peer.to_string()),
            );
            Json(json!({ "device_id": dev.id, "token": token, "projects": dev.projects, "can_control": dev.can_control })).into_response()
        }
        Err(e) => {
            s.audit.log(
                "pairing_failed",
                None,
                &e.to_string(),
                Some(&peer.to_string()),
            );
            let code = if e == PairError::Locked {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::UNAUTHORIZED
            };
            (code, Json(json!({ "error": e.to_string() }))).into_response()
        }
    }
}

async fn status(State(s): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let dev = match auth(&s, &headers) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let projects: Vec<Value> = s
        .projects
        .iter()
        .filter(|(id, _)| dev.projects.contains(id))
        .map(|(id, (name, _))| json!({ "id": id, "name": name }))
        .collect();
    let connected = s.sessions.lock().expect("sessions lock").len();
    Json(json!({ "host": "Conductor Host", "version": env!("CARGO_PKG_VERSION"), "device": dev.name, "can_control": dev.can_control, "projects": projects, "connected": connected })).into_response()
}

#[derive(Deserialize)]
struct PathQ {
    #[serde(default)]
    path: String,
}

fn sync_err(e: SyncError) -> Response {
    let code = match &e {
        SyncError::Conflict { .. } => StatusCode::CONFLICT,
        SyncError::Path(_) => StatusCode::FORBIDDEN,
        SyncError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        SyncError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        SyncError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (code, Json(json!({ "error": e.to_string() }))).into_response()
}

async fn list_dir(
    State(s): State<Arc<Shared>>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
    Query(q): Query<PathQ>,
) -> Response {
    let dev = match auth(&s, &headers) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let g = match project_guard(&s, &dev, &id) {
        Ok(g) => g,
        Err(r) => return r,
    };
    match sync::list(g, &q.path) {
        Ok(v) => Json(json!(v
            .into_iter()
            .map(|(p, d, sz)| json!({ "path": p, "dir": d, "size": sz }))
            .collect::<Vec<_>>()))
        .into_response(),
        Err(e) => sync_err(e),
    }
}

async fn read_file(
    State(s): State<Arc<Shared>>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
    Query(q): Query<PathQ>,
) -> Response {
    let dev = match auth(&s, &headers) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let g = match project_guard(&s, &dev, &id) {
        Ok(g) => g,
        Err(r) => return r,
    };
    if conductor_security::secrets::is_sensitive_path(&q.path) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "sensitive files are not served remotely" })),
        )
            .into_response();
    }
    match sync::read(g, &q.path) {
        Ok(v) => Json(v).into_response(),
        Err(e) => sync_err(e),
    }
}

#[derive(Deserialize)]
struct WriteReq {
    path: String,
    content: String,
    base_hash: Option<String>,
}

async fn write_file(
    State(s): State<Arc<Shared>>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
    Json(req): Json<WriteReq>,
) -> Response {
    let dev = match auth(&s, &headers) {
        Ok(d) => d,
        Err(r) => return r,
    };
    if !dev.can_control {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "this device is view-only" })),
        )
            .into_response();
    }
    let g = match project_guard(&s, &dev, &id) {
        Ok(g) => g,
        Err(r) => return r,
    };
    match sync::write(g, &req.path, &req.content, req.base_hash.as_deref()) {
        Ok(v) => {
            s.audit.log(
                "file_written",
                Some(&dev.id),
                &format!("{id}:{}", v.path),
                None,
            );
            Json(v).into_response()
        }
        Err(e) => sync_err(e),
    }
}

async fn ws(
    State(s): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    up: WebSocketUpgrade,
) -> Response {
    up.on_upgrade(move |socket| session(s, socket, peer))
}

async fn session(s: Arc<Shared>, socket: WebSocket, peer: SocketAddr) {
    let (mut tx, mut rx) = socket.split();
    // First message must authenticate within 5 seconds.
    let dev = match tokio::time::timeout(Duration::from_secs(5), rx.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => {
            let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
            let tok = v.get("token").and_then(Value::as_str).unwrap_or("");
            s.devices
                .lock()
                .expect("devices lock")
                .authenticate(tok, crate::unix_now())
        }
        _ => None,
    };
    let Some(dev) = dev else {
        let _ = tx
            .send(Message::Text(
                json!({ "type": "error", "error": "authentication failed" })
                    .to_string()
                    .into(),
            ))
            .await;
        let _ = tx.close().await;
        s.audit
            .log("ws_auth_failed", None, "", Some(&peer.to_string()));
        return;
    };
    let sid = uuid::Uuid::new_v4().simple().to_string();
    let (kill_tx, mut kill_rx) = watch::channel(false);
    s.sessions
        .lock()
        .expect("sessions lock")
        .insert(sid.clone(), (dev.id.clone(), kill_tx));
    s.audit.log(
        "client_connected",
        Some(&dev.id),
        &dev.name,
        Some(&peer.to_string()),
    );
    start_watchers(&s, &dev);
    let _ = tx.send(Message::Text(json!({ "type": "hello", "device": dev.name, "projects": dev.projects, "can_control": dev.can_control }).to_string().into())).await;
    let _ = s
        .events
        .send(json!({ "type": "client_connected", "device": dev.name }));

    let mut events = s.events.subscribe();
    loop {
        tokio::select! {
            _ = kill_rx.changed() => {
                let _ = tx.send(Message::Text(json!({ "type": "revoked" }).to_string().into())).await;
                break;
            }
            ev = events.recv() => match ev {
                Ok(v) => {
                    if let Some(p) = v.get("project").and_then(Value::as_str) {
                        if !dev.projects.iter().any(|x| x == p) {
                            continue;
                        }
                    }
                    if tx.send(Message::Text(v.to_string().into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let _ = tx.send(Message::Text(json!({ "type": "lagged", "missed": n }).to_string().into())).await;
                }
                Err(_) => break,
            },
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                    let kind = v.get("type").and_then(Value::as_str).unwrap_or("").to_string();
                    if kind == "ping" {
                        let _ = tx.send(Message::Text(json!({ "type": "pong" }).to_string().into())).await;
                        continue;
                    }
                    if !dev.can_control {
                        let _ = tx.send(Message::Text(json!({ "type": "error", "error": "this device is view-only" }).to_string().into())).await;
                        continue;
                    }
                    if !matches!(kind.as_str(), "prompt" | "approval" | "stop" | "answer") {
                        let _ = tx.send(Message::Text(json!({ "type": "error", "error": "unknown message type" }).to_string().into())).await;
                        continue;
                    }
                    s.audit.log("client_action", Some(&dev.id), &kind, None);
                    let _ = s.inbound.try_send(Inbound { device_id: dev.id.clone(), device_name: dev.name.clone(), kind, payload: v });
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            }
        }
    }
    s.sessions.lock().expect("sessions lock").remove(&sid);
    s.audit.log(
        "client_disconnected",
        Some(&dev.id),
        &dev.name,
        Some(&peer.to_string()),
    );
    let _ = s
        .events
        .send(json!({ "type": "client_disconnected", "device": dev.name }));
    stop_idle_watchers(&s);
}

/// Watch files only while someone is connected (idle hosts stay light).
fn start_watchers(s: &Arc<Shared>, dev: &Device) {
    let mut w = s.watchers.lock().expect("watchers lock");
    for pid in &dev.projects {
        if w.contains_key(pid) {
            continue;
        }
        if let Some((_, guard)) = s.projects.get(pid) {
            let tx = s.events.clone();
            let emit: Arc<dyn Fn(sync::FileChange) + Send + Sync> = Arc::new(move |c| {
                let _ = tx.send(json!({ "type": "file_changed", "project": c.project, "path": c.path, "hash": c.hash, "diff": c.diff }));
            });
            match sync::watch(pid.clone(), guard.root(), emit) {
                Ok(watcher) => {
                    w.insert(pid.clone(), watcher);
                }
                Err(e) => tracing::warn!(error = %e, "could not watch project"),
            }
        }
    }
}

fn stop_idle_watchers(s: &Arc<Shared>) {
    let sessions = s.sessions.lock().expect("sessions lock");
    if sessions.is_empty() {
        s.watchers.lock().expect("watchers lock").clear();
    }
}

async fn index() -> Html<&'static str> {
    Html(include_str!("web.html"))
}
