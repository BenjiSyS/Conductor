use conductor_orchestrator::runner::RunnerEvent;
use serde::{Deserialize, Serialize};

/// Everything the engine reports to UIs and remote clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum EngineEvent {
    Goal {
        goal_id: String,
        event: RunnerEvent,
    },
    Delta {
        goal_id: Option<String>,
        task_id: String,
        text: String,
    },
    Tool {
        goal_id: Option<String>,
        task_id: String,
        tool: String,
        summary: String,
        ok: bool,
    },
    Approval {
        id: String,
        goal_id: Option<String>,
        capability: String,
        detail: String,
        risk: String,
    },
    ApprovalResolved {
        id: String,
        allowed: bool,
    },
    GoalSaved {
        goal_id: String,
        state: String,
        done: usize,
        total: usize,
    },
}
