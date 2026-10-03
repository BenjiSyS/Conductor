//! Goal Contract, task graph and Definition of Done.
//!
//! A Goal is complete only when every task is done **and** every
//! Definition-of-Done check has passing evidence. An agent saying "done" is
//! never sufficient.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::roles::Role;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GoalContract {
    pub objective: String,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    /// Commands/criteria that must pass (Test Gate).
    #[serde(default)]
    pub done_checks: Vec<DoneCheck>,
    #[serde(default)]
    pub budget: Option<String>,
    #[serde(default)]
    pub permissions: Option<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    /// Bumped whenever the user edits the contract.
    #[serde(default)]
    pub revision: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DoneCheck {
    /// Shell command that must exit 0 (e.g. `cargo test`).
    Command { command: String },
    /// Manual or reviewer-judged criterion.
    Criterion { text: String },
}

impl DoneCheck {
    pub fn label(&self) -> &str {
        match self {
            DoneCheck::Command { command } => command,
            DoneCheck::Criterion { text } => text,
        }
    }
}

impl GoalContract {
    /// Render as the authoritative block placed at the top of every agent
    /// prompt for this Goal.
    pub fn render(&self) -> String {
        let mut s = format!(
            "# Goal Contract (authoritative, rev {})\nObjective: {}\n",
            self.revision, self.objective
        );
        let list = |s: &mut String, t: &str, v: &[String]| {
            if !v.is_empty() {
                s.push_str(&format!("{t}:\n"));
                for i in v {
                    s.push_str(&format!("- {i}\n"));
                }
            }
        };
        list(&mut s, "Requirements", &self.requirements);
        list(&mut s, "Constraints", &self.constraints);
        list(&mut s, "Target platforms", &self.platforms);
        list(&mut s, "Acceptance criteria", &self.acceptance);
        list(&mut s, "Decisions", &self.decisions);
        if !self.done_checks.is_empty() {
            s.push_str("Definition of Done:\n");
            for c in &self.done_checks {
                s.push_str(&format!("- {}\n", c.label()));
            }
        }
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Ready,
    Running,
    Review,
    Done,
    Failed,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    pub role: Role,
    #[serde(default)]
    pub deps: Vec<String>,
    pub status: TaskStatus,
    #[serde(default)]
    pub assigned_model: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub failed_models: Vec<String>,
    /// Soft exclusion: prefer other models (e.g. an independent reviewer),
    /// but fall back to these when nothing else is available.
    #[serde(default)]
    pub avoid_models: Vec<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub verification: Option<String>,
    #[serde(default)]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub finished_at: Option<u64>,
    #[serde(default)]
    pub tokens: u64,
    /// Important tasks get review under ReviewPolicy::Important.
    #[serde(default)]
    pub important: bool,
}

impl TaskNode {
    pub fn new(id: &str, title: &str, role: Role, deps: &[&str]) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            detail: String::new(),
            role,
            deps: deps.iter().map(|s| s.to_string()).collect(),
            status: TaskStatus::Pending,
            assigned_model: None,
            files: Vec::new(),
            attempts: 0,
            failed_models: Vec::new(),
            avoid_models: Vec::new(),
            result: None,
            verification: None,
            started_at: None,
            finished_at: None,
            tokens: 0,
            important: false,
        }
    }
    pub fn elapsed(&self, now: u64) -> Option<u64> {
        self.started_at
            .map(|s| self.finished_at.unwrap_or(now).saturating_sub(s))
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GraphError {
    #[error("task '{0}' depends on unknown task '{1}'")]
    UnknownDep(String, String),
    #[error("dependency cycle involving '{0}'")]
    Cycle(String),
    #[error("duplicate task id '{0}'")]
    Duplicate(String),
    #[error("plan has no tasks")]
    Empty,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskGraph {
    pub tasks: Vec<TaskNode>,
}

impl TaskGraph {
    pub fn validate(&self) -> Result<(), GraphError> {
        if self.tasks.is_empty() {
            return Err(GraphError::Empty);
        }
        let mut ids = BTreeSet::new();
        for t in &self.tasks {
            if !ids.insert(t.id.as_str()) {
                return Err(GraphError::Duplicate(t.id.clone()));
            }
        }
        for t in &self.tasks {
            for d in &t.deps {
                if !ids.contains(d.as_str()) {
                    return Err(GraphError::UnknownDep(t.id.clone(), d.clone()));
                }
            }
        }
        // Kahn's algorithm for cycle detection.
        let mut indeg: BTreeMap<&str, usize> = self
            .tasks
            .iter()
            .map(|t| (t.id.as_str(), t.deps.len()))
            .collect();
        let mut queue: Vec<&str> = indeg
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(k, _)| *k)
            .collect();
        let mut seen = 0;
        while let Some(n) = queue.pop() {
            seen += 1;
            for t in &self.tasks {
                if t.deps.iter().any(|d| d == n) {
                    let e = indeg.get_mut(t.id.as_str()).expect("known id");
                    *e -= 1;
                    if *e == 0 {
                        queue.push(t.id.as_str());
                    }
                }
            }
        }
        if seen != self.tasks.len() {
            let stuck = indeg
                .iter()
                .find(|(_, d)| **d > 0)
                .map(|(k, _)| k.to_string())
                .unwrap_or_default();
            return Err(GraphError::Cycle(stuck));
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&TaskNode> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut TaskNode> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// Promote pending tasks whose dependencies are done; return ready ids.
    pub fn refresh_ready(&mut self) -> Vec<String> {
        let done: BTreeSet<String> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .map(|t| t.id.clone())
            .collect();
        let blocked: BTreeSet<String> = self
            .tasks
            .iter()
            .filter(|t| {
                matches!(
                    t.status,
                    TaskStatus::Failed | TaskStatus::Blocked | TaskStatus::Cancelled
                )
            })
            .map(|t| t.id.clone())
            .collect();
        for t in &mut self.tasks {
            if t.status == TaskStatus::Pending {
                if t.deps.iter().any(|d| blocked.contains(d)) {
                    t.status = TaskStatus::Blocked;
                } else if t.deps.iter().all(|d| done.contains(d)) {
                    t.status = TaskStatus::Ready;
                }
            }
        }
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Ready)
            .map(|t| t.id.clone())
            .collect()
    }

    /// Ready tasks that can run concurrently without touching the same files.
    pub fn parallel_batch(&self, max: usize) -> Vec<String> {
        let mut owned: BTreeSet<&str> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Running)
            .flat_map(|t| t.files.iter().map(String::as_str))
            .collect();
        let mut out = Vec::new();
        for t in self.tasks.iter().filter(|t| t.status == TaskStatus::Ready) {
            if out.len() >= max {
                break;
            }
            if t.files.iter().any(|f| owned.contains(f.as_str())) {
                continue;
            }
            owned.extend(t.files.iter().map(String::as_str));
            out.push(t.id.clone());
        }
        out
    }

    pub fn all_done(&self) -> bool {
        self.tasks
            .iter()
            .all(|t| matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
    }

    pub fn progress(&self) -> (usize, usize) {
        (
            self.tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Done)
                .count(),
            self.tasks.len(),
        )
    }

    /// Parse a planner's JSON plan. Accepts `{"tasks":[...]}` or an array.
    pub fn parse_plan(model_output: &str) -> Result<TaskGraph, GraphError> {
        #[derive(Deserialize)]
        struct W {
            tasks: Vec<RawTask>,
        }
        #[derive(Deserialize)]
        struct RawTask {
            #[serde(default)]
            id: Option<String>,
            #[serde(alias = "name")]
            title: String,
            #[serde(default, alias = "description")]
            detail: String,
            #[serde(default)]
            role: Option<String>,
            #[serde(default, alias = "depends_on", alias = "dependencies")]
            deps: Vec<String>,
            #[serde(default)]
            files: Vec<String>,
            #[serde(default)]
            important: bool,
        }
        let json = crate::ledger::extract_json(model_output).ok_or(GraphError::Empty)?;
        let raws: Vec<RawTask> = serde_json::from_str::<W>(&json)
            .map(|w| w.tasks)
            .or_else(|_| serde_json::from_str::<Vec<RawTask>>(&json))
            .map_err(|_| GraphError::Empty)?;
        let tasks: Vec<TaskNode> = raws
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let mut t = TaskNode::new(
                    &r.id.unwrap_or_else(|| format!("t{}", i + 1)),
                    &r.title,
                    r.role
                        .as_deref()
                        .and_then(Role::parse)
                        .unwrap_or(Role::Coder),
                    &[],
                );
                t.detail = r.detail;
                t.deps = r.deps;
                t.files = r.files;
                t.important = r.important;
                t
            })
            .collect();
        let g = TaskGraph { tasks };
        g.validate()?;
        Ok(g)
    }

    /// Fallback plan when the planner output is unusable.
    pub fn default_plan(objective: &str) -> TaskGraph {
        let mut impl_task =
            TaskNode::new("implement", "Implement the change", Role::Coder, &["plan"]);
        impl_task.detail = objective.to_string();
        impl_task.important = true;
        TaskGraph {
            tasks: vec![
                TaskNode::new("plan", "Inspect the project and plan", Role::Planner, &[]),
                impl_task,
                TaskNode::new("test", "Run and fix tests", Role::Tester, &["implement"]),
                TaskNode::new("review", "Review the change", Role::Reviewer, &["test"]),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    Clarifying,
    Planning,
    Running,
    Verifying,
    /// Waiting for a user decision (product choice, max effort, permission).
    WaitingForUser,
    Paused,
    Blocked,
    Complete,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: String,
    pub passed: bool,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub contract: GoalContract,
    pub graph: TaskGraph,
    pub state: GoalState,
    #[serde(default)]
    pub checks: Vec<CheckResult>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub verification_rounds: u32,
    #[serde(default)]
    pub pending_question: Option<String>,
}

impl Goal {
    pub fn new(contract: GoalContract) -> Self {
        Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            contract,
            graph: TaskGraph::default(),
            state: GoalState::Planning,
            checks: Vec::new(),
            notes: Vec::new(),
            verification_rounds: 0,
            pending_question: None,
        }
    }

    /// True only with evidence: all tasks done and every check passed.
    pub fn can_complete(&self) -> Result<(), String> {
        if !self.graph.all_done() {
            let (d, t) = self.graph.progress();
            return Err(format!("{d}/{t} tasks done"));
        }
        for c in &self.contract.done_checks {
            match self.checks.iter().rev().find(|r| r.check == c.label()) {
                Some(r) if r.passed => {}
                Some(_) => return Err(format!("check failed: {}", c.label())),
                None => return Err(format!("check not run: {}", c.label())),
            }
        }
        Ok(())
    }

    /// What remains unverified, stated exactly.
    pub fn unverified(&self) -> Vec<String> {
        self.contract
            .done_checks
            .iter()
            .filter(|c| {
                !self
                    .checks
                    .iter()
                    .rev()
                    .find(|r| r.check == c.label())
                    .is_some_and(|r| r.passed)
            })
            .map(|c| c.label().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_validation() {
        let mut g = TaskGraph::default_plan("x");
        g.validate().unwrap();
        g.tasks[0].deps.push("review".into());
        assert!(matches!(g.validate(), Err(GraphError::Cycle(_))));
        let g = TaskGraph {
            tasks: vec![TaskNode::new("a", "A", Role::Coder, &["zzz"])],
        };
        assert!(matches!(g.validate(), Err(GraphError::UnknownDep(..))));
        assert_eq!(TaskGraph::default().validate(), Err(GraphError::Empty));
    }

    #[test]
    fn ready_propagation_and_blocking() {
        let mut g = TaskGraph::default_plan("x");
        assert_eq!(g.refresh_ready(), vec!["plan"]);
        g.get_mut("plan").unwrap().status = TaskStatus::Done;
        assert_eq!(g.refresh_ready(), vec!["implement"]);
        g.get_mut("implement").unwrap().status = TaskStatus::Failed;
        g.refresh_ready();
        assert_eq!(g.get("test").unwrap().status, TaskStatus::Blocked);
    }

    #[test]
    fn parallel_batch_respects_file_ownership() {
        let mut a = TaskNode::new("a", "A", Role::Coder, &[]);
        a.files = vec!["src/x.rs".into()];
        let mut b = TaskNode::new("b", "B", Role::Coder, &[]);
        b.files = vec!["src/x.rs".into()];
        let mut c = TaskNode::new("c", "C", Role::Coder, &[]);
        c.files = vec!["src/y.rs".into()];
        let mut g = TaskGraph {
            tasks: vec![a, b, c],
        };
        g.refresh_ready();
        assert_eq!(g.parallel_batch(4), vec!["a", "c"]);
        assert_eq!(g.parallel_batch(1), vec!["a"]);
    }

    #[test]
    fn parse_plan_lenient() {
        let out = "Plan:\n```json\n{\"tasks\":[{\"id\":\"r\",\"title\":\"Research\",\"role\":\"researcher\"},{\"id\":\"c\",\"title\":\"Code\",\"role\":\"implementer\",\"depends_on\":[\"r\"],\"files\":[\"src/a.rs\"]}]}\n```";
        let g = TaskGraph::parse_plan(out).unwrap();
        assert_eq!(g.tasks.len(), 2);
        assert_eq!(g.tasks[1].role, Role::Coder);
        assert_eq!(g.tasks[1].deps, vec!["r"]);
        assert!(
            TaskGraph::parse_plan("[{\"title\":\"a\",\"deps\":[\"t1\"]}]").is_err(),
            "self cycle"
        );
    }

    #[test]
    fn completion_requires_evidence() {
        let mut goal = Goal::new(GoalContract {
            objective: "x".into(),
            done_checks: vec![DoneCheck::Command {
                command: "cargo test".into(),
            }],
            ..Default::default()
        });
        goal.graph = TaskGraph {
            tasks: vec![TaskNode::new("a", "A", Role::Coder, &[])],
        };
        goal.graph.tasks[0].status = TaskStatus::Done;
        assert_eq!(goal.can_complete(), Err("check not run: cargo test".into()));
        goal.checks.push(CheckResult {
            check: "cargo test".into(),
            passed: false,
            evidence: "1 failed".into(),
        });
        assert!(goal.can_complete().is_err());
        assert_eq!(goal.unverified(), vec!["cargo test"]);
        goal.checks.push(CheckResult {
            check: "cargo test".into(),
            passed: true,
            evidence: "ok".into(),
        });
        assert!(goal.can_complete().is_ok());
        assert!(goal.contract.render().contains("Definition of Done"));
    }
}
