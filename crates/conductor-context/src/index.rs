//! Incremental repository index + Smart Context Cache.
//!
//! The index walks the project respecting `.gitignore`, records per-file
//! metadata (size, mtime, SHA-256, language, outline) and persists it to a
//! cache file. On refresh:
//!
//! * unchanged size+mtime → entry reused without reading the file (cache hit);
//! * changed metadata but identical hash → metadata refreshed, outline reused;
//! * changed hash → outline recomputed for that file only;
//! * deleted files → entries removed.
//!
//! Nothing is thrown away wholesale, and file contents are never kept in
//! memory — they are read lazily when a context pack needs them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::symbols::{self, Lang, Outline};
use crate::tokens;

const CACHE_SCHEMA: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cache error: {0}")]
    Cache(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub mtime_ns: u128,
    pub hash: String,
    pub lang: Lang,
    pub tokens: usize,
    pub outline: Outline,
    /// First meaningful comment/doc lines, used as a cheap summary.
    pub summary: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStats {
    pub files: usize,
    pub cache_hits: usize,
    pub rehashed_unchanged: usize,
    pub reparsed: usize,
    pub removed: usize,
    pub skipped_large: usize,
    pub skipped_binary: usize,
    pub total_tokens: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct IndexOptions {
    pub max_file_bytes: u64,
    pub threads: usize,
    /// Extra ignore globs (gitignore syntax) from conductor.toml.
    pub ignore: Vec<String>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            max_file_bytes: 1024 * 1024,
            threads: 2,
            ignore: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    schema: u32,
    root: String,
    entries: Vec<FileEntry>,
}

#[derive(Debug, Clone)]
pub struct RepoIndex {
    root: PathBuf,
    cache_path: Option<PathBuf>,
    entries: BTreeMap<String, FileEntry>,
    pub last_stats: IndexStats,
}

/// Directories that are never indexed regardless of .gitignore.
const ALWAYS_SKIP: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".svelte-kit",
    "__pycache__",
    ".venv",
    "venv",
    ".godot",
    "Library",
    "Temp",
    "obj",
    "bin",
    ".gradle",
    ".idea",
    ".vs",
    "coverage",
    ".turbo",
    ".cache",
];

impl RepoIndex {
    /// Create an index for `root`, loading the persisted cache if present and
    /// valid. A corrupt or mismatched cache is discarded (it is only a cache).
    pub fn open(root: impl Into<PathBuf>, cache_path: Option<PathBuf>) -> Self {
        let root = root.into();
        let mut entries = BTreeMap::new();
        if let Some(cp) = &cache_path {
            if let Ok(bytes) = std::fs::read(cp) {
                match serde_json::from_slice::<CacheFile>(&bytes) {
                    Ok(c) if c.schema == CACHE_SCHEMA && c.root == root.to_string_lossy() => {
                        for e in c.entries {
                            entries.insert(e.path.clone(), e);
                        }
                    }
                    Ok(_) => tracing::info!("context cache schema/root changed; rebuilding"),
                    Err(e) => tracing::warn!(error = %e, "context cache unreadable; rebuilding"),
                }
            }
        }
        Self {
            root,
            cache_path,
            entries,
            last_stats: IndexStats::default(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn entries(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.values()
    }

    pub fn get(&self, rel: &str) -> Option<&FileEntry> {
        self.entries.get(rel)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Walk the tree and update entries incrementally.
    pub fn refresh(&mut self, opts: &IndexOptions) -> Result<IndexStats, IndexError> {
        let started = Instant::now();
        let mut stats = IndexStats::default();
        let files = self.walk(opts, &mut stats)?;

        // Partition into reused vs needs-read.
        let mut todo: Vec<(String, PathBuf, u64, u128)> = Vec::new();
        let mut seen = BTreeSet::new();
        for (rel, abs, size, mtime) in files {
            seen.insert(rel.clone());
            match self.entries.get(&rel) {
                Some(e) if e.size == size && e.mtime_ns == mtime => stats.cache_hits += 1,
                _ => todo.push((rel, abs, size, mtime)),
            }
        }

        let threads = opts.threads.max(1).min(todo.len().max(1));
        let results: Vec<Option<ReadItem>> = if threads <= 1 {
            todo.into_iter().map(read_one).collect()
        } else {
            let chunk = todo.len().div_ceil(threads);
            let mut parts: Vec<Vec<WalkItem>> = Vec::new();
            let mut it = todo.into_iter().peekable();
            while it.peek().is_some() {
                parts.push(it.by_ref().take(chunk).collect());
            }
            std::thread::scope(|s| {
                let handles: Vec<_> = parts
                    .into_iter()
                    .map(|p| s.spawn(move || p.into_iter().map(read_one).collect::<Vec<_>>()))
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect()
            })
        };

        for (rel, bytes, size, mtime) in results.into_iter().flatten() {
            if is_binary(&bytes) {
                stats.skipped_binary += 1;
                self.entries.remove(&rel);
                seen.remove(&rel);
                continue;
            }
            let hash = hex::encode(Sha256::digest(&bytes));
            if let Some(existing) = self.entries.get_mut(&rel) {
                if existing.hash == hash {
                    existing.size = size;
                    existing.mtime_ns = mtime;
                    stats.rehashed_unchanged += 1;
                    continue;
                }
            }
            let text = String::from_utf8_lossy(&bytes);
            let lang = Lang::from_path(&rel);
            let outline = symbols::outline(lang, &text);
            let entry = FileEntry {
                summary: summarize(lang, &text),
                tokens: tokens::estimate(&text),
                sensitive: conductor_security::secrets::is_sensitive_path(&rel),
                path: rel.clone(),
                size,
                mtime_ns: mtime,
                hash,
                lang,
                outline,
            };
            self.entries.insert(rel, entry);
            stats.reparsed += 1;
        }

        let before = self.entries.len();
        self.entries.retain(|k, _| seen.contains(k));
        stats.removed = before - self.entries.len();
        stats.files = self.entries.len();
        stats.total_tokens = self.entries.values().map(|e| e.tokens).sum();
        stats.elapsed_ms = started.elapsed().as_millis();
        self.last_stats = stats.clone();
        Ok(stats)
    }

    fn walk(
        &self,
        opts: &IndexOptions,
        stats: &mut IndexStats,
    ) -> Result<Vec<(String, PathBuf, u64, u128)>, IndexError> {
        let mut builder = ignore::WalkBuilder::new(&self.root);
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(true)
            .require_git(false)
            .follow_links(false)
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !(e.file_type().is_some_and(|t| t.is_dir()) && ALWAYS_SKIP.contains(&name.as_ref()))
            });
        if !opts.ignore.is_empty() {
            let mut ob = ignore::overrides::OverrideBuilder::new(&self.root);
            for g in &opts.ignore {
                let _ = ob.add(&format!("!{g}"));
            }
            if let Ok(o) = ob.build() {
                builder.overrides(o);
            }
        }
        let mut out = Vec::new();
        for entry in builder.build().flatten() {
            let Some(ft) = entry.file_type() else {
                continue;
            };
            if !ft.is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let rel = entry
                .path()
                .strip_prefix(&self.root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            if meta.len() > opts.max_file_bytes {
                stats.skipped_large += 1;
                continue;
            }
            if is_binary_ext(&rel) {
                stats.skipped_binary += 1;
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            out.push((rel, entry.path().to_path_buf(), meta.len(), mtime));
        }
        Ok(out)
    }

    /// Persist the cache atomically (temp file + rename).
    pub fn save(&self) -> Result<(), IndexError> {
        let Some(cp) = &self.cache_path else {
            return Ok(());
        };
        if let Some(parent) = cp.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = CacheFile {
            schema: CACHE_SCHEMA,
            root: self.root.to_string_lossy().to_string(),
            entries: self.entries.values().cloned().collect(),
        };
        let bytes = serde_json::to_vec(&file).map_err(|e| IndexError::Cache(e.to_string()))?;
        let tmp = cp.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, cp)?;
        Ok(())
    }

    /// Read a file's current text (lazily, never cached in memory).
    pub fn read(&self, rel: &str) -> Option<String> {
        let p = self.root.join(rel);
        std::fs::read(&p)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// Files whose imports reference `rel`, and files `rel` imports.
    pub fn neighbors(&self, rel: &str) -> Vec<String> {
        let mut out = BTreeSet::new();
        if let Some(e) = self.entries.get(rel) {
            for imp in &e.outline.imports {
                for target in self.resolve_import(rel, e.lang, imp) {
                    out.insert(target);
                }
            }
        }
        for other in self.entries.values() {
            if other.path == rel {
                continue;
            }
            for imp in &other.outline.imports {
                if self
                    .resolve_import(&other.path, other.lang, imp)
                    .iter()
                    .any(|t| t == rel)
                {
                    out.insert(other.path.clone());
                }
            }
        }
        out.into_iter().collect()
    }

    fn resolve_import(&self, from: &str, lang: Lang, spec: &str) -> Vec<String> {
        let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let mut cands: Vec<String> = Vec::new();
        match lang {
            Lang::TypeScript | Lang::JavaScript | Lang::Svelte | Lang::Vue => {
                if spec.starts_with('.') {
                    let base = normalize(&join(dir, spec));
                    for ext in [
                        "",
                        ".ts",
                        ".tsx",
                        ".js",
                        ".jsx",
                        ".mjs",
                        ".svelte",
                        ".vue",
                        "/index.ts",
                        "/index.js",
                    ] {
                        cands.push(format!("{base}{ext}"));
                    }
                }
            }
            Lang::Rust => {
                let last = spec.rsplit("::").next().unwrap_or(spec);
                let first = spec.split("::").next().unwrap_or(spec);
                let first = if first == "crate" || first == "self" || first == "super" {
                    spec.split("::").nth(1).unwrap_or(last)
                } else {
                    first
                };
                for name in [last, first] {
                    cands.push(normalize(&join(dir, &format!("{name}.rs"))));
                    cands.push(normalize(&join(dir, &format!("{name}/mod.rs"))));
                    cands.push(format!("src/{name}.rs"));
                    cands.push(format!("src/{name}/mod.rs"));
                }
            }
            Lang::Python => {
                let p = spec.replace('.', "/");
                cands.push(format!("{p}.py"));
                cands.push(format!("{p}/__init__.py"));
                cands.push(normalize(&join(dir, &format!("{p}.py"))));
            }
            Lang::C | Lang::Cpp => {
                cands.push(normalize(&join(dir, spec)));
                cands.push(spec.to_string());
            }
            Lang::GdScript => {
                if let Some(s) = spec.strip_prefix("res://") {
                    cands.push(s.to_string());
                }
            }
            _ => {}
        }
        cands
            .into_iter()
            .filter(|c| self.entries.contains_key(c))
            .collect()
    }

    /// Find files defining a symbol (case-insensitive exact name match).
    pub fn find_symbol(&self, name: &str) -> Vec<(&FileEntry, usize)> {
        let lname = name.to_lowercase();
        let mut out = Vec::new();
        for e in self.entries.values() {
            for s in &e.outline.symbols {
                if s.name.to_lowercase() == lname {
                    out.push((e, s.line));
                }
            }
        }
        out
    }

    /// Compact project structure: directories with file counts. Used instead
    /// of sending a full tree.
    pub fn structure(&self, max_lines: usize) -> String {
        let mut dirs: HashMap<String, usize> = HashMap::new();
        for e in self.entries.values() {
            let d = e
                .path
                .rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .unwrap_or_else(|| ".".into());
            *dirs.entry(d).or_default() += 1;
        }
        let mut v: Vec<_> = dirs.into_iter().collect();
        v.sort();
        let total = v.len();
        let mut out: Vec<String> = v
            .into_iter()
            .take(max_lines)
            .map(|(d, n)| format!("{d}/ ({n})"))
            .collect();
        if total > max_lines {
            out.push(format!("… {} more directories", total - max_lines));
        }
        out.join("\n")
    }
}

/// (relative path, absolute path, size, mtime)
type WalkItem = (String, PathBuf, u64, u128);
/// (relative path, bytes, size, mtime)
type ReadItem = (String, Vec<u8>, u64, u128);

fn read_one(item: WalkItem) -> Option<ReadItem> {
    let (rel, abs, size, mtime) = item;
    std::fs::read(&abs).ok().map(|b| (rel, b, size, mtime))
}

fn join(dir: &str, rel: &str) -> String {
    if dir.is_empty() {
        rel.to_string()
    } else {
        format!("{dir}/{rel}")
    }
}

fn normalize(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

fn is_binary_ext(p: &str) -> bool {
    let l = p.to_ascii_lowercase();
    let ext = l.rsplit('.').next().unwrap_or("");
    matches!(
        ext,
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "bmp"
            | "tga"
            | "psd"
            | "exr"
            | "hdr"
            | "mp3"
            | "wav"
            | "ogg"
            | "flac"
            | "mp4"
            | "mov"
            | "avi"
            | "mkv"
            | "webm"
            | "zip"
            | "gz"
            | "tgz"
            | "7z"
            | "rar"
            | "xz"
            | "zst"
            | "exe"
            | "dll"
            | "so"
            | "dylib"
            | "a"
            | "lib"
            | "o"
            | "obj"
            | "pdb"
            | "class"
            | "jar"
            | "wasm"
            | "pdf"
            | "ttf"
            | "otf"
            | "woff"
            | "woff2"
            | "eot"
            | "fbx"
            | "glb"
            | "gltf"
            | "blend"
            | "uasset"
            | "umap"
            | "unitypackage"
            | "pck"
            | "sqlite"
            | "db"
            | "bin"
            | "dat"
            | "lock"
    ) || l.ends_with("package-lock.json")
        || l.ends_with("pnpm-lock.yaml")
}

fn summarize(lang: Lang, text: &str) -> String {
    let mut out = String::new();
    for line in text.lines().take(40) {
        let t = line.trim();
        let doc = match lang {
            Lang::Rust => t.strip_prefix("//!").or_else(|| t.strip_prefix("///")),
            Lang::Python | Lang::Shell | Lang::Toml | Lang::Yaml | Lang::GdScript => {
                t.strip_prefix('#')
            }
            Lang::Markdown => (!t.is_empty()).then_some(t.trim_start_matches('#')),
            _ => t
                .strip_prefix("//")
                .or_else(|| t.strip_prefix("/**"))
                .or_else(|| t.strip_prefix('*').filter(|s| !s.starts_with('/'))),
        };
        if let Some(d) = doc {
            let d = d.trim();
            if d.is_empty() || d.starts_with('!') && lang != Lang::Rust {
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(d);
            if out.len() > 200 {
                break;
            }
        } else if !out.is_empty() && !t.is_empty() {
            break;
        }
    }
    out.chars().take(220).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn incremental_refresh_and_persistence() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("repo");
        write(
            &root,
            "src/main.rs",
            "//! Entry point.\nmod auth;\nfn main() {}\n",
        );
        write(&root, "src/auth.rs", "/// Login logic\npub fn login() {}\n");
        write(&root, "node_modules/x/index.js", "skip me");
        write(&root, ".gitignore", "ignored.txt\n");
        write(&root, "ignored.txt", "nope");
        write(&root, "logo.png", "\u{0}binary");
        let cache = d.path().join("cache/index.json");

        let mut idx = RepoIndex::open(&root, Some(cache.clone()));
        let s1 = idx.refresh(&IndexOptions::default()).unwrap();
        assert_eq!(s1.reparsed, 3, "{s1:?}"); // main.rs, auth.rs, .gitignore
        assert!(idx.get("node_modules/x/index.js").is_none());
        assert!(idx.get("ignored.txt").is_none());
        assert!(idx.get("logo.png").is_none());
        assert_eq!(idx.get("src/main.rs").unwrap().summary, "Entry point.");
        idx.save().unwrap();

        // Reopen from cache: everything is a hit.
        let mut idx2 = RepoIndex::open(&root, Some(cache.clone()));
        assert_eq!(idx2.len(), 3);
        let s2 = idx2.refresh(&IndexOptions::default()).unwrap();
        assert_eq!(s2.cache_hits, 3);
        assert_eq!(s2.reparsed, 0);

        // Change one file -> only that file reparsed.
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(
            &root,
            "src/auth.rs",
            "/// Login logic\npub fn login() {}\npub fn logout() {}\n",
        );
        std::fs::remove_file(root.join(".gitignore")).unwrap();
        let s3 = idx2.refresh(&IndexOptions::default()).unwrap();
        assert_eq!(s3.reparsed, 2, "{s3:?}"); // auth.rs + ignored.txt now visible
        assert_eq!(s3.removed, 1);
        assert!(idx2
            .get("src/auth.rs")
            .unwrap()
            .outline
            .symbols
            .iter()
            .any(|s| s.name == "logout"));
    }

    #[test]
    fn touch_without_change_reuses_outline() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.py", "def f():\n    pass\n");
        let mut idx = RepoIndex::open(d.path(), None);
        idx.refresh(&IndexOptions::default()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(d.path(), "a.py", "def f():\n    pass\n");
        let s = idx.refresh(&IndexOptions::default()).unwrap();
        assert_eq!(s.rehashed_unchanged + s.cache_hits, 1);
        assert_eq!(s.reparsed, 0);
    }

    #[test]
    fn neighbors_resolve_imports() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "src/app.ts", "import { db } from './db';\n");
        write(d.path(), "src/db.ts", "export const db = 1;\n");
        write(d.path(), "src/main.rs", "mod net;\n");
        write(d.path(), "src/net.rs", "pub fn x() {}\n");
        let mut idx = RepoIndex::open(d.path(), None);
        idx.refresh(&IndexOptions {
            threads: 3,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(idx.neighbors("src/app.ts"), vec!["src/db.ts"]);
        assert_eq!(idx.neighbors("src/db.ts"), vec!["src/app.ts"]);
        assert!(idx
            .neighbors("src/main.rs")
            .contains(&"src/net.rs".to_string()));
        assert_eq!(idx.find_symbol("db").len(), 1);
    }

    #[test]
    fn large_files_skipped_and_sensitive_flagged() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "big.txt", &"x".repeat(5000));
        write(d.path(), ".env", "A=1\n");
        let mut idx = RepoIndex::open(d.path(), None);
        let s = idx
            .refresh(&IndexOptions {
                max_file_bytes: 1000,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(s.skipped_large, 1);
        assert!(idx.get(".env").unwrap().sensitive);
    }

    #[test]
    fn corrupt_cache_is_discarded() {
        let d = tempfile::tempdir().unwrap();
        let cache = d.path().join("c.json");
        std::fs::write(&cache, "{garbage").unwrap();
        write(d.path(), "r/a.rs", "fn a() {}");
        let mut idx = RepoIndex::open(d.path().join("r"), Some(cache));
        assert!(idx.is_empty());
        assert_eq!(idx.refresh(&IndexOptions::default()).unwrap().reparsed, 1);
    }
}
