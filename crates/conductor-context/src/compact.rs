//! Conversation compaction and duplicate removal.
//!
//! Recent turns are kept verbatim; older turns are folded into a
//! deterministic extractive summary that always preserves requirements,
//! decisions, constraints and unresolved blockers. Pinned turns are never
//! compacted.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::tokens;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Compacted {
    /// Summary of compacted turns (empty when nothing was compacted).
    pub summary: String,
    pub kept: Vec<Turn>,
    pub compacted_turns: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
}

/// Lines carrying these markers are preserved verbatim in summaries.
const KEEP_MARKERS: &[&str] = &[
    "must",
    "must not",
    "never",
    "always",
    "require",
    "constraint",
    "decid",
    "decision",
    "use ",
    "don't",
    "do not",
    "should not",
    "acceptance",
    "done when",
    "blocker",
    "blocked",
    "todo",
    "deadline",
    "target",
    "platform",
    "support",
    "invariant",
    "important",
];

pub fn compact(turns: &[Turn], keep_recent: usize, budget_tokens: usize) -> Compacted {
    let tokens_before: usize = turns.iter().map(|t| tokens::estimate(&t.text)).sum();
    let total_recent: usize = turns
        .iter()
        .rev()
        .take(keep_recent)
        .map(|t| tokens::estimate(&t.text))
        .sum();
    if tokens_before <= budget_tokens || turns.len() <= keep_recent {
        return Compacted {
            summary: String::new(),
            kept: turns.to_vec(),
            compacted_turns: 0,
            tokens_before,
            tokens_after: tokens_before,
        };
    }
    let split = turns.len() - keep_recent;
    let (old, recent) = turns.split_at(split);
    let mut summary_lines: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    let mut kept: Vec<Turn> = Vec::new();
    let mut compacted_turns = 0;
    for t in old {
        if t.pinned {
            kept.push(t.clone());
            continue;
        }
        compacted_turns += 1;
        let mut picked = 0;
        for line in t.text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with("```") {
                continue;
            }
            let lower = l.to_lowercase();
            let important = KEEP_MARKERS.iter().any(|m| lower.contains(m));
            if (important || picked == 0) && seen.insert(lower.clone()) {
                let clipped: String = l.chars().take(240).collect();
                summary_lines.push(format!("- {}: {}", t.role, clipped));
                picked += 1;
            }
            if picked >= 4 {
                break;
            }
        }
    }
    // Respect remaining budget for the summary itself.
    let summary_budget = budget_tokens.saturating_sub(total_recent).max(200);
    let mut summary = String::from("Earlier conversation (compacted):\n");
    let mut used = tokens::estimate(&summary);
    let mut dropped = 0;
    for l in summary_lines {
        let t = tokens::estimate(&l) + 1;
        if used + t > summary_budget {
            dropped += 1;
            continue;
        }
        used += t;
        summary.push_str(&l);
        summary.push('\n');
    }
    if dropped > 0 {
        summary.push_str(&format!("- ({dropped} lower-priority line(s) omitted)\n"));
    }
    kept.extend(recent.iter().cloned());
    let tokens_after = tokens::estimate(&summary)
        + kept
            .iter()
            .map(|t| tokens::estimate(&t.text))
            .sum::<usize>();
    Compacted {
        summary,
        kept,
        compacted_turns,
        tokens_before,
        tokens_after,
    }
}

/// Remove duplicate blocks (e.g. the same tool output pasted twice),
/// returning the deduplicated list and how many were dropped.
pub fn dedupe<T, F: Fn(&T) -> &str>(items: Vec<T>, text: F) -> (Vec<T>, usize) {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    let mut dropped = 0;
    for it in items {
        let h = Sha256::digest(normalize_ws(text(&it)).as_bytes());
        if seen.insert(h) {
            out.push(it);
        } else {
            dropped += 1;
        }
    }
    (out, dropped)
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(role: &str, text: &str) -> Turn {
        Turn {
            role: role.into(),
            text: text.into(),
            pinned: false,
        }
    }

    #[test]
    fn keeps_recent_and_preserves_constraints() {
        let mut turns = vec![
            t(
                "user",
                "We must support Windows 10.\nAlso some chatter here.",
            ),
            t(
                "assistant",
                "Sure, noted. Lots of detail ".repeat(200).as_str(),
            ),
            t("user", "Decision: use PostgreSQL, do not use Docker."),
        ];
        for i in 0..6 {
            turns.push(t("assistant", &format!("recent reply {i} ").repeat(50)));
        }
        let c = compact(&turns, 4, 800);
        assert_eq!(c.kept.len(), 4);
        assert!(c.summary.contains("must support Windows 10"));
        assert!(c.summary.contains("do not use Docker"));
        assert!(c.tokens_after < c.tokens_before);
    }

    #[test]
    fn under_budget_untouched() {
        let turns = vec![t("user", "hi"), t("assistant", "hello")];
        let c = compact(&turns, 1, 10_000);
        assert_eq!(c.kept, turns);
        assert!(c.summary.is_empty());
    }

    #[test]
    fn pinned_survive() {
        let mut turns = vec![Turn {
            role: "user".into(),
            text: "PINNED SPEC ".repeat(100),
            pinned: true,
        }];
        for _ in 0..10 {
            turns.push(t("assistant", &"filler ".repeat(100)));
        }
        let c = compact(&turns, 2, 300);
        assert!(c.kept.iter().any(|t| t.pinned));
    }

    #[test]
    fn dedupe_ignores_whitespace() {
        let (v, d) = dedupe(vec!["a  b", "a b", "c"], |s| s);
        assert_eq!(v, vec!["a  b", "c"]);
        assert_eq!(d, 1);
    }
}
