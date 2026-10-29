//! The Home screen (FR-HOME-1..5, D-54).

use cairn_core::home::{self, HomeOverview};
use tauri::State;

use crate::error::ApiResult;
use crate::state::AppState;

/// Everything Home shows, in one call: counts, the Today list, the calendar's due counts.
#[tauri::command]
#[specta::specta]
pub async fn home_overview(state: State<'_, AppState>) -> ApiResult<HomeOverview> {
    state.with_db(|db| home::overview(db.conn())).await
}
