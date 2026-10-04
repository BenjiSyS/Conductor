//! Engine-backed commands: Goals, Combos, approvals, context, integrations,
//! Git, checkpoints, remote host, Caveman, diagnostics.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use conductor_core::domain::*;
use conductor_engine::combos::ComboStore;
use conductor_engine::goals::{GoalRecord, GoalSummary, StartDeps};
use conductor_engine::memory::ProjectMemory;
use conductor_engine::{paths, profiles};
use conductor_orchestrator::clarify::{self, Answer, QuestionRound};
use conductor_orchestrator::combo::Combo;
use conductor_orchestrator::effort::MaxPolicy;
use conductor_orchestrator::goal::{DoneCheck, GoalContract};
use conductor_orchestrator::model::ModelProfile;
use conductor_tools::mcp::{catalog, config::McpConfig, doctor};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;
use tokio_util::sync::CancellationToken;

use crate::chat::{self, project};
use crate::prefs::Prefs;
use crate::state::{err, AppState, CmdResult};

// ---------- app / prefs ----------

#[derive(Serialize)]
pub struct AppInfo {
    version: String,
    data_dir: String,
    portable: bool,
    os: String,
    startup: Vec<(String, u128)>,
    emergency_shortcut_registered: bool,
}

#[tauri::command]
pub async fn app_info(state: State<'_, AppState>) -> CmdResult<AppInfo> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: state.data_dir.display().to_string(),
        portable: state.portable,
        os: std::env::consts::OS.into(),
        startup: state
            .startup_ms
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default(),
        emergency_shortcut_registered: state
            .emergency_registered
            .load(std::sync::atomic::Ordering::SeqCst),
    })
}

#[tauri::command]
pub async fn prefs_get(state: State<'_, AppState>) -> CmdResult<Prefs> {
    Ok(state.prefs())
}

#[tauri::command]
pub async fn prefs_save(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    prefs: Prefs,
) -> CmdResult<()> {
    prefs.save(&state.data_dir)?;
    let old = std::mem::replace(
        &mut *state.prefs.lock().map_err(|_| "Preferences unavailable")?,
        prefs.clone(),
    );
    if old.launch_at_login != prefs.launch_at_login {
        use tauri_plugin_autostart::ManagerExt;
        let a = app.autolaunch();
        let r = if prefs.launch_at_login {
            a.enable()
        } else {
            a.disable()
        };
        r.map_err(|e| format!("Could not change launch at login: {e}"))?;
    }
    Ok(())
}

#[derive(Serialize)]
pub struct Hardware {
    total_memory_gb: f64,
    logical_cpus: usize,
    recommended: Performance,
    reason: String,
}

/// Lightweight, non-intrusive hardware assessment (no benchmark).
#[tauri::command]
pub async fn hardware() -> CmdResult<Hardware> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let gb = sys.total_memory() as f64 / 1024f64.powi(3);
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let (recommended, reason) = if gb < 10.0 || cpus <= 4 {
        (
            Performance::Potato,
            format!("{gb:.0} GB memory, {cpus} threads — keeps Conductor light"),
        )
    } else if gb >= 30.0 && cpus >= 12 {
        (
            Performance::Maximum,
            format!("{gb:.0} GB memory, {cpus} threads — plenty of headroom"),
        )
    } else {
        (
            Performance::Balanced,
            format!("{gb:.0} GB memory, {cpus} threads"),
        )
    };
    Ok(Hardware {
        total_memory_gb: gb,
        logical_cpus: cpus,
        recommended,
        reason,
    })
}

// ---------- models & combos ----------

fn providers(state: &AppState) -> CmdResult<Vec<ProviderConfig>> {
    state.store.list("provider").map_err(err)
}

#[tauri::command]
pub async fn models(state: State<'_, AppState>) -> CmdResult<Vec<ModelProfile>> {
    Ok(profiles::profiles(&providers(&state)?))
}

#[derive(Serialize)]
pub struct CombosView {
    combos: Vec<Combo>,
    default_combo: Option<String>,
}

#[tauri::command]
pub async fn combos_list(state: State<'_, AppState>) -> CmdResult<CombosView> {
    let avail = profiles::profiles(&providers(&state)?);
    let s = ComboStore::open(&state.data_dir);
    Ok(CombosView {
        combos: s.all(&avail),
        default_combo: s.default_combo(),
    })
}

#[tauri::command]
pub async fn combo_save(state: State<'_, AppState>, combo: Combo) -> CmdResult<()> {
    let mut s = ComboStore::open(&state.data_dir);
    // Edits are versioned: keep history of the previous members.
    let avail = profiles::profiles(&providers(&state)?);
    let mut next = combo.clone();
    if let Some(prev) = s.get(&combo.id, &avail) {
        if prev.members != combo.members || prev.strategy != combo.strategy {
            let mut base = prev.clone();
            base.edit("edited in Combo editor", |c| *c = combo.clone())
                .map_err(err)?;
            next = base;
            next.name = combo.name.clone();
        }
    }
    s.upsert(next)
}

#[tauri::command]
pub async fn combo_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    ComboStore::open(&state.data_dir).remove(&id)
}

