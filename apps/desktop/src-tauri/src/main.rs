#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod chat;
mod cmds;
mod extras;
mod prefs;
mod state;
mod usage;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use conductor_core::store::Store;
use conductor_engine::approvals::UiApprovals;
use conductor_engine::goals::GoalService;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WindowEvent};

use crate::state::AppState;

fn data_dir(app: &tauri::App) -> (PathBuf, bool) {
    if let Some(p) = std::env::var_os("CONDUCTOR_DATA_DIR") {
        return (PathBuf::from(p), false);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        if dir.join("portable.flag").exists() {
            return (dir.join("data"), true);
        }
    }
    (
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| conductor_engine::paths::data_dir()),
        false,
    )
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn remote_engine_event(
    state: &AppState,
    event: &conductor_engine::EngineEvent,
) -> Option<serde_json::Value> {
    use conductor_engine::EngineEvent;
    let project = match event {
        EngineEvent::Goal { goal_id, .. }
        | EngineEvent::GoalSaved { goal_id, .. }
        | EngineEvent::Delta {
            goal_id: Some(goal_id),
            ..
        }
        | EngineEvent::Tool {
            goal_id: Some(goal_id),
            ..
        }
        | EngineEvent::Approval {
            goal_id: Some(goal_id),
            ..
        } => state.goals.get(goal_id)?.project_id,
        EngineEvent::Delta {
            goal_id: None,
            task_id,
            ..
        }
        | EngineEvent::Tool {
            goal_id: None,
            task_id,
            ..
        } => {
            let id = task_id.strip_prefix("chat-")?;
            let conversation: conductor_core::domain::Conversation =
                state.store.get("conversation", id).ok()??;
            conversation.project_id
        }
        // Unscoped approvals and resolution events cannot safely be exposed
        // remotely until their originating project is carried explicitly.
        _ => return None,
    };
    let mut value = serde_json::to_value(event).ok()?;
    value["project"] = serde_json::Value::String(project);
    Some(value)
}

