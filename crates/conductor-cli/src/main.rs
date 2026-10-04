//! `conductor` — headless Conductor.
//!
//! Shares data with the desktop app (same data folder, same OS keychain
//! entries), so providers connected in the app work here too.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use conductor_core::domain::{ProviderConfig, Settings};
use conductor_core::store::Store;
use conductor_engine::approvals::{ApprovalRequest, Approver};
use conductor_engine::combos::ComboStore;
use conductor_engine::goals::{GoalService, StartDeps};
use conductor_engine::{paths, profiles, EngineEvent};
use conductor_orchestrator::goal::{DoneCheck, GoalContract, GoalState};
use conductor_orchestrator::runner::RunnerEvent;
use conductor_tools::mcp::{catalog, config::McpConfig, doctor};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(name = "conductor", version, about = "Conductor — local-first AI orchestration", long_about = None)]
struct Cli {
    /// Override the data folder (defaults to the desktop app's).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Detect installed toolchains and problems.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Index a project and print cache statistics.
    Index { path: PathBuf },
    /// Show exactly what context would be sent for a task (Context Inspector).
    Context {
        path: PathBuf,
        task: String,
        #[arg(long, default_value_t = 12000)]
        budget: usize,
        #[arg(long)]
        json: bool,
        /// Print the rendered provider context.
        #[arg(long)]
        render: bool,
    },
    /// Reduce a large build/test log to what matters.
    Logs { file: PathBuf },
    /// List connected providers and models.
    Providers,
    /// List Combos (presets adapt to your models).
    Combos,
    /// Run a Goal until it is verified complete or needs you.
    Goal {
        objective: String,
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        combo: Option<String>,
        /// Definition-of-Done command (repeatable).
        #[arg(long = "check")]
        checks: Vec<String>,
        /// Constraint (repeatable).
        #[arg(long = "constraint")]
        constraints: Vec<String>,
        /// Approve all permission requests (dangerous commands included).
        #[arg(long)]
        yes: bool,
    },
    /// List or inspect Goals.
    Goals,
    /// MCP servers: catalog, add, list, doctor, remove, export.
    Mcp {
        #[command(subcommand)]
        cmd: McpCmd,
    },
    /// Skills: install, list, remove.
    Skills {
        #[command(subcommand)]
        cmd: PkgCmd,
    },
    /// Plugins: install, list, remove.
    Plugins {
        #[command(subcommand)]
        cmd: PkgCmd,
    },
    /// Themes: install, list, remove, new.
    Themes {
        #[command(subcommand)]
        cmd: ThemeCmd,
    },
    /// Checkpoints for the current Git project.
    Checkpoint {
        #[command(subcommand)]
        cmd: CpCmd,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Run Conductor Host for remote access (no account needed).
    Host {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
        #[arg(long, default_value_t = 47820)]
        port: u16,
        /// Allow the paired device to send prompts and edit files.
        #[arg(long)]
        control: bool,
    },
    /// Caveman optimisation component.
    Caveman {
        #[command(subcommand)]
        cmd: CavemanCmd,
    },
    /// Listening local ports (dev servers).
    Ports,
    /// Where Conductor keeps its data.
    Paths,
}

#[derive(Subcommand)]
enum McpCmd {
    Catalog {
        query: Option<String>,
    },
    Add {
        id: String,
        /// Value for a placeholder, e.g. --set path=C:\work
        #[arg(long = "set")]
        set: Vec<String>,
    },
    List,
    Doctor,
    Remove {
        name: String,
    },
    Export {
        target: String,
    },
}

#[derive(Subcommand)]
enum PkgCmd {
    Install {
        source: String,
        #[arg(long)]
        rev: Option<String>,
    },
    List,
    Remove {
        name: String,
    },
    Rollback {
        name: String,
    },
}

#[derive(Subcommand)]
enum ThemeCmd {
    Install { source: String },
    List,
    Remove { name: String },
    New { dir: PathBuf, name: String },
}

#[derive(Subcommand)]
enum CpCmd {
    Create { label: String },
    List,
    Restore { id: String },
}

#[derive(Subcommand)]
enum CavemanCmd {
    Status,
    /// Fetch the latest upstream release, verify and activate it.
    Update,
    Rollback,
}

struct TerminalApprover {
    yes: bool,
}

#[async_trait::async_trait]
impl Approver for TerminalApprover {
    async fn approve(&self, req: ApprovalRequest) -> bool {
        if self.yes {
            eprintln!(
                "  ✓ auto-approved {} ({})",
                req.capability,
                req.detail.lines().next().unwrap_or("")
            );
            return true;
        }
        let prompt = format!(
            "\n  Allow {}? [{} risk]\n  {}\n  [y/N] ",
            req.capability,
            req.risk,
            req.detail.replace('\n', "\n  ")
        );
        tokio::task::spawn_blocking(move || {
            eprint!("{prompt}");
            let _ = std::io::stderr().flush();
            let mut s = String::new();
            std::io::stdin().read_line(&mut s).is_ok() && matches!(s.trim(), "y" | "Y" | "yes")
        })
        .await
        .unwrap_or(false)
    }
}

fn store(data: &Path) -> Result<Store> {
    Store::open(&data.join("state.db")).map_err(|e| anyhow!("{e}"))
}

fn providers(data: &Path) -> Result<Vec<ProviderConfig>> {
    store(data)?
        .list::<ProviderConfig>("provider")
        .map_err(|e| anyhow!("{e}"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let data = cli.data_dir.clone().unwrap_or_else(paths::data_dir);
    match cli.cmd {
        Cmd::Paths => {
            println!("data:       {}", data.display());
            println!("goals:      {}", data.join("goals").display());
            println!("skills:     {}", data.join("skills").display());
            println!("mcp config: {}", data.join("mcp.json").display());
            println!(
                "secrets:    OS credential store (service '{}')",
                paths::APP_ID
            );
        }
        Cmd::Doctor { json } => {
            let r = conductor_tools::envdoctor::check_all(None).await;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                for t in r {
                    let mark = match t.state {
                        conductor_tools::envdoctor::ToolState::Ok => "✓",
                        conductor_tools::envdoctor::ToolState::Missing => "·",
                        _ => "!",
                    };
                    println!(
                        "{mark} {:<14} {:<12} {}",
                        t.name,
                        t.version.unwrap_or_default(),
                        if t.detail.is_empty() {
                            String::new()
                        } else {
                            t.detail
                        }
                    );
                }
            }
        }
        Cmd::Index { path } => {
            let mut idx = conductor_context::RepoIndex::open(&path, Some(cache_path(&data, &path)));
            let s = idx
                .refresh(&Default::default())
                .map_err(|e| anyhow!("{e}"))?;
            idx.save().map_err(|e| anyhow!("{e}"))?;
            println!("{} files · {} cache hits · {} reparsed · {} removed · {} large skipped · {} binary skipped · ~{} tokens (estimate) · {} ms", s.files, s.cache_hits, s.reparsed, s.removed, s.skipped_large, s.skipped_binary, s.total_tokens, s.elapsed_ms);
        }
        Cmd::Context {
            path,
            task,
            budget,
            json,
            render,
        } => {
            let mut idx = conductor_context::RepoIndex::open(&path, Some(cache_path(&data, &path)));
            idx.refresh(&Default::default())
                .map_err(|e| anyhow!("{e}"))?;
            let _ = idx.save();
            let changed = conductor_tools::git::Git::new(&path)
                .status()
                .await
                .map(|s| s.changed())
                .unwrap_or_default();
            let cfg = conductor_tools::project::ProjectConfig::load(&path)
                .ok()
                .flatten();
            let pack = conductor_context::pack::build(
                &idx,
                &conductor_context::PackRequest {
                    task,
                    changed_files: changed,
                    budget_tokens: budget,
                    secret_scanning: true,
                    include_structure: true,
                    always_include: cfg
                        .as_ref()
                        .map(|c| c.context.always_include.clone())
                        .unwrap_or_default(),
                    project_rules: cfg
                        .as_ref()
                        .and_then(|c| c.instructions.text.clone())
                        .into_iter()
                        .collect(),
                    ..Default::default()
                },
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&pack)?);
            } else if render {
                println!("{}", pack.render());
            } else {
                println!("Context budget: ~{} of {} tokens (estimate) · naive ~{} · ratio {:.2} · cache reuse ~{} · {} secret(s) redacted", pack.total_tokens, pack.budget_tokens, pack.naive_tokens, pack.compression_ratio(), pack.cache_reused_tokens, pack.redactions);
                println!("\nIncluded:");
                for i in &pack.items {
                    println!(
                        "  {:<10} {:<40} ~{:>6}  {}",
                        format!("{:?}", i.kind).to_lowercase(),
                        i.source,
                        i.tokens,
                        i.reason
                    );
                }
                if !pack.omitted.is_empty() {
                    println!("\nOmitted:");
                    for o in pack.omitted.iter().take(20) {
                        println!("  {:<40} ~{:>6}  {}", o.source, o.tokens, o.reason);
                    }
                }
                println!("\nPipeline:");
                for s in &pack.stages {
                    println!("  {:<28} {}", s.stage, s.detail);
                }
            }
        }
        Cmd::Logs { file } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            let r = conductor_context::logs::reduce(&text, &Default::default());
            println!("{}", r.text);
            eprintln!(
                "\n[{} → {} lines · {} error line(s) · full log: {}]",
                r.original_lines,
                r.kept_lines,
                r.error_lines,
                file.display()
            );
        }
        Cmd::Providers => {
            let ps = providers(&data)?;
            if ps.is_empty() {
                println!("No providers connected. Connect one in the Conductor app (Settings → Providers).");
            }
            for p in ps {
                let signed_in = paths::secret(&p.id).is_some();
                println!(
                    "{} ({:?}) {} — {}",
                    p.name,
                    p.kind,
                    if p.enabled { "enabled" } else { "disabled" },
                    if signed_in { "key stored" } else { "no key" }
                );
                for m in p.models.iter().take(30) {
                    println!(
                        "    {}/{}{}",
                        p.id,
                        m.id,
                        if m.efforts.is_empty() {
                            String::new()
                        } else {
                            format!("  effort: {}", m.efforts.join(", "))
                        }
                    );
                }
            }
        }
        Cmd::Combos => {
            let avail = profiles::profiles(&providers(&data)?);
            let cs = ComboStore::open(&data);
            for c in cs.all(&avail) {
                println!(
                    "{:<16} {}{}",
                    c.id,
                    c.name,
                    if c.builtin { " (preset)" } else { "" }
                );
                for m in &c.members {
                    println!(
                        "    {} {:?}{}",
                        m.model,
                        m.roles,
                        if m.enabled { "" } else { " [disabled]" }
                    );
                }
            }
        }
        Cmd::Goals => {
            let (tx, _) = tokio::sync::broadcast::channel(4);
            for g in GoalService::new(&data, tx).list() {
                println!(
                    "{}  {:<12} {}/{}  {}",
                    &g.id[..8],
                    format!("{:?}", g.state),
                    g.done,
                    g.total,
                    g.objective
                );
            }
        }
        Cmd::Goal {
            objective,
            project,
            combo,
            checks,
            constraints,
            yes,
        } => run_goal(&data, objective, &project, combo, checks, constraints, yes).await?,
        Cmd::Mcp { cmd } => mcp(&data, cmd).await?,
        Cmd::Skills { cmd } => {
            let s = conductor_tools::skills::Skills::new(data.join("skills"));
            match cmd {
                PkgCmd::Install { source, rev } => {
                    let i = if Path::new(&source).exists() && rev.is_none() {
                        s.install_dir(Path::new(&source))
                    } else {
                        s.install_git(&source, rev.as_deref()).await
                    };
                    let i = i.map_err(|e| anyhow!("{e}"))?;
                    println!(
                        "Installed skill {} {} ({}…)",
                        i.name,
                        i.version,
                        &i.tree_hash[..12]
                    );
                }
                PkgCmd::List => {
                    for k in s.list() {
                        println!(
                            "{} {} — {}{}",
                            k.manifest.name,
                            k.manifest.version,
                            k.manifest.description,
                            if k.sensitive.is_empty() {
                                String::new()
                            } else {
                                format!("  [needs: {}]", k.sensitive.join(", "))
                            }
                        );
                    }
                }
                PkgCmd::Remove { name } => s.remove(&name).map_err(|e| anyhow!("{e}"))?,
                PkgCmd::Rollback { name } => s.rollback(&name).map_err(|e| anyhow!("{e}"))?,
            }
        }
        Cmd::Plugins { cmd } => {
            let p = conductor_tools::plugins::Plugins::new(data.join("plugins"));
            match cmd {
                PkgCmd::Install { source, rev } => {
                    let i = if Path::new(&source).exists() && rev.is_none() {
                        p.install_dir(Path::new(&source))
                    } else {
                        p.install_git(&source, rev.as_deref()).await
                    };
                    let i = i.map_err(|e| anyhow!("{e}"))?;
                    println!("Installed plugin {} {}", i.name, i.version);
                }
                PkgCmd::List => {
                    for m in p.list() {
                        let (h, d) = p.health(&m.name, None);
                        println!(
                            "{} {} — {} [{:?}: {}] permissions: {}",
                            m.name,
                            m.version,
                            m.description,
                            h,
                            d,
                            m.permissions.join(", ")
                        );
                    }
                }
                PkgCmd::Remove { name } => p.remove(&name).map_err(|e| anyhow!("{e}"))?,
                PkgCmd::Rollback { name } => p.rollback(&name).map_err(|e| anyhow!("{e}"))?,
            }
        }
        Cmd::Themes { cmd } => {
            let t = conductor_tools::themes::Themes::new(data.join("themes"));
            match cmd {
                ThemeCmd::Install { source } => {
                    let i = if Path::new(&source).exists() {
                        t.install_dir(Path::new(&source))
                    } else {
                        t.install_git(&source, None).await
                    };
                    println!("Installed theme {}", i.map_err(|e| anyhow!("{e}"))?.name);
                }
                ThemeCmd::List => {
                    for m in t.list() {
                        println!("{} {} — {}", m.name, m.version, m.description);
                    }
                }
                ThemeCmd::Remove { name } => t.remove(&name).map_err(|e| anyhow!("{e}"))?,
                ThemeCmd::New { dir, name } => {
                    conductor_tools::themes::Themes::scaffold(&dir, &name)
                        .map_err(|e| anyhow!("{e}"))?;
                    println!(
                        "Created {}. Edit theme.json, then: conductor themes install {}",
                        dir.display(),
                        dir.display()
                    );
                }
            }
        }
        Cmd::Checkpoint { cmd, project } => {
            let g = conductor_tools::git::Git::new(&project);
            if !g.is_repo().await {
                bail!("{} is not a Git repository", project.display());
            }
            let cps = conductor_tools::checkpoint::Checkpoints::new(&g);
            match cmd {
                CpCmd::Create { label } => {
                    let c = cps.create(&label).await.map_err(|e| anyhow!("{e}"))?;
                    println!("Checkpoint {} created", c.id);
                }
                CpCmd::List => {
                    for c in cps.list().await.map_err(|e| anyhow!("{e}"))? {
                        println!("{}  {}  {}", c.id, c.created, c.label);
                    }
                }
                CpCmd::Restore { id } => {
                    let safety = cps.restore(&id).await.map_err(|e| anyhow!("{e}"))?;
                    println!("Restored {id}. Previous state saved as checkpoint {} (restore it to undo).", safety.id);
                }
            }
        }
        Cmd::Host {
            project,
            bind,
            port,
            control,
        } => host(&data, &project, bind, port, control).await?,
        Cmd::Caveman { cmd } => caveman(&data, cmd).await?,
        Cmd::Ports => {
            for p in conductor_tools::tunnels::listening_ports().await {
                println!(
                    "{:>5}  {:<16} pid {}{}",
                    p.port,
                    p.address,
                    p.pid.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
                    if conductor_tools::tunnels::is_dev_port(p.port) {
                        "  (likely dev server)"
                    } else {
                        ""
                    }
                );
            }
        }
    }
    Ok(())
}