#[tauri::command]
pub async fn combo_duplicate(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> CmdResult<Combo> {
    let avail = profiles::profiles(&providers(&state)?);
    let mut s = ComboStore::open(&state.data_dir);
    let c = s
        .get(&id, &avail)
        .ok_or("Combo not found")?
        .duplicate(&name);
    s.upsert(c.clone())?;
    Ok(c)
}

#[tauri::command]
pub async fn combo_set_default(state: State<'_, AppState>, id: Option<String>) -> CmdResult<()> {
    ComboStore::open(&state.data_dir).set_default(id)
}

#[tauri::command]
pub async fn combo_export(state: State<'_, AppState>, id: String) -> CmdResult<String> {
    let avail = profiles::profiles(&providers(&state)?);
    Ok(ComboStore::open(&state.data_dir)
        .get(&id, &avail)
        .ok_or("Combo not found")?
        .export())
}

#[tauri::command]
pub async fn combo_import(state: State<'_, AppState>, json: String) -> CmdResult<Value> {
    let avail = profiles::profiles(&providers(&state)?);
    let (c, warnings) = ComboStore::open(&state.data_dir).import(&json, &avail)?;
    Ok(json!({ "combo": c, "warnings": warnings }))
}

/// Preview of automatic effort for a prompt (shown in the effort control).
#[tauri::command]
pub async fn effort_preview(
    state: State<'_, AppState>,
    model: String,
    prompt: String,
) -> CmdResult<Value> {
    let avail = profiles::profiles(&providers(&state)?);
    let Some(p) = avail.iter().find(|m| m.key() == model) else {
        return Ok(json!({ "level": null, "reason": "model not available" }));
    };
    let settings = state.store.settings().map_err(err)?;
    let policy = match state.prefs().max_effort.get(&model).map(String::as_str) {
        Some("always_allow") => MaxPolicy::AlwaysAllow,
        Some("never") => MaxPolicy::Never,
        _ => MaxPolicy::Ask,
    };
    let d = conductor_orchestrator::effort::decide(&conductor_orchestrator::effort::EffortInput {
        task: &prompt,
        model: p,
        failed_attempts: 0,
        policy,
        allow_auto_max: settings.allow_highest_effort,
        difficulty: None,
    });
    Ok(json!({ "level": d.level.map(|l| l.as_str()), "reason": d.reason }))
}

// ---------- context ----------

#[tauri::command]
pub async fn context_pack(
    state: State<'_, AppState>,
    project_id: String,
    prompt: String,
    conversation_id: Option<String>,
) -> CmdResult<conductor_context::ContextPack> {
    let p = project(&state, &project_id)?;
    let settings = state.store.settings().map_err(err)?;
    let history = match conversation_id {
        Some(id) => state
            .store
            .get::<Conversation>("conversation", &id)
            .map_err(err)?
            .map(|c| c.messages)
            .unwrap_or_default(),
        None => vec![],
    };
    let prompt = if prompt.trim().is_empty() {
        "project overview".to_string()
    } else {
        prompt
    };
    let st: &AppState = &state;
    Ok(chat::context_pack(
        st,
        &p,
        &prompt,
        &history,
        settings.performance.context_bytes() / 4,
        settings.compression,
    ))
}

// ---------- memory ----------

#[tauri::command]
pub async fn memory_get(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<ProjectMemory> {
    Ok(ProjectMemory::load(&state.data_dir, &project_id))
}

#[tauri::command]
pub async fn memory_save(
    state: State<'_, AppState>,
    project_id: String,
    memory: ProjectMemory,
) -> CmdResult<()> {
    memory.save(&state.data_dir, &project_id)
}

// ---------- project ----------

#[derive(Serialize)]
pub struct ProjectInfo {
    detection: conductor_tools::project::Detection,
    config: Option<conductor_tools::project::ProjectConfig>,
    config_error: Option<String>,
    branch: Option<String>,
    resume: Option<Value>,
}

#[tauri::command]
pub async fn project_info(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<ProjectInfo> {
    let p = project(&state, &project_id)?;
    let root = PathBuf::from(&p.path);
    let detection = conductor_tools::project::detect(&root);
    let (config, config_error) = match conductor_tools::project::ProjectConfig::load(&root) {
        Ok(c) => (c, None),
        Err(e) => (None, Some(e.to_string())),
    };
    let git = conductor_tools::git::Git::new(&root);
    let branch = if detection.is_git {
        git.current_branch().await.ok().flatten()
    } else {
        None
    };
    // Project Resume Summary: only when there's something useful.
    let goals: Vec<GoalSummary> = state
        .goals
        .list()
        .into_iter()
        .filter(|g| g.project_id == project_id)
        .collect();
    let mem = ProjectMemory::load(&state.data_dir, &project_id);
    let last_goal = goals.first().cloned();
    let unfinished = goals
        .iter()
        .filter(|g| !matches!(g.state, conductor_orchestrator::goal::GoalState::Complete))
        .count();
    let resume = if last_goal.is_some() || !mem.decisions.is_empty() {
        Some(json!({
            "last_goal": last_goal,
            "unfinished_goals": unfinished,
            "branch": branch,
            "last_decision": mem.decisions.last().map(|d| format!("{}: {}", d.topic, d.value)),
            "attention": if unfinished > 0 { Some("A Goal is unfinished — resume it from Goals.") } else { None },
        }))
    } else {
        None
    };
    Ok(ProjectInfo {
        detection,
        config,
        config_error,
        branch,
        resume,
    })
}

#[tauri::command]
pub async fn recipes() -> CmdResult<Vec<conductor_tools::project::Recipe>> {
    Ok(conductor_tools::project::builtin_recipes())
}

#[tauri::command]
pub async fn project_remove(state: State<'_, AppState>, project_id: String) -> CmdResult<()> {
    state.store.remove("project", &project_id).map_err(err)
}

// ---------- goals ----------

#[tauri::command]
pub async fn goals_list(state: State<'_, AppState>) -> CmdResult<Vec<GoalSummary>> {
    Ok(state.goals.list())
}

#[tauri::command]
pub async fn goal_get(state: State<'_, AppState>, id: String) -> CmdResult<GoalRecord> {
    state.goals.get(&id).ok_or_else(|| "Goal not found".into())
}

/// Ask the planner for clarifying questions (≤ 7, filtered by decisions).
#[tauri::command]
pub async fn goal_clarify(
    state: State<'_, AppState>,
    project_id: String,
    objective: String,
    target: String,
) -> CmdResult<QuestionRound> {
    // Large pasted text arrives as chips; give the planner the text itself.
    let objective = conductor_engine::pastes::expand(
        &objective,
        &state.data_dir.join("pastes"),
        GOAL_PASTE_BUDGET,
    );
    let p = project(&state, &project_id)?;
    let mem = ProjectMemory::load(&state.data_dir, &project_id);
    let (model_key, _, _) = chat::resolve_target(&state, &target, Mode::Plan, &[])?;
    let (pid, mid) = model_key.split_once('/').ok_or("Choose a model")?;
    let config = providers(&state)?
        .into_iter()
        .find(|c| c.id == pid)
        .ok_or("Provider not connected")?;
    let secret = chat::key(&config)?;
    let det = conductor_tools::project::detect(Path::new(&p.path));
    let prompt = format!(
        "Project: {} ({}). Known decisions: {}.\nGoal: {objective}\n\n{}",
        p.name,
        det.kinds
            .iter()
            .map(|k| k.label())
            .collect::<Vec<_>>()
            .join(", "),
        mem.as_lines().join("; "),
        clarify::PLANNER_INSTRUCTIONS
    );
    let req = conductor_core::providers::ProviderRequest {
        model: mid.into(),
        messages: vec![Message::new(Role::User, prompt)],
        instructions: "You help clarify software goals. Reply with JSON only.".into(),
        effort: None,
        allow_highest_effort: false,
    };
    let mut text = String::new();
    conductor_core::providers::stream(&config, &secret, &req, CancellationToken::new(), |ev| {
        if let conductor_core::providers::StreamEvent::Delta { text: t } = ev {
            text.push_str(&t);
        }
        Ok(())
    })
    .await
    .map_err(err)?;
    Ok(clarify::build_round(
        1,
        clarify::parse_questions(&text),
        &mem.clarify_decisions(),
        false,
    ))
}

#[derive(Deserialize)]
pub struct NewGoal {
    project_id: String,
    objective: String,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    constraints: Vec<String>,
    #[serde(default)]
    platforms: Vec<String>,
    #[serde(default)]
    acceptance: Vec<String>,
    #[serde(default)]
    checks: Vec<String>,
    combo_id: String,
    #[serde(default)]
    round: Option<QuestionRound>,
    #[serde(default)]
    answers: Vec<(String, Answer)>,
}

/// How much pasted text a Goal objective may carry (characters).
const GOAL_PASTE_BUDGET: usize = 60_000;

#[tauri::command]
pub async fn goal_create(state: State<'_, AppState>, mut goal: NewGoal) -> CmdResult<GoalRecord> {
    goal.objective = conductor_engine::pastes::expand(
        &goal.objective,
        &state.data_dir.join("pastes"),
        GOAL_PASTE_BUDGET,
    );
    let p = project(&state, &goal.project_id)?;
    let mut mem = ProjectMemory::load(&state.data_dir, &goal.project_id);
    if let Some(round) = &goal.round {
        for d in clarify::resolve(round, &goal.answers) {
            mem.decide(&d.topic, &d.value, "clarification");
        }
        mem.save(&state.data_dir, &goal.project_id)?;
    }
    let mut checks = goal.checks.clone();
    if checks.is_empty() {
        if let Ok(Some(cfg)) = conductor_tools::project::ProjectConfig::load(Path::new(&p.path)) {
            checks = cfg.verify.commands;
        }
    }
    let contract = GoalContract {
        objective: goal.objective,
        requirements: goal.requirements,
        constraints: goal.constraints,
        platforms: goal.platforms,
        acceptance: goal.acceptance,
        done_checks: checks
            .into_iter()
            .filter(|c| !c.trim().is_empty())
            .map(|c| DoneCheck::Command { command: c })
            .collect(),
        decisions: mem.as_lines(),
        ..Default::default()
    };
    let r = state.goals.create(
        &goal.project_id,
        Path::new(&p.path),
        contract,
        &goal.combo_id,
    )?;
    let _ = state.store.history(
        Some(goal.project_id),
        "goal",
        &format!("Goal created: {}", r.goal.contract.objective),
    );
    Ok(r)
}

#[tauri::command]
pub async fn goal_update_contract(
    state: State<'_, AppState>,
    id: String,
    contract: GoalContract,
) -> CmdResult<GoalRecord> {
    state.goals.update_contract(&id, contract)
}

#[tauri::command]
pub async fn goal_start(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let rec = state.goals.get(&id).ok_or("Goal not found")?;
    let provs = providers(&state)?;
    let avail = profiles::profiles(&provs);
    if avail.is_empty() {
        return Err("Connect a provider first (Settings → Providers).".into());
    }
    let combos = ComboStore::open(&state.data_dir);
    let combo = if let Some(m) = rec.combo_id.strip_prefix("model:") {
        Combo::new(
            m,
            vec![conductor_orchestrator::combo::ComboMember {
                model: m.into(),
                roles: vec![],
                effort: None,
                enabled: true,
                weight: 1.0,
            }],
        )
    } else {
        combos
            .get(&rec.combo_id, &avail)
            .ok_or("That Combo is no longer available")?
    };
    let settings = state.store.settings().map_err(err)?;
    let prefs = state.prefs();
    let max_effort: HashMap<String, MaxPolicy> = prefs
        .max_effort
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                match v.as_str() {
                    "always_allow" => MaxPolicy::AlwaysAllow,
                    "never" => MaxPolicy::Never,
                    _ => MaxPolicy::Ask,
                },
            )
        })
        .collect();
    let caveman = conductor_tools::caveman::Caveman::open(&state.data_dir.join("components"));
    let mut extra = caveman
        .instruction(settings.caveman, "", &[])
        .map(|c| format!("# Response style\n{c}"))
        .unwrap_or_default();
    if !settings.instructions.trim().is_empty() {
        extra.push_str(&format!(
            "\n\n# User instructions\n{}",
            settings.instructions.trim()
        ));
    }
    for (role, text) in &prefs.role_instructions {
        if !text.trim().is_empty() {
            extra.push_str(&format!("\n\n# Notes for the {role} role\n{text}"));
        }
    }
    state.goals.start(
        &id,
        StartDeps {
            providers: provs,
            secrets: Arc::new(|id: &str| paths::secret(id)),
            integration_secrets: Arc::new(|name: &str| paths::integration_secret(name)),
            integration_project_id: Some(rec.project_id.clone()),
            max_parallel: settings.performance.parallel_agents(),
            settings,
            combo,
            models: avail,
            approver: state.approvals.clone(),
            extra_system: extra,
            max_effort,
            live_settings: Some({
                let st = state.store.clone();
                Arc::new(move || st.settings().ok())
            }),
        },
    )?;
    let _ = state.store.history(
        Some(rec.project_id),
        "goal",
        &format!("Goal started: {}", rec.goal.contract.objective),
    );
    Ok(())
}

