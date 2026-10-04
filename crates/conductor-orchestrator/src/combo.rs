//! Combos: reusable orchestration configurations, plus adaptive presets that
//! are generated from the models actually available instead of hard-coded
//! model names.

use serde::{Deserialize, Serialize};

use crate::model::{Capability, EffortLevel, ModelProfile, Tier};
use crate::roles::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    #[default]
    Balanced,
    /// Prefer this provider id first.
    PreferProvider,
    /// Hold back this provider for reserved roles.
    PreserveProvider,
    LowestCost,
    Fastest,
    MaximumQuality,
    SubscriptionFirst,
    ManualWeights,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reserve {
    pub provider: String,
    /// 0.0..=1.0 of estimated capacity to hold back.
    pub fraction: f32,
    /// Roles that may use the reserved capacity.
    #[serde(default)]
    pub for_roles: Vec<Role>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPolicy {
    Off,
    /// Review only important/risky tasks.
    #[default]
    Important,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Architecture,
    Implementation,
    RepetitiveFixes,
    Review,
    Research,
}

/// Explicit, inspectable phase behaviour (adaptive Combo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseRule {
    pub phase: Phase,
    /// Prefer this member model key in this phase.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub min_tier: Option<Tier>,
    #[serde(default)]
    pub max_tier: Option<Tier>,
    #[serde(default)]
    pub effort: Option<EffortLevel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Budget {
    pub max_tokens: Option<u64>,
    pub max_cost_usd: Option<f64>,
    /// Stop and ask once this fraction of a budget is used.
    pub ask_at_fraction: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComboMember {
    /// `provider/model`
    pub model: String,
    #[serde(default)]
    pub roles: Vec<Role>,
    /// None = automatic effort.
    #[serde(default)]
    pub effort: Option<EffortLevel>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "one")]
    pub weight: f32,
}

fn yes() -> bool {
    true
}
fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComboRevision {
    pub version: u32,
    pub members: Vec<ComboMember>,
    pub strategy: Strategy,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Combo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "v1")]
    pub version: u32,
    pub members: Vec<ComboMember>,
    #[serde(default)]
    pub strategy: Strategy,
    /// Provider for PreferProvider/PreserveProvider strategies.
    #[serde(default)]
    pub strategy_provider: Option<String>,
    #[serde(default)]
    pub reserves: Vec<Reserve>,
    #[serde(default = "yes")]
    pub fallback: bool,
    #[serde(default)]
    pub review: ReviewPolicy,
    #[serde(default = "two")]
    pub max_parallel: usize,
    #[serde(default)]
    pub budget: Budget,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub phases: Vec<PhaseRule>,
    /// Built-in presets are regenerated from available models.
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub history: Vec<ComboRevision>,
}

