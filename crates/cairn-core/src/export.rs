//! Export and import of all data (spec §2.1, FR-DATA-1..7).
//!
//! # Archive format (FR-DATA-2)
//! A zip file with
//! - `manifest.json`: app version, export format version, schema version, export date,
//!   row count per table and attachment count (FR-DATA-4),
//! - `tables/<table>.jsonl`: one file per table. The first line is `{"table", "columns"}`, every
//!   other line is one row as a JSON array. SQLite value types are kept exactly: NULL, integer,
//!   real and text map to JSON null/integer/float/string, a blob is `{"$blob": "<base64>"}`
//!   (and an infinite real is `{"$real": "inf" | "-inf"}`),
//! - `attachments/...`: the files of the attachments folder, with their relative paths.
//!
//! # Which tables
//! Every table in the database is exported, so new milestones need no export code. Tables that
//! only hold derived data (search and vector indexes, FR-RAG-ST-2) go in [`DERIVED_TABLES`];
//! FTS virtual tables and their shadow tables are skipped automatically. Secrets are never in
//! the database (FR-DATA-5), so they are never in an archive.
//!
//! # Import
//! The imported database is built in a staging folder inside the data folder, at the archive's
//! schema version and then migrated to the latest one (FR-DATA-4). Only when that fully
//! succeeded is an automatic backup of the current data written to `backups/` (FR-DATA-6) and
//! the current database and attachments replaced ([`Db::replace_from`]). A failure before the
//! swap leaves the current data untouched.
//!
//! # Knowledge mode (D-72)
//! The manifest records the knowledge mode and, in external mode, the root folder
//! (`knowledgeMode`, `externalRoot`; absent in archives of earlier versions, which are internal).
//! A **switch backup** (D-72) is knowledge only ([`export_knowledge`]): categories, groups,
//! pages with their tags, history and task links, and the images their text uses; its manifest
//! says `knowledgeOnly`. It is loaded by switching (`knowledge::mode`), never imported: an import
//! refuses it ([`SWITCH_BACKUP_REFUSED`]). Switch backups made before 0.13.0's change are full
//! exports and still import.
//!
//! An external-mode archive holds the index of the folder, not the folder (FR-KN-EXT-14): it is
//! imported as it is when its folder is on this computer ([`import_from`]), with another folder
//! the user points to ([`import_with_root`]), or as its tasks and categories only
//! ([`import_tasks_only`]), which leaves Cairn in internal mode with no pages.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, params_from_iter};
use rusqlite_migration::Migrations;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use specta::Type;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::migrations::{latest_version, migrate_to, migrations};
use crate::settings::KnowledgeMode;
use crate::{CoreError, Db, Result};

/// Version of the archive layout described above.
pub const EXPORT_FORMAT_VERSION: u32 = 1;

/// Tables whose content can be rebuilt from other data and is therefore not exported
/// (FR-DATA-7): search and vector index tables. Add them here when they are created.
pub const DERIVED_TABLES: &[&str] = &[
    // Unsaved editor content belongs to this device (FR-KN-ED-10), so it isn't exported.
    "drafts",
    // Rowid map of the keyword index (search.rs); rebuilt after import.
    "search_ref",
    // The vector index (vectors.rs) and the model that made it; re-cut and re-embedded after import.
    "chunks",
    "vector_meta",
    // Which items use which glossary entry (glossary.rs); recomputed after import.
    "glossary_uses",
];

const MANIFEST: &str = "manifest.json";
const TABLES_DIR: &str = "tables/";
const ATTACHMENTS_DIR: &str = "attachments/";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    app_version: String,
    export_format: u32,
    schema_version: u32,
    exported_at: String,
    tables: BTreeMap<String, u64>,
    attachments: u64,
    /// D-72: `internal` or `external`; absent before 0.13.0 (internal) and while unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    knowledge_mode: Option<KnowledgeMode>,
    /// D-72: the root folder in external mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_root: Option<String>,
    /// D-72: a switch backup with knowledge only ([`export_knowledge`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    knowledge_only: bool,
}

/// What an import says about a knowledge-only switch backup.
pub const SWITCH_BACKUP_REFUSED: &str = "This is a switch backup: it holds knowledge pages only, not your tasks or settings. Use Settings › General › Switch to internal mode… to load its pages.";

/// The tables of a knowledge-only switch backup (categories, because pages need them).
const KNOWLEDGE_TABLES: &[&str] = &[
    "categories",
    "groups",
    "pages",
    "page_tags",
    "page_versions",
    "task_page_links",
];

/// What an archive's manifest says, for the switch backup list and the import dialog.
#[derive(Debug, Clone)]
pub(crate) struct ArchiveInfo {
    pub exported_at: String,
    pub app_version: String,
    pub schema_version: u32,
    pub mode: Option<KnowledgeMode>,
    pub root: Option<String>,
    /// Rows of `pages` and `tasks`.
    pub pages: u64,
    pub tasks: u64,
    /// A knowledge-only switch backup.
    pub knowledge_only: bool,
}

/// Reads an archive's manifest (leniently, like [`peek_manifest`]).
pub(crate) fn archive_info(archive: &Path) -> Result<ArchiveInfo> {
    let mut zip = ZipArchive::new(BufReader::new(File::open(archive)?))
        .map_err(|_| not_an_export("it is not a zip file"))?;
    let mut text = String::new();
    zip.by_name(MANIFEST)
        .map_err(|_| not_an_export("manifest.json is missing"))?
        .read_to_string(&mut text)
        .map_err(|_| not_an_export("manifest.json cannot be read"))?;
    let m: Manifest =
        serde_json::from_str(&text).map_err(|_| not_an_export("manifest.json is damaged"))?;
    Ok(ArchiveInfo {
        pages: m.tables.get("pages").copied().unwrap_or(0),
        tasks: m.tables.get("tasks").copied().unwrap_or(0),
        exported_at: m.exported_at,
        app_version: m.app_version,
        schema_version: m.schema_version,
        mode: m.knowledge_mode,
        root: m.external_root,
        knowledge_only: m.knowledge_only,
    })
}

/// Rows in one table of an archive.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableCount {
    pub table: String,
    pub rows: u64,
}

fn table_counts(m: &Manifest) -> Vec<TableCount> {
    m.tables
        .iter()
        .map(|(table, rows)| TableCount {
            table: table.clone(),
            rows: *rows,
        })
        .collect()
}

/// Result of an export (FR-DATA-1).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub path: String,
    pub file_size: u64,
    pub schema_version: u32,
    pub tables: Vec<TableCount>,
    pub attachments: u64,
}

/// Result of an import (FR-DATA-3, FR-DATA-6).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub tables: Vec<TableCount>,
    pub attachments: u64,
    /// Schema version the archive was made with (it is migrated to the current one).
    pub archive_schema_version: u32,
    pub archive_app_version: String,
    /// The automatic backup export of the data that was replaced.
    pub backup_path: String,
}

// ---------------------------------------------------------------------------------------------
// Tables

