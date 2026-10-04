//! Desktop notifications (work finished, plan usage running low) and large
//! paste storage.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use conductor_engine::pastes::{self, PasteInfo};
use conductor_engine::subscriptions::{self, Service};
use conductor_engine::EngineEvent;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

use crate::state::{AppState, CmdResult};

/// Plan windows at or above this share used trigger a warning (under 10% left).
const LOW_USAGE_PERCENT: f64 = 90.0;

fn focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
        .unwrap_or(false)
}

/// Show an OS notification and ask the UI to play the ding (if enabled).
pub fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
    let _ = app.emit("notify", json!({ "title": title, "body": body }));
}

/// A chat or Agent run finished. Notify when the user isn't looking, or the
/// run took long enough that they probably switched away.
pub fn run_finished(app: &AppHandle, what: &str, ok: bool, started: Instant) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if !state.prefs().notify_done {
        return;
    }
    if focused(app) && started.elapsed() < Duration::from_secs(20) {
        return;
    }
    let title = if ok {
        "Conductor finished"
    } else {
        "Conductor stopped"
    };
    notify(app, title, what);
}

/// Low-usage warnings already sent, keyed by service/window/reset.
fn warned() -> &'static Mutex<HashSet<String>> {
    static W: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    W.get_or_init(Default::default)
}

/// Warn once per plan window and reset period when under 10% is left.
pub fn check_usage(app: &AppHandle, usage: &subscriptions::SubscriptionUsage) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if !state.prefs().notify_low_usage {
        return;
    }
    for w in &usage.windows {
        if w.used_percent < LOW_USAGE_PERCENT {
            continue;
        }
        let reset = w
            .resets_at
            .clone()
            .or_else(|| w.resets_at_unix.map(|u| u.to_string()))
            .unwrap_or_default();
        let key = format!("{:?}|{}|{}", usage.service, w.label, reset);
        if warned().lock().map(|mut s| s.insert(key)).unwrap_or(false) {
            let left = (100.0 - w.used_percent).max(0.0).round();
            notify(
                app,
                &format!("{} usage is low", usage.service.label()),
                &format!("{left}% of your {} is left.", w.label.to_lowercase()),
            );
        }
    }
}

pub fn start(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        pastes::prune(&state.data_dir.join("pastes"), 30);
    }

    // Goals: notify when one reaches a final state.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(mut rx) = handle.try_state::<AppState>().map(|s| s.events.subscribe()) else {
            return;
        };
        let mut last: HashMap<String, String> = HashMap::new();
        loop {
            match rx.recv().await {
                Ok(EngineEvent::GoalSaved {
                    goal_id,
                    state,
                    done,
                    total,
                }) => {
                    let changed = last.insert(goal_id.clone(), state.clone()).as_deref()
                        != Some(state.as_str());
                    let final_state = matches!(state.as_str(), "complete" | "failed" | "blocked");
                    if changed
                        && final_state
                        && handle
                            .try_state::<AppState>()
                            .is_some_and(|s| s.prefs().notify_done)
                    {
                        let title = handle
                            .try_state::<AppState>()
                            .and_then(|s| s.goals.get(&goal_id))
                            .map(|g| {
                                g.goal
                                    .contract
                                    .objective
                                    .chars()
                                    .take(80)
                                    .collect::<String>()
                            })
                            .unwrap_or_else(|| "Goal".into());
                        let head = match state.as_str() {
                            "complete" => "Goal complete",
                            "failed" => "Goal failed",
                            _ => "Goal needs you",
                        };
                        notify(&handle, head, &format!("{title} ({done}/{total} tasks)"));
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    // Connector sign-ins: renew tokens before they expire.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(state) = handle.try_state::<AppState>() {
                refresh_connectors(&state).await;
            }
            tokio::time::sleep(Duration::from_secs(5 * 60)).await;
        }
    });

    // Plan usage: check signed-in subscriptions every 15 minutes.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            let on = handle
                .try_state::<AppState>()
                .map(|s| {
                    let p = s.prefs();
                    p.subscription_signin && p.notify_low_usage
                })
                .unwrap_or(false);
            if on {
                for service in Service::ALL {
                    if subscriptions::stored(service).is_none() {
                        continue;
                    }
                    if let Ok(u) =
                        subscriptions::usage(service, &crate::usage::endpoints(service)).await
                    {
                        check_usage(&handle, &u);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(15 * 60)).await;
        }
    });
}

