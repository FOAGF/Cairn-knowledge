//! Data folder layout (spec TC-9) and the small config file that points to it.
//!
//! The data folder can be moved in Settings, so its location cannot live inside the
//! database. It is kept in `~/.config/cairn/config.json` instead.
//!
//! **Windows (TC-9, D-56):** until v1.0 Cairn is a portable folder. In a release build on Windows
//! the default data folder is `<exe folder>\data` and `config.json` sits next to the exe, so the
//! folder holds everything except the API keys. The choice is made by [`default_data_dir_in`] and
//! [`config_file_in`], which take the exe folder as a parameter, so Linux tests can run it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

/// Paths inside one data folder.
#[derive(Debug, Clone)]
pub struct DataPaths {
    pub root: PathBuf,
}

impl DataPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// WebView2's own storage on Windows (cookies, cache; D-56): inside the data folder, so the
    /// application folder holds everything except the keys.
    pub fn webview_dir(&self) -> PathBuf {
        self.root.join("webview")
    }

    pub fn db_file(&self) -> PathBuf {
        self.root.join("cairn.sqlite")
    }

    /// Rotated log files (NFR-SEC-3).
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Copies of the database made before a migration (NFR-REL-3).
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    /// Pasted images and other page attachments (FR-KN-ED-3).
    pub fn attachments_dir(&self) -> PathBuf {
        self.root.join("attachments")
    }

    /// Uploaded files while their text is read (FR-KN-UP-6: deleted right after; leftovers are
    /// removed at startup). Inside the data folder so they never leave it.
    pub fn uploads_dir(&self) -> PathBuf {
        self.root.join("tmp").join("uploads")
    }

    /// The search model added after installing, and its download and check folders (FR-RAG-EM-3,
    /// D-82). Not part of backups or exports, which hold the database and attachments only.
    pub fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [
            self.root.clone(),
            self.logs_dir(),
            self.backups_dir(),
            self.attachments_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// Contents of `config.json`. Unknown fields are ignored, missing ones use defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    /// Custom data folder; `None` means the default.
    pub data_folder: Option<PathBuf>,
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "cairn")
}

/// The folder of `cairn.exe` when this run keeps its data beside the exe: release builds on
/// Windows only. `None` everywhere else (Linux, and debug builds, which must never touch real data).
pub fn portable_exe_dir() -> Option<PathBuf> {
    if !cfg!(all(windows, not(debug_assertions))) {
        return None;
    }
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// Default data folder for a given exe folder: `<exe folder>/data` when `portable_exe` is set,
/// else `~/.local/share/cairn` on Linux. Debug builds use `~/.local/share/cairn-dev`, so
/// development never touches real data.
pub fn default_data_dir_in(portable_exe: Option<&Path>) -> PathBuf {
    if let Some(exe_dir) = portable_exe {
        return exe_dir.join("data");
    }
    let dir = project_dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".cairn-data"));
    if cfg!(debug_assertions) {
        dir.with_file_name("cairn-dev")
    } else {
        dir
    }
}

/// `<exe folder>/config.json` when `portable_exe` is set, else `~/.config/cairn/config.json`.
pub fn config_file_in(portable_exe: Option<&Path>) -> Option<PathBuf> {
    match portable_exe {
        Some(exe_dir) => Some(exe_dir.join("config.json")),
        None => project_dirs().map(|d| d.config_dir().join("config.json")),
    }
}

/// Default data folder of this run (see the module comment).
pub fn default_data_dir() -> PathBuf {
    default_data_dir_in(portable_exe_dir().as_deref())
}

/// Where `config.json` is for this run (see the module comment).
pub fn config_file() -> Option<PathBuf> {
    config_file_in(portable_exe_dir().as_deref())
}

pub fn read_config(path: &Path) -> AppConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn write_config(path: &Path, config: &AppConfig) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(config)?)?;
    Ok(())
}

/// Where the data lives: `CAIRN_DATA_DIR` (dev and tests) > `config.json` > the default.
/// Debug builds ignore `config.json`, which belongs to the real installation.
pub fn resolve_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CAIRN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(debug_assertions) {
        return default_data_dir();
    }
    config_file()
        .map(|f| read_config(&f))
        .and_then(|c| c.data_folder)
        .unwrap_or_else(default_data_dir)
}