fn v1() -> u32 {
    1
}
fn two() -> usize {
    2
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ComboError {
    #[error("Combo needs at least one enabled model")]
    NoMembers,
    #[error("Combo name cannot be empty")]
    EmptyName,
    #[error("invalid model reference '{0}' (expected provider/model)")]
    BadModel(String),
    #[error("reserve fraction for {0} must be between 0 and 0.9")]
    BadReserve(String),
    #[error("not a Conductor Combo file: {0}")]
    BadImport(String),
}

impl Combo {
    pub fn new(name: impl Into<String>, members: Vec<ComboMember>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name: name.into(),
            description: String::new(),
            version: 1,
            members,
            strategy: Strategy::Balanced,
            strategy_provider: None,
            reserves: Vec::new(),
            fallback: true,
            review: ReviewPolicy::Important,
            max_parallel: 2,
            budget: Budget::default(),
            instructions: String::new(),
            phases: Vec::new(),
            builtin: false,
            history: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), ComboError> {
        if self.name.trim().is_empty() {
            return Err(ComboError::EmptyName);
        }
        if !self.members.iter().any(|m| m.enabled) {
            return Err(ComboError::NoMembers);
        }
        for m in &self.members {
            match m.model.split_once('/') {
                Some((p, mm)) if !p.is_empty() && !mm.is_empty() => {}
                _ => return Err(ComboError::BadModel(m.model.clone())),
            }
        }
        for r in &self.reserves {
            if !(0.0..=0.9).contains(&r.fraction) {
                return Err(ComboError::BadReserve(r.provider.clone()));
            }
        }
        Ok(())
    }

    pub fn enabled_members(&self) -> impl Iterator<Item = &ComboMember> {
        self.members.iter().filter(|m| m.enabled)
    }

    pub fn providers(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .enabled_members()
            .filter_map(|m| m.model.split_once('/').map(|(p, _)| p.to_string()))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Apply an edit, bumping the version and keeping the previous revision.
    pub fn edit(&mut self, note: &str, f: impl FnOnce(&mut Combo)) -> Result<(), ComboError> {
        let before = ComboRevision {
            version: self.version,
            members: self.members.clone(),
            strategy: self.strategy,
            note: note.to_string(),
        };
        let mut next = self.clone();
        f(&mut next);
        next.validate()?;
        next.version = self.version + 1;
        next.builtin = false;
        next.history.push(before);
        if next.history.len() > 20 {
            next.history.remove(0);
        }
        *self = next;
        Ok(())
    }

    /// Restore a previous revision (itself recorded as a new version).
    pub fn restore(&mut self, version: u32) -> Result<(), ComboError> {
        let Some(rev) = self.history.iter().find(|r| r.version == version).cloned() else {
            return Ok(());
        };
        self.edit(&format!("restore v{version}"), |c| {
            c.members = rev.members.clone();
            c.strategy = rev.strategy;
        })
    }

    pub fn duplicate(&self, new_name: &str) -> Combo {
        let mut c = self.clone();
        c.id = uuid::Uuid::new_v4().simple().to_string();
        c.name = new_name.to_string();
        c.version = 1;
        c.builtin = false;
        c.history.clear();
        c
    }

    pub fn set_member_enabled(&mut self, model: &str, enabled: bool) -> Result<(), ComboError> {
        self.edit(
            if enabled {
                "enable member"
            } else {
                "disable member"
            },
            |c| {
                for m in &mut c.members {
                    if m.model == model {
                        m.enabled = enabled;
                    }
                }
            },
        )
    }

    /// Shareable JSON (no secrets are ever part of a Combo).
    pub fn export(&self) -> String {
        let mut c = self.clone();
        c.history.clear();
        serde_json::to_string_pretty(&ComboFile {
            format: FORMAT.into(),
            schema: 1,
            combo: c,
        })
        .unwrap_or_default()
    }

    /// Import a shared Combo. Returns the Combo (with a fresh id) and warnings
    /// about models not available locally (those members are disabled, not
    /// dropped, so the user can see the original intent).
    pub fn import(
        json: &str,
        available: &[ModelProfile],
    ) -> Result<(Combo, Vec<String>), ComboError> {
        let file: ComboFile =
            serde_json::from_str(json).map_err(|e| ComboError::BadImport(e.to_string()))?;
        if file.format != FORMAT {
            return Err(ComboError::BadImport(format!(
                "unexpected format '{}'",
                file.format
            )));
        }
        let mut c = file.combo;
        c.id = uuid::Uuid::new_v4().simple().to_string();
        c.builtin = false;
        c.history.clear();
        let mut warnings = Vec::new();
        for m in &mut c.members {
            if !available.iter().any(|a| a.key() == m.model) {
                warnings.push(format!(
                    "{} is not available here; member disabled",
                    m.model
                ));
                m.enabled = false;
            }
        }
        if !c.members.iter().any(|m| m.enabled) {
            // Keep it importable; the user can fix members in the editor.
            warnings.push("no members are available on this machine".into());
        } else {
            c.validate()?;
        }
        Ok((c, warnings))
    }
}

const FORMAT: &str = "conductor.combo";

#[derive(Serialize, Deserialize)]
struct ComboFile {
    format: String,
    schema: u32,
    combo: Combo,
}

/// Generate built-in presets from the available models. Presets that cannot
/// be satisfied (e.g. "Local + Cloud" without a local model) are omitted.
pub fn presets(available: &[ModelProfile]) -> Vec<Combo> {
    let usable: Vec<&ModelProfile> = available.iter().filter(|m| !m.deprecated).collect();
    if usable.is_empty() {
        return Vec::new();
    }
    let best_by = |pred: &dyn Fn(&ModelProfile) -> bool,
                   key: &dyn Fn(&ModelProfile) -> i64,
                   exclude_provider: Option<&str>|
     -> Option<&ModelProfile> {
        usable
            .iter()
            .copied()
            .filter(|m| pred(m) && exclude_provider != Some(m.provider.as_str()))
            .max_by_key(|m| key(m))
    };
    let quality = |m: &ModelProfile| {
        (m.tier as i64) * 100
            + m.has(Capability::Reasoning) as i64 * 10
            + m.has(Capability::Coding) as i64
    };
    let cheap = |m: &ModelProfile| -(m.cost as i64) * 10 + m.has(Capability::Coding) as i64;
    let fast = |m: &ModelProfile| -(m.latency as i64) * 10 + (m.tier as i64);
    let any = |_: &ModelProfile| true;
    let coder = |m: &ModelProfile| m.has(Capability::Coding);

    let mut out = Vec::new();
    let mut push = |id: &str,
                    name: &str,
                    desc: &str,
                    members: Vec<(&ModelProfile, Vec<Role>, Option<EffortLevel>)>,
                    f: &dyn Fn(&mut Combo)| {
        let mut seen = Vec::new();
        let mut ms = Vec::new();
        for (m, roles, effort) in members {
            if seen.contains(&m.key()) {
                // Merge roles into existing member.
                if let Some(existing) = ms
                    .iter_mut()
                    .find(|x: &&mut ComboMember| x.model == m.key())
                {
                    for r in roles {
                        if !existing.roles.contains(&r) {
                            existing.roles.push(r);
                        }
                    }
                }
                continue;
            }
            seen.push(m.key());
            ms.push(ComboMember {
                model: m.key(),
                roles,
                effort,
                enabled: true,
                weight: 1.0,
            });
        }
        let mut c = Combo::new(name, ms);
        c.id = id.to_string();
        c.description = desc.to_string();
        c.builtin = true;
        f(&mut c);
        out.push(c);
    };

    let top = best_by(&any, &quality, None);
    let top_coder = best_by(&coder, &quality, None).or(top);
    let reviewer = top_coder
        .and_then(|c| best_by(&any, &quality, Some(&c.provider)))
        .or(top);
    let cheapest = best_by(&any, &cheap, None);
    let fastest = best_by(&any, &fast, None);
    let researcher = best_by(
        &|m: &ModelProfile| m.has(Capability::LongContext) || m.context_window >= 500_000,
        &quality,
        None,
    )
    .or_else(|| best_by(&|m: &ModelProfile| m.tier >= Tier::Fast, &fast, None))
    .or(top);
    let vision = best_by(
        &|m: &ModelProfile| m.has(Capability::Vision),
        &quality,
        None,
    )
    .or(top);
    let local = best_by(&|m: &ModelProfile| m.local, &quality, None);
    let cloud = best_by(&|m: &ModelProfile| !m.local, &quality, None);

    if let (Some(c), Some(r)) = (top_coder, reviewer) {
        let planner = researcher.unwrap_or(c);
        push(
            "balanced",
            "Balanced",
            "Planner, coder and an independent reviewer from different providers when available.",
            vec![
                (planner, vec![Role::Planner, Role::Researcher], None),
                (c, vec![Role::Coder, Role::Debugger], None),
                (r, vec![Role::Reviewer], None),
            ],
            &|_| {},
        );
    }
    if let Some(f) = fastest {
        push(
            "fast",
            "Fast",
            "Lowest latency model for everything.",
            vec![(f, Role::ALL.to_vec(), Some(EffortLevel::Low))],
            &|c| {
                c.strategy = Strategy::Fastest;
                c.review = ReviewPolicy::Off;
            },
        );
    }
    if let Some(ch) = cheapest {
        let mut members = vec![(ch, Role::ALL.to_vec(), None)];
        if let Some(r) = reviewer.filter(|r| r.key() != ch.key()) {
            members.push((r, vec![Role::Reviewer], None));
        }
        push(
            "low_usage",
            "Low Usage",
            "Cheapest capable model does the work; strong model reviews only important changes.",
            members,
            &|c| {
                c.strategy = Strategy::LowestCost;
                for m in &mut c.members {
                    if m.roles == vec![Role::Reviewer] {
                        m.weight = 0.3;
                    }
                }
            },
        );
    }
    if let (Some(t), Some(r)) = (top, reviewer) {
        push(
            "maximum_quality",
            "Maximum Quality",
            "Strongest models for every role with mandatory cross-provider review.",
            vec![
                (
                    t,
                    vec![Role::Architect, Role::Planner, Role::Coder, Role::Debugger],
                    None,
                ),
                (r, vec![Role::Reviewer, Role::SecurityReviewer], None),
            ],
            &|c| {
                c.strategy = Strategy::MaximumQuality;
                c.review = ReviewPolicy::Always;
            },
        );
    }
    if let (Some(c), Some(r)) = (top_coder, reviewer) {
        let cheap_fixer = cheapest.unwrap_or(c);
        push(
            "coding",
            "Coding",
            "Strong coder, cheap model for repetitive fixes, independent reviewer.",
            vec![
                (c, vec![Role::Coder, Role::Debugger, Role::Architect], None),
                (cheap_fixer, vec![Role::Tester, Role::Documentation], None),
                (r, vec![Role::Reviewer], None),
            ],
            &|combo| {
                combo.phases = vec![
                    PhaseRule {
                        phase: Phase::Architecture,
                        model: Some(c.key()),
                        min_tier: None,
                        max_tier: None,
                        effort: Some(EffortLevel::High),
                    },
                    PhaseRule {
                        phase: Phase::RepetitiveFixes,
                        model: Some(cheap_fixer.key()),
                        min_tier: None,
                        max_tier: None,
                        effort: Some(EffortLevel::Low),
                    },
                    PhaseRule {
                        phase: Phase::Review,
                        model: Some(r.key()),
                        min_tier: None,
                        max_tier: None,
                        effort: None,
                    },
                ];
            },
        );
    }
    if let Some(rs) = researcher {
        let mut members = vec![(
            rs,
            vec![Role::Researcher, Role::Planner, Role::Documentation],
            None,
        )];
        if let Some(r) = reviewer.filter(|r| r.key() != rs.key()) {
            members.push((r, vec![Role::Reviewer], None));
        }
        push(
            "research",
            "Research",
            "Long-context researcher with a fact-checking reviewer.",
            members,
            &|_| {},
        );
    }
    if let Some(v) = vision {
        let mut members = vec![(v, vec![Role::Architect, Role::Coder, Role::Reviewer], None)];
        if let Some(c) = top_coder.filter(|c| c.key() != v.key()) {
            members.push((c, vec![Role::Coder], None));
        }
        push(
            "ui_design",
            "UI / Design",
            "Vision-capable model reviews screenshots and layout; coder implements.",
            members,
            &|_| {},
        );
    }
    if let (Some(l), Some(c)) = (local, cloud) {
        push(
            "local_cloud",
            "Local + Cloud",
            "Local model handles cheap sub-tasks; cloud model handles hard reasoning and review.",
            vec![
                (
                    l,
                    vec![Role::Researcher, Role::Tester, Role::Documentation],
                    None,
                ),
                (
                    c,
                    vec![Role::Architect, Role::Coder, Role::Reviewer, Role::Debugger],
                    None,
                ),
            ],
            &|combo| combo.strategy = Strategy::LowestCost,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::test_model;

    fn models() -> Vec<ModelProfile> {
        let mut v = vec![
            test_model("openai", "big", Tier::Frontier),
            test_model("openai", "mini", Tier::Fast),
            test_model("anthropic", "strong", Tier::Frontier),
            test_model("google", "flash", Tier::Fast),
        ];
        v[3].capabilities.push(Capability::LongContext);
        v[2].capabilities.push(Capability::Vision);
        v
    }

    #[test]
    fn presets_only_use_connected_providers_for_one_two_or_three() {
        let all = models();
        for providers in [
            vec!["openai"],
            vec!["openai", "anthropic"],
            vec!["openai", "anthropic", "google"],
        ] {
            let avail: Vec<ModelProfile> = all
                .iter()
                .filter(|m| providers.contains(&m.provider.as_str()))
                .cloned()
                .collect();
            let p = presets(&avail);
            assert!(!p.is_empty(), "{providers:?}: presets offered");
            for c in &p {
                for m in &c.members {
                    let provider = m.model.split('/').next().unwrap();
                    assert!(
                        providers.contains(&provider),
                        "{providers:?}: {} uses {}",
                        c.id,
                        m.model
                    );
                }
            }
            if providers.len() == 1 {
                // One provider: nothing pretends to be cross-provider.
                assert!(p
                    .iter()
                    .all(|c| c.members.iter().all(|m| m.model.starts_with("openai/"))));
            }
        }
        assert!(presets(&[]).is_empty());
    }

    #[test]
    fn presets_adapt_to_available_models() {
        let p = presets(&models());
        let ids: Vec<_> = p.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"balanced"));
        assert!(ids.contains(&"coding"));
        assert!(!ids.contains(&"local_cloud"), "no local model available");
        let bal = p.iter().find(|c| c.id == "balanced").unwrap();
        // reviewer comes from a different provider than the coder
        let coder = bal
            .members
            .iter()
            .find(|m| m.roles.contains(&Role::Coder))
            .unwrap();
        let rev = bal
            .members
            .iter()
            .find(|m| m.roles.contains(&Role::Reviewer))
            .unwrap();
        assert_ne!(coder.model.split('/').next(), rev.model.split('/').next());
        for c in &p {
            c.validate().unwrap();
        }
    }

    #[test]
    fn single_provider_presets_work() {
        let only = vec![
            test_model("google", "pro", Tier::Frontier),
            test_model("google", "flash", Tier::Fast),
        ];
        let p = presets(&only);
        assert!(p.iter().any(|c| c.id == "balanced"));
        assert!(p
            .iter()
            .all(|c| c.providers() == vec!["google".to_string()]));
        let mut with_local = only.clone();
        with_local.push(test_model("ollama", "llama", Tier::Local));
        assert!(presets(&with_local).iter().any(|c| c.id == "local_cloud"));
        assert!(presets(&[]).is_empty());
    }

    #[test]
    fn edit_versions_and_restore() {
        let mut c = Combo::new(
            "Mine",
            vec![ComboMember {
                model: "a/b".into(),
                roles: vec![],
                effort: None,
                enabled: true,
                weight: 1.0,
            }],
        );
        c.edit("add", |c| {
            c.members.push(ComboMember {
                model: "c/d".into(),
                roles: vec![Role::Reviewer],
                effort: None,
                enabled: true,
                weight: 1.0,
            })
        })
        .unwrap();
        assert_eq!(c.version, 2);
        assert_eq!(c.members.len(), 2);
        c.restore(1).unwrap();
        assert_eq!(c.members.len(), 1);
        assert_eq!(c.version, 3);
        // invalid edit rejected, state unchanged
        let r = c.edit("bad", |c| {
            c.members.iter_mut().for_each(|m| m.enabled = false)
        });
        assert_eq!(r, Err(ComboError::NoMembers));
        assert_eq!(c.version, 3);
    }

    #[test]
    fn export_import_roundtrip_disables_unknown() {
        let avail = models();
        let mut c = presets(&avail)
            .into_iter()
            .find(|c| c.id == "balanced")
            .unwrap();
        c.members.push(ComboMember {
            model: "nowhere/x".into(),
            roles: vec![],
            effort: None,
            enabled: true,
            weight: 1.0,
        });
        let json = c.export();
        let (imported, warnings) = Combo::import(&json, &avail).unwrap();
        assert_ne!(imported.id, c.id);
        assert_eq!(warnings.len(), 1);
        assert!(
            !imported
                .members
                .iter()
                .find(|m| m.model == "nowhere/x")
                .unwrap()
                .enabled
        );
        assert!(Combo::import("{\"format\":\"other\",\"schema\":1,\"combo\":{}}", &avail).is_err());
    }

    #[test]
    fn duplicate_and_toggle() {
        let mut c = presets(&models()).remove(0);
        let d = c.duplicate("Copy");
        assert_ne!(d.id, c.id);
        assert!(!d.builtin);
        let first = c.members[0].model.clone();
        c.set_member_enabled(&first, false).unwrap();
        assert!(!c.members[0].enabled);
    }
}