#[tauri::command]
pub async fn goal_stop(state: State<'_, AppState>, id: String) -> CmdResult<bool> {
    Ok(state.goals.stop(&id))
}

#[tauri::command]
pub async fn goal_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.goals.delete(&id)
}

// ---------- approvals / stop ----------

#[tauri::command]
pub async fn approvals_pending(
    state: State<'_, AppState>,
) -> CmdResult<Vec<conductor_engine::approvals::ApprovalRequest>> {
    Ok(state.approvals.pending())
}

#[tauri::command]
pub async fn approval_resolve(
    state: State<'_, AppState>,
    id: String,
    allow: bool,
) -> CmdResult<bool> {
    Ok(state.approvals.resolve(&id, allow))
}

/// Emergency Stop: cancel every chat, agent, Goal, pending approval and
/// tunnel immediately.
#[tauri::command]
pub async fn emergency_stop(state: State<'_, AppState>) -> CmdResult<Value> {
    Ok(do_emergency_stop(&state).await)
}

pub async fn do_emergency_stop(state: &AppState) -> Value {
    let chats = {
        let runs = state.runs.lock().map(|r| r.clone()).unwrap_or_default();
        for t in runs.values() {
            t.cancel();
        }
        runs.len()
    };
    let goals = state.goals.stop_all();
    state.approvals.deny_all();
    state.tunnels.lock().await.close_all().await;
    tracing::warn!(chats, goals, "emergency stop");
    json!({ "chats": chats, "goals": goals })
}

