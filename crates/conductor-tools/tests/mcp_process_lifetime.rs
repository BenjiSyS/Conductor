use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use conductor_tools::mcp::client::McpClient;

// Unique fixture directory and bounded lifetime make even a failing regression
// self-cleaning. No test enumerates or kills arbitrary processes.
const TREE_FIXTURE: &str = r#"
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const [dir, role] = process.argv.slice(2);
fs.writeFileSync(path.join(dir, role + '.pid'), String(process.pid));
if (role !== 'grandchild') {
  spawn(process.execPath, [__filename, dir, role === 'parent' ? 'child' : 'grandchild'],
    { stdio: 'ignore', windowsHide: true, detached: process.platform === 'win32' });
}
setInterval(() => fs.appendFileSync(path.join(dir, role + '.heartbeat'), 'x'), 25);
if (role === 'parent') setInterval(() => {
  if (fs.existsSync(path.join(dir, 'exit-parent'))) process.exit(0);
}, 10);
setTimeout(() => process.exit(0), 10000);
"#;

async fn fixture(dir: &Path) -> (McpClient, Witnesses) {
    assert!(
        conductor_tools::exec::resolve_program("node").is_some(),
        "node required for process lifetime regression"
    );
    let script = dir.join("owned-tree.js");
    std::fs::write(&script, TREE_FIXTURE).unwrap();
    let client = McpClient::spawn(
        "node",
        &[
            script.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            "parent".into(),
        ],
        &BTreeMap::new(),
        None,
        2,
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !dir.join("grandchild.heartbeat").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("three-level fixture ready");
    (client, Witnesses::capture(dir))
}

async fn assert_tree_stopped(dir: &Path, witnesses: Witnesses) {
    witnesses.assert_exited();
    // Allow termination and final already-running file writes to settle.
    tokio::time::sleep(Duration::from_millis(200)).await;
    for role in ["parent", "child", "grandchild"] {
        let path = dir.join(format!("{role}.heartbeat"));
        let before = std::fs::read(&path).unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            before,
            std::fs::read(&path).unwrap(),
            "owned {role} survived cleanup"
        );
    }
}

#[tokio::test]
async fn shutdown_stops_owned_children_and_grandchildren() {
    let dir = tempfile::tempdir().unwrap();
    let (client, witnesses) = fixture(dir.path()).await;
    client.shutdown().await;
    assert_tree_stopped(dir.path(), witnesses).await;
}

#[tokio::test]
async fn drop_stops_owned_children_and_grandchildren() {
    let dir = tempfile::tempdir().unwrap();
    let (client, witnesses) = fixture(dir.path()).await;
    drop(client);
    assert_tree_stopped(dir.path(), witnesses).await;
}

#[tokio::test]
async fn aborting_owner_stops_owned_children_and_grandchildren() {
    let dir = tempfile::tempdir().unwrap();
    let (client, witnesses) = fixture(dir.path()).await;
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(async move {
        let _client = client;
        ready_tx.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    ready_rx.await.unwrap();
    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    assert_tree_stopped(dir.path(), witnesses).await;
}

#[tokio::test]
async fn exited_parent_still_owns_children_and_grandchildren() {
    let dir = tempfile::tempdir().unwrap();
    let (client, witnesses) = fixture(dir.path()).await;
    std::fs::write(dir.path().join("exit-parent"), b"exit").unwrap();
    witnesses.assert_parent_exited();
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(client);
    assert_tree_stopped(dir.path(), witnesses).await;
}

#[test]
fn drop_after_runtime_shutdown_stops_owned_processes() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (client, witnesses) = runtime.block_on(fixture(dir.path()));
    runtime.shutdown_background();
    witnesses.assert_alive();
    drop(client);
    witnesses.assert_exited();
    std::thread::sleep(Duration::from_millis(200));
    let before = std::fs::read(dir.path().join("grandchild.heartbeat")).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        before,
        std::fs::read(dir.path().join("grandchild.heartbeat")).unwrap()
    );
}

#[tokio::test]
async fn cleanup_preserves_unrelated_owned_sibling() {
    struct Sibling(std::process::Child);
    impl Drop for Sibling {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let sibling_dir = tempfile::tempdir().unwrap();
    let (client, witnesses) = fixture(dir.path()).await;
    let node = conductor_tools::exec::resolve_program("node").unwrap();
    let mut command = std::process::Command::new(node);
    command.args([
        dir.path().join("owned-tree.js").as_os_str(),
        sibling_dir.path().as_os_str(),
        std::ffi::OsStr::new("grandchild"),
    ]);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let mut sibling = Sibling(command.spawn().unwrap());
    tokio::time::timeout(Duration::from_secs(2), async {
        while !sibling_dir.path().join("grandchild.heartbeat").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    client.shutdown().await;
    assert_tree_stopped(dir.path(), witnesses).await;
    assert!(
        sibling.0.try_wait().unwrap().is_none(),
        "unrelated owned process must remain alive"
    );
}

#[cfg(windows)]
struct Witnesses(Vec<(String, std::os::windows::io::OwnedHandle)>);

#[cfg(windows)]
impl Witnesses {
    fn capture(dir: &Path) -> Self {
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
                    // Observation only. Holding this exact process object prevents PID
                    // reuse from changing what the post-cleanup assertion observes.
                    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                    assert!(!raw.is_null(), "owned fixture {role} process must be alive");
                    assert_eq!(
                        unsafe { WaitForSingleObject(raw, 0) },
                        WAIT_TIMEOUT,
                        "owned fixture {role} exited before test cleanup"
                    );
                    (role.to_owned(), unsafe {
                        std::os::windows::io::OwnedHandle::from_raw_handle(raw)
                    })
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

    fn assert_parent_exited(&self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        assert_eq!(
            unsafe { WaitForSingleObject(self.0[0].1.as_raw_handle(), 2000) },
            WAIT_OBJECT_0,
            "fixture parent should exit before dropping its owner"
        );
    }

    fn assert_alive(&self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        for (role, handle) in &self.0 {
            assert_eq!(
                unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
                WAIT_TIMEOUT,
                "owned {role} must still be alive before Drop"
            );
        }
    }
}

#[cfg(not(windows))]
struct Witnesses;

#[cfg(not(windows))]
impl Witnesses {
    fn capture(_: &Path) -> Self {
        Self
    }
    fn assert_exited(&self) {}
    fn assert_parent_exited(&self) {}
    fn assert_alive(&self) {}
}