fn cache_path(data: &Path, project: &Path) -> PathBuf {
    let canon = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    let h = conductor_security::integrity::sha256_hex(canon.to_string_lossy().as_bytes());
    data.join("cache").join(format!("index-{}.json", &h[..16]))
}

async fn run_goal(
    data: &Path,
    objective: String,
    project: &Path,
    combo: Option<String>,
    checks: Vec<String>,
    constraints: Vec<String>,
    yes: bool,
) -> Result<()> {
    let root =
        std::fs::canonicalize(project).with_context(|| format!("project {}", project.display()))?;
    let st = store(data)?;
    let settings: Settings = st.settings().map_err(|e| anyhow!("{e}"))?;
    let provs = providers(data)?;
    let avail = profiles::profiles(&provs);
    if avail.is_empty() {
        bail!("No models available. Connect a provider in the Conductor app first.");
    }
    let combos = ComboStore::open(data);
    let combo_id = combo
        .or_else(|| combos.default_combo())
        .unwrap_or_else(|| "balanced".into());
    let combo = combos
        .get(&combo_id, &avail)
        .or_else(|| combos.all(&avail).into_iter().next())
        .ok_or_else(|| anyhow!("no Combo available"))?;
    let mut done_checks: Vec<DoneCheck> = checks
        .into_iter()
        .map(|c| DoneCheck::Command { command: c })
        .collect();
    if done_checks.is_empty() {
        if let Ok(Some(cfg)) = conductor_tools::project::ProjectConfig::load(&root) {
            done_checks = cfg
                .verify
                .commands
                .into_iter()
                .map(|c| DoneCheck::Command { command: c })
                .collect();
        }
    }
    let (tx, mut rx) = tokio::sync::broadcast::channel(1024);
    let svc = GoalService::new(data, tx);
    let rec = svc
        .create(
            "cli",
            &root,
            GoalContract {
                objective,
                constraints,
                done_checks,
                ..Default::default()
            },
            &combo.id,
        )
        .map_err(|e| anyhow!(e))?;
    println!(
        "Goal {} · Combo {} · {}",
        &rec.goal.id[..8],
        combo.name,
        root.display()
    );
    let caveman = conductor_tools::caveman::Caveman::open(&data.join("components"));
    let extra = caveman
        .instruction(settings.caveman, "", &[])
        .unwrap_or_default();
    svc.start(
        &rec.goal.id,
        StartDeps {
            providers: provs,
            secrets: Arc::new(|id: &str| paths::secret(id)),
            integration_secrets: Arc::new(|name: &str| paths::integration_secret(name)),
            integration_project_id: None,
            settings,
            combo,
            models: avail,
            approver: Arc::new(TerminalApprover { yes }),
            max_parallel: 2,
            extra_system: extra,
            max_effort: HashMap::new(),
            live_settings: None,
        },
    )
    .map_err(|e| anyhow!(e))?;
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    let svc2 = svc.clone();
    let gid = rec.goal.id.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\nStopping…");
            svc2.stop(&gid);
            c2.cancel();
        }
    });
    loop {
        match rx.recv().await {
            Ok(EngineEvent::Goal { event, .. }) => match event {
                RunnerEvent::State { state } => println!("· {state:?}"),
                RunnerEvent::Planned { tasks } => println!("· planned {tasks} task(s)"),
                RunnerEvent::TaskStarted {
                    title,
                    model,
                    effort,
                    ..
                } => println!(
                    "▸ {title} — {model}{}",
                    effort
                        .map(|e| format!(" · {}", e.as_str()))
                        .unwrap_or_default()
                ),
                RunnerEvent::TaskFinished { task, status } => println!("  {task}: {status:?}"),
                RunnerEvent::Notice { message } => println!("! {message}"),
                RunnerEvent::Handoff {
                    from,
                    to,
                    packet_tokens,
                    ..
                } => println!("  handoff {from} → {to} (~{packet_tokens} tokens)"),
                RunnerEvent::EffortRequest { model, level, why } => println!(
                    "? {model} may benefit from {} effort: {why} (allow in Settings)",
                    level.as_str()
                ),
                RunnerEvent::Check { check, passed } => {
                    println!("{} {check}", if passed { "✓" } else { "✗" })
                }
                RunnerEvent::Blocked { reason } => println!("■ blocked: {reason}"),
                RunnerEvent::Complete => println!("✓ Goal complete (verified)"),
                RunnerEvent::Reused { task, from_model } => {
                    println!("  {task}: reused earlier result from {from_model}")
                }
            },
            Ok(EngineEvent::Tool {
                tool, summary, ok, ..
            }) => println!("    {} {tool}: {summary}", if ok { "·" } else { "✗" }),
            Ok(EngineEvent::GoalSaved { goal_id, state, .. })
                if goal_id == rec.goal.id
                    && matches!(
                        state.as_str(),
                        "Complete" | "Blocked" | "Stopped" | "WaitingForUser"
                    ) =>
            {
                if !svc.is_running(&goal_id) || cancel.is_cancelled() {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    if !svc.is_running(&goal_id) {
                        break;
                    }
                }
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(_) => break,
        }
    }
    let r = svc
        .get(&rec.goal.id)
        .ok_or_else(|| anyhow!("goal vanished"))?;
    if r.goal.state != GoalState::Complete {
        let unverified = r.goal.unverified();
        if !unverified.is_empty() {
            println!("Unverified: {}", unverified.join(", "));
        }
        std::process::exit(2);
    }
    Ok(())
}

