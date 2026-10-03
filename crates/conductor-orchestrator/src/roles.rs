//! Agent roles. Roles belong to tasks, not providers: any suitable model can
//! take a role, and roles move between models during a Goal.

use serde::{Deserialize, Serialize};

use crate::model::{Capability, Tier};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Architect,
    Planner,
    Coder,
    Reviewer,
    Researcher,
    Debugger,
    Tester,
    ReleaseManager,
    Documentation,
    SecurityReviewer,
}

impl Role {
    pub const ALL: [Role; 10] = [
        Role::Architect,
        Role::Planner,
        Role::Coder,
        Role::Reviewer,
        Role::Researcher,
        Role::Debugger,
        Role::Tester,
        Role::ReleaseManager,
        Role::Documentation,
        Role::SecurityReviewer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Role::Architect => "Architect",
            Role::Planner => "Planner",
            Role::Coder => "Coder",
            Role::Reviewer => "Reviewer",
            Role::Researcher => "Researcher",
            Role::Debugger => "Debugger",
            Role::Tester => "Tester",
            Role::ReleaseManager => "Release Manager",
            Role::Documentation => "Documentation",
            Role::SecurityReviewer => "Security Reviewer",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        let l = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        Role::ALL.into_iter().find(|r| {
            let name = r.label().to_ascii_lowercase().replace(' ', "_");
            name == l
                || (l == "implementer" && *r == Role::Coder)
                || (l == "research" && *r == Role::Researcher)
        })
    }

    /// Capabilities a model needs for this role.
    pub fn required(self) -> &'static [Capability] {
        match self {
            Role::Coder | Role::Debugger | Role::Tester => &[Capability::Coding],
            _ => &[],
        }
    }

    /// Minimum tier the router prefers for this role (soft preference).
    pub fn preferred_tier(self) -> Tier {
        match self {
            Role::Architect | Role::Reviewer | Role::SecurityReviewer | Role::Debugger => {
                Tier::Strong
            }
            Role::Planner | Role::Coder => Tier::Strong,
            Role::Researcher | Role::Tester | Role::Documentation | Role::ReleaseManager => {
                Tier::Fast
            }
        }
    }

    /// Short default role instruction (users can override in Settings).
    pub fn default_instructions(self) -> &'static str {
        match self {
            Role::Architect => "Design the structure. Name components, interfaces and trade-offs. Keep it minimal and decisive.",
            Role::Planner => "Break the goal into small verifiable tasks with dependencies, owners (roles) and acceptance checks.",
            Role::Coder => "Implement the assigned task with focused, idiomatic changes. Stay within the files you own. Run relevant checks.",
            Role::Reviewer => "Review for correctness, requirements, tests, security, regressions and maintainability. Report concrete issues with evidence; approve only when satisfied.",
            Role::Researcher => "Gather facts needed for the task. Cite sources. Summarise findings compactly for reuse; do not redo prior research.",
            Role::Debugger => "Find the root cause from evidence (logs, tests, code). Propose the smallest fix and how to verify it.",
            Role::Tester => "Write or run tests that prove the acceptance criteria. Report exact pass/fail output.",
            Role::ReleaseManager => "Prepare versioning, changelog, packaging and release checks. Never publish without explicit permission.",
            Role::Documentation => "Write clear, accurate documentation for the change. No marketing language.",
            Role::SecurityReviewer => "Look for injection, secret exposure, path traversal, unsafe defaults and privilege issues. Be specific.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roles() {
        assert_eq!(
            Role::parse("Security Reviewer"),
            Some(Role::SecurityReviewer)
        );
        assert_eq!(Role::parse("implementer"), Some(Role::Coder));
        assert_eq!(Role::parse("release-manager"), Some(Role::ReleaseManager));
        assert_eq!(Role::parse("chef"), None);
    }
}
