//! Regression for terminal/plugin programs whose parent exits while descendants
//! retain stdout. All fixture children expire after eight seconds, even on a
//! failing baseline; no unrelated process is enumerated or terminated.
use std::time::{Duration, Instant};

use conductor_tools::exec::{run, ExecEnd, ExecRequest};
use tokio_util::sync::CancellationToken;

// Each tree uses three Node processes plus pipe workers. Bound fixture startup
// contention while retaining all deadlines and termination assertions.
static FIXTURE_LIMIT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[tokio::test]
async fn exited_parent_cannot_leave_descendants_holding_output_open() {
    let _permit = FIXTURE_LIMIT.acquire().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("exec-tree.js");
    std::fs::write(
        &script,
        r#"
const {spawn} = require('child_process');
if (process.argv[2] === 'child') {
  console.log('descendant started');
  setInterval(() => {}, 20);
  setTimeout(() => process.exit(0), 8000);
} else {
  spawn(process.execPath, [__filename, 'child'], {
    stdio: ['ignore', process.stdout, process.stderr],
    windowsHide: true, detached: process.platform === 'win32'
  });
  process.exit(0);
}

"#,
    )
    .unwrap();
    let node = conductor_tools::exec::resolve_program("node").expect("node required");
    let mut request = ExecRequest::new(node.to_string_lossy(), &[], dir.path());
    request.args = vec![script.to_string_lossy().into_owned()];
    request.timeout_secs = Some(1);
    let began = Instant::now();
    let result = run(request, CancellationToken::new()).await;
    assert_eq!(result.end, ExecEnd::Exited);
    assert_eq!(result.code, Some(0));
    assert!(
        began.elapsed() < Duration::from_secs(3),
        "parent exited but descendant held output beyond timeout: {:?}",
        began.elapsed()
    );
}

const TREE: &str = r#"
const fs = require('fs'), path = require('path'), {spawn} = require('child_process');
const [dir, role] = process.argv.slice(2);
fs.writeFileSync(path.join(dir, role + '.pid'), String(process.pid));
console.log(role + ' buffered stdout');
console.error(role + ' buffered stderr');
if (role !== 'grandchild') spawn(process.execPath,
  [__filename, dir, role === 'parent' ? 'child' : 'grandchild'],
  {stdio: ['ignore', process.stdout, process.stderr], windowsHide: true,
   detached: process.platform === 'win32'});
setInterval(() => fs.appendFileSync(path.join(dir, role + '.heartbeat'), 'x'), 20);
if (role === 'parent') setInterval(() => {
  if (fs.existsSync(path.join(dir, 'exit-parent'))) process.exit(3);
}, 10);
setTimeout(() => process.exit(0), 8000);
"#;

fn tree_request(dir: &std::path::Path) -> ExecRequest {
    let script = dir.join("owned-exec-tree.js");
    std::fs::write(&script, TREE).unwrap();
    let node = conductor_tools::exec::resolve_program("node").expect("node required");
    let mut request = ExecRequest::new(node.to_string_lossy(), &[], dir);
    request.args = vec![
        script.to_string_lossy().into_owned(),
        dir.to_string_lossy().into_owned(),
        "parent".into(),
    ];
    request.timeout_secs = Some(5);
    request
}

async fn ready(dir: &std::path::Path) -> Witnesses {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !dir.join("grandchild.heartbeat").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned three-level exec fixture ready");
    Witnesses::capture(dir)
}

async fn stopped(dir: &std::path::Path, witnesses: Witnesses) {
    witnesses.assert_exited();
    tokio::time::sleep(Duration::from_millis(100)).await;
    for role in ["parent", "child", "grandchild"] {
        let path = dir.join(format!("{role}.heartbeat"));
        let before = std::fs::read(&path).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            before,
            std::fs::read(&path).unwrap(),
            "owned {role} survived"
        );
    }
}

#[tokio::test]
async fn natural_exit_ends_owned_tree_and_preserves_status_and_buffered_output() {
    let _permit = FIXTURE_LIMIT.acquire().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let request = tree_request(dir.path());
    let call = tokio::spawn(run(request, CancellationToken::new()));
    let witnesses = ready(dir.path()).await;
    std::fs::write(dir.path().join("exit-parent"), b"exit").unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.end, ExecEnd::Exited);
    assert_eq!(result.code, Some(3));
    assert!(result.stdout.contains("parent buffered stdout"));
    assert!(result.stderr.contains("parent buffered stderr"));
    stopped(dir.path(), witnesses).await;
}

#[tokio::test]
async fn timeout_ends_owned_children_and_grandchildren() {
    let _permit = FIXTURE_LIMIT.acquire().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut request = tree_request(dir.path());
    request.timeout_secs = Some(3);
    let began = Instant::now();
    let call = tokio::spawn(run(request, CancellationToken::new()));
    let witnesses = ready(dir.path()).await;
    let result = call.await.unwrap();
    assert_eq!(result.end, ExecEnd::TimedOut);
    assert!(began.elapsed() < Duration::from_secs(5));
    stopped(dir.path(), witnesses).await;
}

