//! Zero-setup bridge to AI command-line apps the user already has signed in:
//! Antigravity CLI (`agy`, Gemini), Codex CLI (`codex`, ChatGPT) and Claude
//! Code (`claude`).
//!
//! Conductor never sees or stores those apps' credentials. It runs the app
//! the same way a user would in a terminal (non-interactive print mode, an
//! empty scratch folder, read-only / plan mode, with instructions to answer
//! directly), streams its JSON output, and serves it on a loopback
//! OpenAI-compatible endpoint so every chat, Agent and Goal path works
//! unchanged. Requests to that endpoint need a random per-provider key kept
//! in the OS keychain, so other local programs can't spend the user's plan.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cli {
    Agy,
    Codex,
    Claude,
    /// xAI's Grok Build CLI (`grok`), signed in with a SuperGrok / X Premium+ account.
    Grok,
}

impl Cli {
    pub const ALL: [Cli; 4] = [Cli::Agy, Cli::Codex, Cli::Claude, Cli::Grok];

    pub fn id(self) -> &'static str {
        match self {
            Cli::Agy => "agy",
            Cli::Codex => "codex",
            Cli::Claude => "claude",
            Cli::Grok => "grok",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
    /// Product name shown in the UI.
    pub fn label(self) -> &'static str {
        match self {
            Cli::Agy => "Gemini (Antigravity CLI)",
            Cli::Codex => "ChatGPT (Codex CLI)",
            Cli::Claude => "Claude (Claude Code)",
            Cli::Grok => "Grok (Grok Build)",
        }
    }
    /// Which brand's theme the Usage view uses.
    pub fn brand(self) -> &'static str {
        match self {
            Cli::Agy => "gemini",
            Cli::Codex => "chatgpt",
            Cli::Claude => "claude",
            Cli::Grok => "grok",
        }
    }
    /// How the user signs the app in, if it reports it isn't.
    pub fn sign_in_hint(self) -> &'static str {
        match self {
            Cli::Agy => "Open a terminal, run `agy` and sign in with Google.",
            Cli::Codex => "Open a terminal and run `codex login`.",
            Cli::Claude => "Open a terminal, run `claude` and type /login.",
            Cli::Grok => "Open a terminal and run `grok login`.",
        }
    }
    /// The app's own sign-in command (opens the provider's sign-in page).
    pub fn login_args(self) -> &'static [&'static str] {
        match self {
            Cli::Agy => &[], // Antigravity CLI signs in on first run
            Cli::Codex => &["login"],
            Cli::Claude => &["auth", "login"],
            Cli::Grok => &["login"],
        }
    }
    pub fn provider_id(self) -> String {
        format!("cli-{}", self.id())
    }
    pub fn from_provider_id(id: &str) -> Option<Self> {
        Self::parse(id.strip_prefix("cli-")?)
    }

    /// Program plus leading arguments. `CONDUCTOR_CLI_<NAME>` overrides the
    /// program for tests, as `program` or `program|first-arg`.
    fn program(self) -> (String, Vec<String>) {
        let key = format!("CONDUCTOR_CLI_{}", self.id().to_ascii_uppercase());
        match std::env::var(&key) {
            Ok(v) if !v.trim().is_empty() => {
                let mut parts = v.split('|').map(str::to_string);
                let program = parts.next().unwrap_or_default();
                (program, parts.collect())
            }
            _ => (self.id().to_string(), vec![]),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Detected {
    pub cli: Cli,
    pub label: &'static str,
    pub brand: &'static str,
    pub path: String,
    pub version: Option<String>,
}

fn resolve(cli: Cli) -> Option<PathBuf> {
    conductor_tools::exec::resolve_program(&cli.program().0)
}

fn command(cli: Cli, args: &[String]) -> Result<tokio::process::Command, String> {
    let exe = resolve(cli).ok_or_else(|| format!("{} is not installed.", cli.label()))?;
    let (_, mut lead) = cli.program();
    lead.extend_from_slice(args);
    let mut cmd = conductor_tools::exec::command_for(&exe, &lead)?;
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // no console window
    Ok(cmd)
}

/// Whether the app reports being signed in; `None` when it can't tell.
pub async fn signed_in(cli: Cli) -> Option<bool> {
    let args: Vec<String> = match cli {
        Cli::Codex => vec!["login".into(), "status".into()],
        Cli::Claude => vec!["auth".into(), "status".into()],
        _ => return None,
    };
    let mut c = command(cli, &args).ok()?;
    c.stdin(Stdio::null());
    let out = tokio::time::timeout(Duration::from_secs(20), c.output())
        .await
        .ok()?
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Some(match cli {
        Cli::Claude => serde_json::from_str::<Value>(text.trim())
            .ok()
            .and_then(|v| v["loggedIn"].as_bool())
            .unwrap_or(false),
        _ => out.status.success() && text.to_ascii_lowercase().contains("logged in"),
    })
}

/// Full path of the installed app, for opening its sign-in in a terminal.
pub fn executable(cli: Cli) -> Option<PathBuf> {
    resolve(cli)
}

/// Installed bridges, with their versions.
pub async fn detect() -> Vec<Detected> {
    let mut out = vec![];
    for cli in Cli::ALL {
        let Some(path) = resolve(cli) else { continue };
        let version = match command(cli, &["--version".into()]) {
            Ok(mut c) => tokio::time::timeout(Duration::from_secs(15), c.output())
                .await
                .ok()
                .and_then(|r| r.ok())
                .and_then(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .map(|l| l.trim().chars().take(80).collect())
                }),
            Err(_) => None,
        };
        out.push(Detected {
            cli,
            label: cli.label(),
            brand: cli.brand(),
            path: path.display().to_string(),
            version,
        });
    }
    out
}

/// A model the app offers, with the effort levels it accepts (if known).
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeModel {
    pub id: String,
    pub efforts: Vec<String>,
}

fn plain(ids: impl IntoIterator<Item = String>) -> Vec<BridgeModel> {
    ids.into_iter()
        .map(|id| BridgeModel {
            id,
            efforts: vec![],
        })
        .collect()
}

/// Effort levels Grok Build accepts (`--effort`).
const GROK_EFFORTS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Effort levels Conductor passes through to the apps.
const EFFORTS: [&str; 7] = ["minimal", "low", "medium", "high", "xhigh", "max", "ultra"];

/// Models the app offers. `default` lets the app pick its own.
pub async fn models(cli: Cli) -> Vec<BridgeModel> {
    let mut out = plain(["default".to_string()]);
    match cli {
        Cli::Agy => {
            if let Ok(mut c) = command(cli, &["models".into()]) {
                if let Ok(Ok(o)) = tokio::time::timeout(Duration::from_secs(60), c.output()).await {
                    out.extend(plain(parse_agy_models(&String::from_utf8_lossy(&o.stdout))));
                }
            }
        }
        // Aliases always point at the newest Sonnet / Opus.
        Cli::Claude => out.extend(plain(["sonnet".to_string(), "opus".to_string()])),
        // Grok Build has no model-list command; "default" follows the CLI's
        // own (newest) model, and the CLI accepts any API model id.
        Cli::Grok => {
            out[0].efforts = GROK_EFFORTS.iter().map(|e| e.to_string()).collect();
        }
        Cli::Codex => {
            // Codex's own catalog for the signed-in account. Its configured
            // default may not be allowed for the account, so list real models.
            if let Ok(mut c) = command(cli, &["debug".into(), "models".into()]) {
                if let Ok(Ok(o)) = tokio::time::timeout(Duration::from_secs(60), c.output()).await {
                    let listed = parse_codex_models(&String::from_utf8_lossy(&o.stdout));
                    if !listed.is_empty() {
                        out = listed;
                    }
                }
            }
        }
    }
    out
}

/// Update the app with its own updater (`codex update`, `agy update`,
/// `claude update`), so new models it supports become available.
pub async fn update(cli: Cli) -> Result<String, String> {
    let mut c = command(cli, &["update".into()])?;
    c.stdin(Stdio::null());
    let out = tokio::time::timeout(Duration::from_secs(600), c.output())
        .await
        .map_err(|_| format!("Updating {} timed out.", cli.label()))?
        .map_err(|e| e.to_string())?;
    let text = format!(
        "{}
{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let last = text
        .lines()
        .rev()
        .find(|l| {
            let l = l.trim();
            !l.is_empty() && !l.starts_with("npm warn") && !l.starts_with("npm notice")
        })
        .unwrap_or("Up to date")
        .trim()
        .chars()
        .take(200)
        .collect::<String>();
    if out.status.success() {
        Ok(last)
    } else {
        Err(format!("Updating {} failed: {last}", cli.label()))
    }
}

/// Visible models from `codex debug models` (hidden/internal ones skipped).
pub fn parse_codex_models(json: &str) -> Vec<BridgeModel> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return vec![];
    };
    v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["visibility"] == "list" && m["supported_in_api"] != false)
        .filter_map(|m| {
            let id = m["slug"].as_str()?;
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
                .then(|| BridgeModel {
                    id: id.to_string(),
                    efforts: m["supported_reasoning_levels"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|l| l["effort"].as_str())
                        .filter(|e| EFFORTS.contains(e))
                        .map(str::to_string)
                        .collect(),
                })
        })
        .collect()
}

