use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use conductor_core::store::Store;
use conductor_engine::approvals::UiApprovals;
use conductor_engine::goals::GoalService;
use conductor_engine::EngineEvent;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::prefs::Prefs;

pub struct AppState {
    pub store: Arc<Store>,
    pub data_dir: PathBuf,
    pub portable: bool,
    /// conversation id -> cancellation token of an active chat/agent run
    pub runs: Mutex<HashMap<String, CancellationToken>>,
    pub prefs: Mutex<Prefs>,
    pub events: broadcast::Sender<EngineEvent>,
    pub goals: Arc<GoalService>,
    pub approvals: Arc<UiApprovals>,
    pub host: tokio::sync::Mutex<Option<conductor_remote::HostHandle>>,
    pub tunnels: tokio::sync::Mutex<conductor_tools::tunnels::TunnelManager>,
    /// Paths of project-index caches by project id.
    pub started_at: std::time::Instant,
    pub startup_ms: Mutex<Vec<(String, u128)>>,
    pub quitting: std::sync::atomic::AtomicBool,
    /// Whether the global Emergency Stop shortcut was registered with the OS.
    pub emergency_registered: std::sync::atomic::AtomicBool,
}

pub type CmdResult<T> = Result<T, String>;

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl AppState {
    pub fn mark(&self, phase: &str) {
        if let Ok(mut v) = self.startup_ms.lock() {
            v.push((phase.to_string(), self.started_at.elapsed().as_millis()));
        }
    }

    pub fn prefs(&self) -> Prefs {
        self.prefs.lock().map(|p| p.clone()).unwrap_or_default()
    }

    pub fn any_active_work(&self) -> bool {
        !self.runs.lock().map(|r| r.is_empty()).unwrap_or(true)
            || !self.goals.running_ids().is_empty()
    }
}
