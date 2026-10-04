//! Unified tool-call record and capability vocabulary.

use serde::{Deserialize, Serialize};

/// Capabilities every tool/integration declares; permission policy is
/// expressed in these terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Capability {
    #[serde(rename = "filesystem.read")]
    FilesystemRead,
    #[serde(rename = "filesystem.write")]
    FilesystemWrite,
    #[serde(rename = "filesystem.delete")]
    FilesystemDelete,
    #[serde(rename = "terminal.execute")]
    TerminalExecute,
    #[serde(rename = "network")]
    Network,
    #[serde(rename = "browser")]
    Browser,
    #[serde(rename = "computer.control")]
    ComputerControl,
    #[serde(rename = "github")]
    Github,
    #[serde(rename = "secrets.use")]
    SecretsUse,
    #[serde(rename = "tunnels")]
    Tunnels,
    #[serde(rename = "install.software")]
    InstallSoftware,
    #[serde(rename = "mcp")]
    Mcp,
    #[serde(rename = "remote.host")]
    RemoteHost,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::FilesystemRead => "filesystem.read",
            Capability::FilesystemWrite => "filesystem.write",
            Capability::FilesystemDelete => "filesystem.delete",
            Capability::TerminalExecute => "terminal.execute",
            Capability::Network => "network",
            Capability::Browser => "browser",
            Capability::ComputerControl => "computer.control",
            Capability::Github => "github",
            Capability::SecretsUse => "secrets.use",
            Capability::Tunnels => "tunnels",
            Capability::InstallSoftware => "install.software",
            Capability::Mcp => "mcp",
            Capability::RemoteHost => "remote.host",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }
    /// Capabilities that are always high-risk and listed prominently when an
    /// extension asks for them.
    pub fn is_sensitive(self) -> bool {
        matches!(
            self,
            Capability::FilesystemDelete
                | Capability::TerminalExecute
                | Capability::ComputerControl
                | Capability::SecretsUse
                | Capability::InstallSoftware
                | Capability::RemoteHost
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallState {
    Pending,
    AwaitingApproval,
    Running,
    Succeeded,
    Failed,
    Denied,
    Cancelled,
}

/// Where a tool call came from — used for audit and injection defence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// "model:<provider/model>", "user", "plugin:<id>", "remote:<device>"
    pub requested_by: String,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool: String,
    pub input: serde_json::Value,
    pub capabilities: Vec<Capability>,
    pub state: CallState,
    pub result: Option<String>,
    pub logs: Vec<String>,
    pub provenance: Provenance,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

impl ToolCall {
    pub fn new(
        tool: &str,
        input: serde_json::Value,
        capabilities: Vec<Capability>,
        provenance: Provenance,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            tool: tool.to_string(),
            input,
            capabilities,
            state: CallState::Pending,
            result: None,
            logs: Vec::new(),
            provenance,
            started_at: None,
            finished_at: None,
        }
    }
    pub fn finish(&mut self, state: CallState, result: impl Into<String>) {
        self.state = state;
        self.result = Some(result.into());
        self.finished_at = Some(crate::unix_now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_names_roundtrip() {
        for c in [
            Capability::FilesystemRead,
            Capability::TerminalExecute,
            Capability::RemoteHost,
        ] {
            assert_eq!(Capability::parse(c.as_str()), Some(c));
        }
        assert_eq!(Capability::parse("nope"), None);
        assert!(Capability::TerminalExecute.is_sensitive());
        assert!(!Capability::FilesystemRead.is_sensitive());
    }
}