/// A paired device's Stop applies only to its selected authorized project.
/// Host-wide emergency stop remains a local action.
async fn stop_project(state: &AppState, project_id: &str) {
    if let Ok(runs) = state.runs.lock() {
        for (id, token) in runs.iter() {
            let conversation: Option<Conversation> =
                state.store.get("conversation", id).ok().flatten();
            if conversation.is_some_and(|conversation| conversation.project_id == project_id) {
                token.cancel();
            }
        }
    }
    for goal in state
        .goals
        .list()
        .into_iter()
        .filter(|goal| goal.project_id == project_id)
    {
        state.goals.stop(&goal.id);
    }
    for request in state.approvals.pending() {
        if request
            .goal_id
            .as_deref()
            .and_then(|id| state.goals.get(id))
            .is_some_and(|goal| goal.project_id == project_id)
        {
            state.approvals.resolve(&request.id, false);
        }
    }
}

// ---------- MCP ----------

fn mcp_path(state: &AppState) -> PathBuf {
    state.data_dir.join("mcp.json")
}

#[tauri::command]
pub async fn mcp_catalog(query: Option<String>) -> CmdResult<Vec<catalog::CatalogEntry>> {
    let all = catalog::builtin();
    Ok(match query.filter(|q| !q.trim().is_empty()) {
        Some(q) => catalog::search(&all, &q).into_iter().cloned().collect(),
        None => all,
    })
}

#[tauri::command]
pub async fn mcp_list(state: State<'_, AppState>) -> CmdResult<McpConfig> {
    McpConfig::load(&mcp_path(&state)).map_err(err)
}

#[tauri::command]
pub async fn mcp_add(
    state: State<'_, AppState>,
    id: String,
    values: BTreeMap<String, String>,
) -> CmdResult<doctor::Diagnosis> {
    let entries = catalog::builtin();
    let e = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or("Unknown MCP server")?;
    let server = catalog::to_server(e, &values)?;
    let name = server.name.clone();
    let mut cfg = McpConfig::load(&mcp_path(&state)).map_err(err)?;
    cfg.upsert(server).map_err(err)?;
    cfg.save(&mcp_path(&state)).map_err(err)?;
    let mut receipts = conductor_tools::receipts::ReceiptStore::open(
        state.data_dir.join("receipts.json"),
        vec![
            state.data_dir.join("skills"),
            state.data_dir.join("plugins"),
            state.data_dir.join("themes"),
        ],
    )
    .map_err(err)?;
    let _ = receipts.record(conductor_tools::receipts::receipt(
        conductor_tools::receipts::InstallKind::Mcp,
        &name,
        &e.source,
        "latest",
        None,
        None,
        conductor_tools::receipts::Scope::Global,
        conductor_tools::receipts::Removal::Unregister,
    ));
    let d = doctor::diagnose(
        cfg.get(&name).ok_or("not saved")?,
        &|k| paths::integration_secret(k),
        90,
    )
    .await;
    let _ = state.store.history(
        None,
        "install",
        &format!("MCP {name}: {}", d.status.label()),
    );
    Ok(d)
}

#[tauri::command]
pub async fn mcp_add_custom(
    state: State<'_, AppState>,
    server: conductor_tools::mcp::McpServer,
) -> CmdResult<doctor::Diagnosis> {
    let name = server.name.clone();
    let mut cfg = McpConfig::load(&mcp_path(&state)).map_err(err)?;
    cfg.upsert(server).map_err(err)?;
    cfg.save(&mcp_path(&state)).map_err(err)?;
    Ok(doctor::diagnose(
        cfg.get(&name).ok_or("not saved")?,
        &|k| paths::integration_secret(k),
        60,
    )
    .await)
}

#[tauri::command]
pub async fn mcp_remove(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    let mut cfg = McpConfig::load(&mcp_path(&state)).map_err(err)?;
    cfg.remove(&name);
    cfg.save(&mcp_path(&state)).map_err(err)
}

#[tauri::command]
pub async fn mcp_set_enabled(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> CmdResult<()> {
    let mut cfg = McpConfig::load(&mcp_path(&state)).map_err(err)?;
    cfg.set_enabled(&name, enabled);
    cfg.save(&mcp_path(&state)).map_err(err)
}

#[tauri::command]
pub async fn mcp_doctor(
    state: State<'_, AppState>,
    name: Option<String>,
) -> CmdResult<Vec<doctor::Diagnosis>> {
    let cfg = McpConfig::load(&mcp_path(&state)).map_err(err)?;
    let mut out = Vec::new();
    for s in cfg
        .servers
        .iter()
        .filter(|s| name.as_ref().is_none_or(|n| n == &s.name))
    {
        out.push(doctor::diagnose(s, &|k| paths::integration_secret(k), 45).await);
    }
    Ok(out)
}

#[tauri::command]
pub async fn mcp_export(state: State<'_, AppState>, target: String) -> CmdResult<String> {
    let cfg = McpConfig::load(&mcp_path(&state)).map_err(err)?;
    Ok(match target.as_str() {
        "claude" => serde_json::to_string_pretty(&cfg.to_claude_json()).map_err(err)?,
        "gemini" => serde_json::to_string_pretty(&cfg.to_gemini_json()).map_err(err)?,
        "codex" => cfg.to_codex_toml(),
        "grok" => cfg.to_grok_toml(),
        _ => return Err("Unknown target".into()),
    })
}

/// Store an integration secret (e.g. an MCP token) in the OS keychain.
#[tauri::command]
pub async fn secret_set(name: String, value: String) -> CmdResult<()> {
    if name.trim().is_empty()
        || name.len() > 128
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid secret name".into());
    }
    paths::credential(&format!("secret:{name}"))?
        .set_password(&value)
        .map_err(|_| "Could not save to the OS credential store".into())
}

