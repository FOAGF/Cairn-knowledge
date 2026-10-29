//! Non-secret app settings stored in the `settings` table (UI-SET-*). API keys never go here (NFR-SEC-1).
//!
//! One row per field, the value is JSON. Defaults live here: a missing row means the default,
//! and rows this version does not know are ignored. The data folder is not a setting in this
//! table (spec TC-9): it lives in `config.json`, see [`set_data_folder`].
//!
//! The knowledge mode and the external root (D-72) are read here but not part of
//! [`SettingsPatch`]: they change only through `knowledge::mode` (choosing the mode, switching,
//! importing), which also moves the knowledge.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths::{self, AppConfig};
use crate::{CoreError, Db, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// The interface language (NFR-I18N-2, D-79). The values are language tags, so the UI can use them
/// as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum Language {
    /// Chinese when the system language is Chinese, otherwise English (decided by the UI).
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
}

/// Where knowledge pages live (spec §5.11, D-72).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeMode {
    /// Pages are rows in Cairn's database.
    Internal,
    /// Pages are Markdown files in one root folder; the database holds an index of it.
    External,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Shown in greetings; empty means the UI uses a fallback.
    pub user_name: String,
    /// NFR-UX-1.
    pub theme: Theme,
    /// NFR-I18N-2.
    pub language: Language,
    /// Model ids arrive with providers in M4; plain strings for now.
    pub default_model_id: Option<String>,
    pub ocr_model_id: Option<String>,
    /// FR-KN-ED-4: pages above this many words get a warning.
    pub word_warning_threshold: u32,
    /// FR-KN-UP-2: files above this size (MB) get a warning.
    #[serde(rename = "largeFileWarningMB")]
    pub large_file_warning_mb: u32,
    /// FR-KN-DU-1: duplicate-check matches at or above this similarity (0..1) are marked
    /// "likely the same topic".
    pub duplicate_score: f32,
    /// FR-KN-SV-4: pages whose checks run at the same time in *Check all unchecked*.
    pub checks_at_once: u32,
    /// FR-RAG-EM-1a: the embedding model is unloaded after this many idle minutes.
    pub embedding_idle_timeout_min: u32,
    /// FR-LLM-AS-1: a result found only by meaning is a source for the assistant when its
    /// similarity is at least this (and within [`RELEVANCE_MARGIN`] of the best result).
    pub assistant_min_relevance: f32,
    /// FR-LLM-AS-1: tokens of sources sent with a question (less when the model takes less).
    pub assistant_source_budget: u32,
    /// FR-LLM-AS-10: tokens of earlier conversation sent with a question, unless a conversation
    /// was widened.
    pub assistant_history_budget: u32,
    /// FR-LLM-AS-12: most web searches the model may make for one message.
    pub web_searches_per_message: u32,
    /// FR-KN-ED-8.
    pub spellcheck: bool,
    pub grammar: bool,
    pub personal_dictionary: Vec<String>,
    /// NFR-REL-2.
    pub backup_enabled: bool,
    pub backup_folder: String,
    pub backup_keep: u32,
    /// FR-DATA-8: the first-start choice (*Start fresh* or an import) was made.
    pub welcome_done: bool,
    /// FR-KN-EXT-7: `None` on a database made in this run until the user chooses (the full-screen
    /// choice); a database from an earlier version is internal. Changed only by `knowledge::mode`.
    pub knowledge_mode: Option<KnowledgeMode>,
    /// The root folder in external mode (absolute); `None` in internal mode.
    pub external_root: Option<String>,
    /// FR-KN-EXT-9: every start warns when `switch-backups` is larger than this (GB, 1024³ bytes).
    pub switch_backup_warn_gb: f32,
}

/// Default of [`Settings::duplicate_score`].
pub const DEFAULT_DUPLICATE_SCORE: f32 = 0.78;

/// Default of [`Settings::assistant_min_relevance`]; calibrated with
/// `poc/i-assistant/relevance_check.py` (D-44).
pub const DEFAULT_MIN_RELEVANCE: f32 = 0.40;

/// FR-LLM-AS-1: a meaning-only result must also be within this of the question's best result.
/// Not a setting: the floor is the part that depends on the data.
pub const RELEVANCE_MARGIN: f32 = 0.30;

/// The smallest budget the settings accept, in tokens.
pub const MIN_BUDGET_TOKENS: u32 = 1000;

