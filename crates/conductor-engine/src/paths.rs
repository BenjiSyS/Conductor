//! Data locations shared by the desktop app and the CLI.

use std::path::PathBuf;

/// Same identifier the desktop app uses, so the CLI sees the same projects,
/// providers and credentials.
pub const APP_ID: &str = "dev.benjisys.conductor";

/// `CONDUCTOR_DATA_DIR`, then portable `data/` beside the executable when a
/// `portable.flag` exists, then the OS data directory.
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("CONDUCTOR_DATA_DIR") {
        return PathBuf::from(p);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        if dir.join("portable.flag").exists() {
            return dir.join("data");
        }
    }
    directories::BaseDirs::new()
        .map(|b| b.data_dir().join(APP_ID))
        .unwrap_or_else(|| PathBuf::from(".conductor-data"))
}

pub fn credential(id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(APP_ID, id).map_err(|_| {
        "OS credential storage unavailable. No plaintext fallback is used.".to_string()
    })
}

/// Read a provider secret from the OS keychain.
pub fn secret(id: &str) -> Option<String> {
    credential(id).ok()?.get_password().ok()
}

/// Integration secrets (MCP tokens etc.) live under a separate namespace from
/// provider API keys.
pub fn integration_secret(name: &str) -> Option<String> {
    secret(&format!("secret:{name}"))
}
