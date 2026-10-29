//! The *Logs* card and UI errors (NFR-SEC-3, UI-SET-4, D-47; M7 part B).

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::error::ApiResult;
use crate::logging::{Logs, one_line};

/// What the *Logs* card shows.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogStatus {
    pub folder: String,
    /// Bytes taken by the log files (at most 50 MB, NFR-SEC-3).
    pub total_bytes: u64,
    /// *Detailed logs* is on (until the next start).
    pub detailed: bool,
}

fn status(logs: &Logs) -> LogStatus {
    LogStatus {
        folder: logs.dir.display().to_string(),
        total_bytes: logs.total_bytes(),
        detailed: logs.detailed(),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn log_status(logs: State<'_, Logs>) -> ApiResult<LogStatus> {
    Ok(status(&logs))
}

/// *Detailed logs*: adds the debug output until the next start.
#[tauri::command]
#[specta::specta]
pub async fn set_detailed_logs(logs: State<'_, Logs>, on: bool) -> ApiResult<LogStatus> {
    logs.set_detailed(on);
    Ok(status(&logs))
}

/// An error in the UI (an uncaught exception or rejected promise): the error and the screen, never
/// content (NFR-SEC-3). Both are cut short and put on one line.
#[tauri::command]
#[specta::specta]
pub async fn log_ui_error(message: String, screen: String) -> ApiResult<()> {
    tracing::error!(
        target: "ui",
        screen = %one_line(&screen, 120),
        "{}",
        one_line(&message, 500)
    );
    Ok(())
}