async fn mcp(data: &Path, cmd: McpCmd) -> Result<()> {
    let path = data.join("mcp.json");
    let mut cfg = McpConfig::load(&path).map_err(|e| anyhow!("{e}"))?;
    match cmd {
        McpCmd::Catalog { query } => {
            let entries = catalog::builtin();
            let list: Vec<_> = match &query {
                Some(q) => catalog::search(&entries, q),
                None => entries.iter().collect(),
            };
            for e in list {
                println!(
                    "{:<20} {} — {}\n{:<20} source: {}  needs: {}",
                    e.id,
                    e.name,
                    e.description,
                    "",
                    e.source,
                    e.dependencies.join(", ")
                );
            }
        }
        McpCmd::Add { id, set } => {
            let entries = catalog::builtin();
            let e = entries
                .iter()
                .find(|e| e.id == id)
                .or_else(|| catalog::search(&entries, &id).into_iter().next())
                .ok_or_else(|| anyhow!("no catalog entry for '{id}'"))?;
            let values: BTreeMap<String, String> = set
                .iter()
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let server = catalog::to_server(e, &values)
                .map_err(|m| anyhow!("{m}. Use --set {}=VALUE", e.needs.join("=… --set ")))?;
            let name = server.name.clone();
            cfg.upsert(server).map_err(|e| anyhow!("{e}"))?;
            cfg.save(&path).map_err(|e| anyhow!("{e}"))?;
            println!("Added MCP '{name}'. Testing…");
            let d = doctor::diagnose(
                cfg.get(&name).expect("just added"),
                &|k| paths::integration_secret(k),
                60,
            )
            .await;
            println!("{} — {}", d.status.label(), d.summary);
            if let doctor::Fix::InstallDependency { hint, .. } = &d.fix {
                println!("Fix: {hint}");
            }
            if let doctor::Fix::SetSecret { name } = &d.fix {
                println!(
                    "Fix: store the secret '{name}' in Settings → Integrations (OS keychain)."
                );
            }
        }
        McpCmd::List => {
            for s in &cfg.servers {
                println!(
                    "{} {} — {}",
                    if s.enabled { "●" } else { "○" },
                    s.name,
                    s.source
                );
            }
        }
        McpCmd::Doctor => {
            for s in &cfg.servers {
                let d = doctor::diagnose(s, &|k| paths::integration_secret(k), 30).await;
                println!("{:<18} {:<20} {}", s.name, d.status.label(), d.summary);
            }
        }
        McpCmd::Remove { name } => {
            if cfg.remove(&name) {
                cfg.save(&path).map_err(|e| anyhow!("{e}"))?;
                println!("Removed {name}");
            }
        }
        McpCmd::Export { target } => match target.as_str() {
            "claude" => println!("{}", serde_json::to_string_pretty(&cfg.to_claude_json())?),
            "gemini" => println!("{}", serde_json::to_string_pretty(&cfg.to_gemini_json())?),
            "codex" => println!("{}", cfg.to_codex_toml()),
            other => bail!("unknown target {other}; use claude, codex or gemini"),
        },
    }
    Ok(())
}