pub fn parse_agy_models(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let (id, _) = l.split_once('\t')?;
            let id = id.trim();
            (!id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c)))
            .then(|| id.to_string())
        })
        .collect()
}

/// Windows command lines are limited to 32,767 characters and `agy` takes
/// its prompt as an argument, so long conversations keep their newest part.
const MAX_PROMPT_CHARS: usize = 24_000;

const ANSWER_DIRECTLY: &str = "Answer the last user message directly in text. Do not run tools, commands or file edits; Conductor handles those itself.";

/// Flatten OpenAI-style messages into one prompt.
pub fn prompt_from_messages(messages: &[Value]) -> String {
    let text = |m: &Value| match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    let system: Vec<String> = messages
        .iter()
        .filter(|m| m["role"] == "system")
        .map(text)
        .filter(|t| !t.trim().is_empty())
        .collect();
    let turns: Vec<(String, String)> = messages
        .iter()
        .filter(|m| m["role"] != "system")
        .map(|m| {
            let who = if m["role"] == "assistant" {
                "Assistant"
            } else {
                "User"
            };
            (who.to_string(), text(m))
        })
        .collect();
    let head = format!("{}\n\n{ANSWER_DIRECTLY}\n\n", system.join("\n\n"));
    let mut body = String::new();
    if turns.len() == 1 {
        body = turns[0].1.clone();
    } else {
        // Keep the newest turns that fit.
        let mut kept: Vec<String> = vec![];
        let mut used = 0;
        for (who, t) in turns.iter().rev() {
            let line = format!("{who}: {t}\n\n");
            if used + line.len() > MAX_PROMPT_CHARS.saturating_sub(head.len()) && !kept.is_empty() {
                break;
            }
            used += line.len();
            kept.push(line);
        }
        kept.reverse();
        body.push_str("Conversation so far:\n\n");
        body.extend(kept);
    }
    let mut prompt = head + &body;
    if prompt.len() > MAX_PROMPT_CHARS {
        let mut cut = prompt.len() - MAX_PROMPT_CHARS;
        while !prompt.is_char_boundary(cut) {
            cut += 1;
        }
        prompt = prompt[cut..].to_string();
    }
    prompt
}