/// Folder names a OneDrive client creates: `OneDrive`, or `OneDrive - <organisation>`.
fn is_onedrive_name(component: &str) -> bool {
    let c = component.to_lowercase();
    c == "onedrive" || c.starts_with("onedrive - ")
}

/// A path as one comparable string: lower case, `/` separators, no trailing `/`, no `\\?\` prefix.
fn comparable(p: &str) -> String {
    let s = p.replace('\\', "/").to_lowercase();
    let s = s.strip_prefix("//?/").unwrap_or(&s);
    s.trim_end_matches('/').to_string()
}

/// Whether `data_dir` is inside a OneDrive folder (D-56 finding: a live SQLite database must not
/// be synced). Two signs, on every platform: the path starts with the folder named by one of the
/// environment variables `OneDrive`, `OneDriveConsumer`, `OneDriveCommercial` (compared without
/// regard to case or slash direction; `env` reads a variable), or a folder of the path is named
/// `OneDrive` / `OneDrive - ...`, which a sync client creates (also on Linux).
pub fn in_onedrive(data_dir: &Path, env: impl Fn(&str) -> Option<String>) -> bool {
    let path = comparable(&data_dir.to_string_lossy());
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        let Some(root) = env(var).map(|v| comparable(&v)).filter(|v| !v.is_empty()) else {
            continue;
        };
        if path == root || path.starts_with(&format!("{root}/")) {
            return true;
        }
    }
    path.split('/').any(is_onedrive_name)
}

/// The text for Home's *Needs attention* when the data folder is inside OneDrive, using the real
/// environment.
pub fn onedrive_warning(data_dir: &Path) -> Option<String> {
    in_onedrive(data_dir, |name| {
        std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
    })
    .then(|| {
        "Cairn's data folder is inside OneDrive: syncing a live database can damage it. Move the Cairn folder outside OneDrive."
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_layout_puts_data_and_config_next_to_the_exe() {
        let exe = Path::new(r"C:\Apps\Cairn-0.8.0");
        assert_eq!(default_data_dir_in(Some(exe)), exe.join("data"));
        assert_eq!(config_file_in(Some(exe)), Some(exe.join("config.json")));
    }

    #[test]
    fn without_a_portable_folder_the_usual_places_are_kept() {
        assert!(!default_data_dir_in(None).ends_with("data"));
        assert!(config_file_in(None).is_some_and(|f| f.ends_with("config.json")));
        // Tests and Linux are never portable.
        assert_eq!(portable_exe_dir(), None);
    }

    #[test]
    fn webview_storage_is_inside_the_data_folder() {
        assert_eq!(DataPaths::new("/d").webview_dir(), Path::new("/d/webview"));
    }

    #[test]
    fn onedrive_is_found_by_the_environment_without_regard_to_case() {
        let env = |name: &str| match name {
            "OneDrive" => Some(r"C:\Users\Me\Cloud - Contoso".to_string()),
            "OneDriveConsumer" => Some(r"C:\Users\Me\Cloud".to_string()),
            _ => None,
        };
        let inside = |p: &str| in_onedrive(Path::new(p), env);
        assert!(inside(r"c:\users\me\cloud - contoso\Cairn\data"));
        assert!(inside("C:/Users/Me/Cloud/Cairn"));
        assert!(inside(r"\\?\C:\Users\Me\Cloud\Cairn"));
        assert!(inside(r"C:\Users\Me\Cloud"));
        // Only a whole folder counts as a prefix.
        assert!(!inside(r"C:\Users\Me\Cloud2\Cairn"));
        assert!(!inside(r"C:\Users\Me\Documents\Cairn"));
    }

    #[test]
    fn onedrive_is_found_by_a_folder_name_and_empty_variables_are_ignored() {
        let none = |_: &str| Some(String::new());
        assert!(in_onedrive(Path::new("/home/me/OneDrive/cairn"), none));
        assert!(in_onedrive(
            Path::new("/home/me/OneDrive - Contoso/cairn"),
            none
        ));
        assert!(!in_onedrive(Path::new("/home/me/.local/share/cairn"), none));
        assert!(!in_onedrive(
            Path::new("/home/me/OneDriveBackup/cairn"),
            none
        ));
    }
}