#[tokio::test]
async fn cancellation_ends_tree_even_with_backpressured_stdin() {
    let _permit = FIXTURE_LIMIT.acquire().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut request = tree_request(dir.path());
    request.stdin = Some("input".repeat(2 * 1024 * 1024));
    let cancel = CancellationToken::new();
    let call = tokio::spawn(run(request, cancel.clone()));
    let witnesses = ready(dir.path()).await;
    let began = Instant::now();
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.end, ExecEnd::Cancelled);
    assert!(began.elapsed() < Duration::from_secs(2));
    assert!(result.stdout.contains("parent buffered stdout"));
    stopped(dir.path(), witnesses).await;
}

#[tokio::test]
async fn aborting_exec_future_ends_owned_tree() {
    let _permit = FIXTURE_LIMIT.acquire().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let call = tokio::spawn(run(tree_request(dir.path()), CancellationToken::new()));
    let witnesses = ready(dir.path()).await;
    call.abort();
    assert!(call.await.unwrap_err().is_cancelled());
    stopped(dir.path(), witnesses).await;
}

#[tokio::test]
async fn precancelled_exec_never_spawns() {
    let dir = tempfile::tempdir().unwrap();
    let request = tree_request(dir.path());
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = run(request, cancel).await;
    assert_eq!(result.end, ExecEnd::Cancelled);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!dir.path().join("parent.pid").exists());
}

#[tokio::test]
async fn finite_stdin_and_large_output_keep_correct_status_and_caps() {
    let _permit = FIXTURE_LIMIT.acquire().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("stdin-output.js");
    std::fs::write(&script, r#"
let n = 0;
process.stdin.on('data', data => n += data.length);
process.stdin.on('end', () => {
  const payload = String(n) + '\n' + 'x'.repeat(50000);
  process.stdout.write(payload, () => process.stderr.write('buffered stderr', () => process.exit(3)));
});
"#).unwrap();
    let node = conductor_tools::exec::resolve_program("node").unwrap();
    let mut request = ExecRequest::new(node.to_string_lossy(), &[], dir.path());
    request.args = vec![script.to_string_lossy().into_owned()];
    request.stdin = Some("a".repeat(128 * 1024));
    request.max_output_bytes = 4096;
    let result = run(request, CancellationToken::new()).await;
    assert_eq!(result.end, ExecEnd::Exited);
    assert_eq!(result.code, Some(3));
    assert!(result.stdout.starts_with("131072\n"));
    assert_eq!(result.stdout.len(), 4096);
    assert_eq!(result.stdout_truncated_bytes, 50007 - 4096);
    assert_eq!(result.stderr, "buffered stderr");
}

#[tokio::test]
async fn exec_cleanup_preserves_unrelated_owned_sibling() {
    let _permit = FIXTURE_LIMIT.acquire().await.unwrap();
    struct Sibling(std::process::Child);
    impl Drop for Sibling {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let node = conductor_tools::exec::resolve_program("node").unwrap();
    let mut command = std::process::Command::new(node);
    command
        .args([
            "-e",
            "setInterval(() => {}, 20); setTimeout(() => process.exit(0), 8000)",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut sibling = Sibling(command.spawn().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let call = tokio::spawn(run(tree_request(dir.path()), cancel.clone()));
    let witnesses = ready(dir.path()).await;
    cancel.cancel();
    assert_eq!(call.await.unwrap().end, ExecEnd::Cancelled);
    stopped(dir.path(), witnesses).await;
    assert!(
        sibling.0.try_wait().unwrap().is_none(),
        "unrelated sibling killed"
    );
}

#[cfg(windows)]
struct Witnesses(Vec<(String, std::os::windows::io::OwnedHandle)>);

#[cfg(windows)]
impl Witnesses {
    fn capture(dir: &std::path::Path) -> Self {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        Self(
            ["parent", "child", "grandchild"]
                .into_iter()
                .map(|role| {
                    let pid = std::fs::read_to_string(dir.join(format!("{role}.pid")))
                        .unwrap()
                        .parse()
                        .unwrap();
                    // Exact observation handles only; never use these IDs to kill.
                    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                    assert!(!raw.is_null());
                    let handle = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(raw) };
                    assert_eq!(
                        unsafe { WaitForSingleObject(raw, 0) },
                        WAIT_TIMEOUT,
                        "owned fixture {role} must be alive before cleanup"
                    );
                    (role.into(), handle)
                })
                .collect(),
        )
    }
    fn assert_exited(&self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        for (role, handle) in &self.0 {
            assert_eq!(
                unsafe { WaitForSingleObject(handle.as_raw_handle(), 2000) },
                WAIT_OBJECT_0,
                "owned {role} survived cleanup"
            );
        }
    }
}

#[cfg(not(windows))]
struct Witnesses;
#[cfg(not(windows))]
impl Witnesses {
    fn capture(_: &std::path::Path) -> Self {
        Self
    }
    fn assert_exited(&self) {}
}
