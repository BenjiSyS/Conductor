//! Environment Doctor: detect toolchains, versions and common problems.
//!
//! Probes run concurrently with short timeouts and never modify the system.
//! Install/fix actions are suggestions the permission layer may execute.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::exec::{self, ExecEnd, ExecRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    Ok,
    Missing,
    Broken,
    TooOld,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolReport {
    pub id: String,
    pub name: String,
    pub state: ToolState,
    pub version: Option<String>,
    pub path: Option<String>,
    pub detail: String,
    /// Suggested install command (argv) for this OS, if any.
    pub install: Option<Vec<String>>,
}

pub struct Probe {
    pub id: &'static str,
    pub name: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub min_version: Option<&'static str>,
    pub winget: Option<&'static str>,
    pub brew: Option<&'static str>,
    pub apt: Option<&'static str>,
}

const fn p(
    id: &'static str,
    name: &'static str,
    program: &'static str,
    args: &'static [&'static str],
    winget: Option<&'static str>,
    brew: Option<&'static str>,
    apt: Option<&'static str>,
) -> Probe {
    Probe {
        id,
        name,
        program,
        args,
        min_version: None,
        winget,
        brew,
        apt,
    }
}

pub const PROBES: &[Probe] = &[
    p(
        "git",
        "Git",
        "git",
        &["--version"],
        Some("Git.Git"),
        Some("git"),
        Some("git"),
    ),
    p(
        "rustc",
        "Rust",
        "rustc",
        &["--version"],
        Some("Rustlang.Rustup"),
        Some("rustup"),
        None,
    ),
    p(
        "cargo",
        "Cargo",
        "cargo",
        &["--version"],
        Some("Rustlang.Rustup"),
        Some("rustup"),
        None,
    ),
    p(
        "node",
        "Node.js",
        "node",
        &["--version"],
        Some("OpenJS.NodeJS.LTS"),
        Some("node"),
        Some("nodejs"),
    ),
    p("npm", "npm", "npm", &["--version"], None, None, Some("npm")),
    p(
        "pnpm",
        "pnpm",
        "pnpm",
        &["--version"],
        Some("pnpm.pnpm"),
        Some("pnpm"),
        None,
    ),
    p(
        "yarn",
        "Yarn",
        "yarn",
        &["--version"],
        None,
        Some("yarn"),
        None,
    ),
    p(
        "python",
        "Python",
        if cfg!(windows) { "python" } else { "python3" },
        &["--version"],
        Some("Python.Python.3.12"),
        Some("python"),
        Some("python3"),
    ),
    p(
        "uv",
        "uv",
        "uv",
        &["--version"],
        Some("astral-sh.uv"),
        Some("uv"),
        None,
    ),
    p(
        "java",
        "Java",
        "java",
        &["-version"],
        Some("EclipseAdoptium.Temurin.21.JDK"),
        Some("openjdk"),
        Some("default-jdk"),
    ),
    p(
        "gradle",
        "Gradle",
        "gradle",
        &["--version"],
        None,
        Some("gradle"),
        Some("gradle"),
    ),
    p(
        "mvn",
        "Maven",
        "mvn",
        &["--version"],
        None,
        Some("maven"),
        Some("maven"),
    ),
    p(
        "docker",
        "Docker",
        "docker",
        &["--version"],
        Some("Docker.DockerDesktop"),
        None,
        Some("docker.io"),
    ),
    p(
        "cmake",
        "CMake",
        "cmake",
        &["--version"],
        Some("Kitware.CMake"),
        Some("cmake"),
        Some("cmake"),
    ),
    p(
        "gh",
        "GitHub CLI",
        "gh",
        &["--version"],
        Some("GitHub.cli"),
        Some("gh"),
        Some("gh"),
    ),
    p(
        "go",
        "Go",
        "go",
        &["version"],
        Some("GoLang.Go"),
        Some("go"),
        Some("golang"),
    ),
    p(
        "dotnet",
        ".NET SDK",
        "dotnet",
        &["--version"],
        Some("Microsoft.DotNet.SDK.8"),
        None,
        None,
    ),
    p(
        "godot",
        "Godot",
        "godot",
        &["--version"],
        Some("GodotEngine.GodotEngine"),
        Some("godot"),
        None,
    ),
    p(
        "blender",
        "Blender",
        "blender",
        &["--version"],
        Some("BlenderFoundation.Blender"),
        Some("blender"),
        Some("blender"),
    ),
    p("wsl", "WSL", "wsl", &["--status"], None, None, None),
    p(
        "cloudflared",
        "cloudflared",
        "cloudflared",
        &["--version"],
        Some("Cloudflare.cloudflared"),
        Some("cloudflared"),
        None,
    ),
];

fn install_argv(probe: &Probe) -> Option<Vec<String>> {
    let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect());
    if cfg!(windows) {
        probe
            .winget
            .and_then(|id| v(&["winget", "install", "--id", id, "-e", "--source", "winget"]))
    } else if cfg!(target_os = "macos") {
        probe.brew.and_then(|f| v(&["brew", "install", f]))
    } else {
        probe
            .apt
            .and_then(|pkg| v(&["sudo", "apt-get", "install", "-y", pkg]))
    }
}

