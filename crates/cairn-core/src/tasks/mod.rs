//! Tasks (spec §4): keys, CRUD, moving, labels, links, comments, daily history,
//! working days and the completion summary, the list query, and the activity log.
//!
//! The function signatures here are the M2 contract used by `src-tauri/src/commands/tasks.rs`.
//! Every change to a task:
//! - updates `updated_at`;
//! - sets or clears `completed_at` on closing or reopening (FR-TASK-ST-4);
//! - overwrites today's version in `task_versions` (local date; FR-TASK-HI-1, -4);
//! - writes activity: `task_created`, `task_completed`, or one `task_updated` per task per day (replaced).
//!
//! Decisions: tasks can't be created in or moved to an archived category; comments don't touch the task's
//! `updated_at` (only the activity log); moving a task to its own category does nothing; an update that
//! changes nothing writes nothing. Each public function runs in one transaction and delegates to an
//! internal `*_in` function that takes the clock (`Stamp`), so tests can pick the day.

pub(crate) mod history;
mod links;
pub mod model;
mod query;
mod seed;
pub mod workdays;

#[cfg(test)]
mod tests;

use std::collections::HashSet;

use chrono::{DateTime, Local, NaiveDate};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Serialize, de::DeserializeOwned};

use crate::bulk::BulkResult;
use crate::util::{new_id, now_iso};
use crate::{CoreError, Db, Result, categories};
pub use model::*;

// ---------------------------------------------------------------- clock

/// "Now" for one change: the UTC timestamp and the local calendar day (FR-TASK-HI-4).
#[derive(Debug, Clone)]
pub(crate) struct Stamp {
    pub at: String,
    pub day: NaiveDate,
}

impl Stamp {
    pub fn now() -> Stamp {
        Stamp {
            at: now_iso(),
            day: Local::now().date_naive(),
        }
    }

    /// Noon local time on `day`; used by tests to simulate other days.
    #[cfg(test)]
    pub fn on(day: NaiveDate) -> Stamp {
        use chrono::TimeZone;
        let local = Local
            .from_local_datetime(&day.and_hms_opt(12, 0, 0).unwrap())
            .earliest()
            .unwrap();
        Stamp {
            at: local
                .with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            day,
        }
    }
}

/// The local calendar date of an ISO timestamp.
pub(crate) fn local_day(iso: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|d| d.with_timezone(&Local).date_naive())
}

pub(crate) fn date_str(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

// ---------------------------------------------------------------- row helpers

pub(crate) fn enum_str<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

pub(crate) fn parse_enum<T: DeserializeOwned>(idx: usize, s: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Separator for `group_concat` of labels (never part of a label in practice).
const LABEL_SEP: char = '\u{1f}';

/// Columns for [`task_from_row`]. `t` is the alias of the tasks table.
pub(crate) const TASK_COLUMNS: &str = "t.id, t.key, t.category_id, t.title, t.type, t.description, t.status, t.priority, \
     t.due_date, t.created_at, t.updated_at, t.completed_at, \
     (SELECT group_concat(label, char(31)) FROM task_labels WHERE task_id = t.id)";

pub(crate) fn task_from_row(r: &Row) -> rusqlite::Result<Task> {
    let ty: String = r.get(4)?;
    let status: String = r.get(6)?;
    let priority: String = r.get(7)?;
    let labels: Option<String> = r.get(12)?;
    Ok(Task {
        id: r.get(0)?,
        key: r.get(1)?,
        category_id: r.get(2)?,
        title: r.get(3)?,
        task_type: parse_enum(4, &ty)?,
        description: r.get(5)?,
        status: parse_enum(6, &status)?,
        priority: parse_enum(7, &priority)?,
        labels: sort_labels(
            labels
                .map(|l| l.split(LABEL_SEP).map(String::from).collect())
                .unwrap_or_default(),
        ),
        due_date: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        completed_at: r.get(11)?,
    })
}

fn sort_labels(mut labels: Vec<String>) -> Vec<String> {
    labels.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
    labels
}

/// Trimmed, non-empty, case-insensitively unique (first spelling wins), sorted.
fn normalize_labels(labels: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let kept = labels
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && seen.insert(l.to_lowercase()))
        .collect();
    sort_labels(kept)
}

fn clean_title(title: &str) -> Result<String> {
    let t = title.trim();
    if t.is_empty() {
        return Err(CoreError::Invalid("A task needs a title.".into()));
    }
    Ok(t.to_string())
}

/// A real `YYYY-MM-DD` date.
fn check_date(s: &str) -> Result<String> {
    let ok = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|d| date_str(d) == s)
        .unwrap_or(false);
    if ok {
        Ok(s.to_string())
    } else {
        Err(CoreError::Invalid(format!(
            "\"{s}\" is not a valid due date. Use the form YYYY-MM-DD."
        )))
    }
}

