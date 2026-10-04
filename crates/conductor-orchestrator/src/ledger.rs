//! Duplicate-work prevention, disagreement summaries and small JSON helpers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::roles::Role;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkRecord {
    pub key: String,
    pub role: Role,
    pub model: String,
    pub summary: String,
    pub at: u64,
}

/// Remembers completed research/analysis so it is reused rather than redone.
/// Independent validation is allowed only when explicitly requested.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkLedger {
    records: BTreeMap<String, WorkRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reuse<'a> {
    /// Same work already done: reuse the result.
    Reuse(&'a WorkRecord),
    /// Work done before, but the caller asked for independent validation by
    /// a different model.
    IndependentValidation(&'a WorkRecord),
    New,
}

impl WorkLedger {
    /// Normalised key: lowercase significant words, sorted.
    pub fn key(role: Role, topic: &str) -> String {
        let mut words: Vec<String> = topic
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| {
                w.len() > 2
                    && !matches!(
                        *w,
                        "for" | "the" | "and" | "with" | "about" | "into" | "from" | "how" | "what"
                    )
            })
            .map(String::from)
            .collect();
        words.sort();
        words.dedup();
        format!("{:?}:{}", role, words.join(" "))
    }

    pub fn check(&self, role: Role, topic: &str, independent: bool, model: &str) -> Reuse<'_> {
        match self.records.get(&Self::key(role, topic)) {
            Some(r) if independent && r.model != model => Reuse::IndependentValidation(r),
            Some(r) => Reuse::Reuse(r),
            None => Reuse::New,
        }
    }

    pub fn record(&mut self, role: Role, topic: &str, model: &str, summary: &str, at: u64) {
        let key = Self::key(role, topic);
        self.records.insert(
            key.clone(),
            WorkRecord {
                key,
                role,
                model: model.into(),
                summary: summary.into(),
                at,
            },
        );
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Compact disagreement summary shown instead of a raw argument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Disagreement {
    pub topic: String,
    pub option_a: Position,
    pub option_b: Position,
    pub shared: Vec<String>,
    pub unresolved: String,
    /// Product-significant decisions go to the user; technical ones may be
    /// resolved by a third model.
    pub needs_user: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub model: String,
    pub summary: String,
    pub consequences: Vec<String>,
    pub evidence: Vec<String>,
}

impl Disagreement {
    pub fn render(&self) -> String {
        let pos = |label: &str, p: &Position| {
            let mut s = format!("**{label}** ({}): {}\n", p.model, p.summary);
            for c in &p.consequences {
                s.push_str(&format!("  - consequence: {c}\n"));
            }
            for e in &p.evidence {
                s.push_str(&format!("  - evidence: {e}\n"));
            }
            s
        };
        let mut s = format!("### Decision needed: {}\n", self.topic);
        s.push_str(&pos("Option A", &self.option_a));
        s.push_str(&pos("Option B", &self.option_b));
        if !self.shared.is_empty() {
            s.push_str(&format!("Agreed: {}\n", self.shared.join("; ")));
        }
        s.push_str(&format!("Open question: {}\n", self.unresolved));
        s
    }
}

/// Extract the first JSON object/array from model output (handles code fences
/// and leading prose).
pub fn extract_json(text: &str) -> Option<String> {
    let start = text.find(['{', '['])?;
    let bytes = text.as_bytes();
    let open = bytes[start];
    let close = if open == b'{' { b'}' } else { b']' };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuse_vs_independent_validation() {
        let mut l = WorkLedger::default();
        l.record(
            Role::Researcher,
            "Godot multiplayer options",
            "google/flash",
            "Use ENet",
            1,
        );
        assert!(matches!(
            l.check(
                Role::Researcher,
                "options for godot multiplayer",
                false,
                "openai/x"
            ),
            Reuse::Reuse(_)
        ));
        assert!(matches!(
            l.check(
                Role::Researcher,
                "Godot multiplayer options",
                true,
                "openai/x"
            ),
            Reuse::IndependentValidation(_)
        ));
        assert!(matches!(
            l.check(
                Role::Researcher,
                "Godot multiplayer options",
                true,
                "google/flash"
            ),
            Reuse::Reuse(_)
        ));
        assert_eq!(
            l.check(Role::Researcher, "unity rendering", false, "x"),
            Reuse::New
        );
    }

    #[test]
    fn json_extraction() {
        assert_eq!(
            extract_json("x ```json\n{\"a\": \"}\"}\n```").as_deref(),
            Some("{\"a\": \"}\"}")
        );
        assert_eq!(extract_json("[1,[2]] tail").as_deref(), Some("[1,[2]]"));
        assert_eq!(extract_json("none"), None);
        assert_eq!(extract_json("{unclosed"), None);
    }

    #[test]
    fn disagreement_renders_compactly() {
        let d = Disagreement {
            topic: "State management".into(),
            option_a: Position {
                model: "a".into(),
                summary: "Use signals".into(),
                consequences: vec!["less code".into()],
                evidence: vec![],
            },
            option_b: Position {
                model: "b".into(),
                summary: "Use a store".into(),
                consequences: vec![],
                evidence: vec!["existing pattern".into()],
            },
            shared: vec!["keep API".into()],
            unresolved: "Which?".into(),
            needs_user: true,
        };
        let r = d.render();
        assert!(r.contains("Option A") && r.contains("Option B") && r.contains("Agreed: keep API"));
    }
}
