//! Installation receipts and clean removal.
//!
//! Every install Conductor performs leaves a receipt: what, from where, which
//! version and checksum, where it went, its scope, and how to remove it.
//! Removal only ever deletes paths inside Conductor-managed directories, so
//! unrelated user software can't be removed by accident.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallKind {
    Mcp,
    Skill,
    Plugin,
    Theme,
    Tool,
    HelperModel,
    Component,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum Scope {
    Global,
    Project(String),
    Goal(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Removal {
    /// Delete a directory/file inside a managed root.
    Delete { path: PathBuf },
    /// Run an uninstall command (argv).
    Command { argv: Vec<String> },
    /// Only forget the registration (e.g. MCP launched via npx/uvx).
    Unregister,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallReceipt {
    pub id: String,
    pub kind: InstallKind,
    pub name: String,
    pub source: String,
    pub version: String,
    pub location: Option<PathBuf>,
    pub checksum: Option<String>,
    pub signature_verified: bool,
    pub installed_at: u64,
    pub scope: Scope,
    pub removal: Removal,
    /// Goal/project-scoped temporary tool: offer removal on completion.
    #[serde(default)]
    pub temporary: bool,
    /// Decision recorded after completion ("keep"/"remove").
    #[serde(default)]
    pub decision: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReceiptError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("receipt store corrupt: {0}")]
    Corrupt(String),
    #[error("refusing to remove {0}: outside Conductor-managed folders")]
    Unmanaged(PathBuf),
    #[error("receipt {0} not found")]
    NotFound(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ReceiptFile {
    schema: u32,
    receipts: Vec<InstallReceipt>,
}

pub struct ReceiptStore {
    path: PathBuf,
    managed_roots: Vec<PathBuf>,
    receipts: Vec<InstallReceipt>,
}

impl ReceiptStore {
    pub fn open(
        path: impl Into<PathBuf>,
        managed_roots: Vec<PathBuf>,
    ) -> Result<Self, ReceiptError> {
        let path = path.into();
        let receipts = match std::fs::read(&path) {
            Ok(b) => {
                serde_json::from_slice::<ReceiptFile>(&b)
                    .map_err(|e| ReceiptError::Corrupt(e.to_string()))?
                    .receipts
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path,
            managed_roots,
            receipts,
        })
    }

    pub fn all(&self) -> &[InstallReceipt] {
        &self.receipts
    }

    pub fn find(&self, kind: InstallKind, name: &str) -> Option<&InstallReceipt> {
        self.receipts
            .iter()
            .find(|r| r.kind == kind && r.name == name)
    }

    pub fn record(&mut self, r: InstallReceipt) -> Result<(), ReceiptError> {
        self.receipts
            .retain(|x| !(x.kind == r.kind && x.name == r.name && x.scope == r.scope));
        self.receipts.push(r);
        self.save()
    }

    pub fn temporary_for(&self, scope: &Scope) -> Vec<&InstallReceipt> {
        self.receipts
            .iter()
            .filter(|r| r.temporary && &r.scope == scope && r.decision.is_none())
            .collect()
    }

    pub fn decide(&mut self, id: &str, decision: &str) -> Result<(), ReceiptError> {
        let r = self
            .receipts
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| ReceiptError::NotFound(id.into()))?;
        r.decision = Some(decision.into());
        self.save()
    }

    /// Remove an installed item according to its receipt. Returns the
    /// command argv to run for `Removal::Command` (caller executes it under
    /// permission policy); deletions happen here.
    pub fn remove(&mut self, id: &str) -> Result<Option<Vec<String>>, ReceiptError> {
        let idx = self
            .receipts
            .iter()
            .position(|r| r.id == id)
            .ok_or_else(|| ReceiptError::NotFound(id.into()))?;
        let r = self.receipts[idx].clone();
        let cmd = match &r.removal {
            Removal::Delete { path } => {
                if !self.is_managed(path) {
                    return Err(ReceiptError::Unmanaged(path.clone()));
                }
                if path.is_dir() {
                    std::fs::remove_dir_all(path)?;
                } else if path.exists() {
                    std::fs::remove_file(path)?;
                }
                None
            }
            Removal::Command { argv } => Some(argv.clone()),
            Removal::Unregister => None,
        };
        self.receipts.remove(idx);
        self.save()?;
        Ok(cmd)
    }

    fn is_managed(&self, p: &Path) -> bool {
        let canon = |x: &Path| std::fs::canonicalize(x).unwrap_or_else(|_| x.to_path_buf());
        let cp = canon(p);
        self.managed_roots.iter().any(|root| {
            let cr = canon(root);
            cp != cr && cp.starts_with(&cr)
        })
    }

    fn save(&self) -> Result<(), ReceiptError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&ReceiptFile {
            schema: 1,
            receipts: self.receipts.clone(),
        })
        .map_err(|e| ReceiptError::Corrupt(e.to_string()))?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub fn receipt(
    kind: InstallKind,
    name: &str,
    source: &str,
    version: &str,
    location: Option<PathBuf>,
    checksum: Option<String>,
    scope: Scope,
    removal: Removal,
) -> InstallReceipt {
    InstallReceipt {
        id: uuid::Uuid::new_v4().simple().to_string(),
        kind,
        name: name.into(),
        source: source.into(),
        version: version.into(),
        location,
        checksum,
        signature_verified: false,
        installed_at: crate::unix_now(),
        scope,
        removal,
        temporary: false,
        decision: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_remove_and_refuse_unmanaged() {
        let d = tempfile::tempdir().unwrap();
        let managed = d.path().join("skills");
        let item = managed.join("steam");
        std::fs::create_dir_all(&item).unwrap();
        std::fs::write(item.join("skill.toml"), "x").unwrap();
        let outside = d.path().join("user-stuff");
        std::fs::create_dir_all(&outside).unwrap();

        let mut s =
            ReceiptStore::open(d.path().join("receipts.json"), vec![managed.clone()]).unwrap();
        let r = receipt(
            InstallKind::Skill,
            "steam",
            "git+https://x",
            "1.0",
            Some(item.clone()),
            None,
            Scope::Global,
            Removal::Delete { path: item.clone() },
        );
        let id = r.id.clone();
        s.record(r).unwrap();
        let bad = receipt(
            InstallKind::Tool,
            "evil",
            "x",
            "1",
            None,
            None,
            Scope::Global,
            Removal::Delete {
                path: outside.clone(),
            },
        );
        let bad_id = bad.id.clone();
        s.record(bad).unwrap();

        // persisted
        let mut s2 =
            ReceiptStore::open(d.path().join("receipts.json"), vec![managed.clone()]).unwrap();
        assert_eq!(s2.all().len(), 2);
        assert!(matches!(
            s2.remove(&bad_id),
            Err(ReceiptError::Unmanaged(_))
        ));
        assert!(outside.exists(), "unrelated folder untouched");
        assert_eq!(s2.remove(&id).unwrap(), None);
        assert!(!item.exists());
        // managed root itself can never be removed
        let root_rcpt = receipt(
            InstallKind::Tool,
            "root",
            "x",
            "1",
            None,
            None,
            Scope::Global,
            Removal::Delete {
                path: managed.clone(),
            },
        );
        let rid = root_rcpt.id.clone();
        s2.record(root_rcpt).unwrap();
        assert!(s2.remove(&rid).is_err());
    }

    #[test]
    fn temporary_tools_by_scope() {
        let d = tempfile::tempdir().unwrap();
        let mut s = ReceiptStore::open(d.path().join("r.json"), vec![]).unwrap();
        let mut r = receipt(
            InstallKind::Tool,
            "blender",
            "winget",
            "4.2",
            None,
            None,
            Scope::Goal("g1".into()),
            Removal::Command {
                argv: vec!["winget".into(), "uninstall".into(), "Blender".into()],
            },
        );
        r.temporary = true;
        let id = r.id.clone();
        s.record(r).unwrap();
        assert_eq!(s.temporary_for(&Scope::Goal("g1".into())).len(), 1);
        s.decide(&id, "keep").unwrap();
        assert!(s.temporary_for(&Scope::Goal("g1".into())).is_empty());
        assert_eq!(s.remove(&id).unwrap().unwrap()[1], "uninstall");
    }
}
