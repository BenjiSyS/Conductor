//! MCP Doctor: diagnose each configured server and suggest one clear fix.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::client::McpError;
use super::config::{EnvValue, McpServer, Transport};
use super::session::McpSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpStatus {
    Connected,
    Disabled,
    MissingDependency,
    WrongConfiguration,
    AuthenticationRequired,
    PermissionIssue,
    Broken,
    Outdated,
    NotChecked,
}

impl McpStatus {
    pub fn label(self) -> &'static str {
        match self {
            McpStatus::Connected => "Connected",
            McpStatus::Disabled => "Disabled",
            McpStatus::MissingDependency => "Missing dependency",
            McpStatus::WrongConfiguration => "Wrong configuration",
            McpStatus::AuthenticationRequired => "Needs sign-in",
            McpStatus::PermissionIssue => "Permission issue",
            McpStatus::Broken => "Broken",
            McpStatus::Outdated => "Update available",
            McpStatus::NotChecked => "Not checked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Fix {
    InstallDependency { tool: String, hint: String },
    SetSecret { name: String },
    EditConfig { hint: String },
    Reconnect,
    Update,
    Enable,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnosis {
    pub server: String,
    pub status: McpStatus,
    /// Plain-language summary.
    pub summary: String,
    /// Technical detail, shown only when expanded.
    pub detail: String,
    pub tools: Vec<String>,
    pub fix: Fix,
}

/// Resolve secret env values via the provided lookup (OS keychain in the app).
pub fn resolve_env(
    env: &BTreeMap<String, EnvValue>,
    lookup: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (k, v) in env {
        match v {
            EnvValue::Literal(s) => {
                out.insert(k.clone(), s.clone());
            }
            EnvValue::Secret { secret } => match lookup(secret) {
                Some(val) => {
                    out.insert(k.clone(), val);
                }
                None => return Err(secret.clone()),
            },
        }
    }
    Ok(out)
}

fn dependency_hint(cmd: &str) -> String {
    let base = Path::new(cmd)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match base.as_str() {
        "npx" | "node" | "npm" => "Install Node.js LTS from nodejs.org (includes npx).".into(),
        "uvx" | "uv" => "Install uv from astral.sh (provides uvx).".into(),
        "python" | "python3" => "Install Python 3 from python.org.".into(),
        "docker" => "Install Docker Desktop and make sure it is running.".into(),
        other => format!("Install '{other}' and make sure it is on PATH."),
    }
}

pub async fn diagnose(
    server: &McpServer,
    secrets: &(dyn Fn(&str) -> Option<String> + Sync),
    timeout_secs: u64,
) -> Diagnosis {
    let mk = |status, summary: String, detail: String, tools: Vec<String>, fix| Diagnosis {
        server: server.name.clone(),
        status,
        summary,
        detail,
        tools,
        fix,
    };
    if !server.enabled {
        return mk(
            McpStatus::Disabled,
            format!("{} is disabled", server.name),
            String::new(),
            vec![],
            Fix::Enable,
        );
    }
    if let Transport::Stdio { command, .. } = &server.transport {
        if crate::exec::resolve_program(command).is_none() {
            return mk(
                McpStatus::MissingDependency,
                format!("{} needs '{}', which isn't installed", server.name, command),
                format!("'{command}' not found on PATH"),
                vec![],
                Fix::InstallDependency {
                    tool: command.clone(),
                    hint: dependency_hint(command),
                },
            );
        }
    }
    let values = match &server.transport {
        Transport::Stdio { env, .. } => env,
        Transport::Http { headers, .. } => headers,
    };
    for value in values.values() {
        if let EnvValue::Secret { secret } = value {
            if secrets(secret).is_none_or(|s| s.is_empty()) {
                return mk(
                    McpStatus::AuthenticationRequired,
                    format!("{} needs a credential: {secret}", server.name),
                    format!("secret '{secret}' is not set"),
                    vec![],
                    Fix::SetSecret {
                        name: secret.clone(),
                    },
                );
            }
        }
    }
    let cancel = tokio_util::sync::CancellationToken::new();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let operation = async {
        let mut client = McpSession::connect(server, secrets, &cwd, &cancel).await?;
        let result = client.list_tools(&cancel).await.map(|tools| {
            let detail = client
                .server_info()
                .map(|s| format!("{} {} (protocol {})", s.name, s.version, s.protocol_version))
                .unwrap_or_default();
            mk(
                McpStatus::Connected,
                format!("{} connected · {} tool(s)", server.name, tools.len()),
                detail,
                tools.into_iter().map(|t| t.name).collect(),
                Fix::None,
            )
        });
        client.shutdown().await;
        result
    };
    match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs.max(1)),
        operation,
    )
    .await
    {
        Ok(Ok(diagnosis)) => diagnosis,
        Ok(Err(error)) => classify_failure(server, error, String::new()),
        Err(_) => classify_failure(
            server,
            McpError::Timeout(timeout_secs.max(1)),
            String::new(),
        ),
    }
}