#[tauri::command]
pub async fn save_paste(state: State<'_, AppState>, text: String) -> CmdResult<PasteInfo> {
    pastes::save(&state.data_dir.join("pastes"), &text)
}

/// The OS account name, shown on the local profile menu.
#[tauri::command]
pub async fn profile_info() -> CmdResult<serde_json::Value> {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    Ok(json!({ "user": user }))
}

#[tauri::command]
pub async fn test_notification(app: AppHandle) -> CmdResult<()> {
    notify(&app, "Conductor", "Notifications are on.");
    Ok(())
}

// ---------- setup: background preparation while the wizard is open ----------

/// Tools Conductor itself benefits from: Git (checkpoints, Git panel) and
/// Node.js (most MCP servers). Everything else is project-specific.
const ESSENTIALS: [&str; 2] = ["git", "node"];

fn scan_cache() -> &'static tokio::sync::Mutex<Option<(Instant, serde_json::Value)>> {
    static C: std::sync::OnceLock<tokio::sync::Mutex<Option<(Instant, serde_json::Value)>>> =
        std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

/// Local model servers that speak the OpenAI API (fully offline use).
async fn local_servers() -> Vec<serde_json::Value> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(1500))
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut out = vec![];
    for (id, label, url) in [
        ("ollama", "Ollama", "http://localhost:11434/v1"),
        ("lmstudio", "LM Studio", "http://localhost:1234/v1"),
    ] {
        let Ok(r) = client.get(format!("{url}/models")).send().await else {
            continue;
        };
        let Ok(v) = r.json::<serde_json::Value>().await else {
            continue;
        };
        let models = v["data"].as_array().map(|a| a.len()).unwrap_or(0);
        out.push(json!({ "id": id, "label": label, "url": url, "models": models }));
    }
    out
}

async fn scan() -> serde_json::Value {
    let (tools, bridges, local) = tokio::join!(
        conductor_tools::envdoctor::check_all(Some(&ESSENTIALS)),
        conductor_engine::cli_bridge::detect(),
        local_servers()
    );
    json!({ "tools": tools, "bridges": bridges, "local": local })
}

/// Start the scan in the background (called when the wizard opens).
#[tauri::command]
pub async fn setup_prepare() -> CmdResult<()> {
    tauri::async_runtime::spawn(async {
        let mut c = scan_cache().lock().await;
        if c.as_ref()
            .is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(60))
        {
            *c = Some((Instant::now(), scan().await));
        }
    });
    Ok(())
}

/// The scan result (waits for a running scan; refreshes after a minute).
#[tauri::command]
pub async fn setup_scan(fresh: Option<bool>) -> CmdResult<serde_json::Value> {
    let mut c = scan_cache().lock().await;
    if fresh == Some(true)
        || c.as_ref()
            .is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(60))
    {
        *c = Some((Instant::now(), scan().await));
    }
    Ok(c.as_ref().map(|(_, v)| v.clone()).unwrap_or_default())
}

/// Install an essential tool with the OS package manager, after the user
/// clicked Install. Commands that need a password (apt via sudo) are not run;
/// the UI shows them to copy instead.
#[tauri::command]
pub async fn env_install(tool: String) -> CmdResult<String> {
    if !ESSENTIALS.contains(&tool.as_str()) {
        return Err("Only Git and Node.js can be installed from here.".into());
    }
    let report = conductor_tools::envdoctor::check_all(Some(&[tool.as_str()]))
        .await
        .into_iter()
        .next()
        .ok_or("Unknown tool")?;
    let argv = report
        .install
        .ok_or("No installer is known for this system. Install it from the vendor's website.")?;
    if argv.first().map(String::as_str) == Some("sudo") {
        return Err(format!(
            "Run this in a terminal (it asks for your password): {}",
            argv.join(" ")
        ));
    }
    let mut req =
        conductor_tools::exec::ExecRequest::new(&argv[0], &[], std::env::temp_dir()).timeout(900);
    req.args = argv[1..].to_vec();
    let r = conductor_tools::exec::run(req, tokio_util::sync::CancellationToken::new()).await;
    *scan_cache().lock().await = None;
    if r.success() {
        Ok(format!("{} installed.", report.name))
    } else {
        let tail: String = r
            .combined()
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .join(" | ");
        Err(format!("Installing {} failed: {tail}", report.name))
    }
}

