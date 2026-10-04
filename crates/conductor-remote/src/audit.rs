//! Remote session audit log (append-only, bounded, secrets never logged).

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AuditEntry<'a> {
    pub at: u64,
    pub event: &'a str,
    pub device: Option<&'a str>,
    pub detail: &'a str,
    pub peer: Option<&'a str>,
}

pub struct Audit {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Audit {
    const MAX_BYTES: u64 = 2 * 1024 * 1024;

    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn log(&self, event: &str, device: Option<&str>, detail: &str, peer: Option<&str>) {
        let _g = self.lock.lock();
        if let Some(p) = self.path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() > Self::MAX_BYTES) {
            let _ = std::fs::rename(&self.path, self.path.with_extension("jsonl.1"));
        }
        let entry = AuditEntry {
            at: crate::unix_now(),
            event,
            device,
            detail,
            peer,
        };
        if let Ok(mut line) = serde_json::to_vec(&entry) {
            line.push(b'\n');
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                let _ = f.write_all(&line);
            }
        }
    }

    pub fn tail(&self, n: usize) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let mut v: Vec<serde_json::Value> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let start = v.len().saturating_sub(n);
        v.drain(..start);
        v
    }
}
