use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("Approval required: {0}")]
    Approval(String),
    #[error("Action blocked: {0}")]
    Denied(String),
    #[error("Work stopped")]
    Cancelled,
    #[error("State store unavailable")]
    StoreLocked,
    #[error("Storage error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("File operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Provider request failed")]
    Network(#[from] reqwest::Error),
    #[error("{message}")]
    Provider { status: u16, message: String },
}
pub type Result<T> = std::result::Result<T, Error>;
