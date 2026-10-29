//! The Home screen's data in one call (FR-HOME-1..5, D-54): the counts under the greeting, the
//! Today list, the calendar's due counts and whether this copy has any data yet. Home never loads
//! every task or page: at 10,000 pages and 20,000 tasks that was about 25 MB on each visit (M7).
//!
//! "Today" and "this week" are the local calendar day and Monday to Sunday, as in the task list's
//! due filters (`tasks::query`). A task appears in the first Today group it fits: overdue, due
//! today, in progress, blocked.

use chrono::{Datelike, Duration, Local, NaiveDate, SecondsFormat, TimeZone, Utc};
use rusqlite::{Connection, params};
use serde::Serialize;
use specta::Type;

use crate::Result;
use crate::tasks::{TASK_COLUMNS, Task, date_str, task_from_row};

/// Tasks sent per Today group. The UI shows 3 and unfolds the rest; the group's total links to the
/// task list when there are more (FR-HOME-4).
pub const GROUP_TASKS: u32 = 10;

/// Not done and not *won't do*.
const OPEN: &str = "t.status NOT IN ('done', 'wont_do')";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HomeCounts {
    /// In progress or blocked.
    pub active: u32,
    pub blocked: u32,
    /// Open tasks due Monday to Sunday of this week.
    pub due_this_week: u32,
    /// Open tasks due before today.
    pub overdue: u32,
    /// Tasks marked done since Monday 00:00, local time.
    pub done_this_week: u32,
    /// Pages that aren't archived.
    pub pages: u32,
    /// Of those, the pages not checked yet (Home's *Needs attention*).
    pub pages_not_checked: u32,
    /// Active categories.
    pub categories: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TodayGroupKind {
    Overdue,
    DueToday,
    InProgress,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TodayGroup {
    pub kind: TodayGroupKind,
    /// Every task in the group.
    pub total: u32,
    /// The first [`GROUP_TASKS`]: soonest due first, then the highest priority.
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DueDay {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// Open tasks due that day.
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HomeOverview {
    /// The local day this was worked out for (`YYYY-MM-DD`).
    pub today: String,
    pub counts: HomeCounts,
    /// Overdue, due today, in progress and blocked: always all four, in that order.
    pub groups: Vec<TodayGroup>,
    /// The days with open tasks due, oldest first, for the calendar's counts.
    pub due_days: Vec<DueDay>,
    /// Any task, page (archived ones too), conversation, or a category besides General (FR-DATA-8).
    pub has_data: bool,
}

/// Home's data for today, in the computer's time zone.
pub fn overview(conn: &Connection) -> Result<HomeOverview> {
    let today = Local::now().date_naive();
    overview_on(conn, today, &week_start_utc(today))
}

/// Monday 00:00 of `today`'s week in local time, as the UTC timestamp `completed_at` is compared
/// with. A midnight that doesn't exist (a daylight-saving jump) takes the first moment after it.
fn week_start_utc(today: NaiveDate) -> String {
    let monday = monday_of(today).and_hms_opt(0, 0, 0).unwrap_or_default();
    let local = Local.from_local_datetime(&monday).earliest().or_else(|| {
        Local
            .from_local_datetime(&(monday + Duration::hours(1)))
            .earliest()
    });
    match local {
        Some(t) => t
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        None => format!("{}T00:00:00.000Z", date_str(monday.date())),
    }
}

fn monday_of(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_monday() as i64)
}

/// [`overview`] for a given day; `week_start` is that week's Monday 00:00 as a UTC timestamp.
pub(crate) fn overview_on(
    conn: &Connection,
    today: NaiveDate,
    week_start: &str,
) -> Result<HomeOverview> {
    let monday = monday_of(today);
    let today_s = date_str(today);
    let (active, blocked, due_this_week, overdue, done_this_week) = conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(t.status IN ('in_progress', 'blocked')), 0),
                    COALESCE(SUM(t.status = 'blocked'), 0),
                    COALESCE(SUM({OPEN} AND t.due_date BETWEEN ?1 AND ?2), 0),
                    COALESCE(SUM({OPEN} AND t.due_date < ?3), 0),
                    COALESCE(SUM(t.status = 'done' AND t.completed_at >= ?4), 0)
             FROM tasks t"
        ),
        params![
            date_str(monday),
            date_str(monday + Duration::days(6)),
            today_s,
            week_start
        ],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let (pages, pages_not_checked) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(check_status = 'not_checked'), 0) FROM pages WHERE archived = 0",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let categories = conn.query_row(
        "SELECT COUNT(*) FROM categories WHERE status = 'active'",
        [],
        |r| r.get(0),
    )?;
    let has_data = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM tasks) OR EXISTS (SELECT 1 FROM pages)
             OR EXISTS (SELECT 1 FROM conversations) OR (SELECT COUNT(*) FROM categories) > 1",
        [],
        |r| r.get(0),
    )?;

    let groups = [
        (TodayGroupKind::Overdue, "t.due_date < ?1"),
        (TodayGroupKind::DueToday, "t.due_date = ?1"),
        (
            TodayGroupKind::InProgress,
            "t.status = 'in_progress' AND (t.due_date IS NULL OR t.due_date > ?1)",
        ),
        (
            TodayGroupKind::Blocked,
            "t.status = 'blocked' AND (t.due_date IS NULL OR t.due_date > ?1)",
        ),
    ]
    .into_iter()
    .map(|(kind, rule)| group(conn, kind, rule, &today_s))
    .collect::<Result<Vec<_>>>()?;

    let due_days = conn
        .prepare(&format!(
            "SELECT t.due_date, COUNT(*) FROM tasks t WHERE {OPEN} AND t.due_date IS NOT NULL
             GROUP BY t.due_date ORDER BY t.due_date"
        ))?
        .query_map([], |r| {
            Ok(DueDay {
                date: r.get(0)?,
                count: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(HomeOverview {
        today: today_s,
        counts: HomeCounts {
            active,
            blocked,
            due_this_week,
            overdue,
            done_this_week,
            pages,
            pages_not_checked,
            categories,
        },
        groups,
        due_days,
        has_data,
    })
}

/// One Today group: open tasks matching `rule` (`?1` is today).
fn group(conn: &Connection, kind: TodayGroupKind, rule: &str, today: &str) -> Result<TodayGroup> {
    let total = conn.query_row(
        &format!("SELECT COUNT(*) FROM tasks t WHERE {OPEN} AND {rule}"),
        [today],
        |r| r.get(0),
    )?;
    let tasks = conn
        .prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks t WHERE {OPEN} AND {rule}
             ORDER BY (t.due_date IS NULL), t.due_date,
                      CASE t.priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
                      t.created_at, t.rowid
             LIMIT ?2"
        ))?
        .query_map(params![today, GROUP_TASKS], task_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(TodayGroup { kind, total, tasks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use crate::tasks::{self, NewTask, Priority, TaskStatus};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn task(
        db: &mut Db,
        title: &str,
        status: TaskStatus,
        priority: Priority,
        due: Option<&str>,
    ) -> String {
        tasks::create(
            db,
            NewTask {
                category_id: "p-general".into(),
                title: title.into(),
                status: Some(status),
                priority: Some(priority),
                due_date: due.map(Into::into),
                ..Default::default()
            },
        )
        .unwrap()
        .id
    }

    fn titles(g: &TodayGroup) -> Vec<&str> {
        g.tasks.iter().map(|t| t.title.as_str()).collect()
    }

    // Today is Wednesday 2026-10-14; the week is 12..=18 October and began at 2026-10-11T14:00Z
    // (Monday 00:00 at UTC+10).
    const WEEK_START: &str = "2026-10-11T14:00:00.000Z";

    #[test]
    fn an_empty_copy_has_no_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_at(dir.path()).unwrap();
        let o = overview_on(db.conn(), d("2026-10-14"), WEEK_START).unwrap();
        assert!(!o.has_data);
        assert_eq!(o.counts.categories, 1, "only General");
        assert!(o.groups.iter().all(|g| g.total == 0 && g.tasks.is_empty()));
        assert_eq!(o.groups.len(), 4);
    }

    #[test]
    fn groups_counts_and_due_days() {
        use Priority::*;
        use TaskStatus::*;
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_at(dir.path()).unwrap();
        task(&mut db, "A late", Todo, Medium, Some("2026-10-12"));
        task(
            &mut db,
            "B late, in progress",
            InProgress,
            Medium,
            Some("2026-10-13"),
        );
        task(&mut db, "C today", Todo, Medium, Some("2026-10-14"));
        task(
            &mut db,
            "G today, blocked",
            Blocked,
            High,
            Some("2026-10-14"),
        );
        task(
            &mut db,
            "D in progress",
            InProgress,
            Low,
            Some("2026-10-20"),
        );
        task(&mut db, "E in progress, no date", InProgress, High, None);
        task(&mut db, "F blocked", Blocked, Low, None);
        task(&mut db, "K later", Todo, Low, Some("2026-10-25"));
        let h = task(&mut db, "H done this week", Done, Low, Some("2026-10-10"));
        let i = task(&mut db, "I done last week", Done, Low, None);
        task(&mut db, "J dropped", WontDo, Low, Some("2026-10-13"));
        let set_done = |id: &str, at: &str| {
            db.conn()
                .execute(
                    "UPDATE tasks SET completed_at = ?2 WHERE id = ?1",
                    params![id, at],
                )
                .unwrap();
        };
        set_done(&h, "2026-10-12T01:00:00.000Z");
        set_done(&i, "2026-10-11T10:00:00.000Z");

        let o = overview_on(db.conn(), d("2026-10-14"), WEEK_START).unwrap();
        assert!(o.has_data);
        assert_eq!(o.today, "2026-10-14");
        let c = &o.counts;
        assert_eq!((c.active, c.blocked), (5, 2));
        assert_eq!((c.due_this_week, c.overdue, c.done_this_week), (4, 2, 1));

        let kinds: Vec<_> = o.groups.iter().map(|g| g.kind).collect();
        use TodayGroupKind as G;
        assert_eq!(
            kinds,
            vec![G::Overdue, G::DueToday, G::InProgress, G::Blocked]
        );
        // A task sits in the first group it fits; closed tasks never show.
        assert_eq!(titles(&o.groups[0]), vec!["A late", "B late, in progress"]);
        assert_eq!(titles(&o.groups[1]), vec!["G today, blocked", "C today"]);
        assert_eq!(
            titles(&o.groups[2]),
            vec!["D in progress", "E in progress, no date"]
        );
        assert_eq!(titles(&o.groups[3]), vec!["F blocked"]);

        let days: Vec<(&str, u32)> = o
            .due_days
            .iter()
            .map(|x| (x.date.as_str(), x.count))
            .collect();
        assert_eq!(
            days,
            vec![
                ("2026-10-12", 1),
                ("2026-10-13", 1),
                ("2026-10-14", 2),
                ("2026-10-20", 1),
                ("2026-10-25", 1)
            ]
        );

        // On Monday the 19th the week is 19..=25: D and K are due in it, nothing was done in it yet,
        // and everything due up to the 14th is overdue.
        let next = overview_on(db.conn(), d("2026-10-19"), "2026-10-18T14:00:00.000Z").unwrap();
        assert_eq!(
            (next.counts.due_this_week, next.counts.done_this_week),
            (2, 0)
        );
        assert_eq!(next.groups[0].total, 4);
    }

    #[test]
    fn a_group_sends_at_most_ten_tasks_but_counts_all() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_at(dir.path()).unwrap();
        for n in 0..12 {
            task(
                &mut db,
                &format!("work {n}"),
                TaskStatus::InProgress,
                Priority::Medium,
                None,
            );
        }
        let o = overview_on(db.conn(), d("2026-10-14"), WEEK_START).unwrap();
        assert_eq!(o.groups[2].total, 12);
        assert_eq!(o.groups[2].tasks.len(), GROUP_TASKS as usize);
        assert_eq!(
            o.groups[2].tasks[0].title, "work 0",
            "oldest first when nothing else differs"
        );
    }

    #[test]
    fn page_counts_leave_out_archived_pages() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_at(dir.path()).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO pages (id, category_id, title, created_at, updated_at, check_status, archived) VALUES
                   ('p1', 'p-general', 'One', 'x', 'x', 'not_checked', 0),
                   ('p2', 'p-general', 'Two', 'x', 'x', 'checked', 0),
                   ('p3', 'p-general', 'Old', 'x', 'x', 'not_checked', 1);",
            )
            .unwrap();
        let o = overview_on(db.conn(), d("2026-10-14"), WEEK_START).unwrap();
        assert_eq!((o.counts.pages, o.counts.pages_not_checked), (2, 1));
        assert!(o.has_data, "an archived page alone would count too");
    }
}
