//! Universal Skills layer.
//!
//! A skill is installed once in Conductor and exposed to every compatible
//! provider. Format: `skill.toml` manifest + instruction file. Skills written
//! in the common `SKILL.md`-with-frontmatter style are accepted too; a
//! manifest is generated for them on install.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::call::Capability;
use crate::package::{Installed, PackageDir, PackageError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillCommand {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    /// Instruction file inside the package.
    #[serde(default = "default_instructions")]
    pub instructions: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// MCP servers this skill depends on (names from the MCP catalog/config).
    #[serde(default)]
    pub mcp: Vec<String>,
    /// Compatible providers (empty = all).
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub commands: Vec<SkillCommand>,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// "manual" or "auto"
    #[serde(default = "manual")]
    pub update: String,
    #[serde(default)]
    pub repository: Option<String>,
}

fn default_instructions() -> String {
    "SKILL.md".into()
}
fn manual() -> String {
    "manual".into()
}

impl SkillManifest {
    pub fn capabilities(&self) -> Result<Vec<Capability>, PackageError> {
        self.permissions
            .iter()
            .map(|p| {
                Capability::parse(p)
                    .ok_or_else(|| PackageError::Invalid(format!("unknown permission '{p}'")))
            })
            .collect()
    }

    pub fn load(dir: &Path) -> Result<Self, PackageError> {
        let text = std::fs::read_to_string(dir.join("skill.toml"))?;
        let m: SkillManifest =
            toml::from_str(&text).map_err(|e| PackageError::Invalid(e.to_string()))?;
        m.capabilities()?;
        let instr = dir.join(&m.instructions);
        if m.instructions.contains("..") || !instr.exists() {
            return Err(PackageError::Invalid(format!(
                "instruction file '{}' missing",
                m.instructions
            )));
        }
        Ok(m)
    }
}

/// Parse `---\nname: x\ndescription: y\n---` frontmatter.
fn frontmatter(md: &str) -> Option<(String, String)> {
    let rest = md.strip_prefix("---")?.trim_start_matches(['\r', '\n']);
    let end = rest.find("\n---")?;
    let block = &rest[..end];
    let mut name = None;
    let mut desc = None;
    for line in block.lines() {
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            desc = Some(v.trim().trim_matches('"').to_string());
        }
    }
    Some((name?, desc.unwrap_or_default()))
}

pub struct Skills {
    dir: PackageDir,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillInfo {
    pub manifest: SkillManifest,
    pub path: PathBuf,
    /// Permissions that deserve attention in the install dialog.
    pub sensitive: Vec<String>,
}

impl Skills {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            dir: PackageDir::new(root, "skill.toml"),
        }
    }

    fn validate(p: &Path) -> Result<(String, String), PackageError> {
        let m = SkillManifest::load(p)?;
        Ok((m.name, m.version))
    }

    /// Convert a frontmatter-only skill folder into a staged copy with a
    /// generated manifest.
    fn normalize(src: &Path) -> Result<Option<tempdir_lite::TempDir>, PackageError> {
        if src.join("skill.toml").exists() {
            return Ok(None);
        }
        let md = src.join("SKILL.md");
        let text = std::fs::read_to_string(&md)
            .map_err(|_| PackageError::Invalid("no skill.toml or SKILL.md found".into()))?;
        let (name, description) = frontmatter(&text).ok_or_else(|| {
            PackageError::Invalid("SKILL.md has no name in its frontmatter".into())
        })?;
        let tmp = tempdir_lite::TempDir::new()?;
        copy_plain(src, tmp.path())?;
        let manifest = SkillManifest {
            name: name.replace(' ', "-").to_lowercase(),
            version: "0.0.0".into(),
            description,
            instructions: "SKILL.md".into(),
            permissions: vec![],
            mcp: vec![],
            providers: vec![],
            commands: vec![],
            keywords: vec![],
            update: "manual".into(),
            repository: None,
        };
        std::fs::write(
            tmp.path().join("skill.toml"),
            toml::to_string(&manifest).map_err(|e| PackageError::Invalid(e.to_string()))?,
        )?;
        Ok(Some(tmp))
    }

    pub fn install_dir(&self, src: &Path) -> Result<Installed, PackageError> {
        let norm = Self::normalize(src)?;
        let from = norm
            .as_ref()
            .map(|t| t.path().to_path_buf())
            .unwrap_or_else(|| src.to_path_buf());
        self.dir
            .install_dir(&from, &format!("local:{}", src.display()), &Self::validate)
    }

    pub async fn install_git(
        &self,
        url: &str,
        rev: Option<&str>,
    ) -> Result<Installed, PackageError> {
        self.dir.install_git(url, rev, &Self::validate).await
    }

    pub fn rollback(&self, name: &str) -> Result<(), PackageError> {
        self.dir.rollback(name)
    }

    pub fn remove(&self, name: &str) -> Result<(), PackageError> {
        self.dir.remove(name)
    }

    pub fn list(&self) -> Vec<SkillInfo> {
        self.dir
            .installed_names()
            .into_iter()
            .filter_map(|n| {
                let p = self.dir.path_of(&n);
                let m = SkillManifest::load(&p).ok()?;
                let sensitive = m
                    .capabilities()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|c| c.is_sensitive())
                    .map(|c| c.as_str().to_string())
                    .collect();
                Some(SkillInfo {
                    manifest: m,
                    path: p,
                    sensitive,
                })
            })
            .collect()
    }

    /// Instructions for skills relevant to a task and compatible with a
    /// provider. Only relevant skills are compiled in (no prompt bloat).
    pub fn relevant(&self, task: &str, provider: &str, max: usize) -> Vec<(SkillManifest, String)> {
        let t = task.to_lowercase();
        let mut scored: Vec<(usize, SkillInfo)> = self
            .list()
            .into_iter()
            .filter(|s| {
                s.manifest.providers.is_empty()
                    || s.manifest.providers.iter().any(|p| p == provider)
            })
            .filter_map(|s| {
                let mut score = 0;
                if t.contains(&s.manifest.name.to_lowercase()) {
                    score += 10;
                }
                score += s
                    .manifest
                    .keywords
                    .iter()
                    .filter(|k| t.contains(&k.to_lowercase()))
                    .count()
                    * 3;
                score += s
                    .manifest
                    .description
                    .to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| w.len() > 4 && t.contains(w))
                    .count();
                (score > 0).then_some((score, s))
            })
            .collect();
        scored.sort_by_key(|s| std::cmp::Reverse(s.0));
        scored
            .into_iter()
            .take(max)
            .filter_map(|(_, s)| {
                let body = std::fs::read_to_string(s.path.join(&s.manifest.instructions)).ok()?;
                Some((s.manifest, body))
            })
            .collect()
    }
}

