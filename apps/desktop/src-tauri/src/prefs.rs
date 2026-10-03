//! App-level preferences that complement core `Settings` (UI, background
//! behaviour, routing, remote access). Non-secret; stored as JSON.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// "background" | "exit" | "ask"
    pub close_behavior: String,
    /// "system" | "light" | "dark"
    pub theme_mode: String,
    /// Installed theme name or "" for the built-in theme.
    pub theme_name: String,
    pub reduced_motion: bool,
    pub ui_scale: f32,
    /// "local_first" | "standard" | "restricted"
    pub privacy: String,
    pub telemetry: bool,
    /// "stable" | "beta" | "nightly"
    pub update_channel: String,
    pub auto_update_check: bool,
    pub launch_at_login: bool,
    /// Routing strategy for Combos without their own (see orchestrator Strategy).
    pub routing: String,
    /// model key -> "ask" | "always_allow" | "never"
    pub max_effort: BTreeMap<String, String>,
    pub provider_instructions: BTreeMap<String, String>,
    pub role_instructions: BTreeMap<String, String>,
    pub keybindings: BTreeMap<String, String>,
    pub favorites: Vec<String>,
    pub recents: Vec<String>,
    /// "model:<provider/model>" or "combo:<id>"
    pub default_target: Option<String>,
    pub remote_enabled: bool,
    pub remote_bind: String,
    pub remote_port: u16,
    pub remote_continue_on_disconnect: bool,
    pub emergency_shortcut: String,
    /// Opt-in, unsupported subscription sign-in (Claude, ChatGPT, Gemini).
    pub subscription_signin: bool,
    pub seen_resume: BTreeMap<String, u64>,
}

impl Default for Prefs {
    fn default() -> Self {
        let mut keybindings = BTreeMap::new();
        for (k, v) in [
            ("palette", "Mod+K"),
            ("new_chat", "Mod+N"),
            ("open_project", "Mod+O"),
            ("settings", "Mod+,"),
            ("inspector", "Mod+I"),
            ("toggle_sidebar", "Mod+B"),
            ("stop", "Escape"),
        ] {
            keybindings.insert(k.into(), v.into());
        }
        Self {
            close_behavior: "background".into(),
            theme_mode: "system".into(),
            theme_name: String::new(),
            reduced_motion: false,
            ui_scale: 1.0,
            privacy: "standard".into(),
            telemetry: false,
            update_channel: "stable".into(),
            auto_update_check: true,
            launch_at_login: false,
            routing: "balanced".into(),
            max_effort: BTreeMap::new(),
            provider_instructions: BTreeMap::new(),
            role_instructions: BTreeMap::new(),
            keybindings,
            favorites: Vec::new(),
            recents: Vec::new(),
            default_target: None,
            remote_enabled: false,
            remote_bind: "127.0.0.1".into(),
            remote_port: 47820,
            remote_continue_on_disconnect: true,
            // Ctrl+Shift+Esc is reserved by Windows (Task Manager) and
            // Cmd+Alt+Esc by macOS (Force Quit), so use a free chord.
            emergency_shortcut: DEFAULT_EMERGENCY_SHORTCUT.into(),
            subscription_signin: false,
            seen_resume: BTreeMap::new(),
        }
    }
}

pub const DEFAULT_EMERGENCY_SHORTCUT: &str = "CmdOrCtrl+Alt+Shift+X";

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("prefs.json")
}

impl Prefs {
    pub fn load(data_dir: &Path) -> Self {
        let mut p: Self = std::fs::read(path(data_dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // Migrate the old default, which the OS never let us register.
        if p.emergency_shortcut == "CmdOrCtrl+Shift+Escape" {
            p.emergency_shortcut = DEFAULT_EMERGENCY_SHORTCUT.into();
        }
        p
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        let p = path(data_dir);
        let tmp = p.with_extension("tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if p.exists() {
            let _ = std::fs::copy(&p, p.with_extension("json.bak"));
        }
        std::fs::rename(tmp, p).map_err(|e| e.to_string())
    }

    pub fn push_recent(&mut self, target: &str) {
        self.recents.retain(|t| t != target);
        self.recents.insert(0, target.to_string());
        self.recents.truncate(8);
    }
}
