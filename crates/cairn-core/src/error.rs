//! One error type for the whole core. The Tauri layer turns it into `{ kind, message }` for the UI.

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("database upgrade failed: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("data format error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },
    /// Bad input from the user, e.g. an empty name. The message is shown as is.
    #[error("{0}")]
    Invalid(String),
    /// The request clashes with existing data, e.g. a duplicate category key.
    #[error("{0}")]
    Conflict(String),
    /// External mode (FR-KN-EXT-11): the page's file changed on disk after editing started. The UI
    /// offers *Keep mine*, *Take the disk version* and *Show differences*.
    #[error("{0}")]
    ChangedOnDisk(String),
}

impl CoreError {
    /// Stable machine-readable kind, used by the UI to pick a message style.
    pub fn kind(&self) -> &'static str {
        match self {
            CoreError::Db(_) | CoreError::Migration(_) => "database",
            CoreError::Io(_) => "io",
            CoreError::Json(_) => "format",
            CoreError::NotFound { .. } => "not_found",
            CoreError::Invalid(_) => "invalid",
            CoreError::Conflict(_) => "conflict",
            CoreError::ChangedOnDisk(_) => "changed_on_disk",
        }
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
