//! Usage view: provider-reported API rate limits, this session's chat token
//! counts, and (opt-in) subscription sign-in with usage meters.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use conductor_engine::subscriptions::{self, Endpoints, Service, SubscriptionUsage};
use serde_json::{json, Value};
use tauri::State;

use crate::state::{AppState, CmdResult};

#[derive(Default, Clone, Copy)]
struct Session {
    input: u64,
    output: u64,
    requests: u64,
}

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    static S: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Count a completed chat response against its provider.
pub fn record(provider_id: &str, input: u64, output: u64) {
    if let Ok(mut s) = sessions().lock() {
        let e = s.entry(provider_id.to_string()).or_default();
        e.input = e.input.saturating_add(input);
        e.output = e.output.saturating_add(output);
        e.requests += 1;
    }
}

fn test_mode() -> Option<String> {
    (std::env::var("CONDUCTOR_TEST_SUBSCRIPTIONS").as_deref() == Ok("1"))
        .then(|| std::env::var("CONDUCTOR_TEST_SUBSCRIPTION_BASE").ok())
        .flatten()
}

pub(crate) fn endpoints(service: Service) -> Endpoints {
    match test_mode() {
        Some(base) => Endpoints::local(&base, service),
        None => Endpoints::official(service),
    }
}

pub(crate) fn open_browser(url: &str) {
    if test_mode().is_some() {
        // Tests stand in for the browser: follow the authorize redirect to
        // the loopback callback.
        let url = url.to_string();
        tauri::async_runtime::spawn(async move {
            let _ = reqwest::get(url).await;
        });
        return;
    }
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

fn enabled(state: &AppState) -> CmdResult<()> {
    if state.prefs().subscription_signin {
        Ok(())
    } else {
        Err("Subscription sign-in is off. Turn it on in Settings › Usage first.".into())
    }
}

fn brand(p: &conductor_core::domain::ProviderConfig) -> &'static str {
    use conductor_core::domain::ProviderKind;
    if let Some(cli) = conductor_engine::cli_bridge::Cli::from_provider_id(&p.id) {
        return cli.brand();
    }
    let host = url::Url::parse(&p.base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string));
    match (&p.kind, host.as_deref()) {
        (_, Some("api.x.ai")) => "grok",
        (ProviderKind::Anthropic, _) => "claude",
        (ProviderKind::Openai, _) => "chatgpt",
        (ProviderKind::Gemini, _) => "gemini",
        _ => "other",
    }
}

#[tauri::command]
pub async fn usage_overview(state: State<'_, AppState>) -> CmdResult<Value> {
    let snapshot = state
        .store
        .snapshot(String::new())
        .map_err(|e| e.to_string())?;
    let sessions = sessions().lock().map(|s| s.clone()).unwrap_or_default();
    let providers: Vec<Value> = snapshot
        .providers
        .iter()
        .map(|p| {
            let s = sessions.get(&p.id).copied().unwrap_or_default();
            json!({
                "id": p.id,
                "name": p.name,
                "kind": p.kind,
                "enabled": p.enabled,
                // Brand of the signed-in app this provider bridges, if any.
                "bridge": conductor_engine::cli_bridge::Cli::from_provider_id(&p.id).map(|c| c.brand()),
                // Which brand tab (and theme) the provider belongs to.
                "brand": brand(p),
                "limits": conductor_core::limits::get(&p.id),
                "session": { "input": s.input, "output": s.output, "requests": s.requests },
            })
        })
        .collect();
    let on = state.prefs().subscription_signin;
    let subs: Vec<Value> = Service::ALL
        .into_iter()
        .map(|s| {
            let stored = if on { subscriptions::stored(s) } else { None };
            json!({
                "service": s,
                "label": s.label(),
                "signed_in": stored.is_some(),
                "account": stored.as_ref().and_then(|x| x.account.clone()),
                "plan": stored.as_ref().and_then(|x| x.plan.clone()),
            })
        })
        .collect();
    Ok(json!({ "providers": providers, "subscriptions_enabled": on, "subscriptions": subs }))
}

#[tauri::command]
pub async fn subscription_sign_in(state: State<'_, AppState>, service: String) -> CmdResult<Value> {
    enabled(&state)?;
    let service = Service::parse(&service)?;
    let stored = subscriptions::sign_in(service, endpoints(service), open_browser).await?;
    let _ = state.store.history(
        None,
        "settings",
        &format!("Signed in to {} subscription", service.label()),
    );
    Ok(json!({ "account": stored.account, "plan": stored.plan }))
}

#[tauri::command]
pub async fn subscription_usage(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    service: String,
) -> CmdResult<SubscriptionUsage> {
    enabled(&state)?;
    let service = Service::parse(&service)?;
    let u = subscriptions::usage(service, &endpoints(service)).await?;
    crate::extras::check_usage(&app, &u);
    Ok(u)
}

#[tauri::command]
pub async fn subscription_sign_out(state: State<'_, AppState>, service: String) -> CmdResult<()> {
    let service = Service::parse(&service)?;
    subscriptions::sign_out(service)?;
    let _ = state.store.history(
        None,
        "settings",
        &format!("Signed out of {} subscription", service.label()),
    );
    Ok(())
}