/// Default of [`Settings::switch_backup_warn_gb`] (FR-KN-EXT-9).
pub const DEFAULT_SWITCH_BACKUP_WARN_GB: f32 = 2.0;

impl Default for Settings {
    fn default() -> Self {
        Self {
            user_name: String::new(),
            theme: Theme::System,
            language: Language::System,
            default_model_id: None,
            ocr_model_id: None,
            word_warning_threshold: 1000,
            large_file_warning_mb: 10,
            // Calibrated with poc/h-create/similarity_check.py: same topic 0.84–0.95, related 0.63–0.68.
            duplicate_score: DEFAULT_DUPLICATE_SCORE,
            checks_at_once: 5,
            embedding_idle_timeout_min: 5,
            assistant_min_relevance: DEFAULT_MIN_RELEVANCE,
            assistant_source_budget: 8000,
            assistant_history_budget: 30000,
            web_searches_per_message: 5,
            spellcheck: true,
            grammar: true,
            personal_dictionary: Vec::new(),
            // D-45: on by default.
            backup_enabled: true,
            backup_folder: String::new(),
            backup_keep: 7,
            welcome_done: false,
            knowledge_mode: None,
            external_root: None,
            switch_backup_warn_gb: DEFAULT_SWITCH_BACKUP_WARN_GB,
        }
    }
}

/// Only the fields that are `Some` change. For the two model ids, an empty string clears the choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    pub user_name: Option<String>,
    pub theme: Option<Theme>,
    pub language: Option<Language>,
    pub default_model_id: Option<String>,
    pub ocr_model_id: Option<String>,
    pub word_warning_threshold: Option<u32>,
    #[serde(rename = "largeFileWarningMB")]
    pub large_file_warning_mb: Option<u32>,
    pub duplicate_score: Option<f32>,
    pub checks_at_once: Option<u32>,
    pub embedding_idle_timeout_min: Option<u32>,
    pub assistant_min_relevance: Option<f32>,
    pub assistant_source_budget: Option<u32>,
    pub assistant_history_budget: Option<u32>,
    pub web_searches_per_message: Option<u32>,
    pub spellcheck: Option<bool>,
    pub grammar: Option<bool>,
    pub personal_dictionary: Option<Vec<String>>,
    pub backup_enabled: Option<bool>,
    pub backup_folder: Option<String>,
    pub backup_keep: Option<u32>,
    pub welcome_done: Option<bool>,
    pub switch_backup_warn_gb: Option<f32>,
}

fn load_rows(conn: &Connection) -> Result<HashMap<String, Value>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut map = HashMap::new();
    for row in rows {
        let (key, raw) = row?;
        // A damaged value falls back to the default instead of breaking the app.
        if let Ok(v) = serde_json::from_str(&raw) {
            map.insert(key, v);
        }
    }
    Ok(map)
}

