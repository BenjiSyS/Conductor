//! Confine file access to a root directory.
//!
//! Every path supplied by a model, plugin, MCP server or remote client is
//! resolved through [`PathGuard`]. It rejects absolute paths outside the root,
//! `..` traversal, Windows device names, and symlinks whose target leaves the
//! root — including for paths that don't exist yet (the nearest existing
//! ancestor is canonicalised).

use std::path::{Component, Path, PathBuf};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("path is empty")]
    Empty,
    #[error("'{0}' is outside the project folder")]
    Escapes(String),
    #[error("'{0}' is a reserved device name")]
    Reserved(String),
    #[error("'{0}' is blocked by policy")]
    Denied(String),
    #[error("project root is not accessible: {0}")]
    Root(String),
}

#[derive(Debug, Clone)]
pub struct PathGuard {
    root: PathBuf,
    deny: Vec<String>,
}

impl PathGuard {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, PathError> {
        let root = canonical(root.as_ref()).map_err(|e| PathError::Root(e.to_string()))?;
        Ok(Self {
            root,
            deny: vec![".git/".into()],
        })
    }

    /// Additional deny prefixes, relative to root, using `/` separators.
    /// `.git/` is denied for writes by default; reads are allowed via
    /// [`PathGuard::resolve_read`].
    pub fn with_deny(mut self, deny: impl IntoIterator<Item = String>) -> Self {
        self.deny
            .extend(deny.into_iter().map(|d| d.replace('\\', "/")));
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a path for reading.
    pub fn resolve_read(&self, input: &str) -> Result<PathBuf, PathError> {
        self.resolve(input, false)
    }

    /// Resolve a path for writing/deleting (also checks the deny list).
    pub fn resolve_write(&self, input: &str) -> Result<PathBuf, PathError> {
        self.resolve(input, true)
    }

    /// Path relative to root with `/` separators, for display and policy.
    pub fn relative(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.root)
            .unwrap_or(abs)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn resolve(&self, input: &str, writing: bool) -> Result<PathBuf, PathError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(PathError::Empty);
        }
        let candidate = Path::new(trimmed);
        let joined = if candidate.is_absolute() || has_prefix(candidate) {
            candidate.to_path_buf()
        } else {
            self.root.join(candidate)
        };
        // Lexical normalisation first so "a/../../x" is caught even if "a"
        // doesn't exist.
        let mut normal = PathBuf::new();
        for comp in joined.components() {
            match comp {
                Component::ParentDir => {
                    if !normal.pop() {
                        return Err(PathError::Escapes(input.into()));
                    }
                }
                Component::CurDir => {}
                Component::Normal(part) => {
                    let s = part.to_string_lossy();
                    if is_reserved(&s) {
                        return Err(PathError::Reserved(s.into_owned()));
                    }
                    if s.contains(':') && cfg!(windows) {
                        // Alternate data streams: file.txt:hidden
                        return Err(PathError::Escapes(input.into()));
                    }
                    if cfg!(windows) && (s.ends_with('.') || s.ends_with(' ')) {
                        // Windows silently strips trailing dots/spaces, so
                        // ".git." or ".git " would alias ".git".
                        return Err(PathError::Reserved(s.into_owned()));
                    }
                    normal.push(part);
                }
                other => normal.push(other.as_os_str()),
            }
        }
        let resolved = resolve_existing_prefix(&normal);
        if !starts_with(&resolved, &self.root) {
            return Err(PathError::Escapes(input.into()));
        }
        if writing {
            let rel = self.relative(&resolved);
            // Case-insensitive file systems (Windows, default macOS) treat
            // ".GIT" as ".git": compare case-insensitively everywhere.
            let rel_cmp = rel.to_lowercase();
            let rel_dir = format!("{rel_cmp}/");
            for d in &self.deny {
                let d_norm = d.trim_start_matches("./").to_lowercase();
                if rel_dir.starts_with(&d_norm) || rel_cmp == d_norm.trim_end_matches('/') {
                    return Err(PathError::Denied(rel));
                }
            }
        }
        Ok(resolved)
    }
}

