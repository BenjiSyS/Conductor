//! One-click sign-in for remote (HTTP) MCP connectors, following the MCP
//! authorization spec: the server's 401 points at its protected-resource
//! metadata, which names an OAuth authorization server; Conductor registers
//! itself there (dynamic client registration, no developer setup), then runs
//! a PKCE authorization-code flow in the user's browser with a loopback
//! redirect. Tokens are kept in the OS keychain; the access token is used as
//! the connector's `Authorization` header and refreshed before it expires.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;

pub type Result<T> = std::result::Result<T, String>;

/// Secret names (integration namespace) for a connector.
pub fn access_secret(server: &str) -> String {
    format!("mcp-oauth-{server}")
}
pub fn refresh_secret(server: &str) -> String {
    format!("mcp-oauth-{server}-refresh")
}

/// What is needed to refresh the access token later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RefreshState {
    pub token_endpoint: String,
    pub client_id: String,
    pub refresh_token: Option<String>,
    pub resource: String,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Signed {
    pub access_token: String,
    pub refresh: RefreshState,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(concat!("Conductor/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

fn https_or_loopback(url: &str) -> Result<url::Url> {
    let u = url::Url::parse(url).map_err(|_| format!("Invalid URL: {url}"))?;
    let loopback = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if u.scheme() == "https" || (u.scheme() == "http" && loopback) {
        Ok(u)
    } else {
        Err("Sign-in endpoints must use HTTPS.".into())
    }
}

/// `resource_metadata="…"` from a `WWW-Authenticate: Bearer …` header.
pub fn resource_metadata_url(www_authenticate: &str) -> Option<String> {
    let i = www_authenticate.find("resource_metadata=")?;
    let rest = &www_authenticate[i + "resource_metadata=".len()..];
    let rest = rest.trim_start_matches('"');
    let end = rest.find(['"', ',']).unwrap_or(rest.len());
    Some(rest[..end].trim().to_string()).filter(|s| !s.is_empty())
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value> {
    https_or_loopback(url)?;
    let r = client
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("{url} returned {}", r.status()));
    }
    r.json()
        .await
        .map_err(|_| format!("{url} did not return JSON"))
}

/// Discovered OAuth endpoints for an MCP server.
#[derive(Debug, Clone, PartialEq)]
pub struct Discovery {
    pub resource: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub scopes: Vec<String>,
}

pub async fn discover(server_url: &str) -> Result<Discovery> {
    let client = http()?;
    let base = https_or_loopback(server_url)?;
    // 1. Ask the server; a protected server answers 401 with a pointer.
    let probe = client
        .post(server_url)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"Conductor","version":env!("CARGO_PKG_VERSION")}}}))
        .send()
        .await
        .map_err(|e| format!("Could not reach the connector: {e}"))?;
    let header = probe
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if probe.status().is_success() {
        return Err("This connector doesn't need a sign-in.".into());
    }
    let origin = format!(
        "{}://{}",
        base.scheme(),
        base.host_str().unwrap_or_default()
    ) + &base.port().map(|p| format!(":{p}")).unwrap_or_default();
    let prm_url = header
        .as_deref()
        .and_then(resource_metadata_url)
        .unwrap_or_else(|| format!("{origin}/.well-known/oauth-protected-resource"));
    // 2. Protected resource metadata -> authorization server.
    let (resource, issuer, scopes) = match get_json(&client, &prm_url).await {
        Ok(prm) => (
            prm["resource"].as_str().unwrap_or(server_url).to_string(),
            prm["authorization_servers"][0]
                .as_str()
                .unwrap_or(&origin)
                .to_string(),
            prm["scopes_supported"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        ),
        // Older servers: the server itself is the authorization server.
        Err(_) => (server_url.to_string(), origin.clone(), vec![]),
    };
    // 3. Authorization server metadata.
    let issuer = issuer.trim_end_matches('/');
    let iss = url::Url::parse(issuer).map_err(|_| "Invalid authorization server")?;
    let path = iss.path().trim_end_matches('/');
    let host = format!("{}://{}", iss.scheme(), iss.host_str().unwrap_or_default())
        + &iss.port().map(|p| format!(":{p}")).unwrap_or_default();
    let candidates = [
        format!("{host}/.well-known/oauth-authorization-server{path}"),
        format!("{host}/.well-known/openid-configuration{path}"),
        format!("{issuer}/.well-known/openid-configuration"),
    ];
    let mut meta = None;
    for c in &candidates {
        if let Ok(v) = get_json(&client, c).await {
            meta = Some(v);
            break;
        }
    }
    let meta = meta.ok_or("The connector's sign-in server could not be found.")?;
    let s = |k: &str| meta[k].as_str().map(str::to_string);
    Ok(Discovery {
        resource,
        authorization_endpoint: s("authorization_endpoint").ok_or("No authorization endpoint")?,
        token_endpoint: s("token_endpoint").ok_or("No token endpoint")?,
        registration_endpoint: s("registration_endpoint"),
        scopes,
    })
}

