//! Shared MCP configuration and per-provider adapters.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Environment values may reference secrets by name; the integrator resolves
/// them from the OS credential store at launch time so raw values never live
/// in config files or reach a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EnvValue {
    Secret { secret: String },
    Literal(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Transport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, EnvValue>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, EnvValue>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub transport: Transport,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    /// Where it came from (catalog id, URL, "manual").
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub version: Option<String>,
    /// Project id when project-scoped.
    #[serde(default)]
    pub project: Option<String>,
    /// Providers this server should be exposed to (empty = all compatible).
    #[serde(default)]
    pub providers: Vec<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServer>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid MCP config: {0}")]
    Invalid(String),
}

impl McpConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::File::open(path) {
            Ok(file) => {
                let mut b = Vec::new();
                file.take((super::client::MAX_FRAME + 1) as u64)
                    .read_to_end(&mut b)?;
                if b.len() > super::client::MAX_FRAME {
                    return Err(ConfigError::Invalid("config exceeds 2 MiB".into()));
                }
                let config: Self =
                    serde_json::from_slice(&b).map_err(|e| ConfigError::Invalid(e.to_string()))?;
                let mut names = std::collections::BTreeSet::new();
                for server in &config.servers {
                    validate_server(server)?;
                    if !names.insert(&server.name) {
                        return Err(ConfigError::Invalid("duplicate server name".into()));
                    }
                }
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let bytes =
            serde_json::to_vec_pretty(self).map_err(|e| ConfigError::Invalid(e.to_string()))?;
        if bytes.len() > super::client::MAX_FRAME {
            return Err(ConfigError::Invalid("config exceeds 2 MiB".into()));
        }
        for server in &self.servers {
            validate_server(server)?;
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result?;
        Ok(())
    }

    pub fn upsert(&mut self, s: McpServer) -> Result<(), ConfigError> {
        validate_server(&s)?;
        self.servers.retain(|x| x.name != s.name);
        self.servers.push(s);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.servers.len();
        self.servers.retain(|s| s.name != name);
        before != self.servers.len()
    }

    pub fn get(&self, name: &str) -> Option<&McpServer> {
        self.servers.iter().find(|s| s.name == name)
    }

    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        match self.servers.iter_mut().find(|s| s.name == name) {
            Some(s) => {
                s.enabled = enabled;
                true
            }
            None => false,
        }
    }

    fn exposed<'a>(&'a self, provider: &'a str) -> impl Iterator<Item = &'a McpServer> + 'a {
        self.servers.iter().filter(move |s| {
            s.enabled && (s.providers.is_empty() || s.providers.iter().any(|p| p == provider))
        })
    }

    /// Claude Code / Claude Desktop `mcpServers` JSON. Secrets are emitted as
    /// `${NAME}` placeholders for the launcher to fill from the keychain.
    pub fn to_claude_json(&self) -> serde_json::Value {
        let mut servers = serde_json::Map::new();
        for s in self.exposed("anthropic") {
            servers.insert(s.name.clone(), server_json(s));
        }
        serde_json::json!({ "mcpServers": servers })
    }

    /// Gemini CLI `settings.json` fragment (same shape as Claude's).
    pub fn to_gemini_json(&self) -> serde_json::Value {
        let mut servers = serde_json::Map::new();
        for s in self.exposed("google") {
            servers.insert(s.name.clone(), server_json(s));
        }
        serde_json::json!({ "mcpServers": servers })
    }

    /// Codex CLI `config.toml` `[mcp_servers.<name>]` tables.
    pub fn to_codex_toml(&self) -> String {
        let mut root = toml::map::Map::new();
        let mut servers = toml::map::Map::new();
        for s in self.exposed("openai") {
            let mut t = toml::map::Map::new();
            match &s.transport {
                Transport::Stdio { command, args, env } => {
                    t.insert("command".into(), toml::Value::String(command.clone()));
                    t.insert(
                        "args".into(),
                        toml::Value::Array(args.iter().cloned().map(toml::Value::String).collect()),
                    );
                    if !env.is_empty() {
                        let mut e = toml::map::Map::new();
                        for (k, v) in env {
                            e.insert(k.clone(), toml::Value::String(env_str(v)));
                        }
                        t.insert("env".into(), toml::Value::Table(e));
                    }
                }
                Transport::Http { url, .. } => {
                    t.insert("url".into(), toml::Value::String(url.clone()));
                }
            }
            servers.insert(s.name.clone(), toml::Value::Table(t));
        }
        root.insert("mcp_servers".into(), toml::Value::Table(servers));
        toml::to_string_pretty(&toml::Value::Table(root)).unwrap_or_default()
    }
}

fn env_str(v: &EnvValue) -> String {
    match v {
        EnvValue::Literal(s) => s.clone(),
        EnvValue::Secret { secret } => format!("${{{secret}}}"),
    }
}

fn server_json(s: &McpServer) -> serde_json::Value {
    match &s.transport {
        Transport::Stdio { command, args, env } => {
            let env: serde_json::Map<String, serde_json::Value> = env
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(env_str(v))))
                .collect();
            serde_json::json!({ "command": command, "args": args, "env": env })
        }
        Transport::Http { url, headers } => {
            let h: serde_json::Map<String, serde_json::Value> = headers
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(env_str(v))))
                .collect();
            serde_json::json!({ "type": "http", "url": url, "headers": h })
        }
    }
}

