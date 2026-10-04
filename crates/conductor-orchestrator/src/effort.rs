//! Automatic effort selection.
//!
//! Defaults favour efficiency. Escalation is evidence-based and follows
//! Low → Medium → High → alternate model → multi-model review. Max/XHigh are
//! **never** chosen automatically unless the user has allowed it; instead the
//! decision carries a permission request the UI turns into
//! "Allow / Always allow for this model / Don't ask again".

use serde::{Deserialize, Serialize};

use crate::model::{EffortLevel, ModelProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Trivial,
    Normal,
    Hard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MaxPolicy {
    #[default]
    Ask,
    AlwaysAllow,
    Never,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffortDecision {
    pub level: Option<EffortLevel>,
    pub reason: String,
    /// Present when a higher (max-tier) level would likely help but needs
    /// the user's permission.
    pub ask_for: Option<MaxRequest>,
    /// Escalation beyond the model's effort range: try another model.
    pub suggest_alternate_model: bool,
    /// Escalation beyond alternate model: request multi-model review.
    pub suggest_review: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaxRequest {
    pub level: EffortLevel,
    pub model: String,
    pub why: String,
}

/// Classify a task description. Cheap keyword heuristics; the router may also
/// pass explicit difficulty from a planner.
pub fn classify(task: &str) -> (Difficulty, bool, &'static str) {
    let t = task.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    let security = has(&[
        "security",
        "auth",
        "authentication",
        "password",
        "encrypt",
        "crypto",
        "permission",
        "vulnerab",
        "injection",
        "secret",
        "token",
        "oauth",
        "sandbox",
    ]);
    if has(&[
        "rename",
        "typo",
        "spelling",
        "format",
        "comment",
        "bump version",
        "lint",
        "whitespace",
        "reword",
    ]) && t.len() < 300
        && !security
    {
        return (Difficulty::Trivial, false, "small mechanical change");
    }
    if has(&[
        "architecture",
        "architect",
        "design the",
        "race condition",
        "deadlock",
        "concurren",
        "memory leak",
        "intermittent",
        "flaky",
        "migrat",
        "refactor the",
        "performance",
        "optimi",
        "distributed",
        "why does",
        "root cause",
        "debug",
        "crash",
        "segfault",
        "corrupt",
    ]) || security
    {
        return (
            Difficulty::Hard,
            security,
            if security {
                "security-sensitive change"
            } else {
                "complex reasoning or debugging"
            },
        );
    }
    (Difficulty::Normal, false, "normal feature work")
}

#[derive(Debug, Clone)]
pub struct EffortInput<'a> {
    pub task: &'a str,
    pub model: &'a ModelProfile,
    /// How many previous attempts at this task failed.
    pub failed_attempts: u32,
    pub policy: MaxPolicy,
    /// Global "allow automatic max effort" switch.
    pub allow_auto_max: bool,
    /// Planner-provided override.
    pub difficulty: Option<Difficulty>,
}

pub fn decide(input: &EffortInput<'_>) -> EffortDecision {
    let (mut difficulty, security, why) = classify(input.task);
    if let Some(d) = input.difficulty {
        difficulty = d;
    }
    let m = input.model;
    if m.efforts.is_empty() {
        return EffortDecision {
            level: None,
            reason: "model has no effort control".into(),
            ask_for: None,
            suggest_alternate_model: input.failed_attempts >= 2,
            suggest_review: input.failed_attempts >= 3 || security,
        };
    }
    let base = match difficulty {
        Difficulty::Trivial => EffortLevel::Low,
        Difficulty::Normal => EffortLevel::Medium,
        Difficulty::Hard => EffortLevel::High,
    };
    // Escalate one step per failed attempt (Low → Medium → High).
    let ladder = [EffortLevel::Low, EffortLevel::Medium, EffortLevel::High];
    let start = ladder.iter().position(|l| *l == base).unwrap_or(1);
    let step = (start + input.failed_attempts as usize).min(ladder.len() - 1);
    let desired = ladder[step];
    let mut level = m.clamp_effort(desired);
    // Never auto-select a max-tier level.
    if let Some(l) = level {
        if l.is_max_tier() {
            level = m
                .efforts
                .iter()
                .filter(|e| !e.is_max_tier())
                .max()
                .copied()
                .or((input.allow_auto_max || input.policy == MaxPolicy::AlwaysAllow).then_some(l));
        }
    }
    let exhausted_ladder = input.failed_attempts as usize + start >= ladder.len();
    let mut ask_for = None;
    let max_level = m.max_effort().filter(|l| l.is_max_tier());
    let wants_max = (difficulty == Difficulty::Hard && (security || input.failed_attempts >= 1))
        || exhausted_ladder;
    if let (Some(ml), true) = (max_level, wants_max) {
        match (input.policy, input.allow_auto_max) {
            (MaxPolicy::Never, _) => {}
            (MaxPolicy::AlwaysAllow, _) | (_, true) => level = Some(ml),
            (MaxPolicy::Ask, false) => {
                ask_for = Some(MaxRequest {
                    level: ml,
                    model: m.key(),
                    why: if input.failed_attempts > 0 {
                        format!(
                            "{} previous attempt(s) failed; deeper reasoning may solve it",
                            input.failed_attempts
                        )
                    } else {
                        format!("{why}; higher effort is likely to improve correctness")
                    },
                });
            }
        }
    }
    let suggest_alternate_model = exhausted_ladder && input.failed_attempts >= 2;
    let suggest_review = security || input.failed_attempts >= 3;
    EffortDecision {
        reason: format!(
            "{why}{}",
            if input.failed_attempts > 0 {
                format!(", escalated after {} failure(s)", input.failed_attempts)
            } else {
                String::new()
            }
        ),
        level,
        ask_for,
        suggest_alternate_model,
        suggest_review,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{test_model, Tier};

    fn input<'a>(task: &'a str, m: &'a ModelProfile) -> EffortInput<'a> {
        EffortInput {
            task,
            model: m,
            failed_attempts: 0,
            policy: MaxPolicy::Ask,
            allow_auto_max: false,
            difficulty: None,
        }
    }

    #[test]
    fn examples_from_spec() {
        let m = test_model("p", "m", Tier::Strong);
        assert_eq!(
            decide(&input("rename foo to bar", &m)).level,
            Some(EffortLevel::Low)
        );
        assert_eq!(
            decide(&input("add a settings page", &m)).level,
            Some(EffortLevel::Medium)
        );
        assert_eq!(
            decide(&input("debug intermittent crash in the scheduler", &m)).level,
            Some(EffortLevel::High)
        );
    }

    #[test]
    fn never_auto_max_without_permission() {
        let m = test_model("p", "m", Tier::Strong);
        let mut i = input("fix authentication token validation", &m);
        i.failed_attempts = 1;
        let d = decide(&i);
        assert_ne!(d.level, Some(EffortLevel::Max));
        let ask = d.ask_for.expect("should ask for max");
        assert_eq!(ask.level, EffortLevel::Max);
        assert!(d.suggest_review, "security changes get review");
    }

    #[test]
    fn policies_respected() {
        let m = test_model("p", "m", Tier::Strong);
        let mut i = input("debug the race condition", &m);
        i.failed_attempts = 2;
        i.policy = MaxPolicy::Never;
        let d = decide(&i);
        assert_eq!(d.level, Some(EffortLevel::High));
        assert!(d.ask_for.is_none());
        i.policy = MaxPolicy::AlwaysAllow;
        assert_eq!(decide(&i).level, Some(EffortLevel::Max));
        i.policy = MaxPolicy::Ask;
        i.allow_auto_max = true;
        assert_eq!(decide(&i).level, Some(EffortLevel::Max));
    }

    #[test]
    fn escalation_ladder_then_alternate_model() {
        let m = test_model("p", "m", Tier::Strong);
        let mut i = input("add a settings page", &m);
        i.policy = MaxPolicy::Never;
        i.failed_attempts = 1;
        assert_eq!(decide(&i).level, Some(EffortLevel::High));
        i.failed_attempts = 2;
        let d = decide(&i);
        assert!(d.suggest_alternate_model);
        i.failed_attempts = 3;
        assert!(decide(&i).suggest_review);
    }

    #[test]
    fn unsupported_levels_clamped() {
        let mut m = test_model("p", "m", Tier::Strong);
        m.efforts = vec![
            EffortLevel::Minimal,
            EffortLevel::Low,
            EffortLevel::Medium,
            EffortLevel::High,
        ];
        assert_eq!(decide(&input("rename x", &m)).level, Some(EffortLevel::Low));
        m.efforts.clear();
        assert_eq!(decide(&input("rename x", &m)).level, None);
    }
}
