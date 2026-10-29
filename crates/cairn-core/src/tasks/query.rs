//! The task list query (FR-TASK-UI-1). All user input goes in as SQL parameters.

use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::{Connection, params_from_iter, types::Value};

use super::*;

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

fn in_clause(wheres: &mut Vec<String>, args: &mut Vec<Value>, column: &str, values: Vec<String>) {
    if !values.is_empty() {
        wheres.push(format!("{column} IN ({})", placeholders(values.len())));
        args.extend(values.into_iter().map(Value::Text));
    }
}

pub(super) fn list_on(
    conn: &Connection,
    filter: &TaskFilter,
    today: NaiveDate,
) -> Result<Vec<Task>> {
    let mut wheres: Vec<String> = Vec::new();
    let mut args: Vec<Value> = Vec::new();

    if let Some(ids) = &filter.category_ids {
        in_clause(&mut wheres, &mut args, "t.category_id", ids.clone());
    }
    match filter.statuses.as_deref() {
        Some(s) if !s.is_empty() => in_clause(
            &mut wheres,
            &mut args,
            "t.status",
            s.iter().map(enum_str).collect(),
        ),
        _ if filter.include_closed != Some(true) => {
            wheres.push("t.status NOT IN ('done', 'wont_do')".into())
        }
        _ => {}
    }
    if let Some(v) = &filter.types {
        in_clause(
            &mut wheres,
            &mut args,
            "t.type",
            v.iter().map(enum_str).collect(),
        );
    }
    if let Some(v) = &filter.priorities {
        in_clause(
            &mut wheres,
            &mut args,
            "t.priority",
            v.iter().map(enum_str).collect(),
        );
    }
    if let Some(labels) = &filter.labels {
        let labels: Vec<String> = labels
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if !labels.is_empty() {
            // task_labels.label is COLLATE NOCASE.
            wheres.push(format!(
                "EXISTS (SELECT 1 FROM task_labels l WHERE l.task_id = t.id AND l.label IN ({}))",
                placeholders(labels.len())
            ));
            args.extend(labels.into_iter().map(Value::Text));
        }
    }
    match filter.due {
        Some(DueFilter::Overdue) => {
            wheres.push("(t.status NOT IN ('done', 'wont_do') AND t.due_date < ?)".into());
            args.push(Value::Text(date_str(today)));
        }
        Some(DueFilter::Today) => {
            wheres.push("t.due_date = ?".into());
            args.push(Value::Text(date_str(today)));
        }
        Some(DueFilter::ThisWeek) => {
            let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
            wheres.push("t.due_date BETWEEN ? AND ?".into());
            args.push(Value::Text(date_str(monday)));
            args.push(Value::Text(date_str(monday + Duration::days(6))));
        }
        Some(DueFilter::None) => wheres.push("t.due_date IS NULL".into()),
        None => {}
    }
    if let Some(day) = filter.due_on.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        wheres.push("t.due_date = ?".into());
        args.push(Value::Text(day.to_string()));
    }

    let dir = if filter.descending == Some(true) {
        "DESC"
    } else {
        "ASC"
    };
    let order = match filter.sort.unwrap_or_default() {
        TaskSortField::Due => format!("(t.due_date IS NULL) {dir}, t.due_date {dir}"),
        TaskSortField::Priority => {
            format!("CASE t.priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END {dir}")
        }
        // Tasks made within the same millisecond keep their creation (insert) order.
        TaskSortField::Created => format!("t.created_at {dir}, t.rowid {dir}"),
        TaskSortField::Updated => format!("t.updated_at {dir}"),
        TaskSortField::Key => format!("p.key COLLATE NOCASE {dir}, t.number {dir}"),
    };

    // FR-TASK-UI-1, D-52: every word must appear (any order, any case). A run of Han characters
    // without spaces is one word, so it is matched as a substring.
    let words: Option<Vec<String>> = filter
        .text
        .as_deref()
        .map(|s| s.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>())
        .filter(|w| !w.is_empty());
    let mut sql = format!(
        "SELECT {TASK_COLUMNS} FROM tasks t JOIN categories p ON p.id = t.category_id{}{} \
         ORDER BY {order}, t.created_at, t.rowid",
        if wheres.is_empty() { "" } else { " WHERE " },
        wheres.join(" AND "),
    );
    // Text matching is done in Rust: SQLite's lower() and LIKE only fold ASCII case.
    let limit = filter.limit.map(|l| l as usize);
    if words.is_none() {
        if let Some(l) = limit {
            sql.push_str(&format!(" LIMIT {l}"));
        }
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args), task_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        let t = row?;
        if let Some(words) = &words {
            let (key, title, description) = (
                t.key.to_lowercase(),
                t.title.to_lowercase(),
                t.description.to_lowercase(),
            );
            let every = words
                .iter()
                .all(|w| key.contains(w) || title.contains(w) || description.contains(w));
            if !every {
                continue;
            }
        }
        out.push(t);
        if words.is_some() && Some(out.len()) == limit {
            break;
        }
    }
    Ok(out)
}
