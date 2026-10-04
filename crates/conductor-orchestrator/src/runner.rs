//! Goal execution.
//!
//! The runner plans (or accepts) a task graph, routes each task to a model,
//! runs ready tasks in parallel where file ownership allows, reassigns work
//! when a provider fails (with a compact handoff packet — never the full
//! transcript), requests independent review per the Combo policy, and runs
//! the Test Gate. It never reports completion without passing checks.
//!
//! I/O happens only through [`ModelClient`] and [`Verifier`], supplied by the
//! integrator. Tests use scripted fakes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use conductor_context::handoff::HandoffPacket;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::combo::{Combo, Phase, ReviewPolicy};
use crate::effort::{self, EffortInput, MaxPolicy};
use crate::goal::{CheckResult, DoneCheck, Goal, GoalState, TaskGraph, TaskNode, TaskStatus};
use crate::ledger::{Reuse, WorkLedger};
use crate::model::{EffortLevel, ModelProfile};
use crate::roles::Role;
use crate::router::{self, RouteRequest};
use crate::usage::{CallOutcome, UsageTracker};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentCall {
    pub model: String,
    pub effort: Option<EffortLevel>,
    pub role: Role,
    pub system: String,
    pub prompt: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentReply {
    pub text: String,
    pub tokens: u64,
    pub files_changed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModelError {
    #[error("provider error: {0:?}")]
    Provider(CallOutcome),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

#[async_trait]
pub trait ModelClient: Send + Sync {
    async fn run(
        &self,
        call: AgentCall,
        cancel: CancellationToken,
    ) -> Result<AgentReply, ModelError>;
}

#[async_trait]
pub trait Verifier: Send + Sync {
    /// Run a Test Gate command; evidence should already be log-reduced.
    async fn run_check(&self, command: &str, cancel: CancellationToken) -> CheckResult;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RunnerEvent {
    State {
        state: GoalState,
    },
    Planned {
        tasks: usize,
    },
    TaskStarted {
        task: String,
        title: String,
        model: String,
        effort: Option<EffortLevel>,
        reason: String,
    },
    TaskFinished {
        task: String,
        status: TaskStatus,
    },
    Reused {
        task: String,
        from_model: String,
    },
    Handoff {
        task: String,
        from: String,
        to: String,
        reason: String,
        packet_tokens: usize,
    },
    /// Plain-language provider notice, e.g. "Claude usage unavailable.
    /// Continuing implementation with Codex."
    Notice {
        message: String,
    },
    EffortRequest {
        model: String,
        level: EffortLevel,
        why: String,
    },
    Check {
        check: String,
        passed: bool,
    },
    Blocked {
        reason: String,
    },
    Complete,
}

pub type EventSink = Arc<dyn Fn(RunnerEvent) + Send + Sync>;
pub type ProgressFn = Arc<dyn Fn(&Goal) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub max_attempts_per_task: u32,
    pub max_verification_rounds: u32,
    pub max_parallel: usize,
    pub max_effort: HashMap<String, MaxPolicy>,
    pub allow_auto_max: bool,
    pub handoff_budget_tokens: usize,
    /// Extra system text (compiled instructions).
    pub instructions: String,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            max_attempts_per_task: 3,
            max_verification_rounds: 3,
            max_parallel: 2,
            max_effort: HashMap::new(),
            allow_auto_max: false,
            handoff_budget_tokens: 1500,
            instructions: String::new(),
        }
    }
}

pub struct GoalRunner {
    pub combo: Combo,
    pub models: HashMap<String, ModelProfile>,
    pub usage: Arc<Mutex<UsageTracker>>,
    pub ledger: Arc<Mutex<WorkLedger>>,
    pub client: Arc<dyn ModelClient>,
    pub verifier: Arc<dyn Verifier>,
    pub config: RunnerConfig,
    pub events: EventSink,
    /// Called after every state change and task batch so the integrator can
    /// persist the Goal (crash recovery / resume).
    pub on_progress: Option<ProgressFn>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn provider_name(p: &str) -> &str {
    match p {
        "anthropic" => "Claude",
        "openai" => "Codex",
        "google" | "gemini" => "Gemini",
        other => other,
    }
}

fn role_activity(r: Role) -> &'static str {
    match r {
        Role::Coder => "implementation",
        Role::Reviewer | Role::SecurityReviewer => "review",
        Role::Researcher => "research",
        Role::Planner | Role::Architect => "planning",
        Role::Debugger => "debugging",
        Role::Tester => "testing",
        Role::Documentation => "documentation",
        Role::ReleaseManager => "release preparation",
    }
}

impl GoalRunner {
    fn emit(&self, e: RunnerEvent) {
        (self.events)(e);
    }

    fn set_state(&self, goal: &mut Goal, s: GoalState) {
        if goal.state != s {
            goal.state = s;
            self.emit(RunnerEvent::State { state: s });
            self.progress(goal);
        }
    }

    fn progress(&self, goal: &Goal) {
        if let Some(f) = &self.on_progress {
            f(goal);
        }
    }

    /// Drive the goal until complete, blocked, waiting, or cancelled.
    pub async fn run(&self, goal: &mut Goal, cancel: CancellationToken) -> GoalState {
        if goal.graph.tasks.is_empty() {
            self.set_state(goal, GoalState::Planning);
            match self.plan(goal, &cancel).await {
                Ok(g) => goal.graph = g,
                Err(state) => {
                    self.set_state(goal, state);
                    return state;
                }
            }
            self.emit(RunnerEvent::Planned {
                tasks: goal.graph.tasks.len(),
            });
        }
        // Resume: tasks interrupted mid-run go back to ready.
        for t in &mut goal.graph.tasks {
            if t.status == TaskStatus::Running {
                t.status = TaskStatus::Pending;
            }
        }
        self.set_state(goal, GoalState::Running);
        loop {
            if cancel.is_cancelled() {
                self.set_state(goal, GoalState::Stopped);
                return GoalState::Stopped;
            }
            goal.graph.refresh_ready();
            let batch = goal
                .graph
                .parallel_batch(self.config.max_parallel.min(self.combo.max_parallel).max(1));
            if batch.is_empty() {
                if goal.graph.all_done() {
                    match self.verify(goal, &cancel).await {
                        GoalState::Running => continue,
                        s => {
                            self.set_state(goal, s);
                            return s;
                        }
                    }
                }
                let failed: Vec<String> = goal
                    .graph
                    .tasks
                    .iter()
                    .filter(|t| matches!(t.status, TaskStatus::Failed | TaskStatus::Blocked))
                    .map(|t| t.title.clone())
                    .collect();
                let reason = if failed.is_empty() {
                    "no runnable tasks".to_string()
                } else {
                    format!("could not finish: {}", failed.join(", "))
                };
                goal.notes.push(reason.clone());
                self.emit(RunnerEvent::Blocked { reason });
                self.set_state(goal, GoalState::Blocked);
                return GoalState::Blocked;
            }
            for id in &batch {
                if let Some(t) = goal.graph.get_mut(id) {
                    t.status = TaskStatus::Running;
                    t.started_at.get_or_insert(now());
                }
            }
            let snapshot = goal.clone();
            let futs = batch
                .iter()
                .map(|id| self.run_task(&snapshot, id.clone(), cancel.clone()));
            let results = futures_join_all(futs).await;
            for (id, outcome) in results {
                self.apply(goal, &id, outcome);
            }
            self.progress(goal);
            if cancel.is_cancelled() {
                self.set_state(goal, GoalState::Stopped);
                return GoalState::Stopped;
            }
        }
    }

    async fn plan(&self, goal: &Goal, cancel: &CancellationToken) -> Result<TaskGraph, GoalState> {
        let req = RouteRequest {
            role: Some(Role::Planner),
            phase: Some(Phase::Architecture),
            ..Default::default()
        };
        let route = {
            let usage = self.usage.lock().expect("usage lock");
            router::route(&self.combo, &self.models, &usage, &req, now())
        };
        let Ok(route) = route else {
            self.emit(RunnerEvent::Blocked {
                reason: "no model available for planning".into(),
            });
            return Err(GoalState::Blocked);
        };
        let prompt = format!(
            "{}\n\nBreak this into small, verifiable tasks. Reply with JSON only: {{\"tasks\":[{{\"id\":\"t1\",\"title\":\"...\",\"detail\":\"...\",\"role\":\"planner|architect|coder|reviewer|researcher|debugger|tester|documentation|security_reviewer\",\"deps\":[],\"files\":[],\"important\":false}}]}}",
            goal.contract.render()
        );
        let call = AgentCall {
            model: route.model.clone(),
            effort: route.member_effort.or(Some(EffortLevel::Medium)),
            role: Role::Planner,
            system: self.system(Role::Planner),
            prompt,
            task_id: "plan".into(),
        };
        match self.client.run(call, cancel.clone()).await {
            Ok(reply) => {
                self.usage.lock().expect("usage lock").record_tokens(
                    &route.provider,
                    reply.tokens,
                    now(),
                );
                Ok(TaskGraph::parse_plan(&reply.text)
                    .unwrap_or_else(|_| TaskGraph::default_plan(&goal.contract.objective)))
            }
            Err(ModelError::Cancelled) => Err(GoalState::Stopped),
            Err(_) => Ok(TaskGraph::default_plan(&goal.contract.objective)),
        }
    }

    fn system(&self, role: Role) -> String {
        let mut s = format!(
            "You are acting as the {} in a Conductor Goal. {}",
            role.label(),
            role.default_instructions()
        );
        if !self.combo.instructions.is_empty() {
            s.push_str("\n\n");
            s.push_str(&self.combo.instructions);
        }
        if !self.config.instructions.is_empty() {
            s.push_str("\n\n");
            s.push_str(&self.config.instructions);
        }
        s
    }

    async fn run_task(
        &self,
        goal: &Goal,
        id: String,
        cancel: CancellationToken,
    ) -> (String, TaskOutcome) {
        let Some(task) = goal.graph.get(&id).cloned() else {
            return (id, TaskOutcome::Failed("unknown task".into()));
        };
        // Duplicate-work prevention for research/analysis.
        if matches!(task.role, Role::Researcher | Role::Architect) {
            let ledger = self.ledger.lock().expect("ledger lock");
            if let Reuse::Reuse(r) = ledger.check(task.role, &task.title, false, "") {
                self.emit(RunnerEvent::Reused {
                    task: id.clone(),
                    from_model: r.model.clone(),
                });
                return (
                    id,
                    TaskOutcome::Done(
                        AgentReply {
                            text: r.summary.clone(),
                            ..Default::default()
                        },
                        r.model.clone(),
                        false,
                        Vec::new(),
                        0,
                    ),
                );
            }
        }
        let mut attempts = task.attempts;
        let mut failed_models = task.failed_models.clone();
        let mut last_model: Option<String> = task
            .assigned_model
            .clone()
            .filter(|m| failed_models.contains(m));
        let mut last_reason = String::new();
        while attempts < self.config.max_attempts_per_task {
            if cancel.is_cancelled() {
                return (id, TaskOutcome::Cancelled);
            }
            let phase = match task.role {
                Role::Architect | Role::Planner => Some(Phase::Architecture),
                Role::Reviewer | Role::SecurityReviewer => Some(Phase::Review),
                Role::Researcher => Some(Phase::Research),
                _ if attempts > 0 => Some(Phase::RepetitiveFixes),
                _ => Some(Phase::Implementation),
            };
            let mut exclude = failed_models.clone();
            exclude.extend(task.avoid_models.iter().cloned());
            let mut req = RouteRequest {
                role: Some(task.role),
                phase,
                exclude,
                ..Default::default()
            };
            let mut route = {
                let usage = self.usage.lock().expect("usage lock");
                router::route(&self.combo, &self.models, &usage, &req, now())
            };
            if route.is_err() && !task.avoid_models.is_empty() {
                // No independent model available: fall back rather than block.
                req.exclude = failed_models.clone();
                route = {
                    let usage = self.usage.lock().expect("usage lock");
                    router::route(&self.combo, &self.models, &usage, &req, now())
                };
                if let Ok(r) = &route {
                    self.emit(RunnerEvent::Notice {
                        message: format!(
                            "No independent model available; {} reviews its own work.",
                            r.model
                        ),
                    });
                }
            }
            let route = match route {
                Ok(r) => r,
                Err(e) => return (id, TaskOutcome::Failed(e.to_string())),
            };
            let profile = &self.models[&route.model];
            let effort = match route.member_effort {
                Some(e) => profile.clamp_effort(e),
                None => {
                    let d = effort::decide(&EffortInput {
                        task: &format!("{} {}", task.title, task.detail),
                        model: profile,
                        failed_attempts: attempts,
                        policy: self
                            .config
                            .max_effort
                            .get(&route.model)
                            .copied()
                            .unwrap_or_default(),
                        allow_auto_max: self.config.allow_auto_max,
                        difficulty: None,
                    });
                    if let Some(ask) = &d.ask_for {
                        self.emit(RunnerEvent::EffortRequest {
                            model: ask.model.clone(),
                            level: ask.level,
                            why: ask.why.clone(),
                        });
                    }
                    d.level
                }
            };
            let mut prompt = goal.contract.render();
            if let Some(from) = &last_model {
                let packet = self.handoff(goal, &task, from, &last_reason);
                let rendered = packet.render();
                self.emit(RunnerEvent::Handoff {
                    task: id.clone(),
                    from: from.clone(),
                    to: route.model.clone(),
                    reason: last_reason.clone(),
                    packet_tokens: packet.estimated_tokens(),
                });
                prompt.push_str("\n\n");
                prompt.push_str(&rendered);
            } else {
                prompt.push_str(&dependency_context(goal, &task));
            }
            prompt.push_str(&format!("\n\n# Your task\n{}\n{}", task.title, task.detail));
            self.emit(RunnerEvent::TaskStarted {
                task: id.clone(),
                title: task.title.clone(),
                model: route.model.clone(),
                effort,
                reason: route.reason.clone(),
            });
            let call = AgentCall {
                model: route.model.clone(),
                effort,
                role: task.role,
                system: self.system(task.role),
                prompt,
                task_id: id.clone(),
            };
            match self.client.run(call, cancel.clone()).await {
                Ok(reply) => {
                    let mut usage = self.usage.lock().expect("usage lock");
                    usage.record_tokens(&route.provider, reply.tokens, now());
                    usage.record_outcome(&route.provider, CallOutcome::Ok, now());
                    drop(usage);
                    if matches!(task.role, Role::Researcher | Role::Architect) {
                        self.ledger.lock().expect("ledger lock").record(
                            task.role,
                            &task.title,
                            &route.model,
                            &reply.text,
                            now(),
                        );
                    }
                    let review = self.needs_review(&task);
                    return (
                        id,
                        TaskOutcome::Done(
                            reply,
                            route.model.clone(),
                            review,
                            failed_models,
                            attempts,
                        ),
                    );
                }
                Err(ModelError::Cancelled) => return (id, TaskOutcome::Cancelled),
                Err(err) => {
                    attempts += 1;
                    failed_models.push(route.model.clone());
                    let outcome = match &err {
                        ModelError::Provider(o) => *o,
                        _ => CallOutcome::OtherError,
                    };
                    self.usage.lock().expect("usage lock").record_outcome(
                        &route.provider,
                        outcome,
                        now(),
                    );
                    last_reason = match outcome {
                        CallOutcome::UsageExhausted { .. } => "usage exhausted".into(),
                        CallOutcome::RateLimited { .. } => "rate-limited".into(),
                        CallOutcome::SignedOut => "signed out".into(),
                        CallOutcome::InvalidKey => "invalid API key".into(),
                        CallOutcome::ModelMissing => "model missing".into(),
                        CallOutcome::Unavailable => "provider unavailable".into(),
                        _ => err.to_string(),
                    };
                    let p = provider_name(&route.provider);
                    let next = {
                        let usage = self.usage.lock().expect("usage lock");
                        router::route(
                            &self.combo,
                            &self.models,
                            &usage,
                            &RouteRequest {
                                exclude: failed_models.clone(),
                                ..req.clone()
                            },
                            now(),
                        )
                        .ok()
                    };
                    let message = match (&outcome, &next) {
                        (CallOutcome::UsageExhausted { .. }, Some(n)) => format!("{p} usage unavailable. Continuing {} with {}.", role_activity(task.role), provider_name(&n.provider)),
                        (CallOutcome::SignedOut, Some(n)) => format!("{p} signed out — reconnect to continue {p} work. Continuing with {} meanwhile.", provider_name(&n.provider)),
                        (CallOutcome::SignedOut, None) => format!("{p} signed out — reconnect to continue {p} work."),
                        (_, Some(n)) => format!("{p} {last_reason}. Continuing {} with {}.", role_activity(task.role), provider_name(&n.provider)),
                        (_, None) => format!("{p} {last_reason}. No other model available for {}.", role_activity(task.role)),
                    };
                    self.emit(RunnerEvent::Notice { message });
                    last_model = Some(route.model.clone());
                    if !self.combo.fallback || next.is_none() {
                        return (
                            id,
                            TaskOutcome::Retry {
                                attempts,
                                failed_models,
                                reason: last_reason,
                            },
                        );
                    }
                }
            }
        }
        (
            id,
            TaskOutcome::Retry {
                attempts,
                failed_models,
                reason: last_reason,
            },
        )
    }

    fn needs_review(&self, task: &TaskNode) -> bool {
        if matches!(
            task.role,
            Role::Reviewer | Role::SecurityReviewer | Role::Planner | Role::Researcher
        ) {
            return false;
        }
        match self.combo.review {
            ReviewPolicy::Off => false,
            ReviewPolicy::Always => true,
            ReviewPolicy::Important => task.important,
        }
    }

    fn handoff(&self, goal: &Goal, task: &TaskNode, from: &str, reason: &str) -> HandoffPacket {
        let completed: Vec<String> = goal
            .graph
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .map(|t| {
                format!(
                    "{}: {}",
                    t.title,
                    first_line(t.result.as_deref().unwrap_or(""))
                )
            })
            .collect();
        let changed: Vec<String> = goal
            .graph
            .tasks
            .iter()
            .flat_map(|t| t.files.clone())
            .collect();
        let failures: Vec<String> = goal
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| format!("{}: {}", c.check, first_line(&c.evidence)))
            .collect();
        let mut p = HandoffPacket {
            goal: goal.contract.objective.clone(),
            subtask: format!("{} — {}", task.title, task.detail),
            role: task.role.label().into(),
            from_model: Some(from.into()),
            reason: Some(reason.into()),
            completed,
            decisions: goal.contract.decisions.clone(),
            constraints: goal.contract.constraints.clone(),
            changed_files: changed,
            failures,
            next_steps: vec![format!("Finish: {}", task.title)],
            ..Default::default()
        };
        if let Some(r) = &task.result {
            p.tool_state = Some(format!("Partial result: {}", first_line(r)));
        }
        p.fit(self.config.handoff_budget_tokens);
        p
    }

    fn apply(&self, goal: &mut Goal, id: &str, outcome: TaskOutcome) {
        let mut add_review: Option<TaskNode> = None;
        let Some(t) = goal.graph.get_mut(id) else {
            return;
        };
        match outcome {
            TaskOutcome::Done(reply, model, review, failed, attempts) => {
                // Keep the failover history so the UI can show reassignment.
                if !failed.is_empty() {
                    t.failed_models = failed;
                    t.attempts = attempts;
                }
                t.tokens += reply.tokens;
                for f in reply.files_changed {
                    if !t.files.contains(&f) {
                        t.files.push(f);
                    }
                }
                t.assigned_model = Some(model.clone());
                t.finished_at = Some(now());
                if t.role == Role::Reviewer || t.role == Role::SecurityReviewer {
                    let lower = reply.text.to_lowercase();
                    if lower.contains("changes requested")
                        || lower.contains("request changes")
                        || lower.contains("reject")
                    {
                        t.status = TaskStatus::Done;
                        let mut fix = TaskNode::new(
                            &format!("{id}-fix{}", t.attempts + 1),
                            &format!("Address review: {}", first_line(&reply.text)),
                            Role::Coder,
                            &[],
                        );
                        fix.detail = reply.text.clone();
                        fix.files = t.files.clone();
                        add_review = Some(fix);
                    } else {
                        t.status = TaskStatus::Done;
                    }
                } else {
                    t.status = TaskStatus::Done;
                    if review {
                        let mut r = TaskNode::new(
                            &format!("{id}-review"),
                            &format!("Review: {}", t.title),
                            Role::Reviewer,
                            &[id],
                        );
                        r.detail = format!("Independently review the result of '{}'. Reply APPROVE or CHANGES REQUESTED with specifics.\n\nResult:\n{}", t.title, reply.text);
                        r.avoid_models = vec![model]; // prefer an independent reviewer
                        r.files = t.files.clone();
                        add_review = Some(r);
                    }
                }
                t.result = Some(reply.text);
                self.emit(RunnerEvent::TaskFinished {
                    task: id.to_string(),
                    status: TaskStatus::Done,
                });
            }
            TaskOutcome::Retry {
                attempts,
                failed_models,
                reason,
            } => {
                t.attempts = attempts;
                t.failed_models = failed_models;
                t.status = TaskStatus::Failed;
                t.verification = Some(reason);
                self.emit(RunnerEvent::TaskFinished {
                    task: id.to_string(),
                    status: TaskStatus::Failed,
                });
            }
            TaskOutcome::Failed(reason) => {
                t.status = TaskStatus::Failed;
                t.verification = Some(reason);
                self.emit(RunnerEvent::TaskFinished {
                    task: id.to_string(),
                    status: TaskStatus::Failed,
                });
            }
            TaskOutcome::Cancelled => {
                t.status = TaskStatus::Pending;
            }
        }
        if let Some(n) = add_review {
            if goal.graph.get(&n.id).is_none() {
                goal.graph.tasks.push(n);
            }
        }
    }

    async fn verify(&self, goal: &mut Goal, cancel: &CancellationToken) -> GoalState {
        self.set_state(goal, GoalState::Verifying);
        let checks = goal.contract.done_checks.clone();
        let mut failing: Vec<CheckResult> = Vec::new();
        for c in &checks {
            if cancel.is_cancelled() {
                return GoalState::Stopped;
            }
            let result = match c {
                DoneCheck::Command { command } => {
                    self.verifier.run_check(command, cancel.clone()).await
                }
                DoneCheck::Criterion { text } => self.judge(goal, text, cancel).await,
            };
            self.emit(RunnerEvent::Check {
                check: result.check.clone(),
                passed: result.passed,
            });
            if !result.passed {
                failing.push(result.clone());
            }
            goal.checks.push(result);
        }
        if failing.is_empty() && goal.can_complete().is_ok() {
            self.emit(RunnerEvent::Complete);
            return GoalState::Complete;
        }
        goal.verification_rounds += 1;
        if goal.verification_rounds >= self.config.max_verification_rounds {
            let reason = format!(
                "still failing after {} verification round(s): {}",
                goal.verification_rounds,
                goal.unverified().join(", ")
            );
            goal.notes.push(reason.clone());
            self.emit(RunnerEvent::Blocked { reason });
            return GoalState::Blocked;
        }
        for f in failing {
            let mut fix = TaskNode::new(
                &format!("fix-{}-{}", goal.verification_rounds, sanitize(&f.check)),
                &format!("Fix failing check: {}", f.check),
                Role::Debugger,
                &[],
            );
            fix.detail = format!(
                "The Definition of Done check `{}` failed. Evidence:\n{}",
                f.check, f.evidence
            );
            fix.important = true;
            goal.graph.tasks.push(fix);
        }
        GoalState::Running
    }

    async fn judge(&self, goal: &Goal, criterion: &str, cancel: &CancellationToken) -> CheckResult {
        let req = RouteRequest {
            role: Some(Role::Reviewer),
            phase: Some(Phase::Review),
            ..Default::default()
        };
        let route = {
            let usage = self.usage.lock().expect("usage lock");
            router::route(&self.combo, &self.models, &usage, &req, now())
        };
        let Ok(route) = route else {
            return CheckResult {
                check: criterion.into(),
                passed: false,
                evidence: "no reviewer model available".into(),
            };
        };
        let summary: String = goal
            .graph
            .tasks
            .iter()
            .map(|t| {
                format!(
                    "- {}: {}\n",
                    t.title,
                    first_line(t.result.as_deref().unwrap_or(""))
                )
            })
            .collect();
        let call = AgentCall {
            model: route.model,
            effort: route.member_effort,
            role: Role::Reviewer,
            system: self.system(Role::Reviewer),
            prompt: format!("{}\n\nWork done:\n{summary}\nDoes the work satisfy this criterion: \"{criterion}\"? Reply PASS or FAIL on the first line, then evidence.", goal.contract.render()),
            task_id: "verify".into(),
        };
        match self.client.run(call, cancel.clone()).await {
            Ok(r) => CheckResult {
                check: criterion.into(),
                passed: r.text.trim_start().to_uppercase().starts_with("PASS"),
                evidence: r.text,
            },
            Err(e) => CheckResult {
                check: criterion.into(),
                passed: false,
                evidence: e.to_string(),
            },
        }
    }
}