fn not_found(id: &str) -> CoreError {
    CoreError::NotFound {
        entity: "task",
        id: id.to_string(),
    }
}

// ---------------------------------------------------------------- reading

/// Tasks matching `filter`, sorted as requested (FR-TASK-UI-1).
pub fn list(conn: &Connection, filter: &TaskFilter) -> Result<Vec<Task>> {
    query::list_on(conn, filter, Local::now().date_naive())
}

/// A task by id, current key or old key (alias, FR-TASK-UI-6). `NotFound` otherwise.
pub fn get(conn: &Connection, id_or_key: &str) -> Result<Task> {
    find(conn, id_or_key)?.ok_or_else(|| not_found(id_or_key))
}

fn find(conn: &Connection, id_or_key: &str) -> Result<Option<Task>> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM tasks t WHERE t.id = ?1 OR t.key = ?1");
    if let Some(t) = conn
        .query_row(&sql, [id_or_key], task_from_row)
        .optional()?
    {
        return Ok(Some(t));
    }
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task_key_aliases a JOIN tasks t ON t.id = a.task_id WHERE a.key = ?1"
    );
    Ok(conn
        .query_row(&sql, [id_or_key.trim()], task_from_row)
        .optional()?)
}

/// Tasks for the given keys (current or alias), for rendering `#KEY` mentions. Unknown keys are skipped.
pub fn get_by_keys(conn: &Connection, keys: &[String]) -> Result<Vec<Task>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for k in keys {
        if let Some(t) = find(conn, k.trim())? {
            if seen.insert(t.id.clone()) {
                out.push(t);
            }
        }
    }
    Ok(out)
}

/// Every label in use, sorted, for autocomplete.
pub fn labels(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT label FROM task_labels")?;
    let all = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Case-insensitively unique across tasks as well.
    let mut seen = HashSet::new();
    Ok(sort_labels(
        all.into_iter()
            .filter(|l| seen.insert(l.to_lowercase()))
            .collect(),
    ))
}

// ---------------------------------------------------------------- changing

/// An active category's key and the next task number (which is then consumed).
/// The category, if tasks may be created in or moved into it (not archived).
fn open_category(conn: &Connection, category_id: &str) -> Result<categories::Category> {
    let category = categories::get(conn, category_id)?;
    if category.status == categories::CategoryStatus::Archived {
        return Err(CoreError::Invalid(format!(
            "The category \"{}\" is archived. Restore it first, or choose another category.",
            category.name
        )));
    }
    Ok(category)
}

fn take_number(conn: &Connection, category_id: &str) -> Result<(String, i64)> {
    let category = open_category(conn, category_id)?;
    let n: i64 = conn.query_row(
        "SELECT next_task_number FROM categories WHERE id = ?1",
        [category_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "UPDATE categories SET next_task_number = ?2 WHERE id = ?1",
        params![category_id, n + 1],
    )?;
    Ok((category.key, n))
}

/// Writes the task's own columns (not the key) and its labels.
fn write_task(conn: &Connection, t: &Task) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET category_id = ?2, title = ?3, type = ?4, description = ?5, status = ?6,
             priority = ?7, due_date = ?8, updated_at = ?9, completed_at = ?10
         WHERE id = ?1",
        params![
            t.id,
            t.category_id,
            t.title,
            enum_str(&t.task_type),
            t.description,
            enum_str(&t.status),
            enum_str(&t.priority),
            t.due_date,
            t.updated_at,
            t.completed_at,
        ],
    )?;
    conn.execute("DELETE FROM task_labels WHERE task_id = ?1", [&t.id])?;
    for l in &t.labels {
        conn.execute(
            "INSERT INTO task_labels (task_id, label) VALUES (?1, ?2)",
            params![t.id, l],
        )?;
    }
    Ok(())
}

/// Creates a task with the next number of its category (never reused). Title must not be empty.
pub fn create(db: &mut Db, input: NewTask) -> Result<Task> {
    db.tx(|tx| create_in(tx, input, &Stamp::now()))
}

