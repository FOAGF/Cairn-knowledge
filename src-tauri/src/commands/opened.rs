//! What was opened last (D-94): the `#` and `[[` suggestion lists show the most recently opened
//! tasks and pages first.

use cairn_core::opened::{self, OpenedKind};
use tauri::State;

use crate::error::ApiResult;
use crate::state::AppState;

/// Records a task or page as opened (its page was shown, or it was picked from a suggestion
/// list). No event: only the suggestion lists read it, each time they open.
#[tauri::command]
#[specta::specta]
pub async fn record_opened(
    state: State<'_, AppState>,
    kind: OpenedKind,
    id: String,
) -> ApiResult<()> {
    state
        .with_db(move |db| opened::record(db.conn(), kind, &id))
        .await
}

/// The ids of the most recently opened tasks or pages, most recent first.
#[tauri::command]
#[specta::specta]
pub async fn recently_opened(
    state: State<'_, AppState>,
    kind: OpenedKind,
    limit: u32,
) -> ApiResult<Vec<String>> {
    state
        .with_db(move |db| opened::recent(db.conn(), kind, limit))
        .await
}