fn validate_server(server: &McpServer) -> Result<(), ConfigError> {
    validate_name(&server.name)?;
    let values = match &server.transport {
        Transport::Stdio { env, .. } => env,
        Transport::Http { headers, .. } => headers,
    };
    for (key, value) in values {
        match value {
            EnvValue::Literal(text) => {
                let key = key.to_ascii_uppercase().replace('-', "_");
                let sensitive = matches!(
                    key.as_str(),
                    "AUTHORIZATION"
                        | "PROXY_AUTHORIZATION"
                        | "API_KEY"
                        | "TOKEN"
                        | "SECRET"
                        | "PASSWORD"
                        | "PRIVATE_KEY"
                ) || ["_API_KEY", "_TOKEN", "_SECRET", "_PASSWORD", "_PRIVATE_KEY"]
                    .iter()
                    .any(|suffix| key.ends_with(suffix));
                if sensitive || !conductor_security::secrets::scan(text).is_empty() {
                    return Err(ConfigError::Invalid(
                        "credential values must use an OS-vault secret reference".into(),
                    ));
                }
            }
            EnvValue::Secret { secret } => {
                if secret.is_empty()
                    || secret.len() > 128
                    || !secret
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(ConfigError::Invalid(
                        "invalid credential reference name".into(),
                    ));
                }
            }
        }
    }
    match &server.transport {
        Transport::Stdio { command, .. } if command.trim().is_empty() => {
            Err(ConfigError::Invalid("command is empty".into()))
        }
        Transport::Http { url, .. } => super::session::validate_http_url(url)
            .map(|_| ())
            .map_err(|e| ConfigError::Invalid(e.to_string())),
        _ => Ok(()),
    }
}

fn validate_name(n: &str) -> Result<(), ConfigError> {
    if n.is_empty()
        || n.len() > 64
        || !n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ConfigError::Invalid(format!(
            "server name '{n}' must be 1-64 chars of [A-Za-z0-9_-]"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gh() -> McpServer {
        let mut env = BTreeMap::new();
        env.insert(
            "GITHUB_TOKEN".into(),
            EnvValue::Secret {
                secret: "github-token".into(),
            },
        );
        McpServer {
            name: "github".into(),
            transport: Transport::Stdio {
                command: "docker".into(),
                args: vec![
                    "run".into(),
                    "-i".into(),
                    "ghcr.io/github/github-mcp-server".into(),
                ],
                env,
            },
            enabled: true,
            description: String::new(),
            source: "catalog:github".into(),
            version: None,
            project: None,
            providers: vec![],
        }
    }

    #[test]
    fn configure_once_export_everywhere_without_secrets() {
        let mut c = McpConfig::default();
        c.upsert(gh()).unwrap();
        let claude = c.to_claude_json().to_string();
        let gemini = c.to_gemini_json().to_string();
        let codex = c.to_codex_toml();
        for out in [&claude, &gemini, &codex] {
            assert!(out.contains("github"));
            assert!(
                out.contains("${github-token}"),
                "secret must be a placeholder: {out}"
            );
        }
        assert!(codex.contains("[mcp_servers.github]"));
    }

    #[test]
    fn validation_and_provider_scoping() {
        let mut c = McpConfig::default();
        let mut bad = gh();
        bad.name = "bad name!".into();
        assert!(c.upsert(bad).is_err());
        let mut http = gh();
        http.name = "remote".into();
        http.transport = Transport::Http {
            url: "http://evil.example/mcp".into(),
            headers: BTreeMap::new(),
        };
        assert!(c.upsert(http).is_err(), "plain http to remote host refused");
        let mut only_claude = gh();
        only_claude.providers = vec!["anthropic".into()];
        c.upsert(only_claude).unwrap();
        assert!(c.to_claude_json().to_string().contains("github"));
        assert!(!c.to_codex_toml().contains("github"));
        c.set_enabled("github", false);
        assert!(!c.to_claude_json().to_string().contains("github"));
    }

    #[test]
    fn save_load_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("mcp.json");
        let mut c = McpConfig::default();
        c.upsert(gh()).unwrap();
        c.save(&p).unwrap();
        assert_eq!(McpConfig::load(&p).unwrap(), c);
        assert!(McpConfig::load(&d.path().join("none.json"))
            .unwrap()
            .servers
            .is_empty());
    }

    #[test]
    fn plaintext_credentials_are_refused_without_echoing_values() {
        let mut config = McpConfig::default();
        let mut server = gh();
        server.transport = Transport::Http {
            url: "https://example.com/mcp".into(),
            headers: BTreeMap::from([(
                "Authorization".into(),
                EnvValue::Literal("opaque-password-value".into()),
            )]),
        };
        let error = config.upsert(server).unwrap_err().to_string();
        assert!(!error.contains("opaque-password-value"));
        let mut server = gh();
        server.transport = Transport::Stdio {
            command: "node".into(),
            args: vec![],
            env: BTreeMap::from([("NODE_ENV".into(), EnvValue::Literal("development".into()))]),
        };
        config.upsert(server).unwrap();
        let mut server = gh();
        if let Transport::Stdio { env, .. } = &mut server.transport {
            env.insert(
                "GITHUB_TOKEN".into(),
                EnvValue::Literal("opaque-password-value".into()),
            );
        }
        assert!(config.upsert(server).is_err());
    }
}