pub(crate) fn create_in(conn: &Connection, input: NewTask, st: &Stamp) -> Result<Task> {
    let title = clean_title(&input.title)?;
    let due_date = match input.due_date.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(d) => Some(check_date(d)?),
    };
    let (category_key, number) = take_number(conn, &input.category_id)?;
    let status = input.status.unwrap_or(TaskStatus::Todo);
    let task = Task {
        id: new_id(),
        key: format!("{category_key}-{number}"),
        category_id: input.category_id,
        title,
        task_type: input.task_type.unwrap_or(TaskType::General),
        description: input.description.unwrap_or_default(),
        status,
        priority: input.priority.unwrap_or(Priority::Medium),
        labels: normalize_labels(input.labels.unwrap_or_default()),
        due_date,
        created_at: st.at.clone(),
        updated_at: st.at.clone(),
        completed_at: status.is_closed().then(|| st.at.clone()),
    };
    conn.execute(
        "INSERT INTO tasks (id, key, number, category_id, title, type, description, status, priority,
                            due_date, created_at, updated_at, completed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            task.id,
            task.key,
            number,
            task.category_id,
            task.title,
            enum_str(&task.task_type),
            task.description,
            enum_str(&task.status),
            enum_str(&task.priority),
            task.due_date,
            task.created_at,
            task.updated_at,
            task.completed_at,
        ],
    )?;
    write_task(conn, &task)?;
    history::save_version(conn, &task, st)?;
    history::log_activity(conn, ActivityKind::TaskCreated, &task.id, st)?;
    crate::hooks::task_saved(conn, &task)?;
    Ok(task)
}

/// Applies `patch` (only given fields). Validates title and due date format.
pub fn update(db: &mut Db, id: &str, patch: TaskPatch) -> Result<Task> {
    db.tx(|tx| update_in(tx, id, patch, &Stamp::now()))
}

pub(crate) fn update_in(conn: &Connection, id: &str, patch: TaskPatch, st: &Stamp) -> Result<Task> {
    let old = get(conn, id)?;
    let mut t = old.clone();
    if let Some(title) = patch.title {
        t.title = clean_title(&title)?;
    }
    if let Some(v) = patch.task_type {
        t.task_type = v;
    }
    if let Some(v) = patch.description {
        t.description = v;
    }
    if let Some(v) = patch.status {
        t.status = v;
    }
    if let Some(v) = patch.priority {
        t.priority = v;
    }
    if let Some(v) = patch.labels {
        t.labels = normalize_labels(v);
    }
    if let Some(d) = patch.due_date {
        t.due_date = match d.trim() {
            "" => None,
            d => Some(check_date(d)?),
        };
    }
    if t == old {
        return Ok(old);
    }
    // FR-TASK-ST-4: set when moving from an open status into a closed one, cleared on reopening;
    // moving between the closed statuses keeps the original time.
    if t.status.is_closed() && !old.status.is_closed() {
        t.completed_at = Some(st.at.clone());
    } else if !t.status.is_closed() {
        t.completed_at = None;
    }
    t.updated_at = st.at.clone();
    write_task(conn, &t)?;
    history::save_version(conn, &t, st)?;
    let kind = if t.status == TaskStatus::Done && old.status != TaskStatus::Done {
        ActivityKind::TaskCompleted
    } else {
        ActivityKind::TaskUpdated
    };
    history::log_activity(conn, kind, &t.id, st)?;
    crate::hooks::task_saved(conn, &t)?;
    Ok(t)
}

/// Moves a task to another category: new key from that category's counter, old key kept as an alias (FR-TASK-UI-6).
/// Moving a task with sub-tasks or a parent keeps its links.
pub fn move_to_category(db: &mut Db, id: &str, category_id: &str) -> Result<Task> {
    db.tx(|tx| move_in(tx, id, category_id, &Stamp::now()))
}

pub(crate) fn move_in(conn: &Connection, id: &str, category_id: &str, st: &Stamp) -> Result<Task> {
    let old = get(conn, id)?;
    if old.category_id == category_id {
        return Ok(old);
    }
    let category = open_category(conn, category_id)?;
    // FR-TASK-UI-6: a task returning to a category gets back the key it had there (it never
    // belonged to another task); otherwise it takes the category's next number.
    let (key, number) = match previous_key_in(conn, &old.id, &category.key)? {
        Some((key, number)) => {
            conn.execute("DELETE FROM task_key_aliases WHERE key = ?1", [&key])?;
            (key, number)
        }
        None => {
            let (category_key, number) = take_number(conn, category_id)?;
            (format!("{category_key}-{number}"), number)
        }
    };
    let mut t = old.clone();
    t.category_id = category_id.to_string();
    t.key = key;
    t.updated_at = st.at.clone();
    conn.execute(
        "UPDATE tasks SET key = ?2, number = ?3 WHERE id = ?1",
        params![t.id, t.key, number],
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO task_key_aliases (key, task_id) VALUES (?1, ?2)",
        params![old.key, t.id],
    )?;
    write_task(conn, &t)?;
    history::save_version(conn, &t, st)?;
    history::log_activity(conn, ActivityKind::TaskUpdated, &t.id, st)?;
    crate::hooks::task_saved(conn, &t)?;
    Ok(t)
}

