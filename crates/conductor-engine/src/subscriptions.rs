//! Opt-in subscription sign-in for Claude, ChatGPT and Gemini, and their
//! usage meters.
//!
//! **Unsupported by the providers.** These flows reuse the public OAuth
//! clients of each vendor's own command-line tool (Claude Code, Codex CLI,
//! Gemini CLI) and read private usage endpoints. They can change or stop
//! working at any time, and using them from another app may break the
//! provider's terms. The desktop app keeps them behind a setting that is off
//! by default and says so where it is turned on.
//!
//! Flow: PKCE authorization code with a loopback redirect. The user signs in
//! in their own browser; Conductor never sees a password. Only the refresh
//! token and account labels are stored, in the OS keychain (Windows limits
//! keychain entries to about 1,280 characters, which access tokens exceed).
//! Access tokens live in memory and are refreshed when they expire.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Claude,
    Chatgpt,
    Gemini,
}

impl Service {
    pub const ALL: [Service; 3] = [Service::Claude, Service::Chatgpt, Service::Gemini];

    pub fn id(self) -> &'static str {
        match self {
            Service::Claude => "claude",
            Service::Chatgpt => "chatgpt",
            Service::Gemini => "gemini",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Service::Claude => "Claude",
            Service::Chatgpt => "ChatGPT",
            Service::Gemini => "Gemini",
        }
    }
    pub fn parse(id: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|s| s.id() == id)
            .ok_or_else(|| format!("Unknown subscription service: {id}"))
    }
    fn credential_id(self) -> String {
        format!("subscription:{}", self.id())
    }
}

/// Public OAuth client of each vendor's CLI. These identify the CLI, not a
/// user, and are published in the CLIs' sources.
///
/// Google's installed-app flow also needs a client secret; Gemini CLI's
/// published one is built in. `CONDUCTOR_GEMINI_OAUTH_CLIENT_ID` and
/// `CONDUCTOR_GEMINI_OAUTH_CLIENT_SECRET` override it.
struct OAuthClient {
    client_id: String,
    client_secret: Option<String>,
    scopes: &'static str,
}

const GEMINI_CLIENT_ID_ENV: &str = "CONDUCTOR_GEMINI_OAUTH_CLIENT_ID";
const GEMINI_CLIENT_SECRET_ENV: &str = "CONDUCTOR_GEMINI_OAUTH_CLIENT_SECRET";

fn oauth_client(service: Service) -> Result<OAuthClient> {
    Ok(match service {
        Service::Claude => OAuthClient {
            client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e".into(),
            client_secret: None,
            scopes: "org:create_api_key user:profile user:inference",
        },
        Service::Chatgpt => OAuthClient {
            client_id: "app_EMoamEEZ73f0CkXaXp7hrann".into(),
            client_secret: None,
            scopes: "openid profile email offline_access",
        },
        Service::Gemini => {
            // Gemini CLI's published installed-app client (Apache-2.0
            // google-gemini/gemini-cli). Google documents installed-app
            // client secrets as not confidential. Override with your own
            // "Desktop app" client via the environment if you prefer.
            let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
            OAuthClient {
                client_id: var(GEMINI_CLIENT_ID_ENV)
                    .unwrap_or_else(|| "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com".into()),
                client_secret: Some(var(GEMINI_CLIENT_SECRET_ENV).unwrap_or_else(|| "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".into())),
                scopes: "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile",
            }
        }
    })
}

fn callback_path(service: Service) -> &'static str {
    match service {
        Service::Claude => "/callback",
        Service::Chatgpt => "/auth/callback",
        Service::Gemini => "/oauth2callback",
    }
}

/// Where a service's sign-in and usage live. Tests point these at a local
/// server.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    /// Root of the usage API.
    pub api: String,
    /// Loopback port. ChatGPT's client only accepts port 1455; `None` picks
    /// a free port.
    pub port: Option<u16>,
}

impl Endpoints {
    pub fn official(service: Service) -> Self {
        let (authorize, token, api, port) = match service {
            Service::Claude => (
                "https://claude.ai/oauth/authorize",
                "https://console.anthropic.com/v1/oauth/token",
                "https://api.anthropic.com",
                None,
            ),
            Service::Chatgpt => (
                "https://auth.openai.com/oauth/authorize",
                "https://auth.openai.com/oauth/token",
                "https://chatgpt.com",
                Some(1455),
            ),
            Service::Gemini => (
                "https://accounts.google.com/o/oauth2/v2/auth",
                "https://oauth2.googleapis.com/token",
                "https://cloudcode-pa.googleapis.com",
                None,
            ),
        };
        Self {
            authorize: authorize.into(),
            token: token.into(),
            api: api.into(),
            port,
        }
    }

