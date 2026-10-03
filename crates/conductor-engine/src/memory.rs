//! Project Memory and Decision Memory: durable, user-editable project facts.
//! Stored per project in the data folder (never inside the repository unless
//! the user exports it). Not a transcript dump: only decisions and facts.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub topic: String,
    pub value: String,
    pub at: u64,
    /// "user", "clarification", "goal"
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectMemory {
    #[serde(default)]
    pub decisions: Vec<Decision>,
    /// Durable facts: architecture, platforms, dependency policy, naming…
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub instructions: String,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn path(data_dir: &Path, project_id: &str) -> PathBuf {
    let safe: String = project_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    data_dir.join("projects").join(safe).join("memory.json")
}

impl ProjectMemory {
    pub fn load(data_dir: &Path, project_id: &str) -> Self {
        std::fs::read(path(data_dir, project_id))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path, project_id: &str) -> Result<(), String> {
        let p = path(data_dir, project_id);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = p.with_extension("tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, p).map_err(|e| e.to_string())
    }

    /// Record a decision, replacing an earlier one on the same topic.
    pub fn decide(&mut self, topic: &str, value: &str, source: &str) {
        let topic = topic.trim();
        if topic.is_empty() || value.trim().is_empty() {
            return;
        }
        self.decisions
            .retain(|d| !d.topic.eq_ignore_ascii_case(topic));
        self.decisions.push(Decision {
            id: uuid::Uuid::new_v4().simple().to_string(),
            topic: topic.into(),
            value: value.trim().into(),
            at: now(),
            source: source.into(),
        });
    }

    pub fn as_lines(&self) -> Vec<String> {
        self.decisions
            .iter()
            .map(|d| format!("{}: {}", d.topic, d.value))
            .chain(self.facts.iter().cloned())
            .collect()
    }

    pub fn clarify_decisions(&self) -> Vec<conductor_orchestrator::clarify::Decision> {
        self.decisions
            .iter()
            .map(|d| conductor_orchestrator::clarify::Decision {
                topic: d.topic.clone(),
                value: d.value.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_replace_by_topic_and_persist() {
        let d = tempfile::tempdir().unwrap();
        let mut m = ProjectMemory::load(d.path(), "p1");
        m.decide("engine", "Godot", "clarification");
        m.decide("Engine", "Unity", "user");
        m.facts.push("Supports Windows 10".into());
        m.save(d.path(), "p1").unwrap();
        let m2 = ProjectMemory::load(d.path(), "p1");
        assert_eq!(m2.decisions.len(), 1);
        assert_eq!(m2.decisions[0].value, "Unity");
        assert_eq!(m2.as_lines(), vec!["Engine: Unity", "Supports Windows 10"]);
        assert_eq!(
            path(d.path(), "../evil"),
            d.path().join("projects").join("evil").join("memory.json")
        );
    }
}
