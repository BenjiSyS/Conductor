//! Shared MCP configuration and per-provider adapters.

use std::collections::BTreeMap;
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
        match std::fs::read(path) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| ConfigError::Invalid(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(|e| ConfigError::Invalid(e.to_string()))?,
        )?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn upsert(&mut self, s: McpServer) -> Result<(), ConfigError> {
        validate_name(&s.name)?;
        if let Transport::Stdio { command, .. } = &s.transport {
            if command.trim().is_empty() {
                return Err(ConfigError::Invalid("command is empty".into()));
            }
        }
        if let Transport::Http { url, .. } = &s.transport {
            let local = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
            if !url.starts_with("https://") && !local {
                return Err(ConfigError::Invalid(
                    "remote MCP servers must use https".into(),
                ));
            }
        }
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
}
