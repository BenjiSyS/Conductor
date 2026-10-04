//! Process execution.
//!
//! * Programs are spawned with an explicit argument vector; Conductor never
//!   builds shell strings from model output.
//! * Output is captured up to a byte cap per stream (the rest is counted,
//!   not stored), so a runaway build log cannot exhaust memory.
//! * Cancellation and timeouts kill the whole process tree.
//! * Secrets in captured output are redacted before they leave this module.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::process_lifetime::OwnedProcess;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRequest {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default = "default_cap")]
    pub max_output_bytes: usize,
    /// Optional stdin payload.
    #[serde(default)]
    pub stdin: Option<String>,
}

fn default_cap() -> usize {
    256 * 1024
}

impl ExecRequest {
    pub fn new(program: impl Into<String>, args: &[&str], cwd: impl AsRef<Path>) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: cwd.as_ref().to_path_buf(),
            env: BTreeMap::new(),
            timeout_secs: Some(600),
            max_output_bytes: default_cap(),
            stdin: None,
        }
    }
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecEnd {
    Exited,
    TimedOut,
    Cancelled,
    SpawnFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub end: ExecEnd,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated_bytes: usize,
    pub stderr_truncated_bytes: usize,
    pub duration_ms: u128,
    pub redactions: usize,
}

impl ExecResult {
    pub fn success(&self) -> bool {
        self.end == ExecEnd::Exited && self.code == Some(0)
    }
    /// stdout + stderr for log reduction.
    pub fn combined(&self) -> String {
        let mut s = self.stdout.clone();
        if !self.stderr.is_empty() {
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(&self.stderr);
        }
        s
    }
}

/// Resolve a program name to an executable path (PATH + PATHEXT on Windows).
pub fn resolve_program(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.components().count() > 1 || p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    which::which(program).ok()
}