    /// All endpoints under one local base (`<base>/<service>/authorize`,
    /// `/token`, and the API root `<base>/<service>`), on a free port.
    pub fn local(base: &str, service: Service) -> Self {
        let root = format!("{}/{}", base.trim_end_matches('/'), service.id());
        Self {
            authorize: format!("{root}/authorize"),
            token: format!("{root}/token"),
            api: root,
            port: None,
        }
    }
}

/// Tokens from a sign-in or refresh.
#[derive(Clone, PartialEq)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds.
    pub expires_at: Option<u64>,
    pub account: Option<String>,
    pub plan: Option<String>,
    /// ChatGPT workspace id, sent with usage requests.
    pub account_id: Option<String>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("expires_at", &self.expires_at)
            .field("account", &self.account)
            .field("plan", &self.plan)
            .finish()
    }
}

/// What is persisted in the keychain.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    pub refresh_token: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub label: String,
    /// 0–100, as reported by the provider.
    pub used_percent: f64,
    /// RFC 3339 reset time, when the provider reports one that way.
    pub resets_at: Option<String>,
    /// Unix-seconds reset time, when the provider reports one that way.
    pub resets_at_unix: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionUsage {
    pub service: Service,
    pub account: Option<String>,
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("Conductor/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

fn random_token() -> String {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

pub fn pkce() -> Pkce {
    let verifier = format!("{}{}", random_token(), random_token());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Pkce {
        verifier,
        challenge,
    }
}

/// The JSON payload of a JWT, without verifying it. Used only to read
/// display labels (email, plan) from tokens the provider just issued to us
/// over TLS; nothing is authorized from these claims.
fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .or_else(|_| STANDARD.decode(payload))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// A sign-in waiting for the browser to come back.
pub struct Pending {
    pub service: Service,
    pub url: String,
    state: String,
    verifier: String,
    redirect_uri: String,
    listener: TcpListener,
    endpoints: Endpoints,
}

/// Start a sign-in: bind the loopback callback and build the URL to open in
/// the user's browser.
pub async fn begin(service: Service, endpoints: Endpoints) -> Result<Pending> {
    let client = oauth_client(service)?;
    let listener = TcpListener::bind(("127.0.0.1", endpoints.port.unwrap_or(0)))
        .await
        .map_err(|e| match endpoints.port {
            Some(p) => format!(
                "Port {p} is busy (another sign-in, or Codex CLI, may be using it). Close it and try again. ({e})"
            ),
            None => e.to_string(),
        })?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://localhost:{port}{}", callback_path(service));
    let pkce = pkce();
    let state = random_token();
    let mut params: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", &client.client_id),
        ("redirect_uri", &redirect_uri),
        ("scope", client.scopes),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("state", &state),
    ];
    match service {
        Service::Claude => params.push(("code", "true")),
        Service::Chatgpt => params.extend([
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", "codex_cli_rs"),
        ]),
        Service::Gemini => params.extend([("access_type", "offline"), ("prompt", "consent")]),
    }
    let mut url = url::Url::parse(&endpoints.authorize).map_err(|e| e.to_string())?;
    url.query_pairs_mut().extend_pairs(params);
    Ok(Pending {
        service,
        url: url.to_string(),
        state,
        verifier: pkce.verifier,
        redirect_uri,
        listener,
        endpoints,
    })
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Conductor</title><body style=\"font:16px system-ui;display:grid;place-items:center;height:90vh\"><p>Signed in to Conductor. You can close this tab.</p>";

/// Wait for one browser callback on `path`, check `state`, return the code.
pub(crate) async fn wait_for_code(
    listener: &TcpListener,
    path: &str,
    state: &str,
    timeout: Duration,
) -> Result<String> {
    let wait = async {
        loop {
            let (mut sock, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]);
            let target = head
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("GET "))
                .and_then(|l| l.split(' ').next())
                .unwrap_or("");
            let Ok(url) = url::Url::parse(&format!("http://localhost{target}")) else {
                continue;
            };
            if url.path() != path {
                let _ = sock
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }
            let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
            let (status, body, result) = if let Some(e) = q.get("error") {
                let msg = q.get("error_description").unwrap_or(e).clone();
                (
                    "400 Bad Request",
                    format!("<p>Sign-in failed: {}</p>", html_escape(&msg)),
                    Err(format!("Sign-in was not completed: {msg}")),
                )
            } else if q.get("state").map(String::as_str) != Some(state) {
                (
                    "400 Bad Request",
                    "<p>Sign-in failed: the response did not match this request.</p>".to_string(),
                    Err(
                        "Sign-in response did not match this request (state mismatch).".to_string(),
                    ),
                )
            } else if let Some(code) = q.get("code") {
                ("200 OK", DONE_PAGE.to_string(), Ok(code.clone()))
            } else {
                (
                    "400 Bad Request",
                    "<p>Sign-in failed: no code returned.</p>".to_string(),
                    Err("The provider returned no sign-in code.".to_string()),
                )
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
            return result;
        }
    };
    tokio::time::timeout(timeout, wait)
        .await
        .map_err(|_| "Sign-in timed out. Try again.".to_string())?
}

/// Wait for the browser callback, check `state`, and exchange the code.
pub async fn finish(pending: Pending, timeout: Duration) -> Result<Tokens> {
    let path = callback_path(pending.service);
    let code = wait_for_code(&pending.listener, path, &pending.state, timeout).await?;
    exchange(
        pending.service,
        &pending.endpoints,
        &code,
        &pending.verifier,
        &pending.redirect_uri,
        &pending.state,
    )
    .await
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

async fn post_token(
    service: Service,
    endpoints: &Endpoints,
    fields: Vec<(&str, String)>,
) -> Result<Value> {
    let req = http()?.post(&endpoints.token);
    // Claude's token endpoint takes JSON; OpenAI and Google take a form.
    let req = if service == Service::Claude {
        let body: serde_json::Map<String, Value> = fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), Value::String(v)))
            .collect();
        req.json(&body)
    } else {
        req.form(&fields)
    };
    let resp = req
        .send()
        .await
        .map_err(|e| format!("{} sign-in failed: {e}", service.label()))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let why = body
            .get("error_description")
            .or_else(|| body.pointer("/error/message"))
            .or_else(|| body.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("request rejected");
        return Err(format!(
            "{} sign-in failed ({status}): {why}",
            service.label()
        ));
    }
    Ok(body)
}

