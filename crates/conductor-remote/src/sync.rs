//! Hash-versioned file access and change streaming.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use conductor_security::paths::PathGuard;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn content_hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileVersion {
    pub path: String,
    pub hash: String,
    pub size: u64,
    /// Text content (None for binary or oversized files).
    pub content: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("{0}")]
    Path(#[from] conductor_security::paths::PathError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("conflict: file changed (now {current}), expected {expected}")]
    Conflict { current: String, expected: String },
    #[error("file too large for remote editing")]
    TooLarge,
}

pub const MAX_TEXT: u64 = 2 * 1024 * 1024;

pub fn read(guard: &PathGuard, rel: &str) -> Result<FileVersion, SyncError> {
    let p = guard.resolve_read(rel)?;
    let bytes = std::fs::read(&p)?;
    let size = bytes.len() as u64;
    let hash = content_hash(&bytes);
    let content = if size <= MAX_TEXT && !bytes.iter().take(8000).any(|b| *b == 0) {
        String::from_utf8(bytes).ok()
    } else {
        None
    };
    Ok(FileVersion {
        path: guard.relative(&p),
        hash,
        size,
        content,
    })
}

/// Write `content` only if the file's current hash equals `base_hash`
/// (`None` = the file must not exist yet). Never overwrites concurrent edits.
pub fn write(
    guard: &PathGuard,
    rel: &str,
    content: &str,
    base_hash: Option<&str>,
) -> Result<FileVersion, SyncError> {
    if content.len() as u64 > MAX_TEXT {
        return Err(SyncError::TooLarge);
    }
    let p = guard.resolve_write(rel)?;
    let current = std::fs::read(&p).ok().map(|b| content_hash(&b));
    match (current.as_deref(), base_hash) {
        (None, None) => {}
        (Some(c), Some(b)) if c == b => {}
        (cur, exp) => {
            return Err(SyncError::Conflict {
                current: cur.unwrap_or("missing").to_string(),
                expected: exp.unwrap_or("missing").to_string(),
            })
        }
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = p.with_extension(format!("conductor-tmp-{}", std::process::id()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, &p)?;
    Ok(FileVersion {
        path: guard.relative(&p),
        hash: content_hash(content.as_bytes()),
        size: content.len() as u64,
        content: None,
    })
}

/// Directory listing (one level), skipping heavy build folders.
pub fn list(guard: &PathGuard, rel: &str) -> Result<Vec<(String, bool, u64)>, SyncError> {
    let p = if rel.is_empty() || rel == "." {
        guard.root().to_path_buf()
    } else {
        guard.resolve_read(rel)?
    };
    let mut out = Vec::new();
    for e in std::fs::read_dir(&p)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        if matches!(
            name.as_str(),
            ".git" | "node_modules" | "target" | "dist" | ".conductor"
        ) {
            continue;
        }
        let md = e.metadata()?;
        out.push((guard.relative(&e.path()), md.is_dir(), md.len()));
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileChange {
    pub project: String,
    pub path: String,
    /// None when deleted.
    pub hash: Option<String>,
    /// Unified diff from the last known version when small and textual;
    /// otherwise clients fetch the file by hash.
    pub diff: Option<String>,
}

/// Bounded cache of last-seen text so changes can be sent as diffs.
struct TextCache {
    map: HashMap<PathBuf, String>,
    order: VecDeque<PathBuf>,
    cap: usize,
}

impl TextCache {
    fn put(&mut self, k: PathBuf, v: String) {
        if !self.map.contains_key(&k) {
            self.order.push_back(k.clone());
            if self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
        self.map.insert(k, v);
    }
}

pub fn unified_diff(old: &str, new: &str, path: &str) -> String {
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(2)
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}

/// Watches a project and calls `emit` for each changed file. Dropping the
/// returned watcher stops watching (used to idle when no client is
/// connected).
pub fn watch(
    project: String,
    root: &Path,
    emit: Arc<dyn Fn(FileChange) + Send + Sync>,
) -> notify::Result<RecommendedWatcher> {
    let root = root.to_path_buf();
    let cache = Arc::new(Mutex::new(TextCache {
        map: HashMap::new(),
        order: VecDeque::new(),
        cap: 256,
    }));
    // Trailing-edge debounce: handle a path once it has been quiet for 50 ms.
    // A save is often truncate-then-write; acting on the first event read the
    // empty file and dropped the write that followed, losing the final content.
    let (tx, rx) = std::sync::mpsc::channel::<(PathBuf, String)>();
    let root2 = root.clone();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        for p in ev.paths {
            let rel = p
                .strip_prefix(&root2)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            if rel.starts_with(".git/")
                || rel.contains("/node_modules/")
                || rel.starts_with("node_modules/")
                || rel.starts_with("target/")
                || rel.contains("conductor-tmp-")
            {
                continue;
            }
            let _ = tx.send((p, rel));
        }
    })?;
    std::thread::spawn(move || {
        use std::sync::mpsc::RecvTimeoutError;
        let quiet = Duration::from_millis(50);
        let mut pending: HashMap<PathBuf, (String, std::time::Instant)> = HashMap::new();
        loop {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok((p, rel)) => {
                    pending.insert(p, (rel, std::time::Instant::now()));
                }
                Err(RecvTimeoutError::Timeout) => {}
                // The watcher was dropped: flush what is left, then stop.
                Err(RecvTimeoutError::Disconnected) if pending.is_empty() => break,
                Err(RecvTimeoutError::Disconnected) => std::thread::sleep(quiet),
            }
            let now = std::time::Instant::now();
            let ready: Vec<PathBuf> = pending
                .iter()
                .filter(|(_, (_, at))| now.duration_since(*at) >= quiet)
                .map(|(p, _)| p.clone())
                .collect();
            for p in ready {
                let Some((rel, _)) = pending.remove(&p) else {
                    continue;
                };
                let change = match std::fs::read(&p) {
                    Ok(bytes) if p.is_file() => {
                        let hash = content_hash(&bytes);
                        let text = (bytes.len() < 256 * 1024 && !bytes.contains(&0))
                            .then(|| String::from_utf8_lossy(&bytes).into_owned());
                        let mut c = cache.lock().expect("cache lock");
                        let diff = match (&text, c.map.get(&p)) {
                            (Some(new), Some(old)) if old != new => {
                                Some(unified_diff(old, new, &rel))
                            }
                            (Some(_), Some(_)) => continue, // unchanged content
                            _ => None,
                        };
                        if let Some(t) = text {
                            c.put(p.clone(), t);
                        }
                        FileChange {
                            project: project.clone(),
                            path: rel,
                            hash: Some(hash),
                            diff,
                        }
                    }
                    Ok(_) => continue,
                    Err(_) if !p.exists() => FileChange {
                        project: project.clone(),
                        path: rel,
                        hash: None,
                        diff: None,
                    },
                    Err(_) => continue,
                };
                emit(change);
            }
        }
    });
    w.watch(&root, RecursiveMode::Recursive)?;
    Ok(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_writes_detect_conflicts() {
        let d = tempfile::tempdir().unwrap();
        let g = PathGuard::new(d.path()).unwrap();
        let v0 = write(&g, "notes/a.txt", "one", None).unwrap();
        assert!(
            matches!(
                write(&g, "notes/a.txt", "x", None),
                Err(SyncError::Conflict { .. })
            ),
            "create over existing"
        );
        let v1 = write(&g, "notes/a.txt", "two", Some(&v0.hash)).unwrap();
        // stale base -> conflict, file untouched
        assert!(matches!(
            write(&g, "notes/a.txt", "three", Some(&v0.hash)),
            Err(SyncError::Conflict { .. })
        ));
        let r = read(&g, "notes/a.txt").unwrap();
        assert_eq!(r.content.as_deref(), Some("two"));
        assert_eq!(r.hash, v1.hash);
        assert!(write(&g, "../escape.txt", "x", None).is_err());
        assert!(write(&g, ".git/config", "x", None).is_err());
        let l = list(&g, "").unwrap();
        assert_eq!(l[0], ("notes".to_string(), true, l[0].2));
    }

    #[test]
    fn diff_is_compact() {
        let old = (0..100).map(|i| format!("line {i}\n")).collect::<String>();
        let new = old.replace("line 50\n", "line fifty\n");
        let d = unified_diff(&old, &new, "f.txt");
        assert!(d.contains("-line 50") && d.contains("+line fifty"));
        assert!(d.len() < old.len() / 5);
    }

    #[test]
    fn watcher_reports_final_content_after_truncate_then_write() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("b.txt");
        std::fs::write(
            &f, "old
",
        )
        .unwrap();
        let got: Arc<Mutex<Vec<FileChange>>> = Arc::new(Mutex::new(vec![]));
        let g2 = got.clone();
        let _w = watch(
            "p".into(),
            d.path(),
            Arc::new(move |c| g2.lock().unwrap().push(c)),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        // An editor-style save: truncate, then write a moment later.
        drop(std::fs::File::create(&f).unwrap());
        std::thread::sleep(Duration::from_millis(5));
        std::fs::write(
            &f, "final
",
        )
        .unwrap();
        let want = content_hash(
            b"final
",
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if got.lock().unwrap().last().and_then(|c| c.hash.clone()) == Some(want.clone()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("final content never reported: {:?}", got.lock().unwrap());
    }

    #[test]
    fn watcher_emits_changes_with_diffs() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.txt");
        std::fs::write(&f, "hello\n").unwrap();
        let got: Arc<Mutex<Vec<FileChange>>> = Arc::new(Mutex::new(vec![]));
        let g2 = got.clone();
        let _w = watch(
            "p".into(),
            d.path(),
            Arc::new(move |c| g2.lock().unwrap().push(c)),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(&f, "hello\nworld\n").unwrap();
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(&f, "hello\nworld\nagain\n").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            {
                let g = got.lock().unwrap();
                if g.iter()
                    .any(|c| c.diff.as_deref().is_some_and(|d| d.contains("+again")))
                {
                    break;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no diff event: {:?}",
                got.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
