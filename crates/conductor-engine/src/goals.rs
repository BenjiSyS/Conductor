//! Goal service: create, run, stop, persist and resume Goals.
//!
//! Goals are stored one JSON file per Goal (atomic writes) and saved after
//! every state change and task batch. Work runs on background tasks, so it
//! continues while the window is hidden; Goals that were mid-flight when the
//! process died are marked Paused on next start and resume from their task
//! graph — never from scratch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use conductor_core::domain::{Mode, ProviderConfig, Settings};
use conductor_orchestrator::combo::Combo;
use conductor_orchestrator::effort::MaxPolicy;
use conductor_orchestrator::goal::{Goal, GoalContract, GoalState};
use conductor_orchestrator::ledger::WorkLedger;
use conductor_orchestrator::model::ModelProfile;
use conductor_orchestrator::runner::{GoalRunner, RunnerConfig};
use conductor_orchestrator::usage::UsageTracker;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::agent::{AgentEnv, CoreModelClient, SecretFn};
use crate::approvals::Approver;
use crate::events::EngineEvent;
use crate::verifier::GateVerifier;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalRecord {
    pub goal: Goal,
    pub project_id: String,
    pub project_root: PathBuf,
    pub combo_id: String,
    pub created: u64,
    pub updated: u64,
    #[serde(default)]
    pub checkpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalSummary {
    pub id: String,
    pub project_id: String,
    pub objective: String,
    pub state: GoalState,
    pub done: usize,
    pub total: usize,
    pub updated: u64,
}

pub struct StartDeps {
    pub providers: Vec<ProviderConfig>,
    pub secrets: SecretFn,
    pub settings: Settings,
    pub combo: Combo,
    pub models: Vec<ModelProfile>,
    pub approver: Arc<dyn Approver>,
    pub max_parallel: usize,
    pub extra_system: String,
    pub max_effort: HashMap<String, MaxPolicy>,
    pub live_settings: Option<crate::toolbox::LiveSettings>,
}

pub struct GoalService {
    dir: PathBuf,
    pub events: broadcast::Sender<EngineEvent>,
    running: Mutex<HashMap<String, CancellationToken>>,
    usage: Arc<Mutex<UsageTracker>>,
    ledger: Arc<Mutex<WorkLedger>>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn save_record(dir: &Path, r: &GoalRecord) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let p = dir.join(format!("{}.json", r.goal.id));
    let tmp = p.with_extension("tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(r).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(tmp, p).map_err(|e| e.to_string())
}

impl GoalService {
    pub fn new(data_dir: &Path, events: broadcast::Sender<EngineEvent>) -> Arc<Self> {
        Arc::new(Self {
            dir: data_dir.join("goals"),
            events,
            running: Mutex::new(HashMap::new()),
            usage: Arc::new(Mutex::new(UsageTracker::default())),
            ledger: Arc::new(Mutex::new(WorkLedger::default())),
        })
    }

    pub fn usage(&self) -> UsageTracker {
        self.usage.lock().expect("usage lock").clone()
    }

    pub fn get(&self, id: &str) -> Option<GoalRecord> {
        let b = std::fs::read(self.dir.join(format!("{id}.json"))).ok()?;
        serde_json::from_slice(&b).ok()
    }

    pub fn list(&self) -> Vec<GoalSummary> {
        let mut v: Vec<GoalSummary> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| std::fs::read(e.path()).ok())
            .filter_map(|b| serde_json::from_slice::<GoalRecord>(&b).ok())
            .map(|r| {
                let (done, total) = r.goal.graph.progress();
                GoalSummary {
                    id: r.goal.id.clone(),
                    project_id: r.project_id,
                    objective: r.goal.contract.objective,
                    state: r.goal.state,
                    done,
                    total,
                    updated: r.updated,
                }
            })
            .collect();
        v.sort_by_key(|g| std::cmp::Reverse(g.updated));
        v
    }

    pub fn create(
        &self,
        project_id: &str,
        root: &Path,
        contract: GoalContract,
        combo_id: &str,
    ) -> Result<GoalRecord, String> {
        if contract.objective.trim().is_empty() {
            return Err("Describe the goal first".into());
        }
        let goal = Goal::new(contract);
        let r = GoalRecord {
            goal,
            project_id: project_id.into(),
            project_root: root.to_path_buf(),
            combo_id: combo_id.into(),
            created: now(),
            updated: now(),
            checkpoint: None,
        };
        save_record(&self.dir, &r)?;
        Ok(r)
    }

    /// Edit the Goal Contract (authoritative for all agents).
    pub fn update_contract(
        &self,
        id: &str,
        mut contract: GoalContract,
    ) -> Result<GoalRecord, String> {
        let mut r = self.get(id).ok_or("Goal not found")?;
        contract.revision = r.goal.contract.revision + 1;
        r.goal.contract = contract;
        r.updated = now();
        save_record(&self.dir, &r)?;
        Ok(r)
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.lock().expect("running lock").contains_key(id)
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.running
            .lock()
            .expect("running lock")
            .keys()
            .cloned()
            .collect()
    }

    /// Mark Goals interrupted by a crash/exit as Paused (resumable).
    pub fn recover_interrupted(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in self.list() {
            if matches!(
                s.state,
                GoalState::Running | GoalState::Planning | GoalState::Verifying
            ) && !self.is_running(&s.id)
            {
                if let Some(mut r) = self.get(&s.id) {
                    r.goal.state = GoalState::Paused;
                    r.goal.notes.push("Interrupted when Conductor closed. Resume to continue from where it stopped.".into());
                    if save_record(&self.dir, &r).is_ok() {
                        out.push(s.id);
                    }
                }
            }
        }
        out
    }

    /// Start (or resume) a Goal on a background task.
    pub fn start(self: &Arc<Self>, id: &str, deps: StartDeps) -> Result<(), String> {
        let mut record = self.get(id).ok_or("Goal not found")?;
        if matches!(record.goal.state, GoalState::Complete) {
            return Err("This Goal is already complete".into());
        }
        let cancel = CancellationToken::new();
        {
            let mut running = self.running.lock().expect("running lock");
            if running.contains_key(id) {
                return Err("This Goal is already running".into());
            }
            running.insert(id.to_string(), cancel.clone());
        }
        let svc = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            // Checkpoint before autonomous work starts (Git projects).
            if record.checkpoint.is_none() {
                let git = conductor_tools::git::Git::new(&record.project_root);
                if git.is_repo().await {
                    if let Ok(cp) = conductor_tools::checkpoint::Checkpoints::new(&git)
                        .create(&format!(
                            "before goal: {}",
                            record
                                .goal
                                .contract
                                .objective
                                .chars()
                                .take(60)
                                .collect::<String>()
                        ))
                        .await
                    {
                        record.checkpoint = Some(cp.id);
                    }
                }
            }
            let env = Arc::new(AgentEnv {
                providers: deps.providers,
                secrets: deps.secrets,
                settings: deps.settings,
                project_root: record.project_root.clone(),
                approver: deps.approver,
                events: svc.events.clone(),
                goal_id: Some(id.clone()),
                max_steps: 12,
                mode: Mode::Goal,
                extra_system: deps.extra_system,
                live_settings: deps.live_settings,
            });
            let ev_tx = svc.events.clone();
            let gid = id.clone();
            let dir = svc.dir.clone();
            let meta = (
                record.project_id.clone(),
                record.project_root.clone(),
                record.combo_id.clone(),
                record.created,
                record.checkpoint.clone(),
            );
            let ev_tx2 = ev_tx.clone();
            let runner = GoalRunner {
                combo: deps.combo,
                models: deps.models.into_iter().map(|m| (m.key(), m)).collect(),
                usage: svc.usage.clone(),
                ledger: svc.ledger.clone(),
                client: Arc::new(CoreModelClient { env }),
                verifier: Arc::new(GateVerifier {
                    root: record.project_root.clone(),
                    timeout_secs: 1800,
                }),
                config: RunnerConfig {
                    max_parallel: deps.max_parallel.max(1),
                    max_effort: deps.max_effort,
                    ..Default::default()
                },
                events: Arc::new(move |e| {
                    let _ = ev_tx.send(EngineEvent::Goal {
                        goal_id: gid.clone(),
                        event: e,
                    });
                }),
                on_progress: Some(Arc::new(move |g: &Goal| {
                    let r = GoalRecord {
                        goal: g.clone(),
                        project_id: meta.0.clone(),
                        project_root: meta.1.clone(),
                        combo_id: meta.2.clone(),
                        created: meta.3,
                        updated: now(),
                        checkpoint: meta.4.clone(),
                    };
                    let _ = save_record(&dir, &r);
                    let (done, total) = g.graph.progress();
                    let _ = ev_tx2.send(EngineEvent::GoalSaved {
                        goal_id: g.id.clone(),
                        state: format!("{:?}", g.state),
                        done,
                        total,
                    });
                })),
            };
            let mut goal = record.goal.clone();
            if goal.state == GoalState::Paused
                || goal.state == GoalState::Stopped
                || goal.state == GoalState::Blocked
            {
                goal.state = GoalState::Running;
            }
            runner.run(&mut goal, cancel).await;
            record.goal = goal;
            record.updated = now();
            let _ = save_record(&svc.dir, &record);
            let (done, total) = record.goal.graph.progress();
            let _ = svc.events.send(EngineEvent::GoalSaved {
                goal_id: id.clone(),
                state: format!("{:?}", record.goal.state),
                done,
                total,
            });
            svc.running.lock().expect("running lock").remove(&id);
        });
        Ok(())
    }

    pub fn stop(&self, id: &str) -> bool {
        match self.running.lock().expect("running lock").get(id) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// Emergency stop: cancel everything immediately.
    pub fn stop_all(&self) -> usize {
        let r = self.running.lock().expect("running lock");
        for t in r.values() {
            t.cancel();
        }
        r.len()
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if self.is_running(id) {
            return Err("Stop the Goal first".into());
        }
        std::fs::remove_file(self.dir.join(format!("{id}.json"))).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tests::{provider, Script};
    use crate::approvals::Fixed;
    use conductor_core::domain::PermissionLevel;
    use conductor_orchestrator::combo::ComboMember;
    use conductor_orchestrator::goal::DoneCheck;
    use conductor_orchestrator::roles::Role;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer};

    async fn git_repo(root: &Path) {
        let g = conductor_tools::git::Git::new(root);
        g.init().await.unwrap();
        g.run(&["config", "user.email", "t@e.com"]).await.unwrap();
        g.run(&["config", "user.name", "T"]).await.unwrap();
        g.run(&["config", "commit.gpgsign", "false"]).await.unwrap();
        std::fs::write(root.join("README.md"), "# demo\n").unwrap();
        g.commit("init", &["README.md"]).await.unwrap();
    }

    #[tokio::test]
    async fn goal_runs_end_to_end_with_tools_checkpoint_and_test_gate() {
        let server = MockServer::start().await;
        let plan = r#"{"tasks":[{"id":"code","title":"Create the greeting file","role":"coder","important":false}]}"#;
        let script = Script(std::sync::Mutex::new(vec![
            plan.to_string(),
            "<write_file path=\"greeting.txt\">hello world\n</write_file>".into(),
            "<done>Wrote greeting.txt</done>".into(),
        ]));
        Mock::given(method("POST"))
            .respond_with(script)
            .mount(&server)
            .await;
        let data = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        git_repo(proj.path()).await;
        let (tx, mut rx) = broadcast::channel(512);
        let svc = GoalService::new(data.path(), tx);
        let contract = GoalContract {
            objective: "Add a greeting file".into(),
            done_checks: vec![DoneCheck::Command {
                command: "git ls-files --others --exclude-standard --error-unmatch greeting.txt"
                    .into(),
            }],
            ..Default::default()
        };
        let rec = svc.create("p1", proj.path(), contract, "solo").unwrap();
        let p = provider(&format!("{}/v1", server.uri()), "local");
        let models = crate::profiles::profiles(std::slice::from_ref(&p));
        let combo = Combo::new(
            "solo",
            vec![ComboMember {
                model: "local/m".into(),
                roles: vec![],
                effort: None,
                enabled: true,
                weight: 1.0,
            }],
        );
        svc.start(
            &rec.goal.id,
            StartDeps {
                providers: vec![p],
                secrets: crate::agent::map_secrets(Default::default()),
                settings: Settings {
                    permission: PermissionLevel::FullAccess,
                    ..Default::default()
                },
                combo,
                models,
                approver: Arc::new(Fixed(false)),
                max_parallel: 1,
                extra_system: String::new(),
                max_effort: HashMap::new(),
                live_settings: None,
            },
        )
        .unwrap();
        assert!(
            svc.start(
                &rec.goal.id,
                StartDeps {
                    providers: vec![],
                    secrets: crate::agent::map_secrets(Default::default()),
                    settings: Settings::default(),
                    combo: Combo::new(
                        "x",
                        vec![ComboMember {
                            model: "a/b".into(),
                            roles: vec![Role::Coder],
                            effort: None,
                            enabled: true,
                            weight: 1.0
                        }]
                    ),
                    models: vec![],
                    approver: Arc::new(Fixed(false)),
                    max_parallel: 1,
                    extra_system: String::new(),
                    max_effort: HashMap::new(),
                    live_settings: None,
                }
            )
            .is_err(),
            "no double start"
        );
        // Wait for completion.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let ev = tokio::time::timeout_at(deadline, rx.recv())
                .await
                .expect("goal timed out")
                .unwrap();
            if let EngineEvent::GoalSaved { state, .. } = &ev {
                if state == "Complete" || state == "Blocked" {
                    break;
                }
            }
        }
        // allow final save
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let r = svc.get(&rec.goal.id).unwrap();
        assert_eq!(r.goal.state, GoalState::Complete, "{:?}", r.goal.notes);
        assert_eq!(
            std::fs::read_to_string(proj.path().join("greeting.txt")).unwrap(),
            "hello world\n"
        );
        assert!(
            r.checkpoint.is_some(),
            "checkpoint taken before autonomous work"
        );
        assert!(r.goal.checks.iter().any(|c| c.passed));
        assert_eq!(svc.list()[0].state, GoalState::Complete);
        assert!(!svc.is_running(&rec.goal.id));
    }

    #[tokio::test]
    async fn interrupted_goals_become_resumable() {
        let data = tempfile::tempdir().unwrap();
        let (tx, _) = broadcast::channel(16);
        let svc = GoalService::new(data.path(), tx);
        let mut rec = svc
            .create(
                "p",
                data.path(),
                GoalContract {
                    objective: "x".into(),
                    ..Default::default()
                },
                "c",
            )
            .unwrap();
        rec.goal.state = GoalState::Running;
        save_record(&data.path().join("goals"), &rec).unwrap();
        assert_eq!(svc.recover_interrupted(), vec![rec.goal.id.clone()]);
        assert_eq!(svc.get(&rec.goal.id).unwrap().goal.state, GoalState::Paused);
        let r = svc
            .update_contract(
                &rec.goal.id,
                GoalContract {
                    objective: "y".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(r.goal.contract.revision, 1);
        assert!(svc
            .create("p", data.path(), GoalContract::default(), "c")
            .is_err());
    }
}