fn tokens_from(service: Service, body: &Value, previous_refresh: Option<String>) -> Result<Tokens> {
    let s = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_string);
    let access_token = s("access_token").ok_or("Sign-in response had no access token")?;
    let expires_at = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .map(|n| now() + n)
        .or_else(|| jwt_claims(&access_token)?.get("exp")?.as_u64());
    let id_claims = s("id_token").and_then(|t| jwt_claims(&t));
    let (account, plan, account_id) = match service {
        Service::Claude => (
            body.pointer("/account/email_address")
                .and_then(Value::as_str)
                .map(str::to_string),
            None,
            None,
        ),
        Service::Chatgpt => {
            let auth = id_claims
                .as_ref()
                .and_then(|c| c.get("https://api.openai.com/auth"))
                .cloned()
                .or_else(|| {
                    jwt_claims(&access_token)?
                        .get("https://api.openai.com/auth")
                        .cloned()
                });
            let field = |k: &str| auth.as_ref()?.get(k)?.as_str().map(str::to_string);
            (
                id_claims
                    .as_ref()
                    .and_then(|c| c.get("email")?.as_str().map(str::to_string)),
                field("chatgpt_plan_type"),
                field("chatgpt_account_id"),
            )
        }
        Service::Gemini => (
            id_claims
                .as_ref()
                .and_then(|c| c.get("email")?.as_str().map(str::to_string)),
            None,
            None,
        ),
    };
    Ok(Tokens {
        access_token,
        refresh_token: s("refresh_token").or(previous_refresh),
        expires_at,
        account,
        plan,
        account_id,
    })
}

async fn exchange(
    service: Service,
    endpoints: &Endpoints,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    state: &str,
) -> Result<Tokens> {
    let client = oauth_client(service)?;
    let mut fields = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("client_id", client.client_id),
        ("code_verifier", verifier.to_string()),
    ];
    if service == Service::Claude {
        fields.push(("state", state.to_string()));
    }
    if let Some(secret) = client.client_secret {
        fields.push(("client_secret", secret));
    }
    let body = post_token(service, endpoints, fields).await?;
    tokens_from(service, &body, None)
}