#[tauri::command]
pub async fn secret_delete(name: String) -> CmdResult<()> {
    match paths::credential(&format!("secret:{name}"))?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------- skills / plugins / themes ----------

#[tauri::command]
pub async fn skills_list(
    state: State<'_, AppState>,
) -> CmdResult<Vec<conductor_tools::skills::SkillInfo>> {
    Ok(conductor_tools::skills::Skills::new(state.data_dir.join("skills")).list())
}

#[tauri::command]
pub async fn skills_install(
    state: State<'_, AppState>,
    source: String,
    rev: Option<String>,
) -> CmdResult<conductor_tools::package::Installed> {
    let s = conductor_tools::skills::Skills::new(state.data_dir.join("skills"));
    let i = if Path::new(&source).exists() {
        s.install_dir(Path::new(&source))
    } else {
        s.install_git(&source, rev.as_deref()).await
    }
    .map_err(err)?;
    record_install(&state, conductor_tools::receipts::InstallKind::Skill, &i);
    Ok(i)
}

#[tauri::command]
pub async fn skills_remove(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    conductor_tools::skills::Skills::new(state.data_dir.join("skills"))
        .remove(&name)
        .map_err(err)
}

#[tauri::command]
pub async fn plugins_list(state: State<'_, AppState>) -> CmdResult<Vec<Value>> {
    let p = conductor_tools::plugins::Plugins::new(state.data_dir.join("plugins"));
    Ok(p.list()
        .into_iter()
        .map(|m| {
            let (h, d) = p.health(&m.name, None);
            json!({ "manifest": m, "health": h, "detail": d })
        })
        .collect())
}

#[tauri::command]
pub async fn plugins_install(
    state: State<'_, AppState>,
    source: String,
) -> CmdResult<conductor_tools::package::Installed> {
    let p = conductor_tools::plugins::Plugins::new(state.data_dir.join("plugins"));
    let i = if Path::new(&source).exists() {
        p.install_dir(Path::new(&source))
    } else {
        p.install_git(&source, None).await
    }
    .map_err(err)?;
    record_install(&state, conductor_tools::receipts::InstallKind::Plugin, &i);
    Ok(i)
}

#[tauri::command]
pub async fn plugins_remove(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    conductor_tools::plugins::Plugins::new(state.data_dir.join("plugins"))
        .remove(&name)
        .map_err(err)
}

#[tauri::command]
pub async fn themes_list(
    state: State<'_, AppState>,
) -> CmdResult<Vec<conductor_tools::themes::ThemeManifest>> {
    Ok(conductor_tools::themes::Themes::new(state.data_dir.join("themes")).list())
}

#[tauri::command]
pub async fn themes_install(
    state: State<'_, AppState>,
    source: String,
) -> CmdResult<conductor_tools::package::Installed> {
    let t = conductor_tools::themes::Themes::new(state.data_dir.join("themes"));
    let i = if Path::new(&source).exists() {
        t.install_dir(Path::new(&source))
    } else {
        t.install_git(&source, None).await
    }
    .map_err(err)?;
    record_install(&state, conductor_tools::receipts::InstallKind::Theme, &i);
    Ok(i)
}

#[tauri::command]
pub async fn themes_remove(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    conductor_tools::themes::Themes::new(state.data_dir.join("themes"))
        .remove(&name)
        .map_err(err)
}

#[tauri::command]
pub async fn theme_scaffold(dir: String, name: String) -> CmdResult<()> {
    conductor_tools::themes::Themes::scaffold(Path::new(&dir), &name).map_err(err)
}

fn record_install(
    state: &AppState,
    kind: conductor_tools::receipts::InstallKind,
    i: &conductor_tools::package::Installed,
) {
    if let Ok(mut r) = conductor_tools::receipts::ReceiptStore::open(
        state.data_dir.join("receipts.json"),
        vec![
            state.data_dir.join("skills"),
            state.data_dir.join("plugins"),
            state.data_dir.join("themes"),
        ],
    ) {
        let mut rc = conductor_tools::receipts::receipt(
            kind,
            &i.name,
            &i.source,
            &i.version,
            Some(i.path.clone()),
            Some(i.tree_hash.clone()),
            conductor_tools::receipts::Scope::Global,
            conductor_tools::receipts::Removal::Delete {
                path: i.path.clone(),
            },
        );
        rc.signature_verified = false;
        let _ = r.record(rc);
    }
    let _ = state.store.history(
        None,
        "install",
        &format!("Installed {:?} {} {}", kind, i.name, i.version),
    );
}

#[tauri::command]
pub async fn receipts_list(
    state: State<'_, AppState>,
) -> CmdResult<Vec<conductor_tools::receipts::InstallReceipt>> {
    let r =
        conductor_tools::receipts::ReceiptStore::open(state.data_dir.join("receipts.json"), vec![])
            .map_err(err)?;
    Ok(r.all().to_vec())
}

// ---------- environment ----------

#[tauri::command]
pub async fn env_doctor() -> CmdResult<Vec<conductor_tools::envdoctor::ToolReport>> {
    Ok(conductor_tools::envdoctor::check_all(None).await)
}

// ---------- git & checkpoints ----------

#[tauri::command]
pub async fn git_overview(state: State<'_, AppState>, project_id: String) -> CmdResult<Value> {
    let p = project(&state, &project_id)?;
    let g = conductor_tools::git::Git::new(&p.path);
    if !g.is_repo().await {
        return Ok(json!({ "repo": false }));
    }
    let status = g.status().await.map_err(err)?;
    let log = g.log(15).await.unwrap_or_default();
    let branches = g.branches().await.unwrap_or_default();
    let diff = g.diff(false, &[]).await.unwrap_or_default();
    let diff = conductor_security::secrets::redact(&diff).text;
    let diff: String = diff.chars().take(200_000).collect();
    Ok(
        json!({ "repo": true, "status": status, "summary": status.summary(), "log": log, "branches": branches, "diff": diff }),
    )
}

#[tauri::command]
pub async fn checkpoints_list(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<Vec<conductor_tools::checkpoint::Checkpoint>> {
    let p = project(&state, &project_id)?;
    let g = conductor_tools::git::Git::new(&p.path);
    if !g.is_repo().await {
        return Ok(vec![]);
    }
    conductor_tools::checkpoint::Checkpoints::new(&g)
        .list()
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn checkpoint_create(
    state: State<'_, AppState>,
    project_id: String,
    label: String,
) -> CmdResult<conductor_tools::checkpoint::Checkpoint> {
    let p = project(&state, &project_id)?;
    let g = conductor_tools::git::Git::new(&p.path);
    if !g.is_repo().await {
        return Err(
            "Checkpoints need a Git repository. Initialise Git for this project first.".into(),
        );
    }
    let c = conductor_tools::checkpoint::Checkpoints::new(&g)
        .create(&label)
        .await
        .map_err(err)?;
    let _ = state.store.history(
        Some(project_id),
        "checkpoint",
        &format!("Checkpoint: {label}"),
    );
    Ok(c)
}

#[tauri::command]
pub async fn checkpoint_restore(
    state: State<'_, AppState>,
    project_id: String,
    id: String,
) -> CmdResult<conductor_tools::checkpoint::Checkpoint> {
    if state.any_active_work() {
        return Err("Stop active work before restoring a checkpoint".into());
    }
    let p = project(&state, &project_id)?;
    let g = conductor_tools::git::Git::new(&p.path);
    let safety = conductor_tools::checkpoint::Checkpoints::new(&g)
        .restore(&id)
        .await
        .map_err(err)?;
    let _ = state.store.history(
        Some(project_id),
        "checkpoint",
        "Checkpoint restored (previous state saved)",
    );
    Ok(safety)
}

// ---------- remote host ----------

#[derive(Serialize)]
pub struct HostStatus {
    running: bool,
    addr: Option<String>,
    fingerprint: Option<String>,
    devices: Vec<conductor_remote::devices::Device>,
    connected: usize,
}

#[tauri::command]
pub async fn remote_status(state: State<'_, AppState>) -> CmdResult<HostStatus> {
    let h = state.host.lock().await;
    Ok(match h.as_ref() {
        Some(h) => HostStatus {
            running: true,
            addr: Some(h.addr.to_string()),
            fingerprint: Some(h.fingerprint.clone()),
            devices: h.devices(),
            connected: h.connected_clients(),
        },
        None => HostStatus {
            running: false,
            addr: None,
            fingerprint: None,
            devices: vec![],
            connected: 0,
        },
    })
}

#[tauri::command]
pub async fn remote_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<HostStatus> {
    {
        let mut h = state.host.lock().await;
        if h.is_none() {
            let prefs = state.prefs();
            if prefs.privacy == "restricted" {
                return Err("Remote access is off under the Restricted privacy profile.".into());
            }
            let projects: Vec<Project> = state.store.list("project").map_err(err)?;
            let map = projects
                .into_iter()
                .map(|p| (p.id.clone(), (p.name.clone(), PathBuf::from(p.path))))
                .collect();
            let mut handle = conductor_remote::Host::start(conductor_remote::HostConfig {
                bind: prefs.remote_bind.clone(),
                port: prefs.remote_port,
                data_dir: state.data_dir.join("remote"),
                projects: map,
            })
            .await?;
            // Forward remote actions to the UI (prompts, approvals, stop).
            let mut inbound =
                std::mem::replace(&mut handle.inbound, tokio::sync::mpsc::channel(1).1);
            let app2 = app.clone();
            tokio::spawn(async move {
                use tauri::{Emitter, Manager};
                while let Some(i) = inbound.recv().await {
                    // The host checked this project against the paired device.
                    // Resource identities must also resolve to that project.
                    let Some(project_id) = i.payload.get("project").and_then(Value::as_str) else {
                        continue;
                    };
                    if i.kind == "stop" {
                        if let Some(st) = app2.try_state::<AppState>() {
                            stop_project(&st, project_id).await;
                        }
                    } else if i.kind == "approval" {
                        if let (Some(st), Some(id)) = (
                            app2.try_state::<AppState>(),
                            i.payload.get("id").and_then(Value::as_str),
                        ) {
                            let authorized = st
                                .approvals
                                .pending()
                                .into_iter()
                                .find(|request| request.id == id)
                                .and_then(|request| request.goal_id)
                                .and_then(|goal| st.goals.get(&goal))
                                .is_some_and(|goal| goal.project_id == project_id);
                            if !authorized {
                                continue;
                            }
                            st.approvals.resolve(
                                id,
                                i.payload
                                    .get("allow")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            );
                        }
                    } else if i.kind == "answer" {
                        let Some(st) = app2.try_state::<AppState>() else {
                            continue;
                        };
                        if !i
                            .payload
                            .get("goal_id")
                            .and_then(Value::as_str)
                            .and_then(|id| st.goals.get(id))
                            .is_some_and(|goal| goal.project_id == project_id)
                        {
                            continue;
                        }
                    } else if i.kind == "prompt" {
                        if let Some(id) = i.payload.get("conversation_id").and_then(Value::as_str) {
                            let Some(st) = app2.try_state::<AppState>() else {
                                continue;
                            };
                            let conversation: Option<Conversation> =
                                st.store.get("conversation", id).ok().flatten();
                            if !conversation
                                .is_some_and(|conversation| conversation.project_id == project_id)
                            {
                                continue;
                            }
                        }
                    }
                    let _ = app2.emit("remote-inbound", &i);
                }
            });
            *h = Some(handle);
            let mut p = state.prefs.lock().map_err(|_| "Preferences unavailable")?;
            p.remote_enabled = true;
            let _ = p.save(&state.data_dir);
        }
    }
    remote_status(state).await
}

#[tauri::command]
pub async fn remote_stop(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(h) = state.host.lock().await.take() {
        h.shutdown();
    }
    let mut p = state.prefs.lock().map_err(|_| "Preferences unavailable")?;
    p.remote_enabled = false;
    p.save(&state.data_dir)
}

#[tauri::command]
pub async fn remote_pair_code(
    state: State<'_, AppState>,
    projects: Vec<String>,
    can_control: bool,
) -> CmdResult<Value> {
    let h = state.host.lock().await;
    let h = h.as_ref().ok_or("Start remote access first")?;
    let c = h.new_pairing_code(projects, can_control);
    Ok(
        json!({ "code": format!("{}-{}", &c.code[..4], &c.code[4..]), "expires_at": c.expires_at, "fingerprint": h.fingerprint, "addr": h.addr.to_string() }),
    )
}

#[tauri::command]
pub async fn remote_revoke(state: State<'_, AppState>, device_id: String) -> CmdResult<bool> {
    let h = state.host.lock().await;
    Ok(h.as_ref().map(|h| h.revoke(&device_id)).unwrap_or(false))
}

// ---------- caveman ----------

#[tauri::command]
pub async fn caveman_status(state: State<'_, AppState>) -> CmdResult<Value> {
    let c = conductor_tools::caveman::Caveman::open(&state.data_dir.join("components"));
    let enabled = state.store.settings().map(|s| s.caveman).unwrap_or(true);
    Ok(
        json!({ "enabled": enabled, "active": c.active_version(), "pending": c.state.pending, "previous": c.state.previous, "last_error": c.state.last_error, "upstream": format!("https://github.com/{}", conductor_tools::caveman::UPSTREAM_REPO), "license": "Apache-2.0" }),
    )
}

#[tauri::command]
pub async fn caveman_update(state: State<'_, AppState>) -> CmdResult<Value> {
    let http = reqwest::Client::builder()
        .user_agent("Conductor")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(err)?;
    let rel: Value = http
        .get(format!(
            "https://api.github.com/repos/{}/releases/latest",
            conductor_tools::caveman::UPSTREAM_REPO
        ))
        .send()
        .await
        .map_err(|e| format!("Could not reach GitHub: {e}"))?
        .error_for_status()
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    let tag = rel["tag_name"]
        .as_str()
        .ok_or("No release found")?
        .to_string();
    let url = conductor_tools::caveman::upstream_url(&tag);
    let bytes = http
        .get(&url)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .bytes()
        .await
        .map_err(err)?;
    let mut c = conductor_tools::caveman::Caveman::open(&state.data_dir.join("components"));
    c.stage(&tag, &url, &bytes, None).map_err(err)?;
    let activated = c
        .activate_pending(!state.goals.running_ids().is_empty())
        .map_err(err)?;
    Ok(json!({ "version": tag, "activated": activated }))
}

#[tauri::command]
pub async fn caveman_rollback(state: State<'_, AppState>) -> CmdResult<()> {
    conductor_tools::caveman::Caveman::open(&state.data_dir.join("components"))
        .rollback()
        .map_err(err)
}

// ---------- ports / tunnels ----------

#[tauri::command]
pub async fn ports() -> CmdResult<Vec<Value>> {
    Ok(conductor_tools::tunnels::listening_ports().await.into_iter().map(|p| json!({ "port": p.port, "address": p.address, "pid": p.pid, "dev": conductor_tools::tunnels::is_dev_port(p.port) })).collect())
}

#[tauri::command]
pub async fn tunnel_open(
    state: State<'_, AppState>,
    port: u16,
) -> CmdResult<conductor_tools::tunnels::TunnelInfo> {
    state.tunnels.lock().await.open(port, "user").await
}

#[tauri::command]
pub async fn tunnel_close(state: State<'_, AppState>, id: String) -> CmdResult<bool> {
    Ok(state.tunnels.lock().await.close(&id).await)
}

#[tauri::command]
pub async fn tunnels_list(state: State<'_, AppState>) -> CmdResult<Value> {
    Ok(
        json!({ "available": conductor_tools::tunnels::TunnelManager::available(), "tunnels": state.tunnels.lock().await.list() }),
    )
}

// ---------- config export/import & diagnostics ----------

#[tauri::command]
pub async fn config_export(state: State<'_, AppState>) -> CmdResult<String> {
    let avail = profiles::profiles(&providers(&state)?);
    let combos: Vec<Combo> = ComboStore::open(&state.data_dir)
        .all(&avail)
        .into_iter()
        .filter(|c| !c.builtin)
        .collect();
    let mcp = McpConfig::load(&mcp_path(&state)).map_err(err)?;
    let settings = state.store.settings().map_err(err)?;
    let providers: Vec<Value> = providers(&state)?
        .into_iter()
        .map(|p| json!({ "id": p.id, "name": p.name, "kind": p.kind, "base_url": p.base_url }))
        .collect();
    let skills: Vec<Value> = conductor_tools::skills::Skills::new(state.data_dir.join("skills")).list().into_iter().map(|s| json!({ "name": s.manifest.name, "version": s.manifest.version, "repository": s.manifest.repository })).collect();
    serde_json::to_string_pretty(&json!({
        "format": "conductor.config", "schema": 1,
        "note": "No secrets are included. Reconnect providers after importing.",
        "settings": settings, "prefs": state.prefs(), "combos": combos, "mcp": mcp, "providers": providers, "skills": skills
    }))
    .map_err(err)
}

#[tauri::command]
pub async fn config_import(state: State<'_, AppState>, json: String) -> CmdResult<Vec<String>> {
    let v: Value =
        serde_json::from_str(&json).map_err(|e| format!("Not a Conductor config file: {e}"))?;
    if v["format"] != "conductor.config" {
        return Err("Not a Conductor config file".into());
    }
    let mut notes = Vec::new();
    if let Ok(s) = serde_json::from_value::<Settings>(v["settings"].clone()) {
        state.store.save_settings(&s).map_err(err)?;
        notes.push("Settings imported".into());
    }
    if let Ok(p) = serde_json::from_value::<Prefs>(v["prefs"].clone()) {
        p.save(&state.data_dir)?;
        *state.prefs.lock().map_err(|_| "Preferences unavailable")? = p;
        notes.push("Preferences imported".into());
    }
    if let Ok(combos) = serde_json::from_value::<Vec<Combo>>(v["combos"].clone()) {
        let mut s = ComboStore::open(&state.data_dir);
        for c in combos {
            if s.upsert(c).is_ok() {
                notes.push("Combo imported".into());
            }
        }
    }
    if let Ok(m) = serde_json::from_value::<McpConfig>(v["mcp"].clone()) {
        m.save(&mcp_path(&state)).map_err(err)?;
        notes.push(format!("{} MCP server(s) imported", m.servers.len()));
    }
    Ok(notes)
}

/// Write a privacy-safe diagnostic bundle (no prompts, no code, secrets
/// redacted) and return its path.
#[tauri::command]
pub async fn diagnostics_export(state: State<'_, AppState>) -> CmdResult<String> {
    let tools = conductor_tools::envdoctor::check_all(None).await;
    let logs_dir = state.data_dir.join("logs");
    let mut log_tail = String::new();
    if let Ok(rd) = std::fs::read_dir(&logs_dir) {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        if let Some(last) = files.last() {
            let t = std::fs::read_to_string(last).unwrap_or_default();
            let lines: Vec<&str> = t.lines().collect();
            log_tail = lines[lines.len().saturating_sub(400)..].join("\n");
        }
    }
    let info = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "startup_ms": state.startup_ms.lock().map(|v| v.clone()).unwrap_or_default(),
        "providers": providers(&state)?.iter().map(|p| json!({ "kind": p.kind, "enabled": p.enabled, "models": p.models.len() })).collect::<Vec<_>>(),
        "tools": tools,
        "log_tail": conductor_security::secrets::redact(&log_tail).text,
    });
    let out = state.data_dir.join(format!("diagnostics-{}.json", now()));
    std::fs::write(&out, serde_json::to_vec_pretty(&info).map_err(err)?).map_err(err)?;
    Ok(out.display().to_string())
}

#[tauri::command]
pub async fn reveal_path(path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    let target = if p.is_file() {
        p.parent().map(|x| x.to_path_buf()).unwrap_or(p)
    } else {
        p
    };
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(&target).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&target).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(&target).spawn();
    r.map(|_| ()).map_err(err)
}

