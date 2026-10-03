//! Provider-independent handoff packets.
//!
//! When a task moves between models (fallback, quota exhaustion, role change)
//! the next model receives only this packet — never the full transcript.

use serde::{Deserialize, Serialize};

use crate::tokens;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HandoffPacket {
    pub goal: String,
    pub subtask: String,
    pub role: String,
    pub from_model: Option<String>,
    pub reason: Option<String>,
    pub completed: Vec<String>,
    pub decisions: Vec<String>,
    pub constraints: Vec<String>,
    pub changed_files: Vec<String>,
    pub relevant_symbols: Vec<String>,
    pub git_state: Option<String>,
    pub tests_run: Vec<String>,
    pub failures: Vec<String>,
    pub blockers: Vec<String>,
    pub next_steps: Vec<String>,
    pub tool_state: Option<String>,
}

impl HandoffPacket {
    /// Render as compact Markdown for the receiving model.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("# Handoff\n");
        s.push_str(&format!("Goal: {}\n", self.goal.trim()));
        if !self.subtask.is_empty() {
            s.push_str(&format!("Current task: {}\n", self.subtask.trim()));
        }
        if !self.role.is_empty() {
            s.push_str(&format!("Your role: {}\n", self.role));
        }
        if let Some(from) = &self.from_model {
            let why = self.reason.as_deref().unwrap_or("reassigned");
            s.push_str(&format!(
                "Taken over from {from} ({why}). Do not restart from scratch.\n"
            ));
        }
        let section = |s: &mut String, title: &str, items: &[String]| {
            if items.is_empty() {
                return;
            }
            s.push_str(&format!("\n## {title}\n"));
            for i in items {
                s.push_str("- ");
                s.push_str(i.trim());
                s.push('\n');
            }
        };
        section(&mut s, "Constraints (authoritative)", &self.constraints);
        section(&mut s, "Decisions", &self.decisions);
        section(&mut s, "Completed", &self.completed);
        section(&mut s, "Changed files", &self.changed_files);
        section(&mut s, "Relevant symbols", &self.relevant_symbols);
        if let Some(g) = &self.git_state {
            s.push_str(&format!("\n## Git\n{}\n", g.trim()));
        }
        section(&mut s, "Tests run", &self.tests_run);
        section(&mut s, "Failures", &self.failures);
        section(&mut s, "Blockers", &self.blockers);
        section(&mut s, "Next steps", &self.next_steps);
        if let Some(t) = &self.tool_state {
            s.push_str(&format!("\n## Tool state\n{}\n", t.trim()));
        }
        s
    }

    pub fn estimated_tokens(&self) -> usize {
        tokens::estimate(&self.render())
    }

    /// Drop low-priority detail until the packet fits `budget` tokens.
    /// Constraints, decisions, blockers and the goal are never dropped.
    pub fn fit(&mut self, budget: usize) {
        let shrinkable: [fn(&mut HandoffPacket) -> bool; 4] = [
            |p| pop(&mut p.relevant_symbols),
            |p| pop(&mut p.completed),
            |p| pop(&mut p.tests_run),
            |p| pop(&mut p.changed_files),
        ];
        let mut guard = 0;
        while self.estimated_tokens() > budget && guard < 10_000 {
            guard += 1;
            if !shrinkable.iter().any(|f| f(self)) {
                break;
            }
        }
    }
}

fn pop(v: &mut Vec<String>) -> bool {
    if v.len() > 1 {
        v.pop();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_and_fit_keep_constraints() {
        let mut p = HandoffPacket {
            goal: "Add multiplayer".into(),
            subtask: "Implement lobby".into(),
            role: "coder".into(),
            from_model: Some("openai/x".into()),
            reason: Some("usage exhausted".into()),
            constraints: vec!["Target web only".into()],
            decisions: vec!["Use WebRTC".into()],
            completed: (0..200)
                .map(|i| format!("step {i} done with details"))
                .collect(),
            relevant_symbols: (0..200).map(|i| format!("sym_{i}")).collect(),
            ..Default::default()
        };
        let before = p.estimated_tokens();
        p.fit(300);
        let after = p.estimated_tokens();
        assert!(after < before);
        let r = p.render();
        assert!(r.contains("Target web only"));
        assert!(r.contains("Use WebRTC"));
        assert!(r.contains("Do not restart from scratch"));
    }
}