fn has_prefix(p: &Path) -> bool {
    matches!(
        p.components().next(),
        Some(Component::Prefix(_)) | Some(Component::RootDir)
    )
}

fn is_reserved(name: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Canonicalise the longest existing ancestor and re-append the rest. This
/// resolves symlinks in the existing part, which is what an attacker controls.
fn resolve_existing_prefix(p: &Path) -> PathBuf {
    let mut existing = p.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = canonical(&existing) {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (
            existing.file_name().map(|s| s.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return p.to_path_buf(),
        }
    }
}

fn canonical(p: &Path) -> std::io::Result<PathBuf> {
    let c = std::fs::canonicalize(p)?;
    #[cfg(windows)]
    {
        let s = c.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            if !stripped.starts_with("UNC") {
                return Ok(PathBuf::from(stripped));
            }
        }
    }
    Ok(c)
}

fn starts_with(p: &Path, root: &Path) -> bool {
    if cfg!(windows) {
        let a = p.to_string_lossy().to_lowercase().replace('/', "\\");
        let b = root.to_string_lossy().to_lowercase().replace('/', "\\");
        let b = b.trim_end_matches('\\');
        a == b || a.starts_with(&format!("{b}\\"))
    } else {
        p.starts_with(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathGuard) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("src")).unwrap();
        std::fs::write(d.path().join("src/a.rs"), "x").unwrap();
        let g = PathGuard::new(d.path()).unwrap();
        (d, g)
    }

    #[test]
    fn allows_inside_paths_including_new_files() {
        let (_d, g) = setup();
        assert!(g.resolve_read("src/a.rs").is_ok());
        assert!(g.resolve_write("src/new/deep/file.rs").is_ok());
        assert!(g.resolve_read("./src/../src/a.rs").is_ok());
        assert_eq!(g.relative(&g.resolve_read("src/a.rs").unwrap()), "src/a.rs");
    }

    #[test]
    fn rejects_traversal_and_absolute_escape() {
        let (_d, g) = setup();
        assert!(matches!(
            g.resolve_read("../outside"),
            Err(PathError::Escapes(_))
        ));
        assert!(matches!(
            g.resolve_read("src/../../x"),
            Err(PathError::Escapes(_))
        ));
        let outside = std::env::temp_dir().join("definitely-elsewhere.txt");
        assert!(matches!(
            g.resolve_read(&outside.to_string_lossy()),
            Err(PathError::Escapes(_))
        ));
        assert_eq!(g.resolve_read("   "), Err(PathError::Empty));
    }

    #[test]
    fn git_dir_is_write_protected() {
        let (_d, g) = setup();
        assert!(matches!(
            g.resolve_write(".git/config"),
            Err(PathError::Denied(_))
        ));
        assert!(g.resolve_read(".git/config").is_ok());
        let g = g.with_deny(["secrets/".to_string()]);
        assert!(matches!(
            g.resolve_write("secrets/x"),
            Err(PathError::Denied(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_reserved_and_streams() {
        let (_d, g) = setup();
        assert!(matches!(
            g.resolve_write("NUL"),
            Err(PathError::Reserved(_))
        ));
        assert!(matches!(
            g.resolve_write("src/com1.txt"),
            Err(PathError::Reserved(_))
        ));
        assert!(g.resolve_write("src/a.rs:stream").is_err());
        assert!(g.resolve_write(".git./config").is_err(), "trailing-dot alias");
        assert!(g.resolve_write(".git /config").is_err(), "trailing-space alias");
        assert!(g.resolve_write("notes.").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_rejected() {
        let (d, g) = setup();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), d.path().join("link")).unwrap();
        assert!(matches!(
            g.resolve_write("link/evil.txt"),
            Err(PathError::Escapes(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn symlink_escape_rejected_windows_junction() {
        // Junctions don't need admin rights, unlike symlinks.
        let (d, g) = setup();
        let outside = tempfile::tempdir().unwrap();
        let link = d.path().join("link");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .unwrap();
        if !status.status.success() {
            eprintln!("skipping: could not create junction");
            return;
        }
        assert!(matches!(
            g.resolve_write("link/evil.txt"),
            Err(PathError::Escapes(_))
        ));
    }
}