async fn host(data: &Path, project: &Path, bind: String, port: u16, control: bool) -> Result<()> {
    let root = std::fs::canonicalize(project)?;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    let mut projects = BTreeMap::new();
    projects.insert("default".to_string(), (name.clone(), root.clone()));
    let mut h = conductor_remote::Host::start(conductor_remote::HostConfig {
        bind,
        port,
        data_dir: data.join("remote"),
        projects,
    })
    .await
    .map_err(|e| anyhow!(e))?;
    let code = h.new_pairing_code(vec!["default".into()], control);
    println!("Conductor Host running on https://{}", h.addr);
    println!("Project:      {name}");
    println!(
        "Pairing code: {}-{} (expires in 5 minutes, single use)",
        &code.code[..4],
        &code.code[4..]
    );
    println!("Fingerprint:  {}", h.fingerprint);
    println!(
        "Access:       {}",
        if control {
            "control (prompts, approvals, edits)"
        } else {
            "view only"
        }
    );
    println!("Press Ctrl+C to stop.");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => { h.shutdown(); break; }
            m = h.inbound.recv() => match m {
                Some(i) => println!("[{}] {}: {}", i.device_name, i.kind, i.payload),
                None => break,
            }
        }
    }
    Ok(())
}

async fn caveman(data: &Path, cmd: CavemanCmd) -> Result<()> {
    let mut c = conductor_tools::caveman::Caveman::open(&data.join("components"));
    match cmd {
        CavemanCmd::Status => {
            let enabled = store(data)
                .ok()
                .and_then(|s| s.settings().ok())
                .map(|s| s.caveman)
                .unwrap_or(true);
            println!(
                "Caveman: {} · active version {}",
                if enabled { "enabled" } else { "disabled" },
                c.active_version()
            );
            if let Some(p) = &c.state.pending {
                println!(
                    "Pending update {} (activates when no Goal is running)",
                    p.version
                );
            }
        }
        CavemanCmd::Update => {
            let http = reqwest::Client::builder()
                .user_agent("Conductor")
                .timeout(std::time::Duration::from_secs(30))
                .build()?;
            let rel: serde_json::Value = http
                .get(format!(
                    "https://api.github.com/repos/{}/releases/latest",
                    conductor_tools::caveman::UPSTREAM_REPO
                ))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let tag = rel["tag_name"]
                .as_str()
                .ok_or_else(|| anyhow!("no release tag"))?
                .to_string();
            let url = conductor_tools::caveman::upstream_url(&tag);
            let bytes = http
                .get(&url)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            c.stage(&tag, &url, &bytes, None)
                .map_err(|e| anyhow!("{e}"))?;
            let (tx, _) = tokio::sync::broadcast::channel(1);
            let goal_active = GoalService::new(data, tx)
                .list()
                .iter()
                .any(|g| g.state == GoalState::Running);
            if c.activate_pending(goal_active)
                .map_err(|e| anyhow!("{e}"))?
            {
                println!(
                    "Caveman {tag} verified and active (sha256 {}…).",
                    &c.state
                        .active
                        .as_ref()
                        .map(|a| a.sha256.clone())
                        .unwrap_or_default()[..12]
                );
            } else {
                println!("Caveman {tag} staged; it activates when no Goal is running.");
            }
        }
        CavemanCmd::Rollback => {
            c.rollback().map_err(|e| anyhow!("{e}"))?;
            println!("Rolled back to {}", c.active_version());
        }
    }
    Ok(())
}
