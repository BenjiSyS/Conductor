//! Zero-setup providers through AI apps the user already has signed in
//! (Antigravity CLI, Codex CLI, Claude Code). See `conductor_engine::cli_bridge`.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use conductor_core::domain::{ProviderConfig, ProviderKind};
use conductor_engine::cli_bridge::{self, Cli};
use serde_json::{json, Value};
use tauri::{Manager, State};

use crate::state::{AppState, CmdResult};

static PORT: AtomicU16 = AtomicU16::new(0);

fn base_url(cli: Cli, port: u16) -> String {
    format!("http://127.0.0.1:{port}/{}/v1", cli.id())
}

/// Start the loopback bridge and point saved bridge providers at it (the
/// port changes on every launch).
pub fn start(app: &tauri::AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("CLI bridge unavailable: {e}");
                return;
            }
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        PORT.store(port, Ordering::SeqCst);
        if let Some(state) = handle.try_state::<AppState>() {
            if let Ok(snap) = state.store.snapshot(String::new()) {
                for mut p in snap.providers {
                    if let Some(cli) = Cli::from_provider_id(&p.id) {
                        p.base_url = base_url(cli, port);
                        let _ = state.store.put("provider", &p.id.clone(), &p);
                    }
                }
            }
        }
        let key: cli_bridge::KeyLookup = Arc::new(|id: &str| conductor_engine::paths::secret(id));
        // Chat already counts its own tokens; nothing extra to record here.
        let _ = cli_bridge::serve(listener, key, Arc::new(|_, _, _| {})).await;
    });
}

#[tauri::command]
pub async fn cli_bridges(state: State<'_, AppState>) -> CmdResult<Value> {
    let snap = state
        .store
        .snapshot(String::new())
        .map_err(|e| e.to_string())?;
    let found = cli_bridge::detect().await;
    let list: Vec<Value> = Cli::ALL
        .into_iter()
        .map(|cli| {
            let d = found.iter().find(|d| d.cli == cli);
            let connected = snap
                .providers
                .iter()
                .any(|p| p.id == cli.provider_id() && p.enabled);
            json!({
                "cli": cli,
                "label": cli.label(),
                "brand": cli.brand(),
                "installed": d.is_some(),
                "version": d.and_then(|d| d.version.clone()),
                "connected": connected,
                "sign_in_hint": cli.sign_in_hint(),
            })
        })
        .collect();
    Ok(json!({ "running": PORT.load(Ordering::SeqCst) != 0, "bridges": list }))
}

#[tauri::command]
pub async fn cli_bridge_connect(
    state: State<'_, AppState>,
    cli: String,
) -> CmdResult<ProviderConfig> {
    let cli = Cli::parse(&cli).ok_or("Unknown app")?;
    let port = PORT.load(Ordering::SeqCst);
    if port == 0 {
        return Err("The app bridge isn't running. Restart Conductor and try again.".into());
    }
    // Reuse the existing bridge key so reconnecting doesn't break running chats.
    let key = conductor_engine::paths::secret(&cli.provider_id()).unwrap_or_else(|| {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    });
    // The bridge authorizes against the keychain, so store the key before
    // save_provider verifies the connection by listing models.
    crate::chat::credential(&cli.provider_id())?
        .set_password(&key)
        .map_err(|_| "Could not save the bridge key to the OS credential store")?;
    let config = ProviderConfig {
        id: cli.provider_id(),
        name: cli.label().into(),
        kind: ProviderKind::OpenaiCompatible,
        base_url: base_url(cli, port),
        models: vec![],
        enabled: true,
    };
    crate::chat::save_provider(state, config, Some(key)).await
}