async fn register(d: &Discovery, redirect_uri: &str) -> Result<String> {
    let endpoint = d
        .registration_endpoint
        .as_deref()
        .ok_or("This connector requires a pre-registered app, so one-click sign-in isn't available. Add a token in its settings instead.")?;
    https_or_loopback(endpoint)?;
    let r = http()?
        .post(endpoint)
        .json(&json!({
            "client_name": "Conductor",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    v["client_id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("Registering with the connector failed ({status})."))
}

fn token_result(v: &Value, previous: &RefreshState) -> Result<Signed> {
    let access = v["access_token"]
        .as_str()
        .ok_or("Sign-in response had no access token")?;
    Ok(Signed {
        access_token: access.to_string(),
        refresh: RefreshState {
            refresh_token: v["refresh_token"]
                .as_str()
                .map(str::to_string)
                .or(previous.refresh_token.clone()),
            expires_at: v["expires_in"].as_u64().map(|n| now() + n),
            ..previous.clone()
        },
    })
}

async fn post_token(endpoint: &str, fields: &[(&str, &str)]) -> Result<Value> {
    https_or_loopback(endpoint)?;
    let r = http()?
        .post(endpoint)
        .form(fields)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let why = v["error_description"]
            .as_str()
            .or(v["error"].as_str())
            .unwrap_or("rejected");
        return Err(format!("Connector sign-in failed ({status}): {why}"));
    }
    Ok(v)
}

/// Full sign-in. `open` receives the URL to show in the browser.
pub async fn sign_in(server_url: &str, open: impl FnOnce(&str)) -> Result<Signed> {
    let d = discover(server_url).await?;
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let client_id = register(&d, &redirect_uri).await?;
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = uuid::Uuid::new_v4().simple().to_string();
    let mut url =
        url::Url::parse(&d.authorization_endpoint).map_err(|_| "Invalid authorization endpoint")?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("resource", &d.resource);
        if !d.scopes.is_empty() {
            q.append_pair("scope", &d.scopes.join(" "));
        }
    }
    open(url.as_str());
    let code = crate::subscriptions::wait_for_code(
        &listener,
        "/callback",
        &state,
        Duration::from_secs(300),
    )
    .await?;
    let v = post_token(
        &d.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect_uri),
            ("client_id", &client_id),
            ("code_verifier", &verifier),
            ("resource", &d.resource),
        ],
    )
    .await?;
    token_result(
        &v,
        &RefreshState {
            token_endpoint: d.token_endpoint,
            client_id,
            refresh_token: None,
            resource: d.resource,
            expires_at: None,
        },
    )
}

pub async fn refresh(state: &RefreshState) -> Result<Signed> {
    let rt = state
        .refresh_token
        .as_deref()
        .ok_or("No refresh token; sign in again.")?;
    let v = post_token(
        &state.token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", rt),
            ("client_id", &state.client_id),
            ("resource", &state.resource),
        ],
    )
    .await?;
    token_result(&v, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parses_resource_metadata_pointer() {
        assert_eq!(
            resource_metadata_url(r#"Bearer realm="mcp", resource_metadata="https://x.dev/.well-known/oauth-protected-resource""#).as_deref(),
            Some("https://x.dev/.well-known/oauth-protected-resource")
        );
        assert_eq!(resource_metadata_url("Bearer"), None);
    }

    /// The whole flow against a stand-in connector + authorization server,
    /// with a test "browser" that approves and follows the redirect.
    #[tokio::test]
    async fn discovers_registers_signs_in_and_refreshes() {
        let s = MockServer::start().await;
        let base = s.uri();
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(
                ResponseTemplate::new(401).insert_header(
                    "www-authenticate",
                    format!(
                        r#"Bearer resource_metadata="{base}/.well-known/oauth-protected-resource""#
                    )
                    .as_str(),
                ),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/.well-known/oauth-protected-resource"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "resource": format!("{base}/mcp"), "authorization_servers": [format!("{base}/auth")], "scopes_supported": ["read"]
            })))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/.well-known/oauth-authorization-server/auth"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "authorization_endpoint": format!("{base}/auth/authorize"),
                "token_endpoint": format!("{base}/auth/token"),
                "registration_endpoint": format!("{base}/auth/register"),
            })))
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .and(body_string_contains(
                "\"token_endpoint_auth_method\":\"none\"",
            ))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({"client_id": "dyn-client"})),
            )
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code_verifier="))
            .and(body_string_contains("resource="))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"access_token":"at1","refresh_token":"rt1","expires_in":3600}),
            ))
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"access_token":"at2","expires_in":3600})),
            )
            .mount(&s)
            .await;

        let signed = sign_in(&format!("{base}/mcp"), |url| {
            let u = url::Url::parse(url).unwrap();
            let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
            assert_eq!(q["client_id"], "dyn-client");
            assert_eq!(q["code_challenge_method"], "S256");
            assert_eq!(q["scope"], "read");
            let back = format!("{}?code=c1&state={}", q["redirect_uri"], q["state"]);
            tokio::spawn(async move {
                reqwest::get(back).await.unwrap();
            });
        })
        .await
        .unwrap();
        assert_eq!(signed.access_token, "at1");
        assert_eq!(signed.refresh.client_id, "dyn-client");
        assert_eq!(signed.refresh.refresh_token.as_deref(), Some("rt1"));
        let again = refresh(&signed.refresh).await.unwrap();
        assert_eq!(again.access_token, "at2");
        assert_eq!(
            again.refresh.refresh_token.as_deref(),
            Some("rt1"),
            "kept when not rotated"
        );
    }

    #[tokio::test]
    async fn servers_without_registration_get_a_clear_message() {
        let s = MockServer::start().await;
        let base = s.uri();
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/.well-known/oauth-authorization-server"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "authorization_endpoint": format!("{base}/a"), "token_endpoint": format!("{base}/t")
            })))
            .mount(&s)
            .await;
        let e = sign_in(&format!("{base}/mcp"), |_| {}).await.unwrap_err();
        assert!(e.contains("pre-registered"), "{e}");
        assert!(https_or_loopback("http://evil.example/token").is_err());
    }
}