/// Names of the tables to export, sorted.
pub fn exported_tables(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name, sql FROM sqlite_master
         WHERE type = 'table'
           AND (name NOT LIKE 'sqlite\\_%' ESCAPE '\\' OR name = 'sqlite_sequence')
         ORDER BY name",
    )?;
    let all: Vec<(String, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let virtual_tables: Vec<&str> = all
        .iter()
        .filter(|(_, sql)| {
            sql.as_deref().is_some_and(|s| {
                s.trim_start()
                    .to_ascii_uppercase()
                    .starts_with("CREATE VIRTUAL")
            })
        })
        .map(|(n, _)| n.as_str())
        .collect();
    const SHADOW_SUFFIXES: &[&str] = &[
        "_data",
        "_idx",
        "_content",
        "_docsize",
        "_config",
        "_segments",
        "_segdir",
        "_stat",
        "_node",
        "_rowid",
        "_parent",
    ];
    let is_shadow = |name: &str| {
        virtual_tables.iter().any(|v| {
            name.strip_prefix(v)
                .is_some_and(|rest| SHADOW_SUFFIXES.contains(&rest))
        })
    };
    Ok(all
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| !virtual_tables.contains(n) && !is_shadow(n) && !DERIVED_TABLES.contains(n))
        .map(str::to_string)
        .collect())
}

/// `"name"` for use in SQL. Table and column names come from the database or are checked
/// against it before use.
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Writable columns of a table (generated columns are skipped, SQLite recomputes them).
fn columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_xinfo({})", quote(table)))?;
    let cols = stmt
        .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(6)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(cols
        .into_iter()
        .filter(|(_, hidden)| *hidden == 0)
        .map(|(n, _)| n)
        .collect())
}

fn value_to_json(v: ValueRef) -> Result<Json> {
    Ok(match v {
        ValueRef::Null => Json::Null,
        ValueRef::Integer(i) => Json::from(i),
        ValueRef::Real(f) => match serde_json::Number::from_f64(f) {
            Some(n) => Json::Number(n),
            None if f == f64::INFINITY => serde_json::json!({ "$real": "inf" }),
            None => serde_json::json!({ "$real": "-inf" }),
        },
        ValueRef::Text(t) => Json::String(
            std::str::from_utf8(t)
                .map_err(|_| CoreError::Invalid("A text value is not valid UTF-8.".into()))?
                .to_string(),
        ),
        ValueRef::Blob(b) => serde_json::json!({ "$blob": B64.encode(b) }),
    })
}

fn json_to_value(v: Json) -> Result<SqlValue> {
    let bad = || CoreError::Invalid("The export file contains a value Cairn cannot read.".into());
    Ok(match v {
        Json::Null => SqlValue::Null,
        Json::String(s) => SqlValue::Text(s),
        Json::Number(n) => match n.as_i64() {
            Some(i) => SqlValue::Integer(i),
            None => SqlValue::Real(n.as_f64().ok_or_else(bad)?),
        },
        Json::Object(o) => {
            if let Some(Json::String(b)) = o.get("$blob") {
                SqlValue::Blob(B64.decode(b).map_err(|_| bad())?)
            } else {
                match o.get("$real").and_then(Json::as_str) {
                    Some("inf") => SqlValue::Real(f64::INFINITY),
                    Some("-inf") => SqlValue::Real(f64::NEG_INFINITY),
                    _ => return Err(bad()),
                }
            }
        }
        _ => return Err(bad()),
    })
}

/// Writes one table as JSON lines. Returns the number of rows.
fn dump_table(conn: &Connection, table: &str, out: &mut impl Write) -> Result<u64> {
    let cols = columns(conn, table)?;
    serde_json::to_writer(
        &mut *out,
        &serde_json::json!({ "table": table, "columns": cols }),
    )?;
    out.write_all(b"\n")?;
    let sql = format!(
        "SELECT {} FROM {}",
        cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", "),
        quote(table)
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let values = (0..cols.len())
            .map(|i| value_to_json(row.get_ref(i)?))
            .collect::<Result<Vec<_>>>()?;
        serde_json::to_writer(&mut *out, &Json::Array(values))?;
        out.write_all(b"\n")?;
        count += 1;
    }
    Ok(count)
}