fn field<T: serde::de::DeserializeOwned>(
    rows: &HashMap<String, Value>,
    key: &str,
    default: T,
) -> T {
    rows.get(key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(default)
}

pub fn get(conn: &Connection) -> Result<Settings> {
    let rows = load_rows(conn)?;
    let d = Settings::default();
    Ok(Settings {
        user_name: field(&rows, "userName", d.user_name),
        theme: field(&rows, "theme", d.theme),
        language: field(&rows, "language", d.language),
        default_model_id: field(&rows, "defaultModelId", d.default_model_id),
        ocr_model_id: field(&rows, "ocrModelId", d.ocr_model_id),
        word_warning_threshold: field(&rows, "wordWarningThreshold", d.word_warning_threshold),
        large_file_warning_mb: field(&rows, "largeFileWarningMB", d.large_file_warning_mb),
        duplicate_score: field(&rows, "duplicateScore", d.duplicate_score),
        embedding_idle_timeout_min: field(
            &rows,
            "embeddingIdleTimeoutMin",
            d.embedding_idle_timeout_min,
        ),
        assistant_min_relevance: field(&rows, "assistantMinRelevance", d.assistant_min_relevance),
        assistant_source_budget: field(&rows, "assistantSourceBudget", d.assistant_source_budget),
        assistant_history_budget: field(
            &rows,
            "assistantHistoryBudget",
            d.assistant_history_budget,
        ),
        web_searches_per_message: field(
            &rows,
            "webSearchesPerMessage",
            d.web_searches_per_message,
        ),
        checks_at_once: field(&rows, "checksAtOnce", d.checks_at_once),
        spellcheck: field(&rows, "spellcheck", d.spellcheck),
        grammar: field(&rows, "grammar", d.grammar),
        personal_dictionary: field(&rows, "personalDictionary", d.personal_dictionary),
        backup_enabled: field(&rows, "backupEnabled", d.backup_enabled),
        backup_folder: field(&rows, "backupFolder", d.backup_folder),
        backup_keep: field(&rows, "backupKeep", d.backup_keep),
        welcome_done: field(&rows, "welcomeDone", d.welcome_done),
        knowledge_mode: field(&rows, KNOWLEDGE_MODE, d.knowledge_mode),
        external_root: field(&rows, EXTERNAL_ROOT, d.external_root),
        switch_backup_warn_gb: field(&rows, "switchBackupWarnGb", d.switch_backup_warn_gb),
    })
}

const KNOWLEDGE_MODE: &str = "knowledgeMode";
const EXTERNAL_ROOT: &str = "externalRoot";

/// The knowledge mode and, in external mode, the root folder (D-72). Cheap: two rows.
pub fn knowledge_mode(conn: &Connection) -> Result<(Option<KnowledgeMode>, Option<String>)> {
    let read = |key: &str| -> Result<Option<Value>> {
        Ok(conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .and_then(|raw| serde_json::from_str(&raw).ok()))
    };
    let mode = read(KNOWLEDGE_MODE)?.and_then(|v| serde_json::from_value(v).ok());
    let root = read(EXTERNAL_ROOT)?.and_then(|v| serde_json::from_value(v).ok());
    Ok((mode, root))
}

/// Records the knowledge mode and root (`knowledge::mode` only: it moves the knowledge too).
pub(crate) fn set_knowledge_mode(
    conn: &Connection,
    mode: KnowledgeMode,
    root: Option<&str>,
) -> Result<()> {
    put(conn, KNOWLEDGE_MODE, mode)?;
    match root {
        Some(r) if mode == KnowledgeMode::External => put(conn, EXTERNAL_ROOT, r)?,
        _ => {
            conn.execute("DELETE FROM settings WHERE key = ?1", [EXTERNAL_ROOT])?;
        }
    }
    Ok(())
}

/// FR-KN-EXT-7: a database that records no mode becomes internal (an earlier version made it, or an
/// import of an earlier version's export). Used when an existing database opens and after imports.
pub(crate) fn default_mode_to_internal(conn: &Connection) -> Result<()> {
    if knowledge_mode(conn)?.0.is_none() {
        set_knowledge_mode(conn, KnowledgeMode::Internal, None)?;
    }
    Ok(())
}

fn positive(value: Option<u32>, what: &str) -> Result<()> {
    match value {
        Some(0) => Err(CoreError::Invalid(format!(
            "{what} must be greater than 0."
        ))),
        _ => Ok(()),
    }
}

fn validate(p: &SettingsPatch) -> Result<()> {
    positive(p.word_warning_threshold, "The word warning threshold")?;
    positive(p.large_file_warning_mb, "The large file warning size")?;
    positive(p.embedding_idle_timeout_min, "The idle timeout")?;
    if let Some(v) = p.duplicate_score
        && !(v > 0.0 && v < 1.0)
    {
        return Err(CoreError::Invalid(
            "The \"likely the same topic\" score must be between 0 and 1.".into(),
        ));
    }
    if p.backup_keep == Some(0) {
        return Err(CoreError::Invalid("Keep at least 1 backup.".into()));
    }
    // NFR-REL-2: empty means the data folder's `backups`; anything else is a full path.
    if let Some(f) = &p.backup_folder
        && !f.trim().is_empty()
        && !Path::new(f.trim()).is_absolute()
    {
        return Err(CoreError::Invalid(
            "The backup folder must be a full path, for example /home/you/OneDrive/cairn.".into(),
        ));
    }
    if let Some(v) = p.assistant_min_relevance
        && !(v > 0.0 && v < 1.0)
    {
        return Err(CoreError::Invalid(
            "The minimum relevance must be between 0 and 1.".into(),
        ));
    }
    for (v, what) in [
        (p.assistant_source_budget, "The source budget"),
        (p.assistant_history_budget, "The history budget"),
    ] {
        if v.is_some_and(|v| v < MIN_BUDGET_TOKENS) {
            return Err(CoreError::Invalid(format!(
                "{what} must be at least {MIN_BUDGET_TOKENS} tokens."
            )));
        }
    }
    if p.checks_at_once.is_some_and(|v| !(1..=20).contains(&v)) {
        return Err(CoreError::Invalid(
            "Pages checked at once must be between 1 and 20.".into(),
        ));
    }
    if p.web_searches_per_message.is_some_and(|v| !(1..=20).contains(&v)) {
        return Err(CoreError::Invalid(
            "Web searches per message must be between 1 and 20.".into(),
        ));
    }
    if let Some(v) = p.switch_backup_warn_gb
        && !(v > 0.0 && v.is_finite())
    {
        return Err(CoreError::Invalid(
            "The switch backup warning size must be greater than 0 GB.".into(),
        ));
    }
    Ok(())
}

fn put(conn: &Connection, key: &str, value: impl Serialize) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, serde_json::to_string(&value)?],
    )?;
    Ok(())
}