/// Arguments and stdin for one non-interactive run.
pub fn invocation(
    cli: Cli,
    model: &str,
    effort: Option<&str>,
    prompt: &str,
) -> (Vec<String>, Option<String>) {
    let model = (model != "default" && !model.is_empty()).then(|| model.to_string());
    // Only known effort names are passed on; anything else is ignored.
    let effort = effort.filter(|e| EFFORTS.contains(e)).map(str::to_string);
    let s = |v: &str| v.to_string();
    match cli {
        Cli::Agy => {
            let mut a = vec![
                s("-p"),
                prompt.into(),
                s("--output-format"),
                s("stream-json"),
                s("--mode"),
                s("plan"),
                s("--sandbox"),
            ];
            if let Some(m) = model {
                a.extend([s("--model"), m]);
            }
            if let Some(e) =
                effort.filter(|e| ["low", "medium", "high", "xhigh", "max"].contains(&e.as_str()))
            {
                a.extend([s("--effort"), e]);
            }
            (a, None)
        }
        Cli::Codex => {
            let mut a = vec![
                s("exec"),
                s("--json"),
                s("--skip-git-repo-check"),
                s("-s"),
                s("read-only"),
            ];
            if let Some(m) = model {
                a.extend([s("-m"), m]);
            }
            if let Some(e) = effort {
                a.extend([s("-c"), format!("model_reasoning_effort={e}")]);
            }
            a.push(s("-"));
            (a, Some(prompt.into()))
        }
        Cli::Claude => {
            let mut a = vec![
                s("-p"),
                s("--output-format"),
                s("stream-json"),
                s("--verbose"),
                s("--include-partial-messages"),
                s("--max-turns"),
                s("1"),
                s("--permission-mode"),
                s("plan"),
            ];
            if let Some(m) = model {
                a.extend([s("--model"), m]);
            }
            (a, Some(prompt.into()))
        }
        Cli::Grok => {
            // The prompt is written to prompt.txt in the run's scratch
            // folder (see run_in), so it never touches the command line.
            let mut a = vec![
                s("--prompt-file"),
                s("prompt.txt"),
                s("--output-format"),
                s("streaming-json"),
                s("--max-turns"),
                s("1"),
                s("--disable-web-search"),
                s("--no-auto-update"),
            ];
            if let Some(m) = model {
                a.extend([s("--model"), m]);
            }
            if let Some(e) = effort.filter(|e| GROK_EFFORTS.contains(&e.as_str())) {
                a.extend([s("--effort"), e]);
            }
            (a, None)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Delta(String),
    Usage {
        input: u64,
        output: u64,
    },
    /// The reply is complete; the process may be stopped.
    Done,
    Error(String),
}

/// Parser state across lines of one run.
#[derive(Default)]
pub struct Parser {
    got_text: bool,
    partial: bool,
}

fn nested_message(v: &Value) -> String {
    let raw = v["message"]
        .as_str()
        .or_else(|| v.as_str())
        .unwrap_or("The app reported an error.");
    // Codex nests the provider's JSON error inside the message string.
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|inner| {
            inner
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| raw.to_string())
}

impl Parser {
    pub fn line(&mut self, cli: Cli, line: &str) -> Vec<Event> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return vec![];
        };
        let mut out = vec![];
        let usage = |u: &Value, i: &str, o: &str| Event::Usage {
            input: u[i].as_u64().unwrap_or(0),
            output: u[o].as_u64().unwrap_or(0),
        };
        match cli {
            Cli::Agy => {
                if v["event"] == "step_update" {
                    let s = &v["step_update"];
                    if s["step_type"] == "agent_response" {
                        if let Some(t) = s["text_delta"].as_str().filter(|t| !t.is_empty()) {
                            self.got_text = true;
                            out.push(Event::Delta(t.to_string()));
                        }
                        if s["state"] == "DONE" {
                            if s["usage"].is_object() {
                                out.push(usage(&s["usage"], "input_tokens", "output_tokens"));
                            }
                            // agy keeps running ~30 s after replying; stop once
                            // the text answer is complete.
                            if self.got_text {
                                out.push(Event::Done);
                            }
                        }
                    }
                } else if v["event"] == "result" {
                    let r = &v["result"];
                    if r["status"] != "SUCCESS" && !self.got_text {
                        out.push(Event::Error(
                            r["response"]
                                .as_str()
                                .or(r["status"].as_str())
                                .unwrap_or("Antigravity CLI failed")
                                .to_string(),
                        ));
                    } else {
                        if !self.got_text {
                            if let Some(t) = r["response"].as_str().filter(|t| !t.is_empty()) {
                                out.push(Event::Delta(t.to_string()));
                            }
                        }
                        out.push(Event::Done);
                    }
                } else if v["event"] == "error" {
                    out.push(Event::Error(nested_message(&v["error"])));
                }
            }
            Cli::Codex => match v["type"].as_str() {
                Some("item.completed") if v["item"]["type"] == "agent_message" => {
                    if let Some(t) = v["item"]["text"].as_str() {
                        self.got_text = true;
                        out.push(Event::Delta(t.to_string()));
                    }
                }
                Some("turn.completed") => {
                    out.push(usage(&v["usage"], "input_tokens", "output_tokens"));
                    out.push(Event::Done);
                }
                Some("turn.failed") => out.push(Event::Error(nested_message(&v["error"]))),
                Some("error") => out.push(Event::Error(nested_message(&v))),
                _ => {}
            },
            Cli::Grok => match v["type"].as_str() {
                Some("text") => {
                    if let Some(t) = v["data"].as_str().filter(|t| !t.is_empty()) {
                        self.got_text = true;
                        out.push(Event::Delta(t.to_string()));
                    }
                }
                Some("usage") | Some("end") => {
                    let u = if v["usage"].is_object() {
                        &v["usage"]
                    } else {
                        &v
                    };
                    let input = u["input_tokens"].as_u64().or(u["inputTokens"].as_u64());
                    let output = u["output_tokens"].as_u64().or(u["outputTokens"].as_u64());
                    if input.is_some() || output.is_some() {
                        out.push(Event::Usage {
                            input: input.unwrap_or(0),
                            output: output.unwrap_or(0),
                        });
                    }
                    if v["type"] == "end" {
                        out.push(Event::Done);
                    }
                }
                Some("error") => out.push(Event::Error(nested_message(if v["error"].is_null() {
                    &v
                } else {
                    &v["error"]
                }))),
                _ => {}
            },
            Cli::Claude => match v["type"].as_str() {
                Some("stream_event") => {
                    let e = &v["event"];
                    if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta" {
                        if let Some(t) = e["delta"]["text"].as_str() {
                            self.partial = true;
                            self.got_text = true;
                            out.push(Event::Delta(t.to_string()));
                        }
                    }
                }
                Some("assistant") if !self.partial => {
                    for part in v["message"]["content"].as_array().into_iter().flatten() {
                        if part["type"] == "text" {
                            if let Some(t) = part["text"].as_str() {
                                self.got_text = true;
                                out.push(Event::Delta(t.to_string()));
                            }
                        }
                    }
                }
                Some("result") => {
                    if v["is_error"] == true
                        || v["subtype"]
                            .as_str()
                            .is_some_and(|s| s.starts_with("error"))
                    {
                        let msg = v["result"].as_str().unwrap_or("Claude Code failed");
                        out.push(Event::Error(msg.to_string()));
                    } else {
                        out.push(usage(&v["usage"], "input_tokens", "output_tokens"));
                        out.push(Event::Done);
                    }
                }
                _ => {}
            },
        }
        out
    }
}