fn classify_failure(server: &McpServer, e: McpError, stderr: String) -> Diagnosis {
    let lower = format!("{e} {stderr}").to_lowercase();
    let (status, summary, fix) = if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("token")
            && (lower.contains("missing")
                || lower.contains("invalid")
                || lower.contains("required"))
        || lower.contains("api key")
    {
        (
            McpStatus::AuthenticationRequired,
            format!("{} needs valid credentials", server.name),
            Fix::SetSecret {
                name: format!("{}-token", server.name),
            },
        )
    } else if lower.contains("eacces")
        || lower.contains("permission denied")
        || lower.contains("access is denied")
    {
        (
            McpStatus::PermissionIssue,
            format!("{} was blocked by a permission error", server.name),
            Fix::EditConfig {
                hint: "Check folder permissions in the server arguments.".into(),
            },
        )
    } else if lower.contains("404") && lower.contains("npm")
        || lower.contains("e404")
        || lower.contains("not found") && lower.contains("package")
    {
        (
            McpStatus::WrongConfiguration,
            format!("{} package could not be found", server.name),
            Fix::EditConfig {
                hint: "The package name or version is wrong.".into(),
            },
        )
    } else if lower.contains("cannot find module")
        || lower.contains("modulenotfounderror")
        || lower.contains("no module named")
    {
        (
            McpStatus::MissingDependency,
            format!("{} is missing a dependency", server.name),
            Fix::Reconnect,
        )
    } else if matches!(e, McpError::Timeout(_)) {
        (
            McpStatus::Broken,
            format!("{} did not respond", server.name),
            Fix::Reconnect,
        )
    } else {
        (
            McpStatus::Broken,
            format!("{} stopped unexpectedly", server.name),
            Fix::Reconnect,
        )
    };
    let detail = conductor_security::secrets::redact(&format!("{e}\n{}", stderr.trim())).text;
    Diagnosis {
        server: server.name.clone(),
        status,
        summary,
        detail,
        tools: vec![],
        fix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::client::tests::write_fake;

    fn server(command: &str, args: Vec<String>, env: BTreeMap<String, EnvValue>) -> McpServer {
        McpServer {
            name: "test".into(),
            transport: Transport::Stdio {
                command: command.into(),
                args,
                env,
            },
            enabled: true,
            description: String::new(),
            source: "manual".into(),
            version: None,
            project: None,
            providers: vec![],
        }
    }

    #[tokio::test]
    async fn healthy_server_connected() {
        let d = tempfile::tempdir().unwrap();
        let Some(script) = write_fake(d.path()) else {
            return;
        };
        let dg = diagnose(
            &server("node", vec![script], BTreeMap::new()),
            &|_| None,
            10,
        )
        .await;
        assert_eq!(dg.status, McpStatus::Connected, "{dg:?}");
        assert_eq!(dg.tools, vec!["echo"]);
    }

    #[tokio::test]
    async fn missing_dependency_and_disabled() {
        let dg = diagnose(
            &server("uvx-not-installed-xyz", vec![], BTreeMap::new()),
            &|_| None,
            5,
        )
        .await;
        assert_eq!(dg.status, McpStatus::MissingDependency);
        assert!(matches!(dg.fix, Fix::InstallDependency { .. }));
        let mut s = server("node", vec![], BTreeMap::new());
        s.enabled = false;
        assert_eq!(diagnose(&s, &|_| None, 5).await.status, McpStatus::Disabled);
    }

    #[tokio::test]
    async fn missing_secret_needs_auth() {
        let mut env = BTreeMap::new();
        env.insert(
            "TOKEN".into(),
            EnvValue::Secret {
                secret: "gh-token".into(),
            },
        );
        let dg = diagnose(&server("node", vec![], env.clone()), &|_| None, 5).await;
        assert_eq!(dg.status, McpStatus::AuthenticationRequired);
        assert_eq!(
            dg.fix,
            Fix::SetSecret {
                name: "gh-token".into()
            }
        );
        let resolved = resolve_env(&env, &|k| (k == "gh-token").then(|| "v".to_string())).unwrap();
        assert_eq!(resolved["TOKEN"], "v");
    }

    #[tokio::test]
    async fn crashing_server_with_auth_error_classified() {
        if crate::exec::resolve_program("node").is_none() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.js");
        std::fs::write(&p, "console.error('Error: 401 Unauthorized - token invalid ghp_abcdefghijklmnopqrstuvwxyz0123456789'); setTimeout(()=>process.exit(1), 200);").unwrap();
        let dg = diagnose(
            &server(
                "node",
                vec![p.to_string_lossy().to_string()],
                BTreeMap::new(),
            ),
            &|_| None,
            5,
        )
        .await;
        assert!(
            matches!(
                dg.status,
                McpStatus::AuthenticationRequired | McpStatus::Broken
            ),
            "{dg:?}"
        );
        assert!(
            !dg.detail.contains("ghp_abc"),
            "secrets redacted from diagnostics"
        );
    }
}
