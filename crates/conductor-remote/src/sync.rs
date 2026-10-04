//! Hash-versioned file access and change streaming.

use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
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
const WATCH_HASH_CHUNK: usize = 64 * 1024;
const WATCH_TEXT_LIMIT: usize = 256 * 1024;

static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn path_write_lock() -> MutexGuard<'static, ()> {
    WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn read_text_bounded(path: &Path) -> Result<Vec<u8>, SyncError> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX_TEXT {
        return Err(SyncError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(size.min(MAX_TEXT) as usize);
    file.take(MAX_TEXT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TEXT {
        return Err(SyncError::TooLarge);
    }
    Ok(bytes)
}

fn current_hash(path: &Path) -> Result<Option<String>, SyncError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::with_capacity(file.metadata()?.len().min(MAX_TEXT) as usize);
    file.take(MAX_TEXT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TEXT {
        return Err(SyncError::TooLarge);
    }
    Ok(Some(content_hash(&bytes)))
}

fn watch_hash_and_text(path: &Path) -> std::io::Result<(String, Option<String>)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut content = Vec::with_capacity(WATCH_TEXT_LIMIT);
    let mut size = 0usize;
    let mut keep_text = true;
    let mut buffer = [0u8; WATCH_HASH_CHUNK];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        hasher.update(chunk);
        size = size.saturating_add(read);
        if keep_text && size <= WATCH_TEXT_LIMIT && !chunk.contains(&0) {
            content.extend_from_slice(chunk);
        } else {
            keep_text = false;
            content.clear();
        }
    }
    let text = keep_text.then(|| String::from_utf8_lossy(&content).into_owned());
    Ok((hex::encode(hasher.finalize()), text))
}

struct OwnedTemp {
    path: PathBuf,
    file: Option<File>,
}