pub async fn run(req: ExecRequest, cancel: CancellationToken) -> ExecResult {
    let started = Instant::now();
    let fail = |msg: String| ExecResult {
        end: ExecEnd::SpawnFailed,
        code: None,
        stdout: String::new(),
        stderr: msg,
        stdout_truncated_bytes: 0,
        stderr_truncated_bytes: 0,
        duration_ms: started.elapsed().as_millis(),
        redactions: 0,
    };
    if cancel.is_cancelled() {
        return ExecResult {
            end: ExecEnd::Cancelled,
            ..fail(String::new())
        };
    }
    let Some(exe) = resolve_program(&req.program) else {
        return fail(format!("'{}' was not found on PATH", req.program));
    };
    let mut cmd = match command_for(&exe, &req.args) {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    cmd.current_dir(&req.cwd)
        .envs(&req.env)
        .stdin(if req.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut process = match OwnedProcess::spawn(&mut cmd) {
        Ok(c) => c,
        Err(e) => return fail(format!("could not start '{}': {e}", req.program)),
    };
    let out = match process.take_stdout() {
        Ok(Some(stream)) => stream,
        Ok(None) => return fail("spawned process has no stdout pipe".into()),
        Err(error) => return fail(format!("could not register stdout pipe: {error}")),
    };
    let err = match process.take_stderr() {
        Ok(Some(stream)) => stream,
        Ok(None) => return fail("spawned process has no stderr pipe".into()),
        Err(error) => return fail(format!("could not register stderr pipe: {error}")),
    };
    let stdin = match process.take_stdin() {
        Ok(stream) => stream,
        Err(error) => return fail(format!("could not register stdin pipe: {error}")),
    };
    let timeout = req
        .timeout_secs
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(24 * 3600));

    let mut stdout = Captured::new(req.max_output_bytes);
    let mut stderr = Captured::new(req.max_output_bytes);
    // Inline I/O futures belong to this call. Abort drops their pipe handles
    // and the owned process guard; no detached reader/writer tasks survive.
    let mut out_reader = Box::pin(stdout.read(out));
    let mut err_reader = Box::pin(stderr.read(err));
    let mut input_writer = Box::pin(async move {
        if let Some(mut stdin) = stdin {
            if let Some(input) = req.stdin {
                stdin.write_all(input.as_bytes()).await?;
            }
            stdin.shutdown().await?;
        }
        Ok::<(), std::io::Error>(())
    });
    let mut out_done = false;
    let mut err_done = false;
    let mut input_done = false;
    let mut errors = Vec::new();
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut completed = false;
    let (mut end, mut code) = {
        let wait = process.wait();
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break (ExecEnd::Cancelled, None),
                _ = &mut deadline => break (ExecEnd::TimedOut, None),
                status = &mut wait => {
                    break match status {
                        Ok(status) => { completed = true; (ExecEnd::Exited, status.code()) },
                        Err(error) => {
                            errors.push(format!("process wait/cleanup failed: {error}"));
                            (ExecEnd::Exited, None)
                        }
                    };
                }
                _ = &mut out_reader, if !out_done => out_done = true,
                _ = &mut err_reader, if !err_done => err_done = true,
                result = &mut input_writer, if !input_done => {
                    input_done = true;
                    if let Err(error) = result {
                        errors.push(format!("process stdin write failed: {error}"));
                    }
                }
            }
        }
    };
    // A backpressured write cannot delay cleanup or retain stdin after exit.
    drop(input_writer);
    if !completed {
        if let Err(error) = process.shutdown().await {
            errors.push(format!("process tree cleanup failed: {error}"));
        }
    }
    // Finite commands own their descendants until cleanup completes. Drain
    // buffered output after writers close, but escaped/stuck writers cannot
    // extend the call indefinitely. Timeout and cancellation still apply.
    let drain = tokio::time::sleep(Duration::from_secs(1));
    tokio::pin!(drain);
    while !out_done || !err_done {
        tokio::select! {
            _ = &mut out_reader, if !out_done => out_done = true,
            _ = &mut err_reader, if !err_done => err_done = true,
            _ = cancel.cancelled(), if end == ExecEnd::Exited => {
                end = ExecEnd::Cancelled;
                code = None;
                break;
            }
            _ = &mut deadline, if end == ExecEnd::Exited => {
                end = ExecEnd::TimedOut;
                code = None;
                break;
            }
            _ = &mut drain => break,
        }
    }
    drop(out_reader);
    drop(err_reader);
    if !out_done || !err_done {
        errors.push("process output drain stopped before EOF; output may be incomplete".into());
    }
    if let Some(error) = stdout.error.take() {
        errors.push(format!("process stdout read failed: {error}"));
    }
    if let Some(error) = stderr.error.take() {
        errors.push(format!("process stderr read failed: {error}"));
    }
    let so_trunc = stdout.dropped;
    let se_trunc = stderr.dropped;
    let stdout = stdout.text();
    let mut stderr = stderr.text();
    if !errors.is_empty() {
        // A cleanup/I/O failure cannot masquerade as a successful exit.
        code = None;
        if !stderr.is_empty() {
            stderr.push('\n');
        }
        stderr.push_str(&errors.join("\n"));
    }
    let ro = conductor_security::secrets::redact(&stdout);
    let re = conductor_security::secrets::redact(&stderr);
    ExecResult {
        end,
        code,
        redactions: ro.findings.len() + re.findings.len(),
        stdout: ro.text,
        stderr: re.text,
        stdout_truncated_bytes: so_trunc,
        stderr_truncated_bytes: se_trunc,
        duration_ms: started.elapsed().as_millis(),
    }
}

/// Build the command. On Windows, `.cmd`/`.bat` shims (npm, npx, pnpm…) must
/// run through cmd.exe; the command line is assembled explicitly so paths
/// with spaces work and metacharacters in arguments stay literal (quoted).
/// Arguments containing `"`, `%` or line breaks are refused for shims because
/// cmd.exe cannot pass them through safely.
pub fn command_for(exe: &Path, args: &[String]) -> Result<Command, String> {
    #[cfg(windows)]
    {
        let ext = exe
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        if matches!(ext.as_deref(), Some("cmd") | Some("bat")) {
            let mut line = format!("\"{}\"", exe.display());
            for a in args {
                if a.chars().any(|ch| matches!(ch, '"' | '%' | '\r' | '\n')) {
                    return Err(format!(
                        "argument {a:?} cannot be passed safely to {}",
                        exe.display()
                    ));
                }
                line.push(' ');
                if a.is_empty()
                    || a.chars()
                        .any(|ch| ch.is_whitespace() || "&|<>^()".contains(ch))
                {
                    line.push('"');
                    line.push_str(a);
                    line.push('"');
                } else {
                    line.push_str(a);
                }
            }
            let mut c = Command::new("cmd.exe");
            c.raw_arg(format!("/D /S /C \"{line}\""));
            return Ok(c);
        }
    }
    let mut c = Command::new(exe);
    c.args(args);
    Ok(c)
}