// ---------- update verification (test hook) ----------

/// Test-only: check a local update feed and download + verify the package
/// signature with the app's built-in public key, without installing.
/// Enabled only when `CONDUCTOR_TEST_UPDATES=1` and
/// `CONDUCTOR_TEST_UPDATE_ENDPOINT` points at the feed.
#[tauri::command]
pub async fn update_verify_test(app: tauri::AppHandle) -> CmdResult<Value> {
    use tauri_plugin_updater::UpdaterExt;
    if std::env::var("CONDUCTOR_TEST_UPDATES").as_deref() != Ok("1") {
        return Err("not available".into());
    }
    let endpoint = std::env::var("CONDUCTOR_TEST_UPDATE_ENDPOINT").map_err(err)?;
    let url = endpoint.parse().map_err(err)?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(err)?
        // The local test feed uses a self-signed certificate.
        .configure_client(|c| c.danger_accept_invalid_certs(true))
        .build()
        .map_err(err)?;
    let Some(update) = updater.check().await.map_err(err)? else {
        return Ok(json!({ "available": false }));
    };
    match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => Ok(
            json!({ "available": true, "version": update.version, "verified": true, "bytes": bytes.len() }),
        ),
        Err(e) => Ok(
            json!({ "available": true, "version": update.version, "verified": false, "error": e.to_string() }),
        ),
    }
}