/// Plan or rate limits reached: reported as 429 so Combos move on to the
/// next provider instead of stopping.
fn looks_rate_limited(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    [
        "usage limit",
        "rate limit",
        "quota",
        "too many requests",
        "try again at",
        "limit reached",
    ]
    .iter()
    .any(|k| m.contains(k))
}

fn looks_signed_out(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    [
        "not logged in",
        "/login",
        "sign in",
        "login required",
        "unauthenticated",
        "authentication",
    ]
    .iter()
    .any(|k| m.contains(k))
}

/// Run one prompt. `emit` sees deltas and usage; returns once the reply is
/// complete, the app fails, or `cancel` fires (the process tree is killed).
pub async fn run(
    cli: Cli,
    model: &str,
    effort: Option<&str>,
    prompt: &str,
    cancel: CancellationToken,
    mut emit: impl FnMut(Event) + Send,
) -> Result<(), String> {
    let scratch = std::env::temp_dir().join(format!("conductor-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let result = run_in(cli, model, effort, prompt, &scratch, cancel, &mut emit).await;
    let _ = std::fs::remove_dir_all(&scratch);
    result.map_err(|e| {
        if looks_signed_out(&e) {
            format!(
                "{e}\n\n{} isn't signed in. {}",
                cli.label(),
                cli.sign_in_hint()
            )
        } else {
            e
        }
    })
}

async fn run_in(
    cli: Cli,
    model: &str,
    effort: Option<&str>,
    prompt: &str,
    cwd: &std::path::Path,
    cancel: CancellationToken,
    emit: &mut (impl FnMut(Event) + Send),
) -> Result<(), String> {
    let (args, stdin) = invocation(cli, model, effort, prompt);
    if cli == Cli::Grok {
        std::fs::write(cwd.join("prompt.txt"), prompt).map_err(|e| e.to_string())?;
    }
    let mut cmd = command(cli, &args)?;
    cmd.current_dir(cwd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", cli.label()))?;
    let pid = child.id();
    if let Some(mut si) = child.stdin.take() {
        let input = stdin.unwrap_or_default();
        tokio::spawn(async move {
            let _ = si.write_all(input.as_bytes()).await;
            let _ = si.shutdown().await;
        });
    }
    let stderr = child.stderr.take().map(|e| {
        tokio::spawn(async move {
            let mut lines = BufReader::new(e).lines();
            let mut tail = String::new();
            while let Ok(Some(l)) = lines.next_line().await {
                tail.push_str(&l);
                tail.push('\n');
                if tail.len() > 8_000 {
                    tail.drain(..tail.len() - 4_000);
                }
            }
            tail
        })
    });
    let mut lines = BufReader::new(child.stdout.take().ok_or("no output")?).lines();
    let mut parser = Parser::default();
    let mut outcome: Option<Result<(), String>> = None;
    loop {
        let next = tokio::select! {
            biased;
            _ = cancel.cancelled() => { outcome = Some(Err("cancelled".into())); break; }
            l = lines.next_line() => l,
        };
        let Ok(Some(line)) = next else { break };
        for ev in parser.line(cli, &line) {
            match ev {
                Event::Done => outcome = Some(Ok(())),
                Event::Error(e) => outcome = Some(Err(e)),
                other => emit(other),
            }
        }
        if outcome.is_some() {
            break;
        }
    }
    // Stop the app (and anything it started) once we have the answer.
    kill_tree(pid, &mut child).await;
    if let Some(o) = outcome {
        return o;
    }
    let tail = match stderr {
        Some(h) => h.await.unwrap_or_default(),
        None => String::new(),
    };
    let last = tail
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty() && !l.contains(" WARN ") && !l.contains("DEBUG"))
        .unwrap_or("it exited without an answer")
        .trim()
        .chars()
        .take(400)
        .collect::<String>();
    Err(format!("{} stopped: {last}", cli.label()))
}

async fn kill_tree(pid: Option<u32>, child: &mut tokio::process::Child) {
    #[cfg(windows)]
    if let Some(pid) = pid {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    let _ = pid;
    let _ = child.kill().await;
}

// ---------- loopback OpenAI-compatible endpoint ----------

/// Returns the expected key for a bridge provider id (from the keychain).
pub type KeyLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Receives token usage for one bridge provider.
pub type UsageObserver = Arc<dyn Fn(&str, u64, u64) + Send + Sync>;

#[derive(Clone)]
struct ServerState {
    key: KeyLookup,
    on_usage: UsageObserver,
}

fn authorized(state: &ServerState, cli: Cli, headers: &HeaderMap) -> bool {
    let Some(expected) = (state.key)(&cli.provider_id()) else {
        return false;
    };
    let got = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    // Constant-time enough for a loopback-only random key.
    !expected.is_empty()
        && got.len() == expected.len()
        && got.bytes().zip(expected.bytes()).all(|(a, b)| a == b)
}

fn error_json(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"message":message,"status":status.as_u16()}})),
    )
        .into_response()
}