/// Exchange a refresh token for a fresh access token. Providers may rotate
/// the refresh token; the returned one must replace the stored one.
pub async fn refresh(
    service: Service,
    endpoints: &Endpoints,
    refresh_token: &str,
) -> Result<Tokens> {
    let client = oauth_client(service)?;
    let mut fields = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
        ("client_id", client.client_id),
    ];
    if let Some(secret) = client.client_secret {
        fields.push(("client_secret", secret));
    }
    let body = post_token(service, endpoints, fields)
        .await
        .map_err(|e| format!("{e}. Sign in again."))?;
    tokens_from(service, &body, Some(refresh_token.to_string()))
}

fn humanize(key: &str) -> String {
    match key {
        "five_hour" => "5-hour session".into(),
        "seven_day" => "Weekly (all models)".into(),
        "seven_day_opus" => "Weekly Opus".into(),
        "seven_day_sonnet" => "Weekly Sonnet".into(),
        "seven_day_oauth_apps" => "Weekly (apps)".into(),
        other => {
            let s = other.replace('_', " ");
            let mut c = s.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}

fn window_label(seconds: Option<u64>, fallback: &str) -> String {
    match seconds {
        Some(18_000) => "5-hour window".into(),
        Some(604_800) => "Weekly window".into(),
        Some(s) if s % 86_400 == 0 => format!("{}-day window", s / 86_400),
        Some(s) if s % 3_600 == 0 => format!("{}-hour window", s / 3_600),
        _ => fallback.into(),
    }
}

/// Parse Claude's `/api/oauth/usage` response.
pub fn parse_claude_usage(body: &Value) -> Vec<UsageWindow> {
    let Some(obj) = body.as_object() else {
        return vec![];
    };
    let mut out: Vec<UsageWindow> = obj
        .iter()
        .filter_map(|(k, v)| {
            let used = v.get("utilization")?.as_f64()?;
            Some(UsageWindow {
                label: humanize(k),
                used_percent: used.clamp(0.0, 100.0),
                resets_at: v
                    .get("resets_at")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                resets_at_unix: None,
            })
        })
        .collect();
    // Session window first, then weekly ones.
    out.sort_by_key(|w| (!w.label.starts_with("5-hour"), w.label.clone()));
    out
}

/// Parse ChatGPT's `/backend-api/wham/usage` response: (plan, windows).
pub fn parse_chatgpt_usage(body: &Value) -> (Option<String>, Vec<UsageWindow>) {
    let plan = body
        .get("plan_type")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut out = vec![];
    for (key, fallback) in [
        ("primary_window", "Short window"),
        ("secondary_window", "Long window"),
    ] {
        let Some(w) = body
            .pointer(&format!("/rate_limit/{key}"))
            .filter(|w| w.is_object())
        else {
            continue;
        };
        let Some(used) = w.get("used_percent").and_then(Value::as_f64) else {
            continue;
        };
        let reset = w
            .get("reset_at")
            .and_then(Value::as_u64)
            .or_else(|| Some(now() + w.get("reset_after_seconds")?.as_u64()?));
        out.push(UsageWindow {
            label: window_label(
                w.get("limit_window_seconds").and_then(Value::as_u64),
                fallback,
            ),
            used_percent: used.clamp(0.0, 100.0),
            resets_at: None,
            resets_at_unix: reset,
        });
    }
    (plan, out)
}

/// Parse Gemini Code Assist `retrieveUserQuota` buckets.
pub fn parse_gemini_quota(body: &Value) -> Vec<UsageWindow> {
    let mut out: Vec<UsageWindow> = body
        .get("buckets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|b| {
            let remaining = b.get("remainingFraction")?.as_f64()?;
            let model = b
                .get("modelId")
                .and_then(Value::as_str)
                .unwrap_or("All models");
            let kind = b.get("tokenType").and_then(Value::as_str).unwrap_or("");
            let label = if kind.is_empty() || kind.eq_ignore_ascii_case("REQUESTS") {
                model.to_string()
            } else {
                format!("{model} ({})", kind.to_lowercase())
            };
            Some(UsageWindow {
                label,
                used_percent: ((1.0 - remaining) * 100.0).clamp(0.0, 100.0),
                resets_at: b
                    .get("resetTime")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                resets_at_unix: None,
            })
        })
        .collect();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out.dedup_by(|a, b| a.label == b.label);
    out
}

async fn get_json(req: reqwest::RequestBuilder, service: Service) -> Result<Value> {
    let resp = req
        .send()
        .await
        .map_err(|e| format!("{} usage unavailable: {e}", service.label()))?;
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(UNAUTHORIZED.into());
    }
    if !status.is_success() {
        return Err(format!("{} usage unavailable ({status}).", service.label()));
    }
    resp.json().await.map_err(|_| {
        format!(
            "{} returned usage in an unexpected format.",
            service.label()
        )
    })
}