/// Extract the first version-looking token (e.g. "1.85.0").
pub fn parse_version(text: &str) -> Option<String> {
    for tok in text.split(|c: char| c.is_whitespace() || c == ',' || c == '"' || c == '(') {
        let t = tok.trim_start_matches(['v', 'V']);
        let mut parts = t.split('.');
        let first = parts.next().unwrap_or("");
        if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) && t.contains('.') {
            let clean: String = t
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            return Some(clean.trim_end_matches('.').to_string());
        }
    }
    None
}

pub fn version_at_least(have: &str, min: &str) -> bool {
    let nums = |s: &str| {
        s.split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let (a, b) = (nums(have), nums(min));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    true
}

pub async fn check(probe: &Probe) -> ToolReport {
    let mut r = ToolReport {
        id: probe.id.into(),
        name: probe.name.into(),
        state: ToolState::Missing,
        version: None,
        path: None,
        detail: String::new(),
        install: install_argv(probe),
    };
    let Some(path) = exec::resolve_program(probe.program) else {
        r.detail = format!("'{}' not found on PATH", probe.program);
        return r;
    };
    r.path = Some(path.to_string_lossy().to_string());
    // Windows app-execution aliases for python point at the Store stub.
    if cfg!(windows)
        && r.path
            .as_deref()
            .is_some_and(|p| p.to_lowercase().contains("windowsapps\\python"))
    {
        r.state = ToolState::Broken;
        r.detail = "only the Microsoft Store alias is installed".into();
        return r;
    }
    let req = ExecRequest::new(probe.program, probe.args, std::env::temp_dir()).timeout(10);
    let out = tokio::time::timeout(
        Duration::from_secs(15),
        exec::run(req, CancellationToken::new()),
    )
    .await;
    match out {
        Ok(res) if res.end == ExecEnd::Exited => {
            let text = res.combined();
            r.version = parse_version(&text);
            if res.code == Some(0) || r.version.is_some() {
                r.state = ToolState::Ok;
                r.install = None;
                if let (Some(min), Some(v)) = (probe.min_version, &r.version) {
                    if !version_at_least(v, min) {
                        r.state = ToolState::TooOld;
                        r.detail = format!("{v} is older than required {min}");
                    }
                }
            } else {
                r.state = ToolState::Broken;
                r.detail = text
                    .lines()
                    .next()
                    .unwrap_or("failed to run")
                    .chars()
                    .take(200)
                    .collect();
            }
        }
        Ok(res) => {
            r.state = ToolState::Broken;
            r.detail = format!("did not finish ({:?})", res.end);
        }
        Err(_) => {
            r.state = ToolState::Broken;
            r.detail = "timed out".into();
        }
    }
    r
}

/// Run all probes (or a subset by id) concurrently.
pub async fn check_all(only: Option<&[&str]>) -> Vec<ToolReport> {
    let probes: Vec<&Probe> = PROBES
        .iter()
        .filter(|p| only.is_none_or(|o| o.contains(&p.id)))
        .collect();
    let mut set = tokio::task::JoinSet::new();
    for (i, p) in probes.iter().enumerate() {
        let id = p.id;
        set.spawn(async move {
            let probe = PROBES.iter().find(|x| x.id == id).expect("probe exists");
            (i, check(probe).await)
        });
    }
    let mut out: Vec<(usize, ToolReport)> = Vec::new();
    while let Some(Ok(r)) = set.join_next().await {
        out.push(r);
    }
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing() {
        assert_eq!(
            parse_version("git version 2.45.1.windows.1").as_deref(),
            Some("2.45.1")
        );
        assert_eq!(parse_version("v20.11.0").as_deref(), Some("20.11.0"));
        assert_eq!(
            parse_version("rustc 1.85.0 (4d91de4e4 2025-02-17)").as_deref(),
            Some("1.85.0")
        );
        assert_eq!(
            parse_version("openjdk version \"21.0.2\" 2024-01-16").as_deref(),
            Some("21.0.2")
        );
        assert_eq!(parse_version("nothing here"), None);
        assert!(version_at_least("1.85.0", "1.80"));
        assert!(!version_at_least("1.79.9", "1.80"));
        assert!(version_at_least("2.0", "2.0.0"));
    }

    #[tokio::test]
    async fn detects_git_and_missing_tool() {
        let r = check_all(Some(&["git"])).await;
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].state, ToolState::Ok, "{:?}", r[0]);
        assert!(r[0].version.is_some());
        let fake = Probe {
            id: "x",
            name: "X",
            program: "no-such-tool-zzz",
            args: &[],
            min_version: None,
            winget: Some("X.X"),
            brew: Some("x"),
            apt: Some("x"),
        };
        let r = check(&fake).await;
        assert_eq!(r.state, ToolState::Missing);
        assert!(r.install.is_some());
    }
}
