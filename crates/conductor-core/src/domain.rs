use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Chat,
    Plan,
    Goal,
    Agent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    #[default]
    Ask,
    AutoApprove,
    FullAccess,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Performance {
    Potato,
    #[default]
    Balanced,
    Maximum,
}
impl Performance {
    pub fn parallel_agents(self) -> usize {
        match self {
            Self::Potato => 1,
            Self::Balanced => 2,
            Self::Maximum => 4,
        }
    }
    pub fn context_bytes(self) -> usize {
        match self {
            Self::Potato => 32_000,
            Self::Balanced => 64_000,
            Self::Maximum => 128_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub setup_complete: bool,
    pub theme: String,
    pub performance: Performance,
    pub permission: PermissionLevel,
    pub auto_approve: Vec<String>,
    pub instructions: String,
    pub compression: bool,
    pub caveman: bool,
    pub allow_highest_effort: bool,
    pub last_project: Option<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            setup_complete: false,
            theme: "system".into(),
            performance: Performance::Balanced,
            permission: PermissionLevel::Ask,
            auto_approve: vec![],
            instructions: String::new(),
            compression: true,
            caveman: true,
            allow_highest_effort: false,
            last_project: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Openai,
    Anthropic,
    Gemini,
    OpenaiCompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub efforts: Vec<String>,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub vision: bool,
}

// Metadata only. Credentials never belong in this serializable type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<Model>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub git: bool,
    pub opened_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: Role,
    pub text: String,
    pub created_at: i64,
    pub status: String,
    pub provider: Option<String>,
}
impl Message {
    pub fn new(role: Role, text: String) -> Self {
        Self {
            id: id(),
            role,
            text,
            created_at: now(),
            status: "complete".into(),
            provider: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: Mode,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub effort: Option<String>,
    pub messages: Vec<Message>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub project_id: Option<String>,
    pub kind: String,
    pub summary: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub settings: Settings,
    pub projects: Vec<Project>,
    pub providers: Vec<ProviderConfig>,
    pub conversations: Vec<Conversation>,
    pub history: Vec<HistoryEntry>,
    pub data_dir: String,
}