fn copy_plain(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        if e.file_name() == ".git" {
            continue;
        }
        let to = dst.join(e.file_name());
        let ft = e.file_type()?;
        if ft.is_dir() {
            copy_plain(&e.path(), &to)?;
        } else if ft.is_file() {
            std::fs::copy(e.path(), to)?;
        }
    }
    Ok(())
}

/// Tiny self-cleaning temp dir (avoids a runtime dependency on `tempfile`).
pub(crate) mod tempdir_lite {
    use std::path::{Path, PathBuf};
    pub struct TempDir(PathBuf);
    impl TempDir {
        pub fn new() -> std::io::Result<Self> {
            let p =
                std::env::temp_dir().join(format!("conductor-{}", uuid::Uuid::new_v4().simple()));
            std::fs::create_dir_all(&p)?;
            Ok(Self(p))
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_native_and_frontmatter_skills() {
        let d = tempfile::tempdir().unwrap();
        let skills = Skills::new(d.path().join("skills"));
        let a = d.path().join("steam");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(a.join("skill.toml"), "name='steam'\nversion='1.2.0'\ndescription='Publish builds to Steam with steamcmd'\npermissions=['network','terminal.execute']\nkeywords=['steam','steamworks']\n").unwrap();
        std::fs::write(a.join("SKILL.md"), "Use steamcmd +login ...").unwrap();
        let i = skills.install_dir(&a).unwrap();
        assert_eq!((i.name.as_str(), i.version.as_str()), ("steam", "1.2.0"));

        let b = d.path().join("pdf");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(
            b.join("SKILL.md"),
            "---\nname: pdf-tools\ndescription: Work with PDF files\n---\n# PDF\nDo things",
        )
        .unwrap();
        skills.install_dir(&b).unwrap();

        let list = skills.list();
        assert_eq!(list.len(), 2);
        let steam = list.iter().find(|s| s.manifest.name == "steam").unwrap();
        assert_eq!(steam.sensitive, vec!["terminal.execute"]);

        let rel = skills.relevant("upload the build to Steam", "openai", 3);
        assert_eq!(rel.len(), 1);
        assert!(rel[0].1.contains("steamcmd"));
        assert!(skills
            .relevant("refactor the parser", "openai", 3)
            .is_empty());
    }

    #[test]
    fn rejects_unknown_permissions_and_missing_instructions() {
        let d = tempfile::tempdir().unwrap();
        let skills = Skills::new(d.path().join("skills"));
        let a = d.path().join("bad");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(
            a.join("skill.toml"),
            "name='bad'\nversion='1'\ndescription='x'\npermissions=['root.everything']\n",
        )
        .unwrap();
        std::fs::write(a.join("SKILL.md"), "x").unwrap();
        assert!(skills.install_dir(&a).is_err());
        std::fs::write(
            a.join("skill.toml"),
            "name='bad'\nversion='1'\ndescription='x'\ninstructions='../../etc/passwd'\n",
        )
        .unwrap();
        assert!(skills.install_dir(&a).is_err());
    }
}