/// Inserts the rows of a table dump. `conn` must have the table already. Returns the row count.
fn load_table(conn: &Connection, table: &str, input: impl BufRead) -> Result<u64> {
    let invalid = |what: &str| CoreError::Invalid(format!("The export file is damaged ({what})."));
    let mut lines = input.lines();
    let header: Json = serde_json::from_str(&lines.next().ok_or_else(|| invalid("empty table"))??)
        .map_err(|_| invalid("table header"))?;
    if header["table"].as_str() != Some(table) {
        return Err(invalid("table name"));
    }
    let file_cols: Vec<String> =
        serde_json::from_value(header["columns"].clone()).map_err(|_| invalid("table columns"))?;
    let known = columns(conn, table)?;
    if let Some(c) = file_cols.iter().find(|c| !known.contains(c)) {
        return Err(CoreError::Invalid(format!(
            "The export file has data for a column that does not exist: {table}.{c}."
        )));
    }
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote(table),
        file_cols
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(", "),
        (1..=file_cols.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut count = 0;
    for line in lines {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let Json::Array(values) = serde_json::from_str(&line).map_err(|_| invalid("row"))? else {
            return Err(invalid("row"));
        };
        if values.len() != file_cols.len() {
            return Err(invalid("row length"));
        }
        let values = values
            .into_iter()
            .map(json_to_value)
            .collect::<Result<Vec<_>>>()?;
        stmt.execute(params_from_iter(values))?;
        count += 1;
    }
    Ok(count)
}

// ---------------------------------------------------------------------------------------------
// Export

fn zip_io(e: zip::result::ZipError) -> CoreError {
    CoreError::Io(std::io::Error::other(e))
}

/// All files below `dir` as (relative path with `/`, full path), sorted. Missing dir = none.
fn list_files(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(base, &path, out)?;
            } else if kind.is_file() {
                let rel = path
                    .strip_prefix(base)
                    .map_err(std::io::Error::other)?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push((rel, path));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// Exports all data and attachments into the zip file `dest` (FR-DATA-1, FR-DATA-2).
pub fn export_to(db: &Db, dest: &Path) -> Result<ExportSummary> {
    export_from_conn(
        db.conn(),
        &db.paths().attachments_dir(),
        dest,
        &crate::util::now_iso(),
    )
}

/// The export behind [`export_to`], working from any connection (NFR-REL-2). A backup passes its
/// own read-only connection, so the data is read in one read transaction (WAL gives a consistent
/// snapshot) and the app's connection stays free for the UI while a large export runs.
/// `exported_at` (UTC, RFC 3339) goes into the manifest.
pub(crate) fn export_from_conn(
    conn: &Connection,
    attachments: &Path,
    dest: &Path,
    exported_at: &str,
) -> Result<ExportSummary> {
    let part = {
        let mut n = dest.as_os_str().to_owned();
        n.push(".part");
        PathBuf::from(n)
    };
    let result = write_archive(conn, attachments, &part, exported_at, false);
    finish_export(result, &part, dest)
}

/// D-72: a knowledge-only switch backup of `db` at `dest`: the knowledge tables and the images
/// their text (page bodies and history) uses, with `knowledgeOnly` in the manifest.
pub(crate) fn export_knowledge(db: &Db, dest: &Path) -> Result<ExportSummary> {
    let part = {
        let mut n = dest.as_os_str().to_owned();
        n.push(".part");
        PathBuf::from(n)
    };
    let result = write_archive(
        db.conn(),
        &db.paths().attachments_dir(),
        &part,
        &crate::util::now_iso(),
        true,
    );
    finish_export(result, &part, dest)
}

fn finish_export(result: Result<Manifest>, part: &Path, dest: &Path) -> Result<ExportSummary> {
    match result {
        Ok(manifest) => {
            std::fs::rename(part, dest)?;
            Ok(ExportSummary {
                path: dest.to_string_lossy().into_owned(),
                file_size: std::fs::metadata(dest)?.len(),
                schema_version: manifest.schema_version,
                tables: table_counts(&manifest),
                attachments: manifest.attachments,
            })
        }
        Err(e) => {
            let _ = std::fs::remove_file(part);
            Err(e)
        }
    }
}

/// What a backup list needs from an archive's manifest: export time (UTC) and app version.
/// Lenient: it does not require a format or schema this Cairn understands.
pub(crate) fn peek_manifest(archive: &Path) -> Result<(String, String)> {
    let mut zip = ZipArchive::new(BufReader::new(File::open(archive)?))
        .map_err(|_| not_an_export("it is not a zip file"))?;
    let mut text = String::new();
    zip.by_name(MANIFEST)
        .map_err(|_| not_an_export("manifest.json is missing"))?
        .read_to_string(&mut text)
        .map_err(|_| not_an_export("manifest.json cannot be read"))?;
    let m: Manifest =
        serde_json::from_str(&text).map_err(|_| not_an_export("manifest.json is damaged"))?;
    Ok((m.exported_at, m.app_version))
}

/// Writes an archive: everything, or (`knowledge_only`) the knowledge tables and the images they use.
fn write_archive(
    conn: &Connection,
    attachments: &Path,
    file: &Path,
    exported_at: &str,
    knowledge_only: bool,
) -> Result<Manifest> {
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);
    let mut zip = ZipWriter::new(BufWriter::new(File::create(file)?));

    // One read transaction, so all tables come from the same moment.
    let tx = conn.unchecked_transaction()?;
    let (knowledge_mode, external_root) = crate::settings::knowledge_mode(conn)?;
    let mut manifest = Manifest {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        export_format: EXPORT_FORMAT_VERSION,
        schema_version: conn.query_row("PRAGMA user_version", [], |r| r.get(0))?,
        exported_at: exported_at.to_string(),
        tables: BTreeMap::new(),
        attachments: 0,
        knowledge_mode,
        external_root,
        knowledge_only,
    };
    let tables: Vec<String> = exported_tables(conn)?
        .into_iter()
        .filter(|t| !knowledge_only || KNOWLEDGE_TABLES.contains(&t.as_str()))
        .collect();
    // The images the knowledge uses (only those, for a knowledge-only archive).
    let used: Option<std::collections::BTreeSet<String>> = if knowledge_only {
        let mut stmt = conn.prepare("SELECT body FROM pages UNION ALL SELECT snapshot FROM page_versions")?;
        let texts = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Some(texts.iter().flat_map(|t| crate::attachments::referenced_names(t)).collect())
    } else {
        None
    };
    for table in tables {
        zip.start_file(format!("{TABLES_DIR}{table}.jsonl"), opts)
            .map_err(zip_io)?;
        let mut out = BufWriter::new(&mut zip);
        let rows = dump_table(conn, &table, &mut out)?;
        out.flush()?;
        drop(out);
        manifest.tables.insert(table, rows);
    }
    tx.commit()?;

    for (rel, path) in list_files(attachments)? {
        if used.as_ref().is_some_and(|u| !u.contains(&rel)) {
            continue;
        }
        zip.start_file(format!("{ATTACHMENTS_DIR}{rel}"), opts)
            .map_err(zip_io)?;
        std::io::copy(&mut File::open(path)?, &mut zip)?;
        manifest.attachments += 1;
    }

    zip.start_file(MANIFEST, opts).map_err(zip_io)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    zip.finish().map_err(zip_io)?.flush()?;
    Ok(manifest)
}

// ---------------------------------------------------------------------------------------------
// Import

fn not_an_export(why: &str) -> CoreError {
    CoreError::Invalid(format!("This file is not a valid Cairn export ({why})."))
}

fn read_manifest(zip: &mut ZipArchive<BufReader<File>>, latest: u32) -> Result<Manifest> {
    let mut text = String::new();
    zip.by_name(MANIFEST)
        .map_err(|_| not_an_export("manifest.json is missing"))?
        .read_to_string(&mut text)
        .map_err(|_| not_an_export("manifest.json cannot be read"))?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|_| not_an_export("manifest.json is damaged"))?;
    if manifest.export_format != EXPORT_FORMAT_VERSION {
        return Err(CoreError::Invalid(
            if manifest.export_format > EXPORT_FORMAT_VERSION {
                "This export was made by a newer version of Cairn and cannot be imported. Update Cairn first."
                .into()
            } else {
                format!("Export format {} is not supported.", manifest.export_format)
            },
        ));
    }
    if manifest.schema_version > latest {
        return Err(CoreError::Invalid(format!(
            "This export was made by a newer version of Cairn (data version {}, this Cairn \
             understands up to {latest}). Update Cairn first.",
            manifest.schema_version
        )));
    }
    Ok(manifest)
}

/// Version 9 was used by both the released 0.9 schema and D-69. Check table headers before
/// reconstructing an archive: building the newer schema first would skip conversation backfills.
fn archive_source_version(zip: &mut ZipArchive<BufReader<File>>, manifest: &Manifest) -> Result<u32> {
    if manifest.schema_version != 9 {
        return Ok(manifest.schema_version);
    }
    let mut headers = BTreeMap::<String, Vec<String>>::new();
    for name in manifest.tables.keys() {
        let entry = zip.by_name(&format!("{TABLES_DIR}{name}.jsonl"))
            .map_err(|_| not_an_export("a table is missing"))?;
        let line = BufReader::new(entry)
            .lines()
            .next()
            .ok_or_else(|| not_an_export("empty table"))?
            .map_err(|_| not_an_export("table header"))?;
        let header: Json = serde_json::from_str(&line).map_err(|_| not_an_export("table header"))?;
        if header["table"].as_str() != Some(name) {
            return Err(not_an_export("table name"));
        }
        let mut cols: Vec<String> = serde_json::from_value(header["columns"].clone())
            .map_err(|_| not_an_export("table columns"))?;
        cols.sort();
        headers.insert(name.clone(), cols);
    }
    if headers.get("conversations").is_some_and(|cols| cols.iter().any(|c| c == "model_id")) {
        return Ok(9);
    }
    let released = crate::migrations::is_released_09(&headers, |baseline| {
        let mut expected = BTreeMap::new();
        for table in exported_tables(baseline)? {
            let mut cols = columns(baseline, &table)?;
            cols.sort();
            expected.insert(table, cols);
        }
        Ok(expected)
    })?;
    if !released {
        return Err(not_an_export("unrecognized legacy schema"));
    }
    Ok(6)
}

