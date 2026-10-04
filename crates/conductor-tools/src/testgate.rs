//! Test Gate: run Definition-of-Done commands and return reduced evidence.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::exec::{self, ExecEnd, ExecRequest};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateResult {
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    /// Log-reduced output (failure + context), suitable for a model.
    pub evidence: String,
    pub duration_ms: u128,
}

/// Split a configured command line into argv (simple quoting rules: double
/// or single quotes group words). No shell features are interpreted.
pub fn split_command(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in cmd.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"') | (None, '\'') => {
                quote = Some(c);
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub async fn run_gate(
    command: &str,
    cwd: &Path,
    timeout_secs: u64,
    cancel: CancellationToken,
) -> GateResult {
    let argv = split_command(command);
    let Some((prog, args)) = argv.split_first() else {
        return GateResult {
            command: command.into(),
            passed: false,
            exit_code: None,
            evidence: "empty command".into(),
            duration_ms: 0,
        };
    };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut req = ExecRequest::new(prog.as_str(), &args, cwd).timeout(timeout_secs);
    req.max_output_bytes = 2 * 1024 * 1024;
    let r = exec::run(req, cancel).await;
    let reduced = conductor_context::logs::reduce(
        &r.combined(),
        &conductor_context::logs::LogOptions::default(),
    );
    let mut evidence = reduced.text;
    match r.end {
        ExecEnd::TimedOut => evidence = format!("timed out after {timeout_secs}s\n{evidence}"),
        ExecEnd::Cancelled => evidence = format!("cancelled\n{evidence}"),
        ExecEnd::SpawnFailed => evidence = r.stderr.clone(),
        ExecEnd::Exited => {}
    }
    GateResult {
        command: command.into(),
        passed: r.success(),
        exit_code: r.code,
        evidence,
        duration_ms: r.duration_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_commands() {
        assert_eq!(
            split_command("cargo test --workspace"),
            vec!["cargo", "test", "--workspace"]
        );
        assert_eq!(
            split_command("npm run \"build all\""),
            vec!["npm", "run", "build all"]
        );
        assert_eq!(split_command("echo ''"), vec!["echo", ""]);
        assert!(split_command("   ").is_empty());
    }

    #[tokio::test]
    async fn passing_and_failing_gates() {
        let d = tempfile::tempdir().unwrap();
        let ok = run_gate("git --version", d.path(), 30, CancellationToken::new()).await;
        assert!(ok.passed, "{ok:?}");
        let bad = run_gate(
            "git definitely-not-a-subcommand",
            d.path(),
            30,
            CancellationToken::new(),
        )
        .await;
        assert!(!bad.passed);
        assert!(!bad.evidence.is_empty());
        let missing = run_gate(
            "no-such-runner-zz test",
            d.path(),
            30,
            CancellationToken::new(),
        )
        .await;
        assert!(!missing.passed);
        assert!(missing.evidence.contains("not found"));
    }
}