async fn list_models(
    State(st): State<ServerState>,
    Path(cli): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(cli) = Cli::parse(&cli) else {
        return error_json(StatusCode::NOT_FOUND, "Unknown app");
    };
    if !authorized(&st, cli, &headers) {
        return error_json(StatusCode::UNAUTHORIZED, "Bridge key rejected");
    }
    if resolve(cli).is_none() {
        return error_json(
            StatusCode::NOT_FOUND,
            &format!("{} is not installed.", cli.label()),
        );
    }
    let data: Vec<Value> = models(cli)
        .await
        .into_iter()
        .map(|m| json!({"id": m.id, "supported_efforts": m.efforts}))
        .collect();
    Json(json!({"object":"list","data":data})).into_response()
}

async fn chat(
    State(st): State<ServerState>,
    Path(cli): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let Some(cli) = Cli::parse(&cli) else {
        return error_json(StatusCode::NOT_FOUND, "Unknown app");
    };
    if !authorized(&st, cli, &headers) {
        return error_json(StatusCode::UNAUTHORIZED, "Bridge key rejected");
    }
    let model = body["model"].as_str().unwrap_or("default").to_string();
    let effort = body["reasoning_effort"].as_str().map(str::to_string);
    let prompt = prompt_from_messages(
        body["messages"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    );
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(64);
    let cancel = CancellationToken::new();
    let guard = cancel.clone().drop_guard();
    let on_usage = st.on_usage.clone();
    tokio::spawn(async move {
        let frame = |v: Value| format!("data: {v}\n\n");
        let (mut input, mut output) = (0, 0);
        let tx2 = tx.clone();
        let result = run(
            cli,
            &model,
            effort.as_deref(),
            &prompt,
            cancel,
            |ev| match ev {
                Event::Delta(t) => {
                    let _ = tx2.try_send(frame(
                        json!({"choices":[{"index":0,"delta":{"content":t}}]}),
                    ));
                }
                Event::Usage {
                    input: i,
                    output: o,
                } => {
                    input = input.max(i);
                    output = output.max(o);
                }
                _ => {}
            },
        )
        .await;
        match result {
            Ok(()) => {
                on_usage(&cli.provider_id(), input, output);
                let _ = tx.send(frame(json!({"choices":[],"usage":{"prompt_tokens":input,"completion_tokens":output}}))).await;
                let _ = tx.send("data: [DONE]\n\n".into()).await;
            }
            Err(e) if e == "cancelled" => {}
            Err(e) => {
                let status = if looks_rate_limited(&e) {
                    429
                } else if looks_signed_out(&e) {
                    401
                } else {
                    502
                };
                let _ = tx
                    .send(frame(json!({"error":{"message":e,"status":status}})))
                    .await;
            }
        }
    });
    // Dropping the response body (client went away / Stop) cancels the run.
    let stream = futures_util::stream::unfold((rx, guard), |(mut rx, guard)| async move {
        rx.recv()
            .await
            .map(|chunk| (Ok::<_, std::io::Error>(chunk), (rx, guard)))
    });
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| error_json(StatusCode::INTERNAL_SERVER_ERROR, "stream failed"))
}

