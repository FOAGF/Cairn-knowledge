//! Error sent to the UI. Generated bindings throw it, so the UI sees `{ kind, message }`.

use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
pub struct ApiError {
    /// Stable kind: `database`, `io`, `format`, `not_found`, `invalid`, `conflict`,
    /// `changed_on_disk` (external mode: a page's file changed after editing started, D-72) or
    /// `internal`.
    pub kind: String,
    /// Plain-language message for the user.
    pub message: String,
}

impl ApiError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: "internal".into(),
            message: message.into(),
        }
    }

    /// A request Cairn refuses, with a message for the user.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            kind: "invalid".into(),
            message: message.into(),
        }
    }
}

/// Demo data and other development helpers: refused in a release build.
pub fn dev_only() -> ApiResult<()> {
    if cfg!(debug_assertions) {
        Ok(())
    } else {
        Err(ApiError::invalid("Demo data is only available in development builds."))
    }
}

impl From<cairn_core::CoreError> for ApiError {
    fn from(e: cairn_core::CoreError) -> Self {
        // Unexpected kinds are logged with detail; the user sees the message only.
        if matches!(e.kind(), "database" | "io" | "format") {
            tracing::error!(error = %e, "core error");
        }
        Self {
            kind: e.kind().into(),
            message: e.to_string(),
        }
    }
}

impl From<tauri::Error> for ApiError {
    fn from(e: tauri::Error) -> Self {
        tracing::error!(error = %e, "tauri error");
        Self::internal(e.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