fn model_id(v: &str) -> Option<&str> {
    let v = v.trim();
    (!v.is_empty()).then_some(v)
}

/// Applies the given fields (all or nothing) and returns the new settings.
pub fn update(db: &mut Db, patch: SettingsPatch) -> Result<Settings> {
    validate(&patch)?;
    db.tx(|tx| {
        if let Some(v) = &patch.user_name {
            put(tx, "userName", v.trim())?;
        }
        if let Some(v) = patch.theme {
            put(tx, "theme", v)?;
        }
        if let Some(v) = patch.language {
            put(tx, "language", v)?;
        }
        if let Some(v) = &patch.default_model_id {
            put(tx, "defaultModelId", model_id(v))?;
        }
        if let Some(v) = &patch.ocr_model_id {
            put(tx, "ocrModelId", model_id(v))?;
        }
        if let Some(v) = patch.word_warning_threshold {
            put(tx, "wordWarningThreshold", v)?;
        }
        if let Some(v) = patch.large_file_warning_mb {
            put(tx, "largeFileWarningMB", v)?;
        }
        if let Some(v) = patch.duplicate_score {
            put(tx, "duplicateScore", v)?;
        }
        if let Some(v) = patch.embedding_idle_timeout_min {
            put(tx, "embeddingIdleTimeoutMin", v)?;
        }
        if let Some(v) = patch.assistant_min_relevance {
            put(tx, "assistantMinRelevance", v)?;
        }
        if let Some(v) = patch.assistant_source_budget {
            put(tx, "assistantSourceBudget", v)?;
        }
        if let Some(v) = patch.assistant_history_budget {
            put(tx, "assistantHistoryBudget", v)?;
        }
        if let Some(v) = patch.web_searches_per_message {
            put(tx, "webSearchesPerMessage", v)?;
        }
        if let Some(v) = patch.checks_at_once {
            put(tx, "checksAtOnce", v)?;
        }
        if let Some(v) = patch.spellcheck {
            put(tx, "spellcheck", v)?;
        }
        if let Some(v) = patch.grammar {
            put(tx, "grammar", v)?;
        }
        if let Some(words) = &patch.personal_dictionary {
            // Trimmed, no blanks, no duplicates, order kept.
            let mut clean: Vec<String> = Vec::new();
            for w in words.iter().map(|w| w.trim()).filter(|w| !w.is_empty()) {
                if !clean.iter().any(|x| x == w) {
                    clean.push(w.to_string());
                }
            }
            put(tx, "personalDictionary", clean)?;
        }
        if let Some(v) = patch.backup_enabled {
            put(tx, "backupEnabled", v)?;
        }
        if let Some(v) = &patch.backup_folder {
            put(tx, "backupFolder", v.trim())?;
        }
        if let Some(v) = patch.backup_keep {
            put(tx, "backupKeep", v)?;
        }
        if let Some(v) = patch.welcome_done {
            put(tx, "welcomeDone", v)?;
        }
        if let Some(v) = patch.switch_backup_warn_gb {
            put(tx, "switchBackupWarnGb", v)?;
        }
        get(tx)
    })
}