// ---------- remote MCP connectors: one-click OAuth ----------

use conductor_engine::mcp_oauth;
use conductor_tools::mcp::config::EnvValue;
use conductor_tools::mcp::{McpConfig, McpServer, Transport};

fn mcp_file(state: &AppState) -> std::path::PathBuf {
    state.data_dir.join("mcp.json")
}

fn store_integration(name: &str, value: &str) -> CmdResult<()> {
    conductor_engine::paths::credential(&format!("secret:{name}"))?
        .set_password(value)
        .map_err(|_| "Could not save the sign-in to the OS keychain".to_string())
}

fn save_signed(server: &str, signed: &mcp_oauth::Signed) -> CmdResult<()> {
    store_integration(
        &mcp_oauth::access_secret(server),
        &format!("Bearer {}", signed.access_token),
    )?;
    store_integration(
        &mcp_oauth::refresh_secret(server),
        &serde_json::to_string(&signed.refresh).map_err(|e| e.to_string())?,
    )
}

/// Sign in to an HTTP connector and attach the token to it.
async fn sign_in_server(
    state: &AppState,
    name: &str,
) -> CmdResult<conductor_tools::mcp::doctor::Diagnosis> {
    let path = mcp_file(state);
    let mut cfg = McpConfig::load(&path).map_err(|e| e.to_string())?;
    let mut server = cfg.get(name).cloned().ok_or("Connector not found")?;
    let Transport::Http { url, headers } = &mut server.transport else {
        return Err("Only remote (URL) connectors use sign-in.".into());
    };
    match mcp_oauth::sign_in(url, crate::usage::open_browser).await {
        Ok(signed) => {
            save_signed(name, &signed)?;
            headers.insert(
                "Authorization".into(),
                EnvValue::Secret {
                    secret: mcp_oauth::access_secret(name),
                },
            );
        }
        Err(e) if e.contains("doesn't need a sign-in") => {}
        Err(e) => return Err(e),
    }
    cfg.upsert(server).map_err(|e| e.to_string())?;
    cfg.save(&path).map_err(|e| e.to_string())?;
    let saved = cfg.get(name).ok_or("not saved")?;
    Ok(conductor_tools::mcp::doctor::diagnose(
        saved,
        &|k| conductor_engine::paths::integration_secret(k),
        45,
    )
    .await)
}

/// Add a remote connector by URL; opens its sign-in page when it needs one.
#[tauri::command]
pub async fn mcp_connect_remote(
    state: State<'_, AppState>,
    name: String,
    url: String,
) -> CmdResult<conductor_tools::mcp::doctor::Diagnosis> {
    let path = mcp_file(&state);
    let mut cfg = McpConfig::load(&path).map_err(|e| e.to_string())?;
    cfg.upsert(McpServer {
        name: name.clone(),
        transport: Transport::Http {
            url: url.trim().to_string(),
            headers: Default::default(),
        },
        enabled: true,
        description: String::new(),
        source: url.trim().to_string(),
        version: None,
        project: None,
        providers: vec![],
    })
    .map_err(|e| e.to_string())?;
    cfg.save(&path).map_err(|e| e.to_string())?;
    sign_in_server(&state, &name).await
}

#[tauri::command]
pub async fn mcp_sign_in(
    state: State<'_, AppState>,
    name: String,
) -> CmdResult<conductor_tools::mcp::doctor::Diagnosis> {
    sign_in_server(&state, &name).await
}

/// Renew connector tokens that expire within 10 minutes.
pub async fn refresh_connectors(state: &AppState) {
    let Ok(cfg) = McpConfig::load(&mcp_file(state)) else {
        return;
    };
    for s in &cfg.servers {
        let Some(raw) =
            conductor_engine::paths::integration_secret(&mcp_oauth::refresh_secret(&s.name))
        else {
            continue;
        };
        let Ok(rs) = serde_json::from_str::<mcp_oauth::RefreshState>(&raw) else {
            continue;
        };
        let soon = rs.expires_at.is_some_and(|t| t <= now_secs() + 600);
        if soon && rs.refresh_token.is_some() {
            if let Ok(signed) = mcp_oauth::refresh(&rs).await {
                let _ = save_signed(&s.name, &signed);
            }
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