const UNAUTHORIZED: &str = "unauthorized";

/// Fetch usage with an access token.
pub async fn fetch_usage(
    service: Service,
    endpoints: &Endpoints,
    tokens: &Tokens,
) -> Result<SubscriptionUsage> {
    let api = endpoints.api.trim_end_matches('/');
    let client = http()?;
    let bearer = format!("Bearer {}", tokens.access_token);
    let (plan, windows) = match service {
        Service::Claude => {
            let body = get_json(
                client
                    .get(format!("{api}/api/oauth/usage"))
                    .header("authorization", &bearer)
                    .header("anthropic-beta", "oauth-2025-04-20"),
                service,
            )
            .await?;
            (tokens.plan.clone(), parse_claude_usage(&body))
        }
        Service::Chatgpt => {
            let mut req = client
                .get(format!("{api}/backend-api/wham/usage"))
                .header("authorization", &bearer);
            if let Some(id) = &tokens.account_id {
                req = req.header("chatgpt-account-id", id);
            }
            let (plan, windows) = parse_chatgpt_usage(&get_json(req, service).await?);
            (plan.or_else(|| tokens.plan.clone()), windows)
        }
        Service::Gemini => {
            let meta = json!({"metadata": {"ideType": "IDE_UNSPECIFIED", "platform": "PLATFORM_UNSPECIFIED", "pluginType": "GEMINI"}});
            let assist = get_json(
                client
                    .post(format!("{api}/v1internal:loadCodeAssist"))
                    .header("authorization", &bearer)
                    .json(&meta),
                service,
            )
            .await?;
            let project = assist
                .get("cloudaicompanionProject")
                .and_then(|p| p.as_str().or_else(|| p.get("id")?.as_str()))
                .map(str::to_string)
                .ok_or("This Google account isn't set up for Gemini Code Assist yet. Sign in once with Gemini CLI, then try again.")?;
            let tier = assist
                .pointer("/paidTier/name")
                .or_else(|| assist.pointer("/currentTier/name"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let quota = get_json(
                client
                    .post(format!("{api}/v1internal:retrieveUserQuota"))
                    .header("authorization", &bearer)
                    .json(&json!({ "project": project })),
                service,
            )
            .await?;
            (tier, parse_gemini_quota(&quota))
        }
    };
    Ok(SubscriptionUsage {
        service,
        account: tokens.account.clone(),
        plan,
        windows,
    })
}

// ---------- keychain storage and access-token cache ----------

fn cache() -> &'static Mutex<HashMap<Service, Tokens>> {
    static CACHE: OnceLock<Mutex<HashMap<Service, Tokens>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

pub fn stored(service: Service) -> Option<Stored> {
    serde_json::from_str(&crate::paths::secret(&service.credential_id())?).ok()
}

fn save(service: Service, tokens: &Tokens) -> Result<()> {
    let refresh_token = tokens.refresh_token.clone().ok_or_else(|| {
        format!(
            "{} did not return a refresh token, so the sign-in can't be kept.",
            service.label()
        )
    })?;
    let prior = stored(service);
    let record = Stored {
        refresh_token,
        account: tokens
            .account
            .clone()
            .or_else(|| prior.as_ref()?.account.clone()),
        plan: tokens.plan.clone().or_else(|| prior.as_ref()?.plan.clone()),
        account_id: tokens
            .account_id
            .clone()
            .or_else(|| prior.as_ref()?.account_id.clone()),
    };
    let text = serde_json::to_string(&record).map_err(|e| e.to_string())?;
    crate::paths::credential(&service.credential_id())?
        .set_password(&text)
        .map_err(|e| format!("Could not save the sign-in to the OS keychain: {e}"))?;
    if let Ok(mut c) = cache().lock() {
        c.insert(service, tokens.clone());
    }
    Ok(())
}

pub fn sign_out(service: Service) -> Result<()> {
    if let Ok(mut c) = cache().lock() {
        c.remove(&service);
    }
    match crate::paths::credential(&service.credential_id())?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Run a full sign-in. `open` receives the URL to show in the browser.
pub async fn sign_in(
    service: Service,
    endpoints: Endpoints,
    open: impl FnOnce(&str),
) -> Result<Stored> {
    let pending = begin(service, endpoints).await?;
    open(&pending.url);
    let tokens = finish(pending, Duration::from_secs(300)).await?;
    save(service, &tokens)?;
    stored(service).ok_or_else(|| "The sign-in could not be read back from the keychain.".into())
}

async fn access(service: Service, endpoints: &Endpoints, force: bool) -> Result<Tokens> {
    if !force {
        if let Some(t) = cache().lock().ok().and_then(|c| c.get(&service).cloned()) {
            if t.expires_at.is_none_or(|e| e > now() + 60) {
                return Ok(t);
            }
        }
    }
    let s = stored(service).ok_or_else(|| format!("Not signed in to {}.", service.label()))?;
    let mut t = refresh(service, endpoints, &s.refresh_token).await?;
    t.account = t.account.or(s.account);
    t.plan = t.plan.or(s.plan);
    t.account_id = t.account_id.or(s.account_id);
    save(service, &t)?;
    Ok(t)
}

/// Usage for a signed-in service; refreshes the access token as needed.
pub async fn usage(service: Service, endpoints: &Endpoints) -> Result<SubscriptionUsage> {
    let t = access(service, endpoints, false).await?;
    match fetch_usage(service, endpoints, &t).await {
        Err(e) if e == UNAUTHORIZED => {
            let t = access(service, endpoints, true).await?;
            fetch_usage(service, endpoints, &t).await.map_err(|e| {
                if e == UNAUTHORIZED {
                    format!("{} rejected the sign-in. Sign in again.", service.label())
                } else {
                    e
                }
            })
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Gemini needs a configured client; every test uses the same values.
    fn gemini_env() {
        std::env::set_var(GEMINI_CLIENT_ID_ENV, "test-client.apps.example");
        std::env::set_var(GEMINI_CLIENT_SECRET_ENV, "test-client-secret");
    }

    fn jwt(claims: Value) -> String {
        format!(
            "e30.{}.sig",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    #[test]
    fn pkce_challenge_is_s256_of_verifier() {
        let p = pkce();
        assert!(p.verifier.len() >= 43 && p.verifier.len() <= 128);
        assert_eq!(
            p.challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(p.verifier.as_bytes()))
        );
        assert_ne!(pkce().verifier, p.verifier);
    }

    #[tokio::test]
    async fn authorize_urls_carry_pkce_state_and_service_params() {
        gemini_env();
        for service in Service::ALL {
            let mut e = Endpoints::official(service);
            e.port = None;
            let p = begin(service, e).await.unwrap();
            let url = url::Url::parse(&p.url).unwrap();
            let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(q["code_challenge_method"], "S256");
            assert_eq!(q["state"], p.state);
            assert_eq!(q["client_id"], oauth_client(service).unwrap().client_id);
            assert!(q["redirect_uri"].starts_with("http://localhost:"));
            match service {
                Service::Claude => assert_eq!(url.host_str(), Some("claude.ai")),
                Service::Chatgpt => assert_eq!(q["codex_cli_simplified_flow"], "true"),
                Service::Gemini => assert_eq!(q["access_type"], "offline"),
            }
        }
    }

    /// Simulates the browser: follows the authorize redirect to the loopback
    /// callback.
    async fn browser(url: &str, code: &str, state_override: Option<&str>) -> u16 {
        let u = url::Url::parse(url).unwrap();
        let q: HashMap<_, _> = u.query_pairs().into_owned().collect();
        let state = state_override.unwrap_or(&q["state"]);
        let cb = format!("{}?code={code}&state={state}", q["redirect_uri"]);
        reqwest::get(cb).await.unwrap().status().as_u16()
    }

    #[tokio::test]
    async fn chatgpt_sign_in_exchanges_code_and_reads_account() {
        let server = MockServer::start().await;
        let id = jwt(
            json!({"email":"me@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"plus","chatgpt_account_id":"acct-1"}}),
        );
        Mock::given(method("POST"))
            .and(path("/chatgpt/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code=the-code"))
            .and(body_string_contains("code_verifier="))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token":"at","refresh_token":"rt","id_token":id,"expires_in":3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        let p = begin(
            Service::Chatgpt,
            Endpoints::local(&server.uri(), Service::Chatgpt),
        )
        .await
        .unwrap();
        let url = p.url.clone();
        let done = tokio::spawn(finish(p, Duration::from_secs(10)));
        assert_eq!(browser(&url, "the-code", None).await, 200);
        let t = done.await.unwrap().unwrap();
        assert_eq!(t.access_token, "at");
        assert_eq!(t.refresh_token.as_deref(), Some("rt"));
        assert_eq!(t.account.as_deref(), Some("me@example.com"));
        assert_eq!(t.plan.as_deref(), Some("plus"));
        assert_eq!(t.account_id.as_deref(), Some("acct-1"));
        assert!(t.expires_at.unwrap() > now());
        assert!(
            !format!("{t:?}").contains("rt\""),
            "Debug must redact tokens"
        );
    }

    #[tokio::test]
    async fn claude_uses_json_token_request_with_state() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/claude/token"))
            .and(header("content-type", "application/json"))
            .and(body_string_contains("\"state\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token":"cat","refresh_token":"crt","expires_in":28800,
                "account":{"email_address":"c@example.com"}
            })))
            .mount(&server)
            .await;
        let p = begin(
            Service::Claude,
            Endpoints::local(&server.uri(), Service::Claude),
        )
        .await
        .unwrap();
        let url = p.url.clone();
        let done = tokio::spawn(finish(p, Duration::from_secs(10)));
        browser(&url, "c", None).await;
        let t = done.await.unwrap().unwrap();
        assert_eq!(t.account.as_deref(), Some("c@example.com"));
    }

    #[tokio::test]
    async fn gemini_sends_installed_app_secret() {
        gemini_env();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gemini/token"))
            .and(body_string_contains("client_secret=test-client-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token":"gat","refresh_token":"grt","expires_in":3599,
                "id_token": jwt(json!({"email":"g@example.com"}))
            })))
            .mount(&server)
            .await;
        let p = begin(
            Service::Gemini,
            Endpoints::local(&server.uri(), Service::Gemini),
        )
        .await
        .unwrap();
        let url = p.url.clone();
        let done = tokio::spawn(finish(p, Duration::from_secs(10)));
        browser(&url, "g", None).await;
        assert_eq!(
            done.await.unwrap().unwrap().account.as_deref(),
            Some("g@example.com")
        );
    }

    #[tokio::test]
    async fn state_mismatch_and_provider_errors_fail_closed() {
        let server = MockServer::start().await;
        let p = begin(
            Service::Claude,
            Endpoints::local(&server.uri(), Service::Claude),
        )
        .await
        .unwrap();
        let url = p.url.clone();
        let done = tokio::spawn(finish(p, Duration::from_secs(10)));
        assert_eq!(browser(&url, "x", Some("forged")).await, 400);
        assert!(done.await.unwrap().unwrap_err().contains("state mismatch"));

        let p = begin(
            Service::Claude,
            Endpoints::local(&server.uri(), Service::Claude),
        )
        .await
        .unwrap();
        let q: HashMap<_, _> = url::Url::parse(&p.url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        let cb = format!(
            "{}?error=access_denied&state={}",
            q["redirect_uri"], q["state"]
        );
        let done = tokio::spawn(finish(p, Duration::from_secs(10)));
        assert_eq!(reqwest::get(cb).await.unwrap().status().as_u16(), 400);
        assert!(done.await.unwrap().unwrap_err().contains("access_denied"));

        // A rejected code exchange surfaces the provider's reason.
        Mock::given(method("POST"))
            .and(path("/claude/token"))
            .respond_with(
                ResponseTemplate::new(400).set_body_json(
                    json!({"error":"invalid_grant","error_description":"Code expired"}),
                ),
            )
            .mount(&server)
            .await;
        let p = begin(
            Service::Claude,
            Endpoints::local(&server.uri(), Service::Claude),
        )
        .await
        .unwrap();
        let url = p.url.clone();
        let done = tokio::spawn(finish(p, Duration::from_secs(10)));
        browser(&url, "old", None).await;
        assert!(done.await.unwrap().unwrap_err().contains("Code expired"));
    }

    #[tokio::test]
    async fn sign_in_times_out_without_a_callback() {
        let p = begin(
            Service::Claude,
            Endpoints::local("http://127.0.0.1:9", Service::Claude),
        )
        .await
        .unwrap();
        let e = finish(p, Duration::from_millis(100)).await.unwrap_err();
        assert!(e.contains("timed out"));
    }

    #[tokio::test]
    async fn refresh_keeps_old_refresh_token_when_not_rotated() {
        gemini_env();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gemini/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"access_token":"new","expires_in":60})),
            )
            .mount(&server)
            .await;
        let t = refresh(
            Service::Gemini,
            &Endpoints::local(&server.uri(), Service::Gemini),
            "keep",
        )
        .await
        .unwrap();
        assert_eq!(t.access_token, "new");
        assert_eq!(t.refresh_token.as_deref(), Some("keep"));
    }

    fn tokens() -> Tokens {
        Tokens {
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            expires_at: None,
            account: Some("me@example.com".into()),
            plan: None,
            account_id: Some("acct-1".into()),
        }
    }

    #[tokio::test]
    async fn usage_for_each_service() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/claude/api/oauth/usage"))
            .and(header("authorization", "Bearer at"))
            .and(header("anthropic-beta", "oauth-2025-04-20"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "five_hour":{"utilization":42.0,"resets_at":"2026-10-04T15:00:00Z"},
                "seven_day":{"utilization":12.5,"resets_at":"2026-10-09T00:00:00Z"},
                "seven_day_opus":null
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/chatgpt/backend-api/wham/usage"))
            .and(header("chatgpt-account-id", "acct-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "plan_type":"pro",
                "rate_limit":{
                    "primary_window":{"used_percent":7,"limit_window_seconds":18000,"reset_at":1790000000},
                    "secondary_window":{"used_percent":31,"limit_window_seconds":604800,"reset_after_seconds":100}
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/gemini/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "cloudaicompanionProject":"proj-9","currentTier":{"name":"Gemini Code Assist for individuals"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/gemini/v1internal:retrieveUserQuota"))
            .and(body_string_contains("proj-9"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"buckets":[
                {"modelId":"gemini-2.5-pro","tokenType":"REQUESTS","remainingFraction":0.75,"resetTime":"2026-10-05T00:00:00Z"},
                {"modelId":"gemini-2.5-flash","tokenType":"REQUESTS","remainingFraction":1.0}
            ]})))
            .mount(&server)
            .await;

        let c = fetch_usage(
            Service::Claude,
            &Endpoints::local(&server.uri(), Service::Claude),
            &tokens(),
        )
        .await
        .unwrap();
        assert_eq!(c.windows.len(), 2);
        assert_eq!(c.windows[0].label, "5-hour session");
        assert_eq!(c.windows[0].used_percent, 42.0);
        assert_eq!(c.account.as_deref(), Some("me@example.com"));

        let o = fetch_usage(
            Service::Chatgpt,
            &Endpoints::local(&server.uri(), Service::Chatgpt),
            &tokens(),
        )
        .await
        .unwrap();
        assert_eq!(o.plan.as_deref(), Some("pro"));
        assert_eq!(o.windows[0].label, "5-hour window");
        assert_eq!(o.windows[0].resets_at_unix, Some(1_790_000_000));
        assert_eq!(o.windows[1].label, "Weekly window");
        assert_eq!(o.windows[1].used_percent, 31.0);

        let g = fetch_usage(
            Service::Gemini,
            &Endpoints::local(&server.uri(), Service::Gemini),
            &tokens(),
        )
        .await
        .unwrap();
        assert_eq!(
            g.plan.as_deref(),
            Some("Gemini Code Assist for individuals")
        );
        let pro = g
            .windows
            .iter()
            .find(|w| w.label == "gemini-2.5-pro")
            .unwrap();
        assert_eq!(pro.used_percent, 25.0);
    }

    #[tokio::test]
    async fn expired_access_is_reported_as_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/claude/api/oauth/usage"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let e = fetch_usage(
            Service::Claude,
            &Endpoints::local(&server.uri(), Service::Claude),
            &tokens(),
        )
        .await
        .unwrap_err();
        assert_eq!(e, UNAUTHORIZED);
    }

    #[test]
    fn parsers_ignore_malformed_data() {
        assert!(parse_claude_usage(&json!([1, 2])).is_empty());
        assert!(parse_claude_usage(&json!({"five_hour":{"utilization":"lots"}})).is_empty());
        assert_eq!(parse_chatgpt_usage(&json!({})), (None, vec![]));
        assert!(parse_gemini_quota(&json!({"buckets":[{"modelId":"x"}]})).is_empty());
        // Out-of-range values are clamped, not trusted.
        assert_eq!(
            parse_claude_usage(&json!({"five_hour":{"utilization":250.0}}))[0].used_percent,
            100.0
        );
    }
}