enum TaskOutcome {
    /// reply, model, needs review, models that failed first, attempts
    Done(AgentReply, String, bool, Vec<String>, u32),
    Retry {
        attempts: u32,
        failed_models: Vec<String>,
        reason: String,
    },
    Failed(String),
    Cancelled,
}

fn first_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(24)
        .collect()
}

fn dependency_context(goal: &Goal, task: &TaskNode) -> String {
    let mut s = String::new();
    for d in &task.deps {
        if let Some(dep) = goal.graph.get(d) {
            if let Some(r) = &dep.result {
                let clipped: String = r.chars().take(1500).collect();
                s.push_str(&format!("\n\n## Result of '{}'\n{}", dep.title, clipped));
            }
        }
    }
    s
}

/// Minimal join_all to avoid pulling in the `futures` crate.
async fn futures_join_all<F, T>(futs: impl IntoIterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T>,
{
    let futs: Vec<_> = futs.into_iter().map(Box::pin).collect();
    let mut out = Vec::with_capacity(futs.len());
    // Poll concurrently using a simple select loop.
    let mut pending: Vec<(usize, std::pin::Pin<Box<F>>)> = futs.into_iter().enumerate().collect();
    let mut results: Vec<Option<T>> = (0..pending.len()).map(|_| None).collect();
    std::future::poll_fn(|cx| {
        pending.retain_mut(|(i, f)| match f.as_mut().poll(cx) {
            std::task::Poll::Ready(v) => {
                results[*i] = Some(v);
                false
            }
            std::task::Poll::Pending => true,
        });
        if pending.is_empty() {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
    out.extend(results.into_iter().flatten());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combo::ComboMember;
    use crate::goal::GoalContract;
    use crate::model::{test_model, Tier};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeClient {
        /// model key -> behaviour
        fail: HashMap<String, CallOutcome>,
        calls: Mutex<Vec<AgentCall>>,
        review_reply: String,
        count: AtomicUsize,
    }

    #[async_trait]
    impl ModelClient for FakeClient {
        async fn run(
            &self,
            call: AgentCall,
            _cancel: CancellationToken,
        ) -> Result<AgentReply, ModelError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.calls.lock().unwrap().push(call.clone());
            if let Some(o) = self.fail.get(&call.model) {
                return Err(ModelError::Provider(*o));
            }
            let text = match call.role {
                Role::Planner => r#"{"tasks":[{"id":"research","title":"Research approach","role":"researcher"},{"id":"code","title":"Implement feature","role":"coder","deps":["research"],"important":true,"files":["src/lib.rs"]}]}"#.to_string(),
                Role::Reviewer => self.review_reply.clone(),
                _ => format!("done by {}", call.model),
            };
            Ok(AgentReply {
                text,
                tokens: 100,
                files_changed: vec![],
            })
        }
    }

    struct FakeVerifier {
        pass_after: usize,
        runs: AtomicUsize,
    }

    #[async_trait]
    impl Verifier for FakeVerifier {
        async fn run_check(&self, command: &str, _c: CancellationToken) -> CheckResult {
            let n = self.runs.fetch_add(1, Ordering::SeqCst);
            CheckResult {
                check: command.into(),
                passed: n >= self.pass_after,
                evidence: if n >= self.pass_after {
                    "ok".into()
                } else {
                    "test failed: assertion".into()
                },
            }
        }
    }

    fn runner(
        fail: HashMap<String, CallOutcome>,
        pass_after: usize,
        review: &str,
    ) -> (GoalRunner, Arc<FakeClient>, Arc<Mutex<Vec<RunnerEvent>>>) {
        let models: HashMap<_, _> = [
            test_model("openai", "coder", Tier::Frontier),
            test_model("anthropic", "claude", Tier::Frontier),
            test_model("google", "flash", Tier::Fast),
        ]
        .into_iter()
        .map(|m| (m.key(), m))
        .collect();
        let combo = Combo::new(
            "t",
            vec![
                ComboMember {
                    model: "openai/coder".into(),
                    roles: vec![Role::Coder],
                    effort: None,
                    enabled: true,
                    weight: 1.0,
                },
                ComboMember {
                    model: "anthropic/claude".into(),
                    roles: vec![Role::Reviewer],
                    effort: None,
                    enabled: true,
                    weight: 1.0,
                },
                ComboMember {
                    model: "google/flash".into(),
                    roles: vec![Role::Planner, Role::Researcher],
                    effort: None,
                    enabled: true,
                    weight: 1.0,
                },
            ],
        );
        let client = Arc::new(FakeClient {
            fail,
            calls: Mutex::new(vec![]),
            review_reply: review.into(),
            count: AtomicUsize::new(0),
        });
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let r = GoalRunner {
            combo,
            models,
            usage: Arc::new(Mutex::new(UsageTracker::default())),
            ledger: Arc::new(Mutex::new(WorkLedger::default())),
            client: client.clone(),
            verifier: Arc::new(FakeVerifier {
                pass_after,
                runs: AtomicUsize::new(0),
            }),
            config: RunnerConfig::default(),
            events: Arc::new(move |e| ev.lock().unwrap().push(e)),
            on_progress: None,
        };
        (r, client, events)
    }

    fn goal() -> Goal {
        Goal::new(GoalContract {
            objective: "Add a feature".into(),
            constraints: vec!["No new dependencies".into()],
            done_checks: vec![DoneCheck::Command {
                command: "cargo test".into(),
            }],
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn plans_runs_reviews_and_verifies() {
        let (r, client, events) = runner(HashMap::new(), 0, "APPROVE looks good");
        let mut g = goal();
        let s = r.run(&mut g, CancellationToken::new()).await;
        assert_eq!(s, GoalState::Complete, "{:?}", g.notes);
        // planner, researcher, coder, reviewer
        let calls = client.calls.lock().unwrap();
        let roles: Vec<Role> = calls.iter().map(|c| c.role).collect();
        assert_eq!(
            roles,
            vec![Role::Planner, Role::Researcher, Role::Coder, Role::Reviewer]
        );
        // reviewer is independent from implementer
        let coder_model = &calls.iter().find(|c| c.role == Role::Coder).unwrap().model;
        let reviewer_model = &calls
            .iter()
            .find(|c| c.role == Role::Reviewer)
            .unwrap()
            .model;
        assert_ne!(coder_model, reviewer_model);
        // contract is authoritative in every prompt
        assert!(calls
            .iter()
            .all(|c| c.prompt.contains("No new dependencies")));
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, RunnerEvent::Complete)));
    }

    #[tokio::test]
    async fn exhausted_provider_hands_off_without_restart() {
        let mut fail = HashMap::new();
        fail.insert(
            "openai/coder".to_string(),
            CallOutcome::UsageExhausted {
                retry_after_secs: 3600,
            },
        );
        let (r, client, events) = runner(fail, 0, "APPROVE");
        let mut g = goal();
        let s = r.run(&mut g, CancellationToken::new()).await;
        assert_eq!(s, GoalState::Complete, "{:?}", g.notes);
        let ev = events.lock().unwrap();
        let notice = ev
            .iter()
            .find_map(|e| match e {
                RunnerEvent::Notice { message } => Some(message.clone()),
                _ => None,
            })
            .unwrap();
        assert!(
            notice.starts_with("Codex usage unavailable. Continuing implementation with"),
            "{notice}"
        );
        assert!(ev.iter().any(|e| matches!(e, RunnerEvent::Handoff { .. })));
        let calls = client.calls.lock().unwrap();
        let handoff_call = calls
            .iter()
            .filter(|c| c.role == Role::Coder)
            .nth(1)
            .unwrap();
        assert!(handoff_call.prompt.contains("Do not restart from scratch"));
        assert_ne!(handoff_call.model, "openai/coder");
        let coder_task = g
            .graph
            .tasks
            .iter()
            .find(|t| t.role == Role::Coder)
            .unwrap();
        assert_eq!(
            coder_task.failed_models,
            vec!["openai/coder".to_string()],
            "failover history kept"
        );
        assert_ne!(coder_task.assigned_model.as_deref(), Some("openai/coder"));
        // Research was not redone after handoff.
        assert_eq!(
            calls.iter().filter(|c| c.role == Role::Researcher).count(),
            1
        );
    }

    #[tokio::test]
    async fn failing_checks_create_fix_tasks_then_pass() {
        let (r, _c, events) = runner(HashMap::new(), 1, "APPROVE");
        let mut g = goal();
        let s = r.run(&mut g, CancellationToken::new()).await;
        assert_eq!(s, GoalState::Complete);
        assert!(g
            .graph
            .tasks
            .iter()
            .any(|t| t.title.starts_with("Fix failing check")));
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, RunnerEvent::Check { .. }))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn never_completes_with_failing_checks() {
        let (r, _c, _e) = runner(HashMap::new(), usize::MAX, "APPROVE");
        let mut g = goal();
        let s = r.run(&mut g, CancellationToken::new()).await;
        assert_eq!(s, GoalState::Blocked);
        assert_eq!(g.unverified(), vec!["cargo test"]);
        assert!(g.can_complete().is_err());
    }

    #[tokio::test]
    async fn review_changes_requested_adds_fix() {
        let (r, _c, _e) = runner(HashMap::new(), 0, "CHANGES REQUESTED: missing null check");
        let mut g = goal();
        let s = r.run(&mut g, CancellationToken::new()).await;
        assert_eq!(s, GoalState::Complete);
        assert!(g
            .graph
            .tasks
            .iter()
            .any(|t| t.title.starts_with("Address review")));
    }

    #[tokio::test]
    async fn single_model_goal_reviews_itself_instead_of_blocking() {
        let (mut r, client, events) = runner(HashMap::new(), 0, "APPROVE");
        r.combo = Combo::new(
            "solo",
            vec![ComboMember {
                model: "openai/coder".into(),
                roles: vec![],
                effort: None,
                enabled: true,
                weight: 1.0,
            }],
        );
        let mut g = goal();
        let s = r.run(&mut g, CancellationToken::new()).await;
        assert_eq!(s, GoalState::Complete, "{:?}", g.notes);
        assert!(client
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.role == Role::Reviewer));
        assert!(events.lock().unwrap().iter().any(|e| matches!(e, RunnerEvent::Notice { message } if message.contains("reviews its own work"))));
    }

    #[tokio::test]
    async fn cancellation_stops() {
        let (r, _c, _e) = runner(HashMap::new(), 0, "APPROVE");
        let mut g = goal();
        let c = CancellationToken::new();
        c.cancel();
        assert_eq!(r.run(&mut g, c).await, GoalState::Stopped);
    }

    #[tokio::test]
    async fn all_providers_down_blocks_with_reason() {
        let mut fail = HashMap::new();
        for m in ["openai/coder", "anthropic/claude", "google/flash"] {
            fail.insert(m.to_string(), CallOutcome::SignedOut);
        }
        let (r, _c, events) = runner(fail, 0, "APPROVE");
        let mut g = goal();
        g.graph = TaskGraph::default_plan("x");
        let s = r.run(&mut g, CancellationToken::new()).await;
        assert_eq!(s, GoalState::Blocked);
        assert!(events.lock().unwrap().iter().any(
            |e| matches!(e, RunnerEvent::Notice { message } if message.contains("signed out"))
        ));
    }
}
