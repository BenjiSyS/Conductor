//! Combo storage. User Combos persist in `combos.json`; built-in presets are
//! regenerated from the models actually configured.

use std::path::{Path, PathBuf};

use conductor_orchestrator::combo::{presets, Combo};
use conductor_orchestrator::model::ModelProfile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct ComboFile {
    schema: u32,
    combos: Vec<Combo>,
    #[serde(default)]
    default_combo: Option<String>,
}

pub struct ComboStore {
    path: PathBuf,
    file: ComboFile,
}

impl ComboStore {
    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("combos.json");
        let file = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self { path, file }
    }

    fn save(&self) -> Result<(), String> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&ComboFile {
                schema: 1,
                combos: self.file.combos.clone(),
                default_combo: self.file.default_combo.clone(),
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, &self.path).map_err(|e| e.to_string())
    }

    /// Presets (adapted to `available`) followed by user Combos.
    pub fn all(&self, available: &[ModelProfile]) -> Vec<Combo> {
        let mut v = presets(available);
        v.retain(|p| !self.file.combos.iter().any(|c| c.id == p.id));
        v.extend(self.file.combos.iter().cloned());
        v
    }

    pub fn get(&self, id: &str, available: &[ModelProfile]) -> Option<Combo> {
        self.all(available).into_iter().find(|c| c.id == id)
    }

    pub fn upsert(&mut self, c: Combo) -> Result<(), String> {
        c.validate().map_err(|e| e.to_string())?;
        let mut c = c;
        c.builtin = false;
        self.file.combos.retain(|x| x.id != c.id);
        self.file.combos.push(c);
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        self.file.combos.retain(|c| c.id != id);
        if self.file.default_combo.as_deref() == Some(id) {
            self.file.default_combo = None;
        }
        self.save()
    }

    pub fn default_combo(&self) -> Option<String> {
        self.file.default_combo.clone()
    }

    pub fn set_default(&mut self, id: Option<String>) -> Result<(), String> {
        self.file.default_combo = id;
        self.save()
    }

    pub fn import(
        &mut self,
        json: &str,
        available: &[ModelProfile],
    ) -> Result<(Combo, Vec<String>), String> {
        let (c, warnings) = Combo::import(json, available).map_err(|e| e.to_string())?;
        self.file.combos.push(c.clone());
        self.save()?;
        Ok((c, warnings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conductor_orchestrator::combo::ComboMember;
    use conductor_orchestrator::model::Tier;

    fn prof(p: &str, m: &str, tier: Tier) -> ModelProfile {
        ModelProfile {
            provider: p.into(),
            model: m.into(),
            display: m.into(),
            tier,
            capabilities: vec![conductor_orchestrator::model::Capability::Coding],
            efforts: vec![],
            context_window: 100_000,
            cost: 1,
            latency: 1,
            deprecated: false,
            replacement: None,
            subscription: false,
            local: false,
        }
    }

    #[test]
    fn presets_plus_user_combos_persist() {
        let d = tempfile::tempdir().unwrap();
        let avail = vec![prof("a", "x", Tier::Frontier), prof("b", "y", Tier::Fast)];
        let mut s = ComboStore::open(d.path());
        assert!(s.all(&avail).iter().any(|c| c.id == "balanced"));
        let c = Combo::new(
            "Mine",
            vec![ComboMember {
                model: "a/x".into(),
                roles: vec![],
                effort: None,
                enabled: true,
                weight: 1.0,
            }],
        );
        let id = c.id.clone();
        s.upsert(c).unwrap();
        s.set_default(Some(id.clone())).unwrap();
        let s2 = ComboStore::open(d.path());
        assert!(s2.get(&id, &avail).is_some());
        assert_eq!(s2.default_combo(), Some(id));
        assert!(
            s2.all(&[]).iter().all(|c| !c.builtin),
            "no presets without models"
        );
    }
}