fn main() {
    let started = std::time::Instant::now();
    // One instance per data folder: isolated test/portable data dirs may run
    // alongside the normal app.
    let mut builder = tauri::Builder::default();
    if std::env::var_os("CONDUCTOR_DATA_DIR").is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }));
    }
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Some(st) = app.try_state::<AppState>() {
                                let r = cmds::do_emergency_stop(&st).await;
                                let _ = app.emit("emergency-stop", r);
                            }
                        });
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let (dir, portable) = data_dir(app);
            std::fs::create_dir_all(&dir)?;
            // Structured, rotating, bounded logs (secrets are redacted at the
            // source by the tool layer).
            let appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .max_log_files(5)
                .filename_prefix("conductor")
                .filename_suffix("log")
                .build(dir.join("logs"));
            if let Ok(appender) = appender {
                let _ = tracing_subscriber::fmt()
                    .with_env_filter("warn,conductor=info")
                    .with_ansi(false)
                    .with_writer(appender)
                    .try_init();
            }
            let store = Arc::new(Store::open(&dir.join("state.db"))?);
            let (events, _) = tokio::sync::broadcast::channel(2048);
            let goals = GoalService::new(&dir, events.clone());
            let recovered = goals.recover_interrupted();
            let approvals = UiApprovals::new(events.clone());
            let prefs = prefs::Prefs::load(&dir);
            let shortcut = prefs.emergency_shortcut.clone();
            let remote_on = prefs.remote_enabled;
            let state = AppState {
                store,
                data_dir: dir,
                portable,
                runs: Mutex::new(HashMap::new()),
                prefs: Mutex::new(prefs),
                events: events.clone(),
                goals,
                approvals,
                host: tokio::sync::Mutex::new(None),
                tunnels: tokio::sync::Mutex::new(conductor_tools::tunnels::TunnelManager::new()),
                started_at: started,
                startup_ms: Mutex::new(Vec::new()),
                quitting: std::sync::atomic::AtomicBool::new(false),
                emergency_registered: std::sync::atomic::AtomicBool::new(false),
            };
            state.mark("state ready");
            app.manage(state);
            bridge::start(app.handle());
            extras::start(app.handle());

            // Main window. Isolated (CONDUCTOR_DATA_DIR) and portable data
            // folders keep their own WebView2/WebKit profile inside them.
            if let Some(cfg) = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
            {
                let mut wb = tauri::WebviewWindowBuilder::from_config(app.handle(), &cfg)?;
                if portable || std::env::var_os("CONDUCTOR_DATA_DIR").is_some() {
                    let dir = app.state::<AppState>().data_dir.join("webview");
                    wb = wb.data_directory(dir);
                }
                wb.build()?;
            }

            // Engine events -> UI (and remote clients when the host runs).
            let handle = app.handle().clone();
            let mut rx = events.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            let _ = handle.emit("engine", &ev);
                            if let Some(st) = handle.try_state::<AppState>() {
                                if let Some(h) = st.host.lock().await.as_ref() {
                                    if let Some(v) = remote_engine_event(&st, &ev) {
                                        h.publish(v);
                                    }
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            });

            // Emergency Stop shortcut (global).
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                match app.global_shortcut().register(shortcut.as_str()) {
                    Ok(()) => app
                        .state::<AppState>()
                        .emergency_registered
                        .store(true, Ordering::SeqCst),
                    Err(e) => {
                        tracing::warn!(error = %e, "could not register emergency stop shortcut")
                    }
                }
            }

            // Tray / menu bar.
            let show = MenuItem::with_id(app, "show", "Show Conductor", true, None::<&str>)?;
            let stop = MenuItem::with_id(app, "stop", "Stop all work", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Conductor", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&show, &stop, &PredefinedMenuItem::separator(app)?, &quit],
            )?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("Conductor")
                .menu(&menu)
                .show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(|app, e| match e.id.as_ref() {
                "show" => show_main(app),
                "stop" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(st) = app.try_state::<AppState>() {
                            let r = cmds::do_emergency_stop(&st).await;
                            let _ = app.emit("emergency-stop", r);
                        }
                    });
                }
                "quit" => {
                    if let Some(st) = app.try_state::<AppState>() {
                        st.quitting.store(true, Ordering::SeqCst);
                    }
                    app.exit(0);
                }
                _ => {}
            })
            .on_tray_icon_event(|tray, e| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = e
                {
                    show_main(tray.app_handle());
                }
            })
            .build(app)?;

            // Started at login with --background: stay in the tray.
            if std::env::args().any(|a| a == "--background") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            if !recovered.is_empty() {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let _ = h.emit("goals-recovered", recovered);
                });
            }
            if remote_on {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(st) = h.try_state::<AppState>() {
                        let _ = cmds::remote_start(h.clone(), st).await;
                    }
                });
            }
            if let Some(st) = app.try_state::<AppState>() {
                st.mark("setup complete");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let Some(st) = app.try_state::<AppState>() else {
                    return;
                };
                if st.quitting.load(Ordering::SeqCst) {
                    return;
                }
                match st.prefs().close_behavior.as_str() {
                    "exit" => {}
                    "ask" => {
                        api.prevent_close();
                        let _ = window.emit("close-requested", st.any_active_work());
                    }
                    _ => {
                        // Keep running in the tray so active Goals continue.
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            chat::snapshot,
            chat::save_settings,
            chat::open_project,
            chat::new_project,
            chat::save_provider,
            chat::disconnect_provider,
            chat::remove_provider,
            chat::create_conversation,
            chat::rename_conversation,
            chat::delete_conversation,
            chat::send_message,
            chat::stop,
            cmds::app_info,
            cmds::prefs_get,
            usage::usage_overview,
            bridge::cli_bridges,
            extras::save_paste,
            extras::test_notification,
            extras::profile_info,
            extras::setup_prepare,
            extras::setup_scan,
            extras::env_install,
            extras::mcp_connect_remote,
            extras::mcp_sign_in,
            extras::models_refresh,
            extras::cli_bridge_update,
            bridge::cli_bridge_connect,
            usage::subscription_sign_in,
            usage::subscription_usage,
            usage::subscription_sign_out,
            cmds::prefs_save,
            cmds::hardware,
            cmds::models,
            cmds::combos_list,
            cmds::combo_save,
            cmds::combo_delete,
            cmds::combo_duplicate,
            cmds::combo_set_default,
            cmds::combo_export,
            cmds::combo_import,
            cmds::effort_preview,
            cmds::context_pack,
            cmds::memory_get,
            cmds::memory_save,
            cmds::project_info,
            cmds::recipes,
            cmds::project_remove,
            cmds::goals_list,
            cmds::goal_get,
            cmds::goal_clarify,
            cmds::goal_create,
            cmds::goal_update_contract,
            cmds::goal_start,
            cmds::goal_stop,
            cmds::goal_delete,
            cmds::approvals_pending,
            cmds::approval_resolve,
            cmds::emergency_stop,
            cmds::mcp_catalog,
            cmds::mcp_list,
            cmds::mcp_add,
            cmds::mcp_add_custom,
            cmds::mcp_remove,
            cmds::mcp_set_enabled,
            cmds::mcp_doctor,
            cmds::mcp_export,
            cmds::secret_set,
            cmds::secret_delete,
            cmds::skills_list,
            cmds::skills_install,
            cmds::skills_remove,
            cmds::plugins_list,
            cmds::plugins_install,
            cmds::plugins_remove,
            cmds::themes_list,
            cmds::themes_install,
            cmds::themes_remove,
            cmds::theme_scaffold,
            cmds::receipts_list,
            cmds::env_doctor,
            cmds::git_overview,
            cmds::checkpoints_list,
            cmds::checkpoint_create,
            cmds::checkpoint_restore,
            cmds::remote_status,
            cmds::remote_start,
            cmds::remote_stop,
            cmds::remote_pair_code,
            cmds::remote_revoke,
            cmds::caveman_status,
            cmds::caveman_update,
            cmds::caveman_rollback,
            cmds::ports,
            cmds::tunnel_open,
            cmds::tunnel_close,
            cmds::tunnels_list,
            cmds::config_export,
            cmds::config_import,
            cmds::diagnostics_export,
            cmds::reveal_path,
            cmds::update_verify_test,
            cmds::github_overview,
            quit_app,
            hide_window,
        ]);
    if let Err(e) = builder.run(tauri::generate_context!()) {
        eprintln!("Conductor failed to start: {e}");
        std::process::exit(1);
    }
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    if let Some(st) = app.try_state::<AppState>() {
        st.quitting.store(true, Ordering::SeqCst);
    }
    app.exit(0);
}

#[tauri::command]
fn hide_window(window: tauri::Window) {
    let _ = window.hide();
}