/// Builds the database from the archive in `staging/cairn.sqlite` at the archive's schema
/// version, then migrates it to `latest` (FR-DATA-4). Attachments go to `staging/attachments`.
fn build_staging(
    zip: &mut ZipArchive<BufReader<File>>,
    manifest: &Manifest,
    staging: &Path,
    migs: &Migrations<'_>,
    latest: u32,
) -> Result<()> {
    let attachments = staging.join("attachments");
    std::fs::create_dir_all(&attachments)?;

    let mut conn = Connection::open(staging.join("cairn.sqlite"))?;
    // Foreign keys are checked once at the end, so the table order does not matter.
    conn.execute_batch("PRAGMA foreign_keys = OFF")?;
    let source_version = archive_source_version(zip, manifest)?;
    migrate_to(&mut conn, migs, source_version as usize)?;

    let tables = exported_tables(&conn)?;
    for name in manifest.tables.keys() {
        if !tables.contains(name) {
            return Err(CoreError::Invalid(format!(
                "The export contains a table that does not exist: {name}."
            )));
        }
    }
    let tx = conn.transaction()?;
    // Migrations may have seeded rows (e.g. the General category); the archive replaces them.
    for t in &tables {
        tx.execute(&format!("DELETE FROM {}", quote(t)), [])?;
    }
    // sqlite_sequence (AUTOINCREMENT counters) last: inserting rows already updates it.
    let mut order: Vec<&String> = manifest.tables.keys().collect();
    order.sort_by_key(|n| n.as_str() == "sqlite_sequence");
    for name in order {
        let expected = &manifest.tables[name];
        if name == "sqlite_sequence" {
            tx.execute("DELETE FROM sqlite_sequence", [])?;
        }
        let entry = zip
            .by_name(&format!("{TABLES_DIR}{name}.jsonl"))
            .map_err(|_| not_an_export(&format!("the data of table {name} is missing")))?;
        let rows = load_table(&tx, name, BufReader::new(entry))?;
        if rows != *expected {
            return Err(not_an_export(&format!("table {name} is incomplete")));
        }
    }
    tx.commit()?;
    // A knowledge-only switch backup refers to tasks it doesn't hold (task links, a page's source
    // task); loading it keeps only those that exist where it is loaded.
    let references = !manifest.knowledge_only;
    check_integrity(&conn, references)?;

    migrate_to(&mut conn, migs, latest as usize)?;
    debug_assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?,
        latest
    );
    check_integrity(&conn, references)?;
    conn.close().map_err(|(_, e)| e)?;

    let mut extracted = 0;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|_| not_an_export("a file is damaged"))?;
        if entry.is_dir() {
            continue;
        }
        let Some(rel) = entry
            .enclosed_name()
            .and_then(|p| p.strip_prefix("attachments").ok().map(Path::to_path_buf))
        else {
            continue;
        };
        let target = attachments.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::io::copy(&mut entry, &mut File::create(target)?)
            .map_err(|_| not_an_export("an attachment is damaged"))?;
        extracted += 1;
    }
    if extracted != manifest.attachments {
        return Err(not_an_export("attachments are missing"));
    }
    Ok(())
}

/// SQLite's integrity check, and (`references`) that every reference finds its row.
fn check_integrity(conn: &Connection, references: bool) -> Result<()> {
    let bad = if references {
        conn.prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| r.get::<_, String>(0))?
            .next()
            .transpose()?
    } else {
        None
    };
    if let Some(table) = bad {
        return Err(CoreError::Invalid(format!(
            "The export is inconsistent: {table} refers to data that is not in the file."
        )));
    }
    let ok: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if ok != "ok" {
        return Err(CoreError::Invalid(format!(
            "The imported data is damaged: {ok}"
        )));
    }
    Ok(())
}

/// Replaces all current data with the contents of the export `archive` (FR-DATA-3).
///
/// Order: validate, build the new database in a staging folder, write the pre-import backup
/// (FR-DATA-6), swap. Any error before the swap leaves the current data as it was.
/// An external-mode archive whose folder isn't on this computer is refused (FR-KN-EXT-14): the
/// UI asks for the folder ([`import_with_root`]) or offers the tasks only ([`import_tasks_only`]).
pub fn import_from(db: &mut Db, archive: &Path) -> Result<ImportSummary> {
    import_with(db, archive, &migrations(), latest_version(), &ImportAs::Whole)
}

/// FR-KN-EXT-14: imports an external-mode archive with its knowledge in the folder `root` (where
/// the user says it is now). The index then needs a scan of that folder.
pub fn import_with_root(db: &mut Db, archive: &Path, root: &str) -> Result<ImportSummary> {
    let root = crate::knowledge::mode::check_root(db, root)?;
    import_with(db, archive, &migrations(), latest_version(), &ImportAs::WithRoot(root))
}

/// FR-KN-EXT-14: imports only the archive's tasks (with their comments, history, links, labels
/// and images) and categories; nothing else of it (no pages, glossary, conversations or
/// settings). This computer's settings and AI providers (and their models) stay as they are; the
/// rest of the current data is replaced as by any import (pages, glossary and conversations end
/// up empty), after the usual backup, and Cairn is in internal mode.
pub fn import_tasks_only(db: &mut Db, archive: &Path) -> Result<ImportSummary> {
    import_with(db, archive, &migrations(), latest_version(), &ImportAs::TasksOnly)
}

/// How an archive is imported.
enum ImportAs {
    Whole,
    WithRoot(String),
    TasksOnly,
}

/// The tables a tasks-only import keeps (the rest are emptied; `activity` and `mentions` keep
/// only their task rows).
const TASK_TABLES: &[&str] = &[
    "categories",
    "tasks",
    "task_labels",
    "task_key_aliases",
    "task_links",
    "task_comments",
    "task_versions",
    "activity",
    "mentions",
    "attachments",
    "sqlite_sequence",
];

/// The tables a tasks-only import takes from this computer's current data, not from the archive:
/// the settings and the AI providers with their models (the owner's choice, 2026-10-20).
const KEPT_FROM_HERE: &[&str] = &["settings", "providers", "models"];

/// Leaves only the tasks and categories in a staged database (FR-KN-EXT-14), with `current`'s
/// settings and AI providers.
fn keep_tasks_only(db_file: &Path, current: &Connection) -> Result<()> {
    let conn = Connection::open(db_file)?;
    conn.execute_batch("PRAGMA foreign_keys = OFF")?;
    let tx = conn.unchecked_transaction()?;
    for t in exported_tables(&tx)? {
        if !TASK_TABLES.contains(&t.as_str()) {
            tx.execute(&format!("DELETE FROM {}", quote(&t)), [])?;
        }
    }
    tx.execute_batch(
        "DELETE FROM activity WHERE kind LIKE 'page%';
         DELETE FROM mentions WHERE source_kind = 'page' OR target_kind = 'page';
         UPDATE categories SET folder = NULL;",
    )?;
    for table in KEPT_FROM_HERE {
        let mut rows = Vec::new();
        dump_table(current, table, &mut rows)?;
        load_table(&tx, table, &rows[..])?;
    }
    crate::settings::set_knowledge_mode(&tx, KnowledgeMode::Internal, None)?;
    tx.commit()?;
    check_integrity(&conn, true)?;
    conn.close().map_err(|(_, e)| e)?;
    Ok(())
}