/// Serve the bridge on `listener` (bind it to 127.0.0.1).
pub async fn serve(
    listener: tokio::net::TcpListener,
    key: KeyLookup,
    on_usage: UsageObserver,
) -> std::io::Result<()> {
    let app = Router::new()
        .route("/{cli}/v1/models", get(list_models))
        .route("/{cli}/v1/chat/completions", post(chat))
        .with_state(ServerState { key, on_usage });
    axum::serve(listener, app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(cli: Cli, lines: &[&str]) -> Vec<Event> {
        let mut p = Parser::default();
        lines.iter().flat_map(|l| p.line(cli, l)).collect()
    }

    /// A stand-in `codex` (Node script) behind the real loopback endpoint,
    /// driven by core's own OpenAI-compatible client.
    #[tokio::test]
    async fn bridge_serves_core_provider_client_end_to_end() {
        use conductor_core::domain::{Message, Model, ProviderConfig, ProviderKind, Role};
        use conductor_core::providers::{self, ProviderRequest, StreamEvent};
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-codex.mjs");
        std::fs::write(
            &script,
            r#"let input = '';
process.stdin.on('data', (d) => (input += d));
process.stdin.on('end', () => {
  const last = input.trim().split('\n').pop();
  if (last.includes('FAIL')) {
    console.log(JSON.stringify({ type: 'turn.failed', error: { message: 'Not logged in' } }));
    return;
  }
  console.log(JSON.stringify({ type: 'thread.started' }));
  console.log(JSON.stringify({ type: 'item.completed', item: { type: 'agent_message', text: 'echo: ' + last } }));
  console.log(JSON.stringify({ type: 'turn.completed', usage: { input_tokens: 11, output_tokens: 3 } }));
});
"#,
        )
        .unwrap();
        std::env::set_var("CONDUCTOR_CLI_CODEX", format!("node|{}", script.display()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let used = Arc::new(std::sync::Mutex::new(vec![]));
        let used2 = used.clone();
        tokio::spawn(serve(
            listener,
            Arc::new(|id: &str| (id == "cli-codex").then(|| "bridge-key".to_string())),
            Arc::new(move |id: &str, i, o| used2.lock().unwrap().push((id.to_string(), i, o))),
        ));
        let config = ProviderConfig {
            id: "cli-codex".into(),
            name: "ChatGPT (Codex CLI)".into(),
            kind: ProviderKind::OpenaiCompatible,
            base_url: format!("http://127.0.0.1:{port}/codex/v1"),
            models: vec![Model {
                id: "default".into(),
                name: "default".into(),
                efforts: vec![],
                context_window: None,
                tools: false,
                vision: false,
            }],
            enabled: true,
        };
        let models = providers::list_models(&config, "bridge-key", CancellationToken::new())
            .await
            .unwrap();
        assert!(models.iter().any(|m| m.id == "default"));
        assert!(
            providers::list_models(&config, "wrong", CancellationToken::new())
                .await
                .is_err()
        );

        let request = |text: &str| ProviderRequest {
            model: "default".into(),
            messages: vec![Message::new(Role::User, text.into())],
            instructions: "Be brief.".into(),
            effort: None,
            allow_highest_effort: false,
        };
        let mut events = vec![];
        providers::stream(
            &config,
            "bridge-key",
            &request("what is \"2\" + 2? 100%"),
            CancellationToken::new(),
            |e| {
                events.push(e);
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(
            matches!(&events[0], StreamEvent::Delta { text } if text == "echo: what is \"2\" + 2? 100%")
        );
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::Usage {
                input_tokens: 11,
                output_tokens: 3
            }
        )));
        assert_eq!(
            used.lock().unwrap().as_slice(),
            &[("cli-codex".to_string(), 11, 3)]
        );

        // A signed-out app becomes a provider auth error with a sign-in hint.
        let err = providers::stream(
            &config,
            "bridge-key",
            &request("FAIL"),
            CancellationToken::new(),
            |_| Ok(()),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, conductor_core::Error::Provider { status: 401, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn codex_catalog_lists_visible_models_and_limits_map_to_429() {
        let json = r#"{"models":[
            {"slug":"gpt-6-astra","visibility":"list","supported_in_api":true},
            {"slug":"gpt-reserve","visibility":"hide","supported_in_api":true},
            {"slug":"gpt-5.6-luna","visibility":"list"},
            {"slug":"bad slug","visibility":"list"}]}"#;
        let ids: Vec<String> = parse_codex_models(json).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["gpt-6-astra", "gpt-5.6-luna"]);
        let with_efforts = r#"{"models":[{"slug":"gpt-6.1-sol","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"ultra"},{"effort":"bogus"}]}]}"#;
        assert_eq!(
            parse_codex_models(with_efforts)[0].efforts,
            vec!["low", "ultra"]
        );
        let (a, _) = invocation(Cli::Codex, "gpt-6.1-sol", Some("xhigh"), "q");
        assert!(a
            .windows(2)
            .any(|w| w == ["-c", "model_reasoning_effort=xhigh"]));
        let (a, _) = invocation(Cli::Codex, "gpt-6.1-sol", Some("evil\" --flag"), "q");
        assert!(
            !a.iter().any(|x| x.contains("evil")),
            "unknown effort names are dropped"
        );
        assert!(parse_codex_models("not json").is_empty());
        assert!(looks_rate_limited(
            "You've hit your usage limit. Upgrade to Pro or try again at 3:13 PM."
        ));
        assert!(!looks_rate_limited("Not logged in · Please run /login"));
    }

    /// npm-installed apps run through .cmd shims, which refuse arguments with
    /// quotes, percent signs or line breaks. Prompts go through stdin and no
    /// generated argument may contain those characters.
    #[test]
    fn shim_launched_apps_get_shim_safe_arguments() {
        for cli in [Cli::Codex, Cli::Claude, Cli::Grok] {
            for effort in [None, Some("low"), Some("ultra")] {
                let (args, _) = invocation(
                    cli,
                    "gpt-6.1-sol",
                    effort,
                    "a \"quoted\" 100% prompt\nwith lines",
                );
                for a in &args {
                    assert!(
                        !a.chars().any(|c| matches!(c, '"' | '%' | '\r' | '\n')),
                        "{cli:?}: {a}"
                    );
                }
            }
        }
    }

    #[test]
    fn grok_build_stream_and_invocation() {
        let ev = feed(
            Cli::Grok,
            &[
                r#"{"type":"thought","data":"thinking"}"#,
                r#"{"type":"text","data":"po"}"#,
                r#"{"type":"text","data":"ng"}"#,
                r#"{"type":"usage","usage":{"input_tokens":12,"output_tokens":2}}"#,
                r#"{"type":"end","stopReason":"end_turn"}"#,
            ],
        );
        assert_eq!(
            ev,
            vec![
                Event::Delta("po".into()),
                Event::Delta("ng".into()),
                Event::Usage {
                    input: 12,
                    output: 2
                },
                Event::Done,
            ]
        );
        let ev = feed(
            Cli::Grok,
            &[r#"{"type":"error","message":"Not logged in. Run grok login"}"#],
        );
        assert!(matches!(&ev[0], Event::Error(m) if looks_signed_out(m)));
        let (a, stdin) = invocation(
            Cli::Grok,
            "grok-4.7",
            Some("xhigh"),
            "a \"quoted\" prompt
line",
        );
        assert!(
            stdin.is_none() && !a.iter().any(|x| x.contains("quoted")),
            "prompt goes to a file"
        );
        assert!(
            a.windows(2).any(|w| w == ["--model", "grok-4.7"])
                && a.windows(2).any(|w| w == ["--effort", "xhigh"])
        );
        assert_eq!(Cli::from_provider_id("cli-grok"), Some(Cli::Grok));
        assert_eq!(Cli::Claude.login_args(), ["auth", "login"]);
        assert_eq!(Cli::Grok.login_args(), ["login"]);
    }

    #[test]
    fn agy_stream_ends_once_text_is_done() {
        let ev = feed(
            Cli::Agy,
            &[
                r#"{"event":"init","init":{}}"#,
                r#"{"event":"step_update","step_update":{"step_type":"user_input","state":"DONE"}}"#,
                r#"{"event":"step_update","step_update":{"step_type":"agent_response","state":"ACTIVE","text_delta":"po"}}"#,
                r#"{"event":"step_update","step_update":{"step_type":"agent_response","state":"DONE","text_delta":"ng","usage":{"input_tokens":10,"output_tokens":2}}}"#,
            ],
        );
        assert_eq!(
            ev,
            vec![
                Event::Delta("po".into()),
                Event::Delta("ng".into()),
                Event::Usage {
                    input: 10,
                    output: 2
                },
                Event::Done,
            ]
        );
        // A thinking-only step with no text does not end the reply.
        let ev = feed(
            Cli::Agy,
            &[
                r#"{"event":"step_update","step_update":{"step_type":"agent_response","state":"DONE","usage":{"input_tokens":1,"output_tokens":1}}}"#,
            ],
        );
        assert!(!ev.contains(&Event::Done));
        let ev = feed(
            Cli::Agy,
            &[r#"{"event":"result","result":{"status":"ERROR","response":"Please sign in"}}"#],
        );
        assert_eq!(ev, vec![Event::Error("Please sign in".into())]);
    }

    #[test]
    fn codex_events_and_nested_errors() {
        let ev = feed(
            Cli::Codex,
            &[
                r#"{"type":"thread.started"}"#,
                r#"{"type":"item.completed","item":{"type":"error","message":"warning only"}}"#,
                r#"{"type":"item.completed","item":{"type":"agent_message","text":"pong"}}"#,
                r#"{"type":"turn.completed","usage":{"input_tokens":5,"cached_input_tokens":0,"output_tokens":1}}"#,
            ],
        );
        assert_eq!(
            ev,
            vec![
                Event::Delta("pong".into()),
                Event::Usage {
                    input: 5,
                    output: 1
                },
                Event::Done
            ]
        );
        let ev = feed(
            Cli::Codex,
            &[
                r#"{"type":"turn.failed","error":{"message":"{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'x' model is not supported\"}}"}}"#,
            ],
        );
        assert_eq!(
            ev,
            vec![Event::Error("The 'x' model is not supported".into())]
        );
    }

    #[test]
    fn claude_partial_deltas_without_duplicates_and_login_errors() {
        let ev = feed(
            Cli::Claude,
            &[
                r#"{"type":"system","subtype":"init"}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"po"}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"ng"}}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"pong"}]}}"#,
                r#"{"type":"result","subtype":"success","is_error":false,"result":"pong","usage":{"input_tokens":7,"output_tokens":2}}"#,
            ],
        );
        assert_eq!(
            ev,
            vec![
                Event::Delta("po".into()),
                Event::Delta("ng".into()),
                Event::Usage {
                    input: 7,
                    output: 2
                },
                Event::Done
            ]
        );
        let ev = feed(
            Cli::Claude,
            &[
                r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#,
            ],
        );
        assert!(matches!(&ev[0], Event::Error(m) if looks_signed_out(m)));
    }

    #[test]
    fn prompt_keeps_system_and_newest_turns_within_limit() {
        let sys = json!({"role":"system","content":"Be brief."});
        let one = prompt_from_messages(&[sys.clone(), json!({"role":"user","content":"hi"})]);
        assert!(one.starts_with("Be brief."));
        assert!(one.contains(ANSWER_DIRECTLY));
        assert!(one.ends_with("hi"));
        let mut msgs = vec![sys];
        for i in 0..400 {
            msgs.push(json!({"role": if i % 2 == 0 {"user"} else {"assistant"}, "content": format!("turn {i} {}", "x".repeat(200))}));
        }
        msgs.push(json!({"role":"user","content":[{"type":"text","text":"latest question"}]}));
        let p = prompt_from_messages(&msgs);
        assert!(p.len() <= MAX_PROMPT_CHARS);
        assert!(p.contains("latest question"));
        assert!(!p.contains("turn 0 "));
    }

    #[test]
    fn invocations_are_read_only_and_never_put_prompts_on_shim_command_lines() {
        let (a, stdin) = invocation(Cli::Codex, "default", None, "q \"quoted\" 100%");
        assert!(a.contains(&"read-only".to_string()) && !a.iter().any(|x| x.contains("quoted")));
        assert_eq!(stdin.as_deref(), Some("q \"quoted\" 100%"));
        let (a, stdin) = invocation(Cli::Claude, "opus", None, "q");
        assert!(a.windows(2).any(|w| w == ["--permission-mode", "plan"]));
        assert!(a.windows(2).any(|w| w == ["--model", "opus"]));
        assert!(stdin.is_some());
        let (a, stdin) = invocation(Cli::Agy, "gemini-3.8-flash-low", Some("high"), "q");
        assert!(a.windows(2).any(|w| w == ["--effort", "high"]));
        assert!(
            a.contains(&"--sandbox".to_string()) && a.windows(2).any(|w| w == ["--mode", "plan"])
        );
        assert!(stdin.is_none());
        assert_eq!(
            parse_agy_models("Fetching available models...\ngemini-x-high\tGemini X\nbad line\n"),
            vec!["gemini-x-high"]
        );
        assert_eq!(Cli::from_provider_id("cli-agy"), Some(Cli::Agy));
        assert_eq!(Cli::from_provider_id("openai"), None);
    }
}
