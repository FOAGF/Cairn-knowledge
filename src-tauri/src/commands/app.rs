//! App-level information.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::error::{ApiError, ApiResult};
use crate::sidecar::{SidecarStatus, Sidecars};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// When this build was made, in UTC (RFC 3339), for Settings › About (UI-SET-5).
    pub build_time: Option<String>,
    /// The commit (full SHA) this build was made from; `None` when built without git.
    pub commit: Option<String>,
    /// A release build made with tracked files that differ from `commit`.
    pub local_changes: bool,
    pub data_dir: String,
    pub schema_version: u32,
    /// Set when the data folder is inside OneDrive (D-56): Home's *Needs attention* shows it.
    pub data_folder_warning: Option<String>,
}

/// Version, data folder and database schema version (Settings → Data, About).
#[tauri::command]
#[specta::specta]
pub async fn app_info(state: State<'_, AppState>) -> ApiResult<AppInfo> {
    let data_dir = state.paths.root.display().to_string();
    let schema_version = state.with_db(|db| db.schema_version()).await?;
    let data_folder_warning = cairn_core::paths::onedrive_warning(&state.paths.root);
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        build_time: build_time(),
        commit: option_env!("CAIRN_GIT_COMMIT").map(String::from),
        local_changes: option_env!("CAIRN_GIT_DIRTY").is_some(),
        data_dir,
        data_folder_warning,
        schema_version,
    })
}

/// A release build's compile time (`build.rs`); a development build's program file time, since its
/// build script doesn't run on every build.
fn build_time() -> Option<String> {
    let secs = match option_env!("CAIRN_BUILD_EPOCH") {
        Some(s) => s.parse::<i64>().ok()?,
        None => {
            let modified = std::env::current_exe()
                .ok()?
                .metadata()
                .ok()?
                .modified()
                .ok()?;
            modified
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs() as i64
        }
    };
    chrono::DateTime::from_timestamp(secs, 0).map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// State of the two AI worker processes (TC-12). Changes arrive as the `SidecarStatus` event.
#[tauri::command]
#[specta::specta]
pub async fn sidecar_status(sidecars: State<'_, Sidecars>) -> ApiResult<SidecarStatus> {
    Ok(sidecars.status())
}

/// Opens a web page in the system browser: the cited pages of a web answer (FR-LLM-AS-12). The
/// window itself never navigates away from the app (D-43), so links go through here. Only `http`
/// and `https` addresses are opened.
#[tauri::command]
#[specta::specta]
pub async fn open_url(url: String) -> ApiResult<()> {
    let url = url.trim().to_string();
    let scheme_ok = url.starts_with("https://") || url.starts_with("http://");
    if !scheme_ok || url.chars().any(char::is_whitespace) {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "Only web addresses (http or https) can be opened.".into(),
        });
    }
    open_with_system(&url).map_err(|e| {
        tracing::warn!(error = %e, "could not start the browser");
        ApiError::internal("The browser could not be opened.")
    })
}

/// One of Cairn's own folders, for *Open folder* buttons (UI-SET-4).
#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AppFolder {
    /// The data folder.
    Data,
    /// The backup folder in use (the setting, or the data folder's `backups`).
    Backups,
    /// The log files (NFR-SEC-3).
    Logs,
    /// External mode: the knowledge folder (D-72). Never created by opening it.
    Knowledge,
    /// The switch backups in the data folder (FR-KN-EXT-9).
    SwitchBackups,
}

/// Opens one of Cairn's folders in the file manager. Only these folders can be opened, so the UI
/// never passes a path. The folder is created first if it doesn't exist yet (not the knowledge
/// folder, which must be there).
#[tauri::command]
#[specta::specta]
pub async fn open_folder(state: State<'_, AppState>, which: AppFolder) -> ApiResult<()> {
    let dir = match which {
        AppFolder::Data => state.paths.root.clone(),
        AppFolder::Logs => state.paths.logs_dir(),
        AppFolder::Backups => {
            state
                .with_db(|db| cairn_core::backup::backup_folder(db))
                .await?
        }
        AppFolder::SwitchBackups => {
            state
                .with_db(|db| Ok(cairn_core::knowledge::mode::switch_backups_dir(db)))
                .await?
        }
        AppFolder::Knowledge => {
            let storage = state
                .with_db(|db| cairn_core::knowledge::mode::storage(db.conn()))
                .await?;
            match storage.root {
                Some(root) if storage.root_exists => std::path::PathBuf::from(root),
                Some(root) => {
                    return Err(ApiError {
                        kind: "invalid".into(),
                        message: format!("The knowledge folder \"{root}\" can't be found."),
                    });
                }
                None => {
                    return Err(ApiError {
                        kind: "invalid".into(),
                        message: "Knowledge is not kept in a folder.".into(),
                    });
                }
            }
        }
    };
    std::fs::create_dir_all(&dir).map_err(|e| ApiError {
        kind: "io".into(),
        message: format!("The folder {} could not be created: {e}", dir.display()),
    })?;
    open_with_system(&dir.to_string_lossy()).map_err(|e| {
        tracing::warn!(error = %e, "could not start the file manager");
        ApiError::internal("The file manager could not be opened.")
    })
}

/// Hands a web address or a folder to the desktop's default program.
fn open_with_system(target: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", target]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(target);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        c
    };
    // The program outlives the call; its exit status says nothing useful.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn().map(|_| ())
}
