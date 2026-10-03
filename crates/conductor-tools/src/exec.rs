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
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

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
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: don't flash console windows from the GUI app.
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(unix)]
    {
        // New process group so we can kill the whole tree.
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return fail(format!("could not start '{}': {e}", req.program)),
    };
    if let Some(input) = req.stdin.clone() {
        if let Some(mut si) = child.stdin.take() {
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;
                let _ = si.write_all(input.as_bytes()).await;
            });
        }
    }
    let pid = child.id();
    let out = child
        .stdout
        .take()
        .map(|s| tokio::spawn(read_capped(s, req.max_output_bytes)));
    let err = child
        .stderr
        .take()
        .map(|s| tokio::spawn(read_capped(s, req.max_output_bytes)));
    let timeout = req
        .timeout_secs
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(24 * 3600));

    let (end, code) = tokio::select! {
        status = child.wait() => match status {
            Ok(s) => (ExecEnd::Exited, s.code()),
            Err(_) => (ExecEnd::Exited, None),
        },
        _ = tokio::time::sleep(timeout) => {
            kill_tree(pid, &mut child).await;
            (ExecEnd::TimedOut, None)
        }
        _ = cancel.cancelled() => {
            kill_tree(pid, &mut child).await;
            (ExecEnd::Cancelled, None)
        }
    };
    let (stdout, so_trunc) = match out {
        Some(h) => h.await.unwrap_or_default(),
        None => (String::new(), 0),
    };
    let (stderr, se_trunc) = match err {
        Some(h) => h.await.unwrap_or_default(),
        None => (String::new(), 0),
    };
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
pub(crate) fn command_for(exe: &Path, args: &[String]) -> Result<Command, String> {
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

async fn read_capped<R: AsyncRead + Unpin>(mut r: R, cap: usize) -> (String, usize) {
    let mut buf = Vec::with_capacity(cap.min(64 * 1024));
    let mut chunk = [0u8; 8192];
    let mut dropped = 0usize;
    loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = cap.saturating_sub(buf.len());
                if room >= n {
                    buf.extend_from_slice(&chunk[..n]);
                } else {
                    buf.extend_from_slice(&chunk[..room]);
                    dropped += n - room;
                }
            }
        }
    }
    (String::from_utf8_lossy(&buf).into_owned(), dropped)
}

async fn kill_tree(pid: Option<u32>, child: &mut tokio::process::Child) {
    #[cfg(windows)]
    if let Some(pid) = pid {
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags_compat()
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    if let Some(pid) = pid {
        // Negative pid = process group.
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .status();
    }
    let _ = child.kill().await;
}

#[cfg(windows)]
trait CreationFlagsCompat {
    fn creation_flags_compat(&mut self) -> &mut Self;
}
#[cfg(windows)]
impl CreationFlagsCompat for std::process::Command {
    fn creation_flags_compat(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000)
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