/// Writes the data folder to `config_path` so the next start uses it (TC-9). Nothing is moved:
/// the folder is created if needed and must be writable.
pub fn set_data_folder(config_path: &Path, folder: &str) -> Result<()> {
    let folder = folder.trim();
    if folder.is_empty() {
        return Err(CoreError::Invalid("Choose a folder.".into()));
    }
    let path = Path::new(folder);
    if !path.is_absolute() {
        return Err(CoreError::Invalid(
            "The folder must be a full path, for example /home/you/cairn-data.".into(),
        ));
    }
    std::fs::create_dir_all(path)
        .map_err(|e| CoreError::Invalid(format!("That folder cannot be created: {e}")))?;
    let probe = path.join(".cairn-write-test");
    std::fs::write(&probe, b"")
        .map_err(|e| CoreError::Invalid(format!("That folder is not writable: {e}")))?;
    let _ = std::fs::remove_file(probe);
    let config = AppConfig {
        data_folder: Some(path.to_path_buf()),
    };
    paths::write_config(config_path, &config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_at(dir.path()).unwrap();
        (dir, db)
    }

    fn patch() -> SettingsPatch {
        SettingsPatch::default()
    }

    #[test]
    fn defaults_when_empty() {
        let (_d, db) = open();
        assert_eq!(get(db.conn()).unwrap(), Settings::default());
    }

    #[test]
    fn partial_update_keeps_other_fields() {
        let (_d, mut db) = open();
        let s = update(
            &mut db,
            SettingsPatch {
                theme: Some(Theme::Dark),
                user_name: Some("  YJ ".into()),
                ..patch()
            },
        )
        .unwrap();
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.user_name, "YJ");
        assert_eq!(s.word_warning_threshold, 1000);
        let s = update(
            &mut db,
            SettingsPatch {
                spellcheck: Some(false),
                ..patch()
            },
        )
        .unwrap();
        assert_eq!(s.theme, Theme::Dark);
        assert!(!s.spellcheck);
        assert_eq!(get(db.conn()).unwrap(), s);
    }

    #[test]
    fn language_is_stored_as_its_tag() {
        let (_d, mut db) = open();
        assert_eq!(get(db.conn()).unwrap().language, Language::System);
        let s = update(
            &mut db,
            SettingsPatch {
                language: Some(Language::ZhCn),
                ..patch()
            },
        )
        .unwrap();
        assert_eq!(s.language, Language::ZhCn);
        let raw: String = db
            .conn()
            .query_row("SELECT value FROM settings WHERE key = 'language'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(raw, "\"zh-CN\"");
    }

    #[test]
    fn validation_is_all_or_nothing() {
        let (_d, mut db) = open();
        let err = update(
            &mut db,
            SettingsPatch {
                theme: Some(Theme::Dark),
                backup_keep: Some(0),
                ..patch()
            },
        )
        .unwrap_err();
        assert_eq!(err.kind(), "invalid");
        assert_eq!(get(db.conn()).unwrap().theme, Theme::System);
        for p in [
            SettingsPatch {
                word_warning_threshold: Some(0),
                ..patch()
            },
            SettingsPatch {
                large_file_warning_mb: Some(0),
                ..patch()
            },
            SettingsPatch {
                embedding_idle_timeout_min: Some(0),
                ..patch()
            },
            SettingsPatch {
                checks_at_once: Some(0),
                ..patch()
            },
            SettingsPatch {
                checks_at_once: Some(21),
                ..patch()
            },
        ] {
            assert_eq!(update(&mut db, p).unwrap_err().kind(), "invalid");
        }
    }

    #[test]
    fn backups_are_on_by_default_and_the_folder_must_be_a_full_path() {
        let (_d, mut db) = open();
        let s = get(db.conn()).unwrap();
        assert!(s.backup_enabled && s.backup_folder.is_empty() && !s.welcome_done);
        let bad = update(
            &mut db,
            SettingsPatch {
                backup_folder: Some("relative/dir".into()),
                ..patch()
            },
        );
        assert_eq!(bad.unwrap_err().kind(), "invalid");
        let folder = _d.path().join("backups").to_string_lossy().into_owned();
        let s = update(
            &mut db,
            SettingsPatch {
                backup_folder: Some(format!(" {folder} ")),
                welcome_done: Some(true),
                ..patch()
            },
        )
        .unwrap();
        assert_eq!(s.backup_folder, folder);
        assert!(s.welcome_done);
        // Empty goes back to the default folder.
        let s = update(
            &mut db,
            SettingsPatch {
                backup_folder: Some(String::new()),
                ..patch()
            },
        )
        .unwrap();
        assert_eq!(s.backup_folder, "");
    }

    #[test]
    fn dictionary_is_cleaned_and_model_ids_clear() {
        let (_d, mut db) = open();
        let s = update(
            &mut db,
            SettingsPatch {
                personal_dictionary: Some(vec![
                    "Cairn".into(),
                    " ".into(),
                    "Cairn".into(),
                    "Tauri ".into(),
                ]),
                ocr_model_id: Some("m1".into()),
                ..patch()
            },
        )
        .unwrap();
        assert_eq!(s.personal_dictionary, vec!["Cairn", "Tauri"]);
        assert_eq!(s.ocr_model_id.as_deref(), Some("m1"));
        let s = update(
            &mut db,
            SettingsPatch {
                ocr_model_id: Some(String::new()),
                ..patch()
            },
        )
        .unwrap();
        assert_eq!(s.ocr_model_id, None);
    }

    #[test]
    fn unknown_and_damaged_rows_are_tolerated() {
        let (_d, db) = open();
        db.conn()
            .execute(
                "INSERT INTO settings (key, value) VALUES ('future', '1'), ('backupKeep', 'oops'), ('grammar', 'false')",
                [],
            )
            .unwrap();
        let s = get(db.conn()).unwrap();
        assert_eq!(s.backup_keep, 7);
        assert!(!s.grammar);
    }

    /// D-72: the mode is unset on a database made in this run, internal on an existing one, and
    /// never changed through `update`; the warning size is a normal setting.
    #[test]
    fn the_knowledge_mode_is_set_apart_from_the_other_settings() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_at(dir.path()).unwrap();
        let s = get(db.conn()).unwrap();
        assert_eq!((s.knowledge_mode, s.external_root.as_deref()), (None, None));
        assert_eq!(s.switch_backup_warn_gb, DEFAULT_SWITCH_BACKUP_WARN_GB);
        let s = update(
            &mut db,
            SettingsPatch {
                switch_backup_warn_gb: Some(0.5),
                ..patch()
            },
        )
        .unwrap();
        assert_eq!(s.switch_backup_warn_gb, 0.5);
        let bad = SettingsPatch {
            switch_backup_warn_gb: Some(0.0),
            ..patch()
        };
        assert_eq!(update(&mut db, bad).unwrap_err().kind(), "invalid");
        // The patch can't name the mode: unknown fields are ignored.
        let patch: SettingsPatch =
            serde_json::from_str(r#"{"knowledgeMode":"external","externalRoot":"/x"}"#).unwrap();
        assert_eq!(update(&mut db, patch).unwrap().knowledge_mode, None);
        set_knowledge_mode(db.conn(), KnowledgeMode::External, Some("/notes")).unwrap();
        let s = get(db.conn()).unwrap();
        assert_eq!(s.knowledge_mode, Some(KnowledgeMode::External));
        assert_eq!(s.external_root.as_deref(), Some("/notes"));
        drop(db);
        // Reopened: the recorded mode stays.
        let db = Db::open_at(dir.path()).unwrap();
        assert_eq!(knowledge_mode(db.conn()).unwrap().0, Some(KnowledgeMode::External));
        set_knowledge_mode(db.conn(), KnowledgeMode::Internal, Some("/ignored")).unwrap();
        assert_eq!(knowledge_mode(db.conn()).unwrap(), (Some(KnowledgeMode::Internal), None));
        // An existing database that records no mode is internal when it opens.
        db.conn().execute("DELETE FROM settings WHERE key = 'knowledgeMode'", []).unwrap();
        drop(db);
        let db = Db::open_at(dir.path()).unwrap();
        assert_eq!(get(db.conn()).unwrap().knowledge_mode, Some(KnowledgeMode::Internal));
    }

    #[test]
    fn data_folder_validation_and_write() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("cfg").join("config.json");
        assert!(set_data_folder(&cfg, "").is_err());
        assert!(set_data_folder(&cfg, "relative/dir").is_err());
        assert!(!cfg.exists());
        let target = dir.path().join("data");
        set_data_folder(&cfg, target.to_str().unwrap()).unwrap();
        assert!(target.is_dir());
        assert_eq!(paths::read_config(&cfg).data_folder, Some(target));
    }
}
