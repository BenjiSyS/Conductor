//! Plugins: declarative extensions with explicit permissions.
//!
//! Plugins never run code inside Conductor. A plugin contributes
//! instructions and *tools* that map to commands; each tool declares the
//! capabilities it needs, and the plugin's manifest must declare a superset.
//! Tools run through the normal permission policy like any other tool call,
//! so a plugin cannot silently gain unrestricted access.

use std::collections::BTreeMap;
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::call::Capability;
use crate::package::{Installed, PackageDir, PackageError};

const MAX_PLUGIN_MANIFEST: usize = 128 * 1024;

fn read_manifest(path: &Path) -> Result<String, PackageError> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_PLUGIN_MANIFEST as u64 {
        return Err(PackageError::Invalid(
            "plugin manifest exceeds 131072 bytes".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(MAX_PLUGIN_MANIFEST.min(16 * 1024));
    file.take(MAX_PLUGIN_MANIFEST as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PLUGIN_MANIFEST {
        return Err(PackageError::Invalid(
            "plugin manifest exceeds 131072 bytes".into(),
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| PackageError::Invalid("plugin manifest is not valid UTF-8".into()))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|component| match component {
            Component::Normal(name) => {
                let text = name.to_string_lossy();
                let stem = text.split('.').next().unwrap_or("").to_ascii_uppercase();
                !text.is_empty()
                    && !text.contains(':')
                    && !text.ends_with([' ', '.'])
                    && !matches!(
                        stem.as_str(),
                        "CON"
                            | "PRN"
                            | "AUX"
                            | "NUL"
                            | "COM1"
                            | "COM2"
                            | "COM3"
                            | "COM4"
                            | "COM5"
                            | "COM6"
                            | "COM7"
                            | "COM8"
                            | "COM9"
                            | "LPT1"
                            | "LPT2"
                            | "LPT3"
                            | "LPT4"
                            | "LPT5"
                            | "LPT6"
                            | "LPT7"
                            | "LPT8"
                            | "LPT9"
                    )
            }
            _ => false,
        })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginTool {
    pub name: String,
    pub description: String,
    /// argv with `{param}` placeholders; each placeholder fills exactly one
    /// argument (no shell interpolation).
    pub argv: Vec<String>,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default = "manual")]
    pub update: String,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub tools: Vec<PluginTool>,
    #[serde(default)]
    pub requires: Vec<String>,
}

fn manual() -> String {
    "manual".into()
}

impl PluginManifest {
    pub fn load(dir: &Path) -> Result<Self, PackageError> {
        let text = read_manifest(&dir.join("plugin.toml"))?;
        let m: PluginManifest =
            toml::from_str(&text).map_err(|e| PackageError::Invalid(e.to_string()))?;
        m.validate()?;
        if let Some(path) = &m.instructions {
            let relative = Path::new(path);
            let base = dir.canonicalize()?;
            if !safe_relative(relative)
                || !dir
                    .join(relative)
                    .canonicalize()
                    .is_ok_and(|path| path.starts_with(&base))
            {
                return Err(PackageError::Invalid(
                    "plugin instructions path must stay inside its package".into(),
                ));
            }
        }
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), PackageError> {
        if !valid_name(&self.name) {
            return Err(PackageError::Invalid(
                "plugin name must use 1-64 ASCII letters, digits, '-' or '_'".into(),
            ));
        }
        if let Some(path) = &self.instructions {
            let path = Path::new(path);
            if !safe_relative(path) {
                return Err(PackageError::Invalid(
                    "plugin instructions path must stay inside its package".into(),
                ));
            }
        }
        let declared: Vec<Capability> = self
            .permissions
            .iter()
            .map(|p| {
                Capability::parse(p)
                    .ok_or_else(|| PackageError::Invalid(format!("unknown permission '{p}'")))
            })
            .collect::<Result<_, _>>()?;
        for t in &self.tools {
            if t.argv.is_empty() {
                return Err(PackageError::Invalid(format!(
                    "tool '{}' has no command",
                    t.name
                )));
            }
            if t.argv[0].contains('{') {
                return Err(PackageError::Invalid(format!(
                    "tool '{}' must not template the program name",
                    t.name
                )));
            }
            let mut needs = vec![Capability::TerminalExecute];
            for p in &t.permissions {
                needs.push(
                    Capability::parse(p).ok_or_else(|| {
                        PackageError::Invalid(format!("unknown permission '{p}'"))
                    })?,
                );
            }
            if let Some(missing) = needs.iter().find(|c| !declared.contains(c)) {
                return Err(PackageError::Invalid(format!(
                    "tool '{}' needs '{}' which the plugin does not declare",
                    t.name,
                    missing.as_str()
                )));
            }
        }
        Ok(())
    }

    /// Build argv for a tool invocation. Values fill whole arguments only.
    pub fn build_argv(
        &self,
        tool: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<Vec<String>, PackageError> {
        let t = self
            .tools
            .iter()
            .find(|t| t.name == tool)
            .ok_or_else(|| PackageError::Invalid(format!("no tool '{tool}'")))?;
        t.argv
            .iter()
            .map(|a| {
                if a.starts_with('{') && a.ends_with('}') {
                    let k = &a[1..a.len() - 1];
                    values
                        .get(k)
                        .cloned()
                        .ok_or_else(|| PackageError::Invalid(format!("missing parameter '{k}'")))
                } else {
                    Ok(a.clone())
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginHealth {
    Healthy,
    MissingRequirement,
    Tampered,
    Invalid,
}

pub struct Plugins {
    dir: PackageDir,
}

impl Plugins {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            dir: PackageDir::new(root, "plugin.toml"),
        }
    }

    fn validate(p: &Path) -> Result<(String, String), PackageError> {
        let m = PluginManifest::load(p)?;
        Ok((m.name, m.version))
    }

    pub fn install_dir(&self, src: &Path) -> Result<Installed, PackageError> {
        self.dir
            .install_dir(src, &format!("local:{}", src.display()), &Self::validate)
    }

    pub async fn install_git(
        &self,
        url: &str,
        rev: Option<&str>,
    ) -> Result<Installed, PackageError> {
        self.dir.install_git(url, rev, &Self::validate).await
    }

    pub fn remove(&self, name: &str) -> Result<(), PackageError> {
        self.dir.remove(name)
    }

    pub fn rollback(&self, name: &str) -> Result<(), PackageError> {
        self.dir.rollback(name)
    }

    pub fn list(&self) -> Vec<PluginManifest> {
        let Some(root) = self.dir.root().canonicalize().ok() else {
            return Vec::new();
        };
        self.dir
            .installed_names()
            .iter()
            .filter_map(|n| {
                let path = self.dir.path_of(n);
                let metadata = std::fs::symlink_metadata(&path).ok()?;
                if !metadata.file_type().is_dir() || !path.canonicalize().ok()?.starts_with(&root) {
                    return None;
                }
                let manifest = PluginManifest::load(&path).ok()?;
                (manifest.name == *n).then_some(manifest)
            })
            .collect()
    }

    pub fn health(&self, name: &str, expected_tree_hash: Option<&str>) -> (PluginHealth, String) {
        let Ok(m) = PluginManifest::load(&self.dir.path_of(name)) else {
            return (PluginHealth::Invalid, "manifest missing or invalid".into());
        };
        if let Some(h) = expected_tree_hash {
            if self.dir.verify(name, h).is_err() {
                return (PluginHealth::Tampered, "files changed since install".into());
            }
        }
        for r in &m.requires {
            if crate::exec::resolve_program(r).is_none() {
                return (PluginHealth::MissingRequirement, format!("needs '{r}'"));
            }
        }
        (PluginHealth::Healthy, format!("{} {}", m.name, m.version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
name = "formatter"
version = "1.0.0"
description = "Runs prettier"
permissions = ["terminal.execute", "filesystem.write"]
requires = ["git"]
[[tools]]
name = "format"
description = "Format a file"
argv = ["npx", "prettier", "--write", "{file}"]
params = { file = "path to format" }
permissions = ["filesystem.write"]
"#;

    #[test]
    fn install_validate_and_build_argv() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("plugin.toml"), GOOD).unwrap();
        let p = Plugins::new(d.path().join("plugins"));
        let i = p.install_dir(&src).unwrap();
        assert_eq!(
            p.health("formatter", Some(&i.tree_hash)).0,
            PluginHealth::Healthy
        );
        let m = &p.list()[0];
        let mut v = BTreeMap::new();
        v.insert("file".to_string(), "a b; rm -rf /".to_string());
        let argv = m.build_argv("format", &v).unwrap();
        assert_eq!(
            argv.last().unwrap(),
            "a b; rm -rf /",
            "value stays one literal argument"
        );
        std::fs::write(
            p.dir.path_of("formatter").join("plugin.toml"),
            GOOD.replace("Runs", "Evil"),
        )
        .unwrap();
        assert_eq!(
            p.health("formatter", Some(&i.tree_hash)).0,
            PluginHealth::Tampered
        );
    }

    #[test]
    fn undeclared_permissions_rejected() {
        let bad = GOOD.replace(
            "permissions = [\"terminal.execute\", \"filesystem.write\"]",
            "permissions = [\"filesystem.write\"]",
        );
        let m: PluginManifest = toml::from_str(&bad).unwrap();
        assert!(
            m.validate().is_err(),
            "tool needs terminal.execute which is undeclared"
        );
        let templ = GOOD.replace("argv = [\"npx\"", "argv = [\"{prog}\"");
        let m: PluginManifest = toml::from_str(&templ).unwrap();
        assert!(m.validate().is_err());
    }

    #[test]
    fn runtime_manifest_reads_are_bounded_and_reject_invalid_utf8() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("plugin.toml");
        std::fs::write(
            &path,
            format!(
                "name='x'\nversion='1'\ndescription='{}'\npermissions=[]",
                "x".repeat(MAX_PLUGIN_MANIFEST)
            ),
        )
        .unwrap();
        assert!(
            PluginManifest::load(d.path()).is_err(),
            "oversized manifest must be rejected before parsing"
        );
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(
            PluginManifest::load(d.path()).is_err(),
            "invalid UTF-8 manifest must be rejected"
        );
        let unsafe_path: PluginManifest = toml::from_str(
            "name='x'\nversion='1'\ndescription='x'\npermissions=[]\ninstructions='../outside.md'",
        )
        .unwrap();
        assert!(
            unsafe_path.validate().is_err(),
            "instruction path cannot escape package"
        );
        let unsafe_name: PluginManifest =
            toml::from_str("name='../outside'\nversion='1'\ndescription='x'\npermissions=[]")
                .unwrap();
        assert!(
            unsafe_name.validate().is_err(),
            "plugin name cannot escape its install root"
        );
    }
}