impl OwnedTemp {
    fn create(parent: &Path) -> Result<Self, SyncError> {
        for _ in 0..8 {
            let path = parent.join(format!(
                ".conductor-tmp-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create a unique remote-write temporary file",
        )
        .into())
    }

    fn write_all_and_sync(&mut self, content: &[u8]) -> Result<(), SyncError> {
        let file = self.file.as_mut().expect("owned temp file is open");
        file.write_all(content)?;
        file.sync_all()?;
        self.file.take();
        Ok(())
    }

    fn replace(mut self, destination: &Path) -> Result<(), SyncError> {
        std::fs::rename(&self.path, destination)?;
        self.file.take();
        self.path.clear();
        Ok(())
    }
}

impl Drop for OwnedTemp {
    fn drop(&mut self) {
        self.file.take();
        if !self.path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub fn read(guard: &PathGuard, rel: &str) -> Result<FileVersion, SyncError> {
    let p = guard.resolve_read(rel)?;
    let bytes = read_text_bounded(&p)?;
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
/// (`None` = the file must not exist yet). Host-process writes serialize and
/// recheck immediately before replacement. Uncoordinated external editors can
/// still modify the file between that final check and the atomic rename.
pub fn write(
    guard: &PathGuard,
    rel: &str,
    content: &str,
    base_hash: Option<&str>,
) -> Result<FileVersion, SyncError> {
    write_with_hook(guard, rel, content, base_hash, || {})
}

fn write_with_hook(
    guard: &PathGuard,
    rel: &str,
    content: &str,
    base_hash: Option<&str>,
    before_final_check: impl FnOnce(),
) -> Result<FileVersion, SyncError> {
    if content.len() as u64 > MAX_TEXT {
        return Err(SyncError::TooLarge);
    }
    let p = guard.resolve_write(rel)?;
    let _lock = path_write_lock();
    let current = current_hash(&p)?;
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
    let parent = p.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination has no parent",
        )
    })?;
    let mut tmp = OwnedTemp::create(parent)?;
    tmp.write_all_and_sync(content.as_bytes())?;
    before_final_check();
    let current = current_hash(&p)?;
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
    tmp.replace(&p)?;
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
                let change = match p.is_file().then(|| watch_hash_and_text(&p)) {
                    Some(Ok((hash, text))) => {
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
                    Some(Err(_)) if !p.exists() => FileChange {
                        project: project.clone(),
                        path: rel,
                        hash: None,
                        diff: None,
                    },
                    _ => continue,
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
    fn concurrent_updates_with_one_base_have_one_winner() {
        let d = tempfile::tempdir().unwrap();
        let guard = PathGuard::new(d.path()).unwrap();
        let initial = write(&guard, "shared.txt", "base", None).unwrap();
        let workers = 8;
        let barrier = Arc::new(std::sync::Barrier::new(workers));
        let handles: Vec<_> = (0..workers)
            .map(|index| {
                let guard = guard.clone();
                let barrier = barrier.clone();
                let base_hash = initial.hash.clone();
                std::thread::spawn(move || {
                    let content = format!("concurrent update {index}");
                    barrier.wait();
                    write(&guard, "shared.txt", &content, Some(&base_hash))
                        .map(|version| (content, version))
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        let winners: Vec<_> = results
            .iter()
            .filter_map(|result| result.as_ref().ok())
            .collect();
        let conflicts = results
            .iter()
            .filter(|result| matches!(result, Err(SyncError::Conflict { .. })))
            .count();
        assert_eq!(winners.len(), 1);
        assert_eq!(conflicts, workers - 1);
        let final_version = read(&guard, "shared.txt").unwrap();
        assert_eq!(
            Some(final_version.hash.as_str()),
            Some(winners[0].1.hash.as_str())
        );
        assert_eq!(
            final_version.content.as_deref(),
            Some(winners[0].0.as_str())
        );
    }

    #[cfg(windows)]
    #[test]
    fn new_file_writes_with_mixed_case_paths_share_the_host_lock() {
        let d = tempfile::tempdir().unwrap();
        let guard = PathGuard::new(d.path()).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["NewFile.txt", "newfile.TXT"]
            .into_iter()
            .enumerate()
            .map(|(index, path)| {
                let guard = guard.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let content = format!("writer {index}");
                    barrier.wait();
                    write(&guard, path, &content, None).map(|version| (content, version))
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        let winner = results
            .iter()
            .filter_map(|result| result.as_ref().ok())
            .next()
            .expect("one case-aliased writer succeeds");
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(SyncError::Conflict { .. })))
                .count(),
            1
        );
        let actual = read(&guard, "NEWFILE.TXT").unwrap();
        assert_eq!(actual.content.as_deref(), Some(winner.0.as_str()));
    }

    #[test]
    fn failed_final_check_cleans_only_our_temp_and_preserves_legacy_pid_temp() {
        let d = tempfile::tempdir().unwrap();
        let guard = PathGuard::new(d.path()).unwrap();
        let initial = write(&guard, "notes.txt", "base", None).unwrap();
        let destination = d.path().join("notes.txt");
        let legacy_temp =
            destination.with_extension(format!("conductor-tmp-{}", std::process::id()));
        std::fs::write(&legacy_temp, "legacy sentinel").unwrap();

        let result = write_with_hook(
            &guard,
            "notes.txt",
            "remote update",
            Some(&initial.hash),
            || std::fs::write(&destination, "external update").unwrap(),
        );
        assert!(matches!(result, Err(SyncError::Conflict { .. })));
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "external update"
        );
        assert_eq!(
            std::fs::read_to_string(&legacy_temp).unwrap(),
            "legacy sentinel"
        );
        assert!(std::fs::read_dir(d.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".conductor-tmp-")
        }));
    }

    #[test]
    fn reads_and_conflict_checks_are_bounded_and_io_errors_are_not_conflicts() {
        let d = tempfile::tempdir().unwrap();
        let guard = PathGuard::new(d.path()).unwrap();
        let large = vec![b'x'; MAX_TEXT as usize + 1];
        std::fs::write(d.path().join("large.txt"), &large).unwrap();
        assert!(matches!(
            read(&guard, "large.txt"),
            Err(SyncError::TooLarge)
        ));
        assert!(matches!(
            write(&guard, "large.txt", "replacement", Some("old-hash")),
            Err(SyncError::TooLarge)
        ));

        std::fs::create_dir(d.path().join("folder")).unwrap();
        assert!(matches!(read(&guard, "folder"), Err(SyncError::Io(_))));
        assert!(matches!(
            write(&guard, "folder", "text", None),
            Err(SyncError::Io(_))
        ));
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
