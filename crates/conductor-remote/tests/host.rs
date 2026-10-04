use std::collections::BTreeMap;
use std::time::Duration;

use conductor_remote::client::{ClientError, RemoteClient};
use conductor_remote::{Host, HostConfig};
use serde_json::{json, Value};

async fn start() -> (tempfile::TempDir, conductor_remote::HostHandle) {
    let d = tempfile::tempdir().unwrap();
    let proj = d.path().join("proj");
    let other = d.path().join("other");
    std::fs::create_dir_all(proj.join("src")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(proj.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(proj.join(".env"), "SECRET=1\n").unwrap();
    let mut projects = BTreeMap::new();
    projects.insert("p1".to_string(), ("Proj".to_string(), proj));
    projects.insert("p2".to_string(), ("Other".to_string(), other));
    let h = Host::start(HostConfig {
        bind: "127.0.0.1".into(),
        port: 0,
        data_dir: d.path().join("host"),
        projects,
    })
    .await
    .unwrap();
    (d, h)
}

async fn next_of(rx: &mut tokio::sync::mpsc::Receiver<Value>, ty: &str) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        let v = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {ty}"))
            .expect("channel open");
        if v["type"] == ty {
            return v;
        }
    }
}

#[tokio::test]
async fn pairing_scoped_files_conflicts_and_state_channel() {
    let (d, mut h) = start().await;
    let addr = h.addr.to_string();

    // Wrong fingerprint: TLS refuses (encrypted + pinned).
    let mut evil = RemoteClient::new(&addr, "00:11").unwrap();
    assert!(matches!(
        evil.pair("X", "x").await,
        Err(ClientError::Connect(_))
    ));

    // Unpaired client is rejected.
    let mut c = RemoteClient::new(&addr, &h.fingerprint).unwrap();
    assert!(matches!(
        c.status().await,
        Err(ClientError::Http { status: 401, .. })
    ));
    assert!(matches!(
        c.pair("WRONGCOD", "Phone").await,
        Err(ClientError::Http { status: 401, .. })
    ));

    let code = h.new_pairing_code(vec!["p1".into()], true);
    c.pair(&code.code, "Phone").await.unwrap();
    let st = c.status().await.unwrap();
    assert_eq!(
        st["projects"].as_array().unwrap().len(),
        1,
        "scoped to p1 only"
    );

    // Files: read, conflict-safe write, scope, sensitive files, traversal.
    let f = c.read_file("p1", "src/main.rs").await.unwrap();
    assert_eq!(f["content"], "fn main() {}\n");
    let base = f["hash"].as_str().unwrap().to_string();
    c.write_file(
        "p1",
        "src/main.rs",
        "fn main() { println!(\"hi\"); }\n",
        Some(&base),
    )
    .await
    .unwrap();
    match c
        .write_file("p1", "src/main.rs", "stale edit\n", Some(&base))
        .await
    {
        Err(ClientError::Http { status: 409, .. }) => {}
        other => panic!("expected conflict, got {other:?}"),
    }
    assert!(std::fs::read_to_string(d.path().join("proj/src/main.rs"))
        .unwrap()
        .contains("hi"));
    assert!(matches!(
        c.read_file("p2", "x").await,
        Err(ClientError::Http { status: 403, .. })
    ));
    assert!(matches!(
        c.read_file("p1", ".env").await,
        Err(ClientError::Http { status: 403, .. })
    ));
    assert!(matches!(
        c.read_file("p1", "../other/x").await,
        Err(ClientError::Http { status: 403, .. })
    ));
    assert!(c
        .list("p1", "")
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["path"] == "src"));

    // Real-time channel.
    let (tx, mut rx) = c.connect().await.unwrap();
    let hello = next_of(&mut rx, "hello").await;
    assert_eq!(hello["device"], "Phone");
    assert_eq!(h.connected_clients(), 1);
    h.publish(json!({ "type": "task", "project": "p1", "text": "Implementing lobby" }));
    h.publish(json!({ "type": "task", "project": "p2", "text": "hidden from this device" }));
    h.publish(json!({ "type": "goal_progress", "done": 2, "total": 5 }));
    let t = next_of(&mut rx, "task").await;
    assert_eq!(t["text"], "Implementing lobby");
    let g = next_of(&mut rx, "goal_progress").await;
    assert_eq!(g["done"], 2);

    // Local file change appears remotely as a diff.
    tokio::time::sleep(Duration::from_millis(400)).await;
    std::fs::write(
        d.path().join("proj/src/main.rs"),
        "fn main() { println!(\"hi\"); }\n// local edit\n",
    )
    .unwrap();
    let fc = next_of(&mut rx, "file_changed").await;
    assert_eq!(fc["path"], "src/main.rs");

    // Client actions reach the integrator.
    tx.send(json!({ "type": "prompt", "text": "add tests" }))
        .await
        .unwrap();
    let inbound = tokio::time::timeout(Duration::from_secs(5), h.inbound.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inbound.kind, "prompt");
    assert_eq!(inbound.device_name, "Phone");

    // Revocation closes the live session and blocks the token.
    let dev_id = h.devices()[0].id.clone();
    assert!(h.revoke(&dev_id));
    next_of(&mut rx, "revoked").await;
    assert!(matches!(
        c.status().await,
        Err(ClientError::Http { status: 401, .. })
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.connected_clients(), 0);

    let audit = h.audit_tail(50);
    for ev in [
        "device_paired",
        "client_connected",
        "device_revoked",
        "file_written",
    ] {
        assert!(
            audit.iter().any(|a| a["event"] == ev),
            "missing audit event {ev}"
        );
    }
    h.shutdown();
}

#[tokio::test]
async fn view_only_device_cannot_write_or_control() {
    let (_d, h) = start().await;
    let mut c = RemoteClient::new(&h.addr.to_string(), &h.fingerprint).unwrap();
    let code = h.new_pairing_code(vec!["p1".into()], false);
    c.pair(&code.code, "Tablet").await.unwrap();
    assert!(matches!(
        c.write_file("p1", "x.txt", "x", None).await,
        Err(ClientError::Http { status: 403, .. })
    ));
    let (tx, mut rx) = c.connect().await.unwrap();
    next_of(&mut rx, "hello").await;
    tx.send(json!({ "type": "prompt", "text": "x" }))
        .await
        .unwrap();
    let e = next_of(&mut rx, "error").await;
    assert!(e["error"].as_str().unwrap().contains("view-only"));
}

#[tokio::test]
async fn devices_survive_host_restart() {
    let (d, h) = start().await;
    let mut c = RemoteClient::new(&h.addr.to_string(), &h.fingerprint).unwrap();
    let code = h.new_pairing_code(vec!["p1".into()], true);
    c.pair(&code.code, "Laptop").await.unwrap();
    let fp = h.fingerprint.clone();
    h.shutdown();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut projects = BTreeMap::new();
    projects.insert(
        "p1".to_string(),
        ("Proj".to_string(), d.path().join("proj")),
    );
    let h2 = Host::start(HostConfig {
        bind: "127.0.0.1".into(),
        port: 0,
        data_dir: d.path().join("host"),
        projects,
    })
    .await
    .unwrap();
    assert_eq!(h2.fingerprint, fp, "same identity after restart");
    let mut c2 = RemoteClient::new(&h2.addr.to_string(), &fp).unwrap();
    c2.token = c.token.clone();
    assert_eq!(c2.status().await.unwrap()["device"], "Laptop");
}
