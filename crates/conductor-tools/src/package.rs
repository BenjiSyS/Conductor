//! Shared install/update/rollback machinery for skills, plugins and themes.
//!
//! Install flow: fetch (local folder or `git clone --depth 1` over https) →
//! stage → validate manifest, reject symlinks and oversized content → hash
//! the tree → move the current version to `.previous/` → activate. A bad
//! update can be rolled back to the previous known-good version, and a
//! recorded tree hash detects tampering later.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::exec::{self, ExecRequest};

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid package: {0}")]
    Invalid(String),
    #[error("source not allowed: {0}")]
    Source(String),
    #[error("download failed: {0}")]
    Fetch(String),
    #[error("integrity check failed: {0}")]
    Integrity(String),
    #[error("nothing to roll back to")]
    NoPrevious,
    #[error("'{0}' is not installed")]
    NotInstalled(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub tree_hash: String,
    pub source: String,
}

/// Parses and checks a staged package manifest, returning (name, version).
pub type Validator = dyn Fn(&Path) -> Result<(String, String), PackageError> + Sync;

pub struct PackageDir {
    root: PathBuf,
    manifest_file: &'static str,
    max_bytes: u64,
}

impl PackageDir {
    pub fn new(root: impl Into<PathBuf>, manifest_file: &'static str) -> Self {
        Self {
            root: root.into(),
            manifest_file,
            max_bytes: 50 * 1024 * 1024,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_of(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn installed_names(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join(self.manifest_file).exists())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| !n.starts_with('.'))
            .collect();
        v.sort();
        v
    }

    /// Install from a local folder. `validate` parses the manifest and
    /// returns (name, version).
    pub fn install_dir(
        &self,
        src: &Path,
        source_label: &str,
        validate: &Validator,
    ) -> Result<Installed, PackageError> {
        if !src.join(self.manifest_file).exists() {
            return Err(PackageError::Invalid(format!(
                "missing {}",
                self.manifest_file
            )));
        }
        let staging_root = self.root.join(".staging");
        std::fs::create_dir_all(&staging_root)?;
        let staging = staging_root.join(uuid::Uuid::new_v4().simple().to_string());
        let mut total = 0u64;
        copy_tree(src, &staging, &mut total, self.max_bytes)?;
        let result = (|| {
            let (name, version) = validate(&staging)?;
            validate_name(&name)?;
            let tree_hash = conductor_security::integrity::sha256_tree(&staging)
                .map_err(|e| PackageError::Integrity(e.to_string()))?;
            let dest = self.root.join(&name);
            let prev_root = self.root.join(".previous");
            std::fs::create_dir_all(&prev_root)?;
            let prev = prev_root.join(&name);
            if dest.exists() {
                if prev.exists() {
                    std::fs::remove_dir_all(&prev)?;
                }
                std::fs::rename(&dest, &prev)?;
            }
            std::fs::rename(&staging, &dest)?;
            Ok(Installed {
                name,
                version,
                path: dest,
                tree_hash,
                source: source_label.to_string(),
            })
        })();
        if staging.exists() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        result
    }

    /// Install from a Git repository (https or local path). `rev` pins a tag
    /// or branch.
    pub async fn install_git(
        &self,
        url: &str,
        rev: Option<&str>,
        validate: &Validator,
    ) -> Result<Installed, PackageError> {
        let local = Path::new(url).exists();
        if !local && !url.starts_with("https://") {
            return Err(PackageError::Source(
                "only https:// Git URLs or local folders are allowed".into(),
            ));
        }
        if url.starts_with('-') || rev.is_some_and(|r| r.starts_with('-')) {
            return Err(PackageError::Source("invalid URL or revision".into()));
        }
        let tmp =
            std::env::temp_dir().join(format!("conductor-pkg-{}", uuid::Uuid::new_v4().simple()));
        let tmp_s = tmp.to_string_lossy().to_string();
        let mut args = vec![
            "clone",
            "--depth",
            "1",
            "--no-tags",
            "--config",
            "core.symlinks=false",
        ];
        if let Some(r) = rev {
            args.push("--branch");
            args.push(r);
        }
        args.push("--");
        args.push(url);
        args.push(&tmp_s);
        let mut req = ExecRequest::new("git", &args, std::env::temp_dir()).timeout(300);
        req.env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
        let r = exec::run(req, CancellationToken::new()).await;
        if !r.success() {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(PackageError::Fetch(r.stderr.trim().to_string()));
        }
        let commit = exec::run(
            ExecRequest::new("git", &["rev-parse", "HEAD"], &tmp).timeout(30),
            CancellationToken::new(),
        )
        .await
        .stdout
        .trim()
        .to_string();
        let label = format!(
            "{url}@{}",
            if commit.is_empty() {
                rev.unwrap_or("HEAD").to_string()
            } else {
                commit
            }
        );
        let res = self.install_dir(&tmp, &label, validate);
        let _ = std::fs::remove_dir_all(&tmp);
        res
    }

    pub fn rollback(&self, name: &str) -> Result<(), PackageError> {
        let prev = self.root.join(".previous").join(name);
        if !prev.exists() {
            return Err(PackageError::NoPrevious);
        }
        let dest = self.root.join(name);
        let failed = self.root.join(".failed").join(name);
        if dest.exists() {
            std::fs::create_dir_all(failed.parent().unwrap_or(&self.root))?;
            if failed.exists() {
                std::fs::remove_dir_all(&failed)?;
            }
            std::fs::rename(&dest, &failed)?;
        }
        std::fs::rename(&prev, &dest)?;
        Ok(())
    }

    pub fn remove(&self, name: &str) -> Result<(), PackageError> {
        validate_name(name)?;
        let dest = self.root.join(name);
        if !dest.exists() {
            return Err(PackageError::NotInstalled(name.into()));
        }
        std::fs::remove_dir_all(&dest)?;
        let prev = self.root.join(".previous").join(name);
        if prev.exists() {
            let _ = std::fs::remove_dir_all(prev);
        }
        Ok(())
    }

    /// Detect tampering since install.
    pub fn verify(&self, name: &str, expected_tree_hash: &str) -> Result<(), PackageError> {
        let actual = conductor_security::integrity::sha256_tree(&self.root.join(name))
            .map_err(|e| PackageError::Integrity(e.to_string()))?;
        if actual == expected_tree_hash {
            Ok(())
        } else {
            Err(PackageError::Integrity(format!(
                "{name} changed since it was installed"
            )))
        }
    }
}

fn validate_name(n: &str) -> Result<(), PackageError> {
    if n.is_empty()
        || n.len() > 64
        || n.starts_with('.')
        || !n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(PackageError::Invalid(format!(
            "name '{n}' must be 1-64 chars of [A-Za-z0-9_-]"
        )));
    }
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path, total: &mut u64, max: u64) -> Result<(), PackageError> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let name = e.file_name();
        if name == ".git" {
            continue;
        }
        let ft = e.file_type()?;
        let to = dst.join(&name);
        if ft.is_symlink() {
            return Err(PackageError::Invalid(format!(
                "symlinks are not allowed in packages ({})",
                name.to_string_lossy()
            )));
        } else if ft.is_dir() {
            copy_tree(&e.path(), &to, total, max)?;
        } else {
            *total += e.metadata()?.len();
            if *total > max {
                return Err(PackageError::Invalid("package is too large".into()));
            }
            std::fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_validate(p: &Path) -> Result<(String, String), PackageError> {
        let t = std::fs::read_to_string(p.join("pkg.toml"))?;
        let v: toml::Value =
            toml::from_str(&t).map_err(|e| PackageError::Invalid(e.to_string()))?;
        Ok((
            v["name"].as_str().unwrap_or("").into(),
            v["version"].as_str().unwrap_or("").into(),
        ))
    }

    fn make(dir: &Path, name: &str, version: &str, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("pkg.toml"),
            format!("name='{name}'\nversion='{version}'\n"),
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), body).unwrap();
    }

    #[test]
    fn install_update_rollback_remove_and_tamper() {
        let d = tempfile::tempdir().unwrap();
        let pd = PackageDir::new(d.path().join("skills"), "pkg.toml");
        make(&d.path().join("src1"), "demo", "1.0.0", "v1");
        let i1 = pd
            .install_dir(&d.path().join("src1"), "local", &simple_validate)
            .unwrap();
        assert_eq!(i1.version, "1.0.0");
        assert_eq!(pd.installed_names(), vec!["demo"]);
        pd.verify("demo", &i1.tree_hash).unwrap();

        make(&d.path().join("src2"), "demo", "2.0.0", "v2 broken");
        pd.install_dir(&d.path().join("src2"), "local", &simple_validate)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(pd.path_of("demo").join("README.md")).unwrap(),
            "v2 broken"
        );
        pd.rollback("demo").unwrap();
        assert_eq!(
            std::fs::read_to_string(pd.path_of("demo").join("README.md")).unwrap(),
            "v1"
        );
        pd.verify("demo", &i1.tree_hash).unwrap();

        std::fs::write(pd.path_of("demo").join("README.md"), "tampered").unwrap();
        assert!(matches!(
            pd.verify("demo", &i1.tree_hash),
            Err(PackageError::Integrity(_))
        ));

        pd.remove("demo").unwrap();
        assert!(pd.installed_names().is_empty());
        assert!(matches!(pd.rollback("demo"), Err(PackageError::NoPrevious)));
    }

    #[test]
    fn rejects_bad_names_and_missing_manifest() {
        let d = tempfile::tempdir().unwrap();
        let pd = PackageDir::new(d.path().join("p"), "pkg.toml");
        make(&d.path().join("bad"), "../escape", "1", "x");
        assert!(pd
            .install_dir(&d.path().join("bad"), "l", &simple_validate)
            .is_err());
        std::fs::create_dir_all(d.path().join("empty")).unwrap();
        assert!(pd
            .install_dir(&d.path().join("empty"), "l", &simple_validate)
            .is_err());
        // failed installs leave no staging debris
        let staging: Vec<_> = std::fs::read_dir(pd.root().join(".staging"))
            .map(|r| r.count())
            .into_iter()
            .collect();
        assert!(staging.is_empty() || staging == vec![0]);
    }

    #[tokio::test]
    async fn install_from_local_git_repo_and_refuse_http() {
        let d = tempfile::tempdir().unwrap();
        let repo = d.path().join("repo");
        make(&repo, "gitpkg", "0.3.0", "from git");
        let g = crate::git::Git::new(&repo);
        g.init().await.unwrap();
        g.run(&["config", "user.email", "t@e.com"]).await.unwrap();
        g.run(&["config", "user.name", "T"]).await.unwrap();
        g.run(&["config", "commit.gpgsign", "false"]).await.unwrap();
        g.commit("init", &["pkg.toml", "README.md"]).await.unwrap();
        let pd = PackageDir::new(d.path().join("installed"), "pkg.toml");
        let i = pd
            .install_git(&repo.to_string_lossy(), None, &simple_validate)
            .await
            .unwrap();
        assert_eq!(i.name, "gitpkg");
        assert!(i.source.contains('@'));
        assert!(!pd.path_of("gitpkg").join(".git").exists());
        assert!(matches!(
            pd.install_git("http://example.com/x.git", None, &simple_validate)
                .await,
            Err(PackageError::Source(_))
        ));
        assert!(matches!(
            pd.install_git("git@github.com:x/y.git", None, &simple_validate)
                .await,
            Err(PackageError::Source(_))
        ));
    }
}
