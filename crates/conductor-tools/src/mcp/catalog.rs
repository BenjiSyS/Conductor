//! Known MCP servers for "Install this MCP" requests.
//!
//! Each entry pins its source (official org package or image) and a version
//! where the ecosystem supports pinning, lists runtime dependencies, and
//! declares the secrets it needs. Entries are data; the catalog can be
//! refreshed independently of app releases (signed, see
//! `conductor_security::integrity`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::config::{EnvValue, McpServer, Transport};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Canonical source for verification (repository / registry URL).
    pub source: String,
    pub command: String,
    pub args: Vec<String>,
    /// Placeholder args the user must fill, e.g. `{path}`.
    #[serde(default)]
    pub needs: Vec<String>,
    /// env var -> secret name
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

pub fn builtin() -> Vec<CatalogEntry> {
    let e = |id: &str,
             name: &str,
             desc: &str,
             source: &str,
             cmd: &str,
             args: &[&str],
             deps: &[&str],
             kw: &[&str]| CatalogEntry {
        id: id.into(),
        name: name.into(),
        description: desc.into(),
        source: source.into(),
        command: cmd.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        needs: args
            .iter()
            .filter(|a| a.starts_with('{'))
            .map(|a| a.trim_matches(|c| c == '{' || c == '}').to_string())
            .collect(),
        secrets: BTreeMap::new(),
        dependencies: deps.iter().map(|s| s.to_string()).collect(),
        keywords: kw.iter().map(|s| s.to_string()).collect(),
    };
    let mut github = e(
        "github",
        "GitHub",
        "Repositories, issues, pull requests and Actions via GitHub's official MCP server.",
        "https://github.com/github/github-mcp-server",
        "docker",
        &[
            "run",
            "-i",
            "--rm",
            "-e",
            "GITHUB_PERSONAL_ACCESS_TOKEN",
            "ghcr.io/github/github-mcp-server",
        ],
        &["docker"],
        &["github", "git", "issues", "pull request", "pr"],
    );
    github
        .secrets
        .insert("GITHUB_PERSONAL_ACCESS_TOKEN".into(), "github-token".into());
    vec![
        e(
            "filesystem",
            "Filesystem",
            "Read and write files in an allowed folder.",
            "https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem",
            "npx",
            &["-y", "@modelcontextprotocol/server-filesystem", "{path}"],
            &["node"],
            &["files", "filesystem", "folder"],
        ),
        e(
            "memory",
            "Memory",
            "Knowledge-graph memory store.",
            "https://github.com/modelcontextprotocol/servers/tree/main/src/memory",
            "npx",
            &["-y", "@modelcontextprotocol/server-memory"],
            &["node"],
            &["memory", "knowledge"],
        ),
        e(
            "everything",
            "Everything (test server)",
            "Reference server exercising every MCP feature; useful for testing.",
            "https://github.com/modelcontextprotocol/servers/tree/main/src/everything",
            "npx",
            &["-y", "@modelcontextprotocol/server-everything"],
            &["node"],
            &["test", "example", "everything"],
        ),
        e(
            "sequential-thinking",
            "Sequential Thinking",
            "Structured step-by-step reasoning tool.",
            "https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking",
            "npx",
            &["-y", "@modelcontextprotocol/server-sequential-thinking"],
            &["node"],
            &["thinking", "reasoning"],
        ),
        e(
            "playwright",
            "Playwright browser",
            "Browser automation via Microsoft's Playwright MCP.",
            "https://github.com/microsoft/playwright-mcp",
            "npx",
            &["-y", "@playwright/mcp"],
            &["node"],
            &["browser", "playwright", "web"],
        ),
        e(
            "blender",
            "Blender",
            "Control Blender (requires the Blender MCP add-on running inside Blender).",
            "https://github.com/ahujasid/blender-mcp",
            "uvx",
            &["blender-mcp"],
            &["uv", "blender"],
            &["blender", "3d", "modeling"],
        ),
        github,
    ]
}

/// Find catalog entries matching a free-text request such as
/// "set up Blender MCP".
pub fn search<'a>(entries: &'a [CatalogEntry], query: &str) -> Vec<&'a CatalogEntry> {
    let q = query.to_lowercase();
    let mut scored: Vec<(usize, &CatalogEntry)> = entries
        .iter()
        .filter_map(|e| {
            let mut s = 0;
            if q.contains(&e.id) || q.contains(&e.name.to_lowercase()) {
                s += 10;
            }
            s += e.keywords.iter().filter(|k| q.contains(k.as_str())).count() * 3;
            (s > 0).then_some((s, e))
        })
        .collect();
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));
    scored.into_iter().map(|(_, e)| e).collect()
}

/// Build a server config from a catalog entry, filling placeholders.
pub fn to_server(
    entry: &CatalogEntry,
    values: &BTreeMap<String, String>,
) -> Result<McpServer, String> {
    let mut args = Vec::new();
    for a in &entry.args {
        if a.starts_with('{') && a.ends_with('}') {
            let key = a.trim_matches(|c| c == '{' || c == '}');
            let v = values
                .get(key)
                .ok_or_else(|| format!("missing value for {key}"))?;
            args.push(v.clone());
        } else {
            args.push(a.clone());
        }
    }
    let env = entry
        .secrets
        .iter()
        .map(|(k, s)| (k.clone(), EnvValue::Secret { secret: s.clone() }))
        .collect();
    Ok(McpServer {
        name: entry.id.clone(),
        transport: Transport::Stdio {
            command: entry.command.clone(),
            args,
            env,
        },
        enabled: true,
        description: entry.description.clone(),
        source: format!("catalog:{} ({})", entry.id, entry.source),
        version: None,
        project: None,
        providers: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_and_build() {
        let c = builtin();
        assert_eq!(search(&c, "Set up Blender MCP")[0].id, "blender");
        assert_eq!(search(&c, "add this GitHub MCP")[0].id, "github");
        assert!(search(&c, "quantum toaster").is_empty());
        let fs = c.iter().find(|e| e.id == "filesystem").unwrap();
        assert_eq!(fs.needs, vec!["path"]);
        assert!(to_server(fs, &BTreeMap::new()).is_err());
        let mut v = BTreeMap::new();
        v.insert("path".into(), "/tmp/x".into());
        let s = to_server(fs, &v).unwrap();
        let Transport::Stdio { args, .. } = &s.transport else {
            panic!()
        };
        assert_eq!(args.last().unwrap(), "/tmp/x");
        let gh = to_server(
            c.iter().find(|e| e.id == "github").unwrap(),
            &BTreeMap::new(),
        )
        .unwrap();
        let Transport::Stdio { env, .. } = &gh.transport else {
            panic!()
        };
        assert!(matches!(
            env.get("GITHUB_PERSONAL_ACCESS_TOKEN"),
            Some(EnvValue::Secret { .. })
        ));
    }
}