/// The lowest-numbered old key (alias) this task had in the category with key `category_key`.
fn previous_key_in(
    conn: &Connection,
    task_id: &str,
    category_key: &str,
) -> Result<Option<(String, i64)>> {
    let prefix = format!("{}-", category_key.to_uppercase());
    let mut stmt = conn.prepare("SELECT key FROM task_key_aliases WHERE task_id = ?1")?;
    let keys = stmt
        .query_map([task_id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(keys
        .into_iter()
        .filter_map(|k| {
            let n = k
                .to_uppercase()
                .strip_prefix(&prefix)?
                .parse::<i64>()
                .ok()?;
            Some((format!("{}{n}", prefix), n))
        })
        .min_by_key(|(_, n)| *n))
}

/// Deletes a task permanently with its labels, aliases, links, comments, history and activity (FR-TASK-UI-3).
pub fn delete(db: &mut Db, id: &str) -> Result<()> {
    db.tx(|tx| delete_in(tx, id))
}

fn delete_in(conn: &Connection, id: &str) -> Result<()> {
    let t = get(conn, id)?;
    crate::hooks::task_deleted(conn, &t.id)?;
    conn.execute("DELETE FROM activity WHERE ref_id = ?1", [&t.id])?;
    // Labels, aliases, links, comments and versions go with it (ON DELETE CASCADE).
    conn.execute("DELETE FROM tasks WHERE id = ?1", [&t.id])?;
    Ok(())
}

// ---------------------------------------------------------------- many at once

/// "KEY title", for naming a skipped task.
fn bulk_name(conn: &Connection, id: &str) -> Option<String> {
    get(conn, id).ok().map(|t| format!("{} {}", t.key, t.title))
}

/// Applies one change to many tasks (FR-TASK-UI-7). A task that can't take it (e.g. the target
/// category is archived) is skipped and named; the others are changed. An empty label is refused.
pub fn update_many(db: &mut Db, ids: &[String], change: TaskBulkChange) -> Result<BulkResult> {
    if let TaskBulkChange::AddLabel { label } | TaskBulkChange::RemoveLabel { label } = &change
        && label.trim().is_empty()
    {
        return Err(CoreError::Invalid("A label can't be empty.".into()));
    }
    let st = Stamp::now();
    db.tx(|tx| crate::bulk::each(tx, ids, bulk_name, |c, id| change_in(c, id, &change, &st)))
}

fn change_in(conn: &Connection, id: &str, change: &TaskBulkChange, st: &Stamp) -> Result<()> {
    let patch = match change {
        TaskBulkChange::Status { status } => TaskPatch {
            status: Some(*status),
            ..Default::default()
        },
        TaskBulkChange::Priority { priority } => TaskPatch {
            priority: Some(*priority),
            ..Default::default()
        },
        TaskBulkChange::Category { category_id } => {
            return move_in(conn, id, category_id, st).map(|_| ());
        }
        TaskBulkChange::AddLabel { label } => {
            let mut labels = get(conn, id)?.labels;
            labels.push(label.clone());
            TaskPatch {
                labels: Some(labels),
                ..Default::default()
            }
        }
        TaskBulkChange::RemoveLabel { label } => {
            let gone = label.trim().to_lowercase();
            let labels = get(conn, id)?.labels;
            TaskPatch {
                labels: Some(
                    labels
                        .into_iter()
                        .filter(|l| l.to_lowercase() != gone)
                        .collect(),
                ),
                ..Default::default()
            }
        }
    };
    update_in(conn, id, patch, st).map(|_| ())
}

/// Deletes many tasks permanently, as `delete` does one (FR-TASK-UI-7).
pub fn delete_many(db: &mut Db, ids: &[String]) -> Result<BulkResult> {
    db.tx(|tx| crate::bulk::each(tx, ids, bulk_name, delete_in))
}

// ---------------------------------------------------------------- comments

fn comment_from_row(r: &Row) -> rusqlite::Result<TaskComment> {
    Ok(TaskComment {
        id: r.get(0)?,
        task_id: r.get(1)?,
        text: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
    })
}

const COMMENT_COLUMNS: &str = "id, task_id, text, created_at, updated_at";

fn get_comment(conn: &Connection, id: &str) -> Result<TaskComment> {
    conn.query_row(
        &format!("SELECT {COMMENT_COLUMNS} FROM task_comments WHERE id = ?1"),
        [id],
        comment_from_row,
    )
    .optional()?
    .ok_or_else(|| CoreError::NotFound {
        entity: "comment",
        id: id.to_string(),
    })
}

fn check_comment(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(CoreError::Invalid("A comment can't be empty.".into()));
    }
    Ok(())
}

/// Oldest first.
pub fn comments(conn: &Connection, task_id: &str) -> Result<Vec<TaskComment>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COMMENT_COLUMNS} FROM task_comments WHERE task_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map([task_id], comment_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Comments don't create task versions (FR-TASK-HI-2) but count as an update for activity.
pub fn add_comment(db: &mut Db, task_id: &str, text: &str) -> Result<TaskComment> {
    db.tx(|tx| add_comment_in(tx, task_id, text, &Stamp::now()))
}

pub(crate) fn add_comment_in(
    conn: &Connection,
    task_id: &str,
    text: &str,
    st: &Stamp,
) -> Result<TaskComment> {
    check_comment(text)?;
    let task = get(conn, task_id)?;
    let c = TaskComment {
        id: new_id(),
        task_id: task.id.clone(),
        text: text.to_string(),
        created_at: st.at.clone(),
        updated_at: None,
    };
    conn.execute(
        "INSERT INTO task_comments (id, task_id, text, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![c.id, c.task_id, c.text, c.created_at],
    )?;
    history::log_activity(conn, ActivityKind::TaskCommented, &task.id, st)?;
    crate::hooks::comment_saved(conn, &c)?;
    Ok(c)
}

pub fn update_comment(db: &mut Db, id: &str, text: &str) -> Result<TaskComment> {
    db.tx(|tx| {
        check_comment(text)?;
        let st = Stamp::now();
        let mut c = get_comment(tx, id)?;
        c.text = text.to_string();
        c.updated_at = Some(st.at.clone());
        tx.execute(
            "UPDATE task_comments SET text = ?2, updated_at = ?3 WHERE id = ?1",
            params![c.id, c.text, c.updated_at],
        )?;
        history::log_activity(tx, ActivityKind::TaskUpdated, &c.task_id, &st)?;
        crate::hooks::comment_saved(tx, &c)?;
        Ok(c)
    })
}

pub fn delete_comment(db: &mut Db, id: &str) -> Result<()> {
    db.tx(|tx| {
        let c = get_comment(tx, id)?;
        tx.execute("DELETE FROM task_comments WHERE id = ?1", [id])?;
        crate::hooks::comment_deleted(tx, &c)?;
        history::log_activity(tx, ActivityKind::TaskUpdated, &c.task_id, &Stamp::now())?;
        Ok(())
    })
}

// ---------------------------------------------------------------- links

/// Links where the task is on either side.
pub fn links(conn: &Connection, task_id: &str) -> Result<Vec<TaskLink>> {
    links::links(conn, task_id)
}

/// Adds a link (FR-TASK-LN-1, -2, -4): no self links or duplicates (`relates` is symmetric);
/// `parent`: one parent per task, one level deep. Breaking a rule gives `Invalid` / `Conflict`.
/// A `relates` link is stored with its two ids in a fixed order, so the returned `from`/`to` may be swapped.
pub fn add_link(
    db: &mut Db,
    link_type: TaskLinkType,
    from_task_id: &str,
    to_task_id: &str,
) -> Result<TaskLink> {
    db.tx(|tx| links::add_link(tx, link_type, from_task_id, to_task_id))
}

pub fn remove_link(db: &mut Db, id: &str) -> Result<()> {
    db.tx(|tx| links::remove_link(tx, id))
}

// ---------------------------------------------------------------- history and summary

/// Daily versions, oldest first (FR-TASK-HI-3).
pub fn versions(conn: &Connection, task_id: &str) -> Result<Vec<TaskVersion>> {
    history::versions(conn, task_id)
}

/// Working days and blocked days of a done task; `None` for any other status (FR-TASK-SUM-1..4).
pub fn completion_summary(conn: &Connection, task_id: &str) -> Result<Option<CompletionSummary>> {
    history::completion_summary(conn, task_id)
}

/// Newest first.
pub fn activity(conn: &Connection, limit: u32) -> Result<Vec<ActivityEvent>> {
    history::activity(conn, limit)
}

// ---------------------------------------------------------------- dev seed

/// Debug builds only: inserts the prototype's sample data where its ids are missing; never overwrites.
/// Also raises each category's `next_task_number` above the seeded numbers.
pub fn seed_if_missing(db: &mut Db, seed: TaskSeed) -> Result<()> {
    db.tx(|tx| seed::seed_if_missing(tx, seed))
}