struct Captured {
    bytes: Vec<u8>,
    cap: usize,
    dropped: usize,
    error: Option<std::io::Error>,
}

impl Captured {
    fn new(cap: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(cap.min(64 * 1024)),
            cap,
            dropped: 0,
            error: None,
        }
    }

    async fn read<R: AsyncRead + Unpin>(&mut self, mut stream: R) {
        let mut chunk = [0u8; 8192];
        loop {
            match stream.read(&mut chunk).await {
                Ok(0) => break,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
                    ) =>
                {
                    break
                }
                Err(error) => {
                    self.error = Some(error);
                    break;
                }
                Ok(n) => {
                    let kept = n.min(self.cap.saturating_sub(self.bytes.len()));
                    self.bytes.extend_from_slice(&chunk[..kept]);
                    self.dropped = self.dropped.saturating_add(n - kept);
                }
            }
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> ExecRequest {
        if cfg!(windows) {
            ExecRequest::new(
                "powershell",
                &["-NoProfile", "-Command", script],
                std::env::temp_dir(),
            )
        } else {
            ExecRequest::new("sh", &["-c", script], std::env::temp_dir())
        }
    }

    #[tokio::test]
    async fn captures_output_and_exit_code() {
        let r = run(shell("echo hello; exit 3"), CancellationToken::new()).await;
        assert_eq!(r.end, ExecEnd::Exited);
        assert_eq!(r.code, Some(3));
        assert!(r.stdout.contains("hello"));
        assert!(!r.success());
    }

    #[tokio::test]
    async fn missing_program_is_reported() {
        let r = run(
            ExecRequest::new("definitely-not-a-real-binary-xyz", &[], "."),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(r.end, ExecEnd::SpawnFailed);
        assert!(r.stderr.contains("not found"));
    }

    #[tokio::test]
    async fn timeout_kills() {
        let script = if cfg!(windows) {
            "Start-Sleep -Seconds 30"
        } else {
            "sleep 30"
        };
        let r = run(shell(script).timeout(1), CancellationToken::new()).await;
        assert_eq!(r.end, ExecEnd::TimedOut);
        assert!(r.duration_ms < 15_000);
    }

    #[tokio::test]
    async fn cancellation_kills_quickly() {
        let script = if cfg!(windows) {
            "Start-Sleep -Seconds 30"
        } else {
            "sleep 30"
        };
        let c = CancellationToken::new();
        let c2 = c.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            c2.cancel();
        });
        let r = run(shell(script), c).await;
        assert_eq!(r.end, ExecEnd::Cancelled);
        assert!(r.duration_ms < 15_000);
    }

    #[tokio::test]
    async fn output_is_capped_and_redacted() {
        let script = if cfg!(windows) {
            "1..20000 | ForEach-Object { 'line of output' }; Write-Output 'token ghp_abcdefghijklmnopqrstuvwxyz0123456789'"
        } else {
            "for i in $(seq 1 20000); do echo 'line of output'; done; echo 'token ghp_abcdefghijklmnopqrstuvwxyz0123456789'"
        };
        let mut req = shell(script);
        req.max_output_bytes = 4096;
        let r = run(req, CancellationToken::new()).await;
        assert!(r.stdout.len() <= 4096);
        assert!(r.stdout_truncated_bytes > 0);
        let mut req2 = shell(if cfg!(windows) {
            "Write-Output 'ghp_abcdefghijklmnopqrstuvwxyz0123456789'"
        } else {
            "echo ghp_abcdefghijklmnopqrstuvwxyz0123456789"
        });
        req2.max_output_bytes = 4096;
        let r2 = run(req2, CancellationToken::new()).await;
        assert!(!r2.stdout.contains("ghp_abc"));
        assert_eq!(r2.redactions, 1);
    }
}