// ---------- GitHub (via the official `gh` CLI, which owns authentication) ----------

async fn gh_json(root: &Path, args: &[&str]) -> Result<Value, String> {
    let req = conductor_tools::exec::ExecRequest::new("gh", args, root).timeout(30);
    let r = conductor_tools::exec::run(req, CancellationToken::new()).await;
    if !r.success() {
        return Err(r.stderr.lines().next().unwrap_or("gh failed").to_string());
    }
    serde_json::from_str(&r.stdout).map_err(err)
}

#[tauri::command]
pub async fn github_overview(state: State<'_, AppState>, project_id: String) -> CmdResult<Value> {
    let p = project(&state, &project_id)?;
    let root = PathBuf::from(&p.path);
    if conductor_tools::exec::resolve_program("gh").is_none() {
        return Ok(
            json!({ "status": "missing", "message": "Install the GitHub CLI (gh) to see pull requests, issues and CI here." }),
        );
    }
    let auth = conductor_tools::exec::run(
        conductor_tools::exec::ExecRequest::new("gh", &["auth", "status"], &root).timeout(20),
        CancellationToken::new(),
    )
    .await;
    if !auth.success() {
        return Ok(
            json!({ "status": "signed_out", "message": "GitHub CLI is signed out. Run `gh auth login` in a terminal, then refresh." }),
        );
    }
    let prs = gh_json(
        &root,
        &[
            "pr",
            "list",
            "--limit",
            "10",
            "--json",
            "number,title,state,headRefName,url,isDraft,author",
        ],
    )
    .await;
    let issues = gh_json(
        &root,
        &[
            "issue",
            "list",
            "--limit",
            "10",
            "--json",
            "number,title,state,url,labels",
        ],
    )
    .await;
    let runs = gh_json(
        &root,
        &[
            "run",
            "list",
            "--limit",
            "6",
            "--json",
            "status,conclusion,name,headBranch,url,createdAt",
        ],
    )
    .await;
    if let (Err(e), Err(_), Err(_)) = (&prs, &issues, &runs) {
        return Ok(json!({ "status": "no_repo", "message": e }));
    }
    Ok(json!({
        "status": "ok",
        "prs": prs.unwrap_or(json!([])),
        "issues": issues.unwrap_or(json!([])),
        "runs": runs.unwrap_or(json!([])),
    }))
}