fn import_with(
    db: &mut Db,
    archive: &Path,
    migs: &Migrations<'_>,
    latest: u32,
    how: &ImportAs,
) -> Result<ImportSummary> {
    let mut zip = ZipArchive::new(BufReader::new(File::open(archive)?))
        .map_err(|_| not_an_export("it is not a zip file"))?;
    let manifest = read_manifest(&mut zip, latest)?;
    if manifest.knowledge_only {
        return Err(CoreError::Invalid(SWITCH_BACKUP_REFUSED.into()));
    }
    if matches!(how, ImportAs::Whole)
        && manifest.knowledge_mode == Some(KnowledgeMode::External)
    {
        let root = manifest.external_root.clone().unwrap_or_default();
        if root.is_empty() || !Path::new(&root).is_dir() {
            return Err(CoreError::Invalid(format!(
                "This backup keeps its knowledge in the folder \"{root}\", which isn't on this computer. Choose where that folder is now, or import only the tasks."
            )));
        }
    }

    // FR-DATA-6, D-58: the backup folder is a path that only fits this computer, so it is kept.
    let backup_folder = crate::settings::get(db.conn())?.backup_folder;

    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let staging = db.paths().root.join(format!("import-staging-{stamp}"));
    let result = (|| -> Result<PathBuf> {
        build_staging(&mut zip, &manifest, &staging, migs, latest)?;
        match how {
            ImportAs::Whole => {}
            ImportAs::TasksOnly => keep_tasks_only(&staging.join("cairn.sqlite"), db.conn())?,
            ImportAs::WithRoot(root) => {
                let conn = Connection::open(staging.join("cairn.sqlite"))?;
                if manifest.knowledge_mode == Some(KnowledgeMode::External) {
                    crate::settings::set_knowledge_mode(&conn, KnowledgeMode::External, Some(root))?;
                }
                conn.close().map_err(|(_, e)| e)?;
            }
        }
        let backup = db
            .paths()
            .backups_dir()
            .join(format!("pre-import-{stamp}.zip"));
        export_to(db, &backup)?;
        db.replace_from(&staging.join("cairn.sqlite"), &staging.join("attachments"))?;
        Ok(backup)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    let backup = result?;

    // Every other setting comes from the file; the backup folder stays this computer's.
    if crate::settings::get(db.conn())?.backup_folder != backup_folder {
        crate::settings::update(
            db,
            crate::settings::SettingsPatch {
                backup_folder: Some(backup_folder),
                ..Default::default()
            },
        )?;
    }

    // D-72: an archive of an earlier version records no knowledge mode: it is internal.
    crate::settings::default_mode_to_internal(db.conn())?;
    if matches!(how, ImportAs::TasksOnly)
        && let Err(e) = crate::attachments::collect_garbage(db)
    {
        tracing::warn!(error = %e, "could not remove the images of pages not imported");
    }

    // FR-DATA-7: derived data is rebuilt after an import: the keyword search index (not exported)
    // the mention index and the vector pieces (all new, waiting for the indexer).
    crate::hooks::rebuild_all(db.conn())?;

    tracing::info!(from_schema = manifest.schema_version, "import finished");
    // A tasks-only import reports what it took from the archive: its task tables and the images
    // they still use, not the archive's other contents (nor this computer's settings).
    let (tables, attachments) = match how {
        ImportAs::TasksOnly => {
            let mut counts = Vec::new();
            for table in TASK_TABLES.iter().filter(|t| **t != "sqlite_sequence") {
                let rows: i64 =
                    db.conn().query_row(&format!("SELECT COUNT(*) FROM {}", quote(table)), [], |r| r.get(0))?;
                counts.push(TableCount {
                    table: table.to_string(),
                    rows: rows as u64,
                });
            }
            let images = list_files(&db.paths().attachments_dir())?.len() as u64;
            (counts, images)
        }
        _ => (table_counts(&manifest), manifest.attachments),
    };
    Ok(ImportSummary {
        tables,
        attachments,
        archive_schema_version: manifest.schema_version,
        archive_app_version: manifest.app_version,
        backup_path: backup.to_string_lossy().into_owned(),
    })
}

/// An archive built into a staging folder in the data folder (at the current schema), for reading
/// parts of it (switching modes, D-72). The folder is removed when this is dropped.
pub(crate) struct Staged {
    pub dir: PathBuf,
}

impl Staged {
    pub fn db_file(&self) -> PathBuf {
        self.dir.join("cairn.sqlite")
    }
    pub fn attachments(&self) -> PathBuf {
        self.dir.join("attachments")
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Builds `archive` into a staging folder of the data folder, as an import does before its swap.
pub(crate) fn stage(db: &Db, archive: &Path) -> Result<Staged> {
    let mut zip = ZipArchive::new(BufReader::new(File::open(archive)?))
        .map_err(|_| not_an_export("it is not a zip file"))?;
    let latest = latest_version();
    let manifest = read_manifest(&mut zip, latest)?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let staged = Staged {
        dir: db.paths().root.join(format!("import-staging-read-{stamp}")),
    };
    build_staging(&mut zip, &manifest, &staged.dir, &migrations(), latest)?;
    Ok(staged)
}

/// Replaces all data with an empty database (*Demo data › Reset* in development builds,
/// UI-SET-4). Built in a staging folder like an import, then swapped in with
/// [`Db::replace_from`], so a failure leaves the current data as it was. Demo data is internal
/// mode's, so the new database is internal (D-72).
pub fn reset_to_empty(db: &mut Db) -> Result<()> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let staging = db
        .paths()
        .root
        .join(format!("import-staging-reset-{stamp}"));
    let result = (|| -> Result<()> {
        drop(Db::open_at(&staging)?);
        db.replace_from(&staging.join("cairn.sqlite"), &staging.join("attachments"))
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result?;
    crate::settings::set_knowledge_mode(db.conn(), KnowledgeMode::Internal, None)?;
    crate::hooks::rebuild_all(db.conn())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn open(dir: &Path) -> Db {
        Db::open_at(dir).unwrap()
    }

    /// Every exported table as sorted JSON rows.
    fn dump_all(db: &Db) -> BTreeMap<String, Vec<String>> {
        let conn = db.conn();
        exported_tables(conn)
            .unwrap()
            .into_iter()
            .map(|t| {
                let mut buf = Vec::new();
                dump_table(conn, &t, &mut buf).unwrap();
                let mut lines: Vec<String> = String::from_utf8(buf)
                    .unwrap()
                    .lines()
                    .map(String::from)
                    .collect();
                lines.sort();
                (t, lines)
            })
            .collect()
    }

    fn files(db: &Db) -> BTreeMap<String, Vec<u8>> {
        list_files(&db.paths().attachments_dir())
            .unwrap()
            .into_iter()
            .map(|(rel, p)| (rel, std::fs::read(p).unwrap()))
            .collect()
    }

    /// One row in each task table, so the round trip covers the M2 schema.
    fn fill_tasks(c: &Connection) {
        let snapshot = r#"{"title":"修复 bug","type":"bug","description":"详情 😀","status":"blocked","priority":"high","labels":["后端"],"dueDate":"2026-03-01","categoryId":"p1"}"#;
        for sql in [
            "UPDATE categories SET next_task_number = 9 WHERE id = 'p1'",
            "INSERT INTO tasks (id, key, number, category_id, title, type, description, status, priority, due_date, created_at, updated_at, completed_at)
             VALUES ('t1', 'ALP-7', 7, 'p1', '修复 bug', 'bug', '详情 😀', 'done', 'high', '2026-03-01', '2026-01-03T00:00:00.000Z', '2026-01-04T00:00:00.000Z', '2026-01-04T00:00:00.000Z')",
            "INSERT INTO tasks (id, key, number, category_id, title, created_at, updated_at)
             VALUES ('t2', 'ALP-8', 8, 'p1', 'Second', '2026-01-03T00:00:00.000Z', '2026-01-03T00:00:00.000Z')",
            "INSERT INTO task_labels (task_id, label) VALUES ('t1', '后端')",
            "INSERT INTO task_key_aliases (key, task_id) VALUES ('GEN-3', 't1')",
            "INSERT INTO task_links (id, type, from_task_id, to_task_id, created_at)
             VALUES ('l1', 'parent', 't1', 't2', '2026-01-03T00:00:00.000Z')",
            "INSERT INTO task_comments (id, task_id, text, created_at, updated_at)
             VALUES ('c1', 't1', '评论 \"quoted\"', '2026-01-03T00:00:00.000Z', '2026-01-03T01:00:00.000Z')",
            "INSERT INTO activity (id, kind, ref_id, at)
             VALUES ('a1', 'task_created', 't1', '2026-01-03T00:00:00.000Z')",
        ] {
            c.execute(sql, []).unwrap();
        }
        c.execute(
            "INSERT INTO task_versions (task_id, day, snapshot) VALUES ('t1', '2026-01-03', ?1)",
            [snapshot],
        )
        .unwrap();
    }

    /// One row in each M3 knowledge table. A draft is filled too, but drafts are not exported.
    fn fill_knowledge(c: &Connection) {
        let snapshot = r#"{"title":"知识 页面","body":"正文 [任务](page:pg1) #ALP-7 ![i](attachment:image.bin)","tags":["标签"],"groupId":"g1","sourceInfo":"来源 😀"}"#;
        for sql in [
            "INSERT INTO groups (id, category_id, name, description, created_at, updated_at)
             VALUES ('g1', 'p1', '分组', 'Ünï description', '2026-01-05T00:00:00.000Z', '2026-01-05T00:00:00.000Z')",
            "INSERT INTO pages (id, category_id, group_id, title, body, source_info, origin, origin_task_id, check_status, archived, created_at, updated_at)
             VALUES ('pg1', 'p1', 'g1', '知识 页面', '正文 [任务](page:pg1) #ALP-7 ![i](attachment:image.bin)', '来源 😀', 'task', 't1', 'checked', 1, '2026-01-05T00:00:00.000Z', '2026-01-06T00:00:00.000Z')",
            "INSERT INTO page_tags (page_id, tag) VALUES ('pg1', '标签')",
            "INSERT INTO task_page_links (id, task_id, page_id, created_at)
             VALUES ('tl1', 't1', 'pg1', '2026-01-05T00:00:00.000Z')",
            "INSERT INTO mentions (source_kind, source_id, target_kind, target_id)
             VALUES ('page', 'pg1', 'task', 't1')",
            "INSERT INTO attachments (name, mime, size, created_at)
             VALUES ('image.bin', 'image/png', 100000, '2026-01-05T00:00:00.000Z')",
            "INSERT INTO drafts (kind, ref_id, content, updated_at)
             VALUES ('new_page', 'x', '{\"title\":\"草稿\"}', '2026-01-05T00:00:00.000Z')",
        ] {
            c.execute(sql, []).unwrap();
        }
        c.execute(
            "INSERT INTO page_versions (page_id, day, snapshot) VALUES ('pg1', '2026-01-05', ?1)",
            [snapshot],
        )
        .unwrap();
    }

    fn fill(db: &Db) {
        let c = db.conn();
        c.execute(
            "INSERT INTO categories (id, name, key, description, color, status, is_general, created_at, updated_at)
             VALUES ('p1', '知识库 Alpha', 'ALP', 'Ünïcödé: 中文描述 \"quoted\" 😀', 'blue', 'archived', 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z')",
            [],
        )
        .unwrap();
        c.execute(
            "UPDATE categories SET description = '通用', color = 'red' WHERE is_general = 1",
            [],
        )
        .unwrap();
        for (k, v) in [
            ("theme", "\"dark\""),
            ("语言", "{\"a\":[1,2.5,null],\"b\":\"中文\"}"),
        ] {
            c.execute("INSERT INTO settings (key, value) VALUES (?1, ?2)", [k, v])
                .unwrap();
        }
        fill_tasks(c);
        fill_knowledge(c);
        let att = db.paths().attachments_dir();
        std::fs::create_dir_all(att.join("sub/文件夹")).unwrap();
        std::fs::write(
            att.join("image.bin"),
            (0..=255u8).cycle().take(100_000).collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(att.join("sub/文件夹/笔记 note.txt"), "你好，世界\n").unwrap();
        std::fs::write(att.join("empty"), b"").unwrap();
    }

    /// TC-11, the M1 exit check: export, import into a fresh data folder, everything identical.
    #[test]
    fn export_import_round_trip() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let source = open(a.path());
        fill(&source);
        let zip_file = a.path().join("out.zip");
        let summary = export_to(&source, &zip_file).unwrap();
        assert!(summary.file_size > 0);
        assert_eq!(summary.attachments, 3);
        assert!(
            summary
                .tables
                .iter()
                .any(|t| t.table == "categories" && t.rows == 2)
        );

        // The target has other data, which must disappear. It is an existing (internal) one.
        let mut target = open(b.path());
        target
            .conn()
            .execute("INSERT INTO settings VALUES ('old', '1')", [])
            .unwrap();
        target
            .conn()
            .execute("INSERT INTO settings VALUES ('knowledgeMode', '\"internal\"')", [])
            .unwrap();
        std::fs::write(target.paths().attachments_dir().join("old.txt"), "old").unwrap();
        let before = dump_all(&target);

        let imported = import_from(&mut target, &zip_file).unwrap();
        assert_eq!(imported.archive_schema_version, latest_version());

        // D-72: the source recorded no mode (made in this run); the import makes it internal.
        source
            .conn()
            .execute("INSERT INTO settings VALUES ('knowledgeMode', '\"internal\"')", [])
            .unwrap();
        assert_eq!(dump_all(&target), dump_all(&source));
        // The vector pieces are derived data: not in the archive, cut again after the import
        // (FR-DATA-7), all waiting for the indexer.
        assert!(
            !summary
                .tables
                .iter()
                .any(|t| t.table == "chunks" || t.table == "vector_meta")
        );
        let pieces = |db: &Db| -> (i64, i64) {
            db.conn()
                .query_row("SELECT COUNT(*), COUNT(vector) FROM chunks", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap()
        };
        let (n, ready) = pieces(&target);
        assert!(n >= 2 && ready == 0, "{n} pieces, {ready} with vectors");
        // Knowledge rows arrive; drafts stay on the device (FR-KN-ED-10).
        let count = |db: &Db, table: &str| -> i64 {
            db.conn()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        };
        for table in [
            "groups",
            "pages",
            "page_tags",
            "page_versions",
            "task_page_links",
            "mentions",
            "attachments",
        ] {
            assert_eq!(count(&target, table), 1, "{table}");
        }
        assert_eq!(count(&source, "drafts"), 1);
        assert_eq!(count(&target, "drafts"), 0);
        assert_eq!(files(&target), files(&source));
        assert_eq!(files(&target).len(), 3);
        // The connection works after the swap.
        target
            .conn()
            .execute("INSERT INTO settings VALUES ('after', '1')", [])
            .unwrap();
        target
            .tx(|tx| {
                tx.execute("DELETE FROM settings WHERE key = 'after'", [])?;
                Ok(())
            })
            .unwrap();

        // FR-DATA-6: the replaced data was exported to backups/ first.
        let backup = PathBuf::from(&imported.backup_path);
        assert!(backup.starts_with(target.paths().backups_dir()));
        let restore = tempfile::tempdir().unwrap();
        let mut again = open(restore.path());
        import_from(&mut again, &backup).unwrap();
        assert_eq!(dump_all(&again), before);

        // No staging or stash leftovers.
        let leftovers: Vec<_> = std::fs::read_dir(b.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("staging") || n.contains("pre-import"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    fn rewrite_manifest(src: &Path, dst: &Path, edit: impl Fn(&mut Json)) {
        let mut input = ZipArchive::new(File::open(src).unwrap()).unwrap();
        let mut out = ZipWriter::new(File::create(dst).unwrap());
        for i in 0..input.len() {
            let mut e = input.by_index(i).unwrap();
            let name = e.name().to_string();
            let mut data = Vec::new();
            e.read_to_end(&mut data).unwrap();
            if name == MANIFEST {
                let mut m: Json = serde_json::from_slice(&data).unwrap();
                edit(&mut m);
                data = serde_json::to_vec(&m).unwrap();
            }
            out.start_file(name, SimpleFileOptions::default()).unwrap();
            out.write_all(&data).unwrap();
        }
        out.finish().unwrap();
    }

    fn assert_untouched_after_failure(db: &mut Db, archive: &Path, expect: &str) {
        let before = (dump_all(db), files(db));
        let err = import_from(db, archive).unwrap_err().to_string();
        assert!(err.contains(expect), "{err}");
        assert_eq!((dump_all(db), files(db)), before);
        assert_eq!(
            std::fs::read_dir(db.paths().backups_dir()).unwrap().count(),
            0
        );
        assert!(
            std::fs::read_dir(&db.paths().root).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("staging"))
        );
        // Still usable.
        db.conn()
            .execute("INSERT INTO settings VALUES ('x', '1')", [])
            .unwrap();
        db.conn()
            .execute("DELETE FROM settings WHERE key = 'x'", [])
            .unwrap();
    }

    #[test]
    fn import_refuses_bad_archives_without_touching_data() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let source = open(a.path());
        fill(&source);
        let good = a.path().join("good.zip");
        export_to(&source, &good).unwrap();
        let mut target = open(b.path());
        fill(&target);

        let newer = a.path().join("newer.zip");
        rewrite_manifest(&good, &newer, |m| {
            m["schemaVersion"] = Json::from(latest_version() + 1)
        });
        assert_untouched_after_failure(&mut target, &newer, "newer version of Cairn");

        let newer_fmt = a.path().join("newer-format.zip");
        rewrite_manifest(&good, &newer_fmt, |m| m["exportFormat"] = Json::from(99));
        assert_untouched_after_failure(&mut target, &newer_fmt, "newer version of Cairn");

        let damaged = a.path().join("damaged.zip");
        rewrite_manifest(&good, &damaged, |m| *m = Json::String("nonsense".into()));
        assert_untouched_after_failure(&mut target, &damaged, "manifest.json is damaged");

        let wrong_count = a.path().join("count.zip");
        rewrite_manifest(&good, &wrong_count, |m| {
            m["tables"]["settings"] = Json::from(99)
        });
        assert_untouched_after_failure(&mut target, &wrong_count, "incomplete");

        let unknown = a.path().join("unknown.zip");
        rewrite_manifest(&good, &unknown, |m| m["tables"]["nope"] = Json::from(0));
        assert_untouched_after_failure(&mut target, &unknown, "does not exist");

        let garbage = a.path().join("garbage.zip");
        std::fs::write(&garbage, b"this is not a zip").unwrap();
        assert_untouched_after_failure(&mut target, &garbage, "not a valid Cairn export");

        let no_manifest = a.path().join("nomanifest.zip");
        let mut z = ZipWriter::new(File::create(&no_manifest).unwrap());
        z.start_file("other.txt", SimpleFileOptions::default())
            .unwrap();
        z.finish().unwrap();
        assert_untouched_after_failure(&mut target, &no_manifest, "manifest.json is missing");

        assert_untouched_after_failure(&mut target, &a.path().join("missing.zip"), "file error");
    }

    /// FR-DATA-4: an archive from an older schema is imported at its own version and migrated.
    /// Simulated with a second, test-only migration step.
    #[test]
    fn older_archive_is_migrated() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let source = open(a.path());
        fill(&source);
        let zip_file = a.path().join("v1.zip");
        export_to(&source, &zip_file).unwrap();

        // Pretend the code is one version newer than the archive: the real steps plus one more.
        let current = crate::migrations::latest_version();
        let mut steps: Vec<_> = crate::migrations::steps()
            .iter()
            .map(|sql| rusqlite_migration::M::up(sql))
            .collect();
        steps.push(rusqlite_migration::M::up(
            "ALTER TABLE categories ADD COLUMN extra TEXT NOT NULL DEFAULT 'x'",
        ));
        let migs = Migrations::new(steps);
        let mut zip = ZipArchive::new(BufReader::new(File::open(&zip_file).unwrap())).unwrap();
        let manifest = read_manifest(&mut zip, current + 1).unwrap();
        assert_eq!(manifest.schema_version, current);
        let staging = b.path().join("staging");
        build_staging(&mut zip, &manifest, &staging, &migs, current + 1).unwrap();

        let conn = Connection::open(staging.join("cairn.sqlite")).unwrap();
        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, current + 1);
        let (name, extra): (String, String) = conn
            .query_row(
                "SELECT name, extra FROM categories WHERE id = 'p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), extra.as_str()), ("知识库 Alpha", "x"));
        let categories: u32 = conn
            .query_row("SELECT count(*) FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(categories, 2);
        assert_eq!(list_files(&staging.join("attachments")).unwrap().len(), 3);
    }

    /// D-72: an export made before step 10 (schema version 9) still imports: its pages and groups
    /// come in, migrated, and the data is in internal mode.
    #[test]
    fn an_export_from_before_the_external_mode_imports() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut conn = Connection::open(a.path().join("old.sqlite")).unwrap();
        crate::migrations::migrations().to_version(&mut conn, 9).unwrap();
        conn.execute_batch(
            "INSERT INTO categories (id, name, key, created_at, updated_at) VALUES ('c1', 'Ops', 'OPS', 'x', 'x');
             INSERT INTO groups (id, category_id, name, description, created_at, updated_at)
             VALUES ('g1', 'c1', 'Pumps', '', 'x', 'x');
             INSERT INTO pages (id, category_id, group_id, title, body, created_at, updated_at)
             VALUES ('pg1', 'c1', 'g1', 'Pump check', 'Text', 'x', 'x');
             INSERT INTO page_tags (page_id, tag) VALUES ('pg1', 'pumps');",
        )
        .unwrap();
        let zip_file = a.path().join("v9.zip");
        let summary =
            export_from_conn(&conn, &a.path().join("attachments"), &zip_file, "2026-10-01T00:00:00.000Z").unwrap();
        assert_eq!(summary.schema_version, 9);
        let mut target = open(b.path());
        let imported = import_from(&mut target, &zip_file).unwrap();
        assert_eq!(imported.archive_schema_version, 9);
        let page = crate::knowledge::get_page(target.conn(), "pg1").unwrap();
        assert_eq!((page.group_id.as_deref(), page.tags.clone(), page.path), (Some("g1"), vec!["pumps".to_string()], None));
        assert_eq!(
            crate::settings::knowledge_mode(target.conn()).unwrap(),
            (Some(KnowledgeMode::Internal), None)
        );
        // A new export records the mode.
        let again = b.path().join("again.zip");
        export_to(&target, &again).unwrap();
        let info = archive_info(&again).unwrap();
        assert_eq!((info.mode, info.root, info.pages), (Some(KnowledgeMode::Internal), None, 1));
    }

    #[test]
    fn released_090_archive_backfills_conversation_preferences() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../tests/fixtures/legacy-0.9.0.sql")).unwrap();
        crate::migrations::tests::fill_legacy_conversation(&c);
        let archive = a.path().join("legacy.zip");
        export_from_conn(&c, &a.path().join("attachments"), &archive, "2026-10-01T00:00:00.000Z").unwrap();
        let mut target = open(b.path());
        import_from(&mut target, &archive).unwrap();
        crate::migrations::tests::assert_legacy_conversation(target.conn());
    }

    /// A version-9 archive whose table header is damaged is refused with the plain message.
    #[test]
    fn a_damaged_table_header_is_refused_plainly() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../tests/fixtures/legacy-0.9.0.sql")).unwrap();
        let good = a.path().join("legacy.zip");
        export_from_conn(&c, &a.path().join("attachments"), &good, "2026-10-01T00:00:00.000Z").unwrap();
        let bad = a.path().join("damaged.zip");
        let mut from = ZipArchive::new(File::open(&good).unwrap()).unwrap();
        let mut to = ZipWriter::new(File::create(&bad).unwrap());
        for i in 0..from.len() {
            let mut f = from.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            if name.ends_with("conversations.jsonl") {
                let text = String::from_utf8(bytes).unwrap();
                let (header, rest) = text.split_once('\n').unwrap();
                bytes = format!("{}\n{rest}", &header[..header.len() / 2]).into_bytes();
            }
            to.start_file(name, SimpleFileOptions::default()).unwrap();
            to.write_all(&bytes).unwrap();
        }
        to.finish().unwrap();
        let err = import_from(&mut open(b.path()), &bad).unwrap_err().to_string();
        assert!(err.contains("not a valid Cairn export (table header)"), "{err}");
    }

    /// All SQLite value types survive a dump and load unchanged.
    #[test]
    fn value_types_are_preserved() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, a, b, c, d, e, \"we\"\"ird\" TEXT)",
        )
        .unwrap();
        let blob: Vec<u8> = (0..=255).collect();
        conn.execute(
            "INSERT INTO t VALUES (1, NULL, 9223372036854775807, 1.0, '中文 \"x\" \\ \n', ?1, 'q')",
            params![blob],
        )
        .unwrap();
        conn.execute("INSERT INTO t VALUES (2, -5, 0.1, 1e300, '', x'', 'r')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO t VALUES (3, 1, 1e999, -1e999, '1', x'00', NULL)",
            [],
        )
        .unwrap();
        let mut buf = Vec::new();
        assert_eq!(dump_table(&conn, "t", &mut buf).unwrap(), 3);

        let other = Connection::open_in_memory().unwrap();
        other
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, a, b, c, d, e, \"we\"\"ird\" TEXT)",
            )
            .unwrap();
        assert_eq!(load_table(&other, "t", &buf[..]).unwrap(), 3);

        let dump = |c: &Connection| {
            let mut v = Vec::new();
            let mut stmt = c
                .prepare("SELECT id, typeof(a), a, typeof(b), b, typeof(c), c, typeof(d), d, typeof(e), e, \"we\"\"ird\" FROM t ORDER BY id")
                .unwrap();
            let mut rows = stmt.query([]).unwrap();
            while let Some(r) = rows.next().unwrap() {
                v.push(format!(
                    "{:?}",
                    (0..12)
                        .map(|i| r.get::<_, SqlValue>(i).unwrap())
                        .collect::<Vec<_>>()
                ));
            }
            v
        };
        assert_eq!(dump(&conn), dump(&other));
    }

    #[test]
    fn derived_and_internal_tables_are_skipped() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);
             CREATE VIRTUAL TABLE notes_fts USING fts5(body);
             CREATE TABLE autoinc (id INTEGER PRIMARY KEY AUTOINCREMENT);
             INSERT INTO autoinc DEFAULT VALUES;",
        )
        .unwrap();
        // notes_fts, notes_fts_data, ..._idx etc. are skipped; sqlite_sequence is kept.
        assert_eq!(
            exported_tables(&conn).unwrap(),
            ["autoinc", "notes", "sqlite_sequence"]
        );
    }

    /// FR-DATA-5: secrets never live in the database, so no column may look like one.
    #[test]
    fn no_table_has_a_secret_column() {
        let dir = tempfile::tempdir().unwrap();
        let db = open(dir.path());
        for t in exported_tables(db.conn()).unwrap() {
            for c in columns(db.conn(), &t).unwrap() {
                let c = c.to_lowercase();
                assert!(
                    !["secret", "password", "token", "api_key", "apikey"]
                        .iter()
                        .any(|w| c.contains(w)),
                    "{t}.{c} looks like a secret"
                );
            }
        }
    }

    /// UI-SET-4: the dev-only reset leaves an empty database (only the General category) and no
    /// staging folder behind.
    #[test]
    fn reset_to_empty_replaces_everything() {
        let a = tempfile::tempdir().unwrap();
        let mut db = open(a.path());
        fill(&db);
        std::fs::write(db.paths().attachments_dir().join("old.txt"), "old").unwrap();
        reset_to_empty(&mut db).unwrap();
        let categories: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(categories, 1);
        assert!(files(&db).is_empty());
        assert!(
            std::fs::read_dir(a.path()).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("staging"))
        );
        // The database is usable.
        db.conn()
            .execute("INSERT INTO settings VALUES ('x', '1')", [])
            .unwrap();
    }
}
