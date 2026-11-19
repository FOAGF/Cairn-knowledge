//! Working-day arithmetic for the completion summary (FR-TASK-SUM-1..4).
//! Rules: date-based, inclusive, Monday to Friday, no public holidays. Each working day is judged by the
//! task's status at the end of that day; the status carries forward over days without a version.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate, Weekday};

use super::model::TaskStatus;

fn is_working_day(d: NaiveDate) -> bool {
    !matches!(d.weekday(), Weekday::Sat | Weekday::Sun)
}

/// Working days in `start..=end` for which `counts(day, status at the end of that day)` is true.
/// `versions` (date, status) may come in any order; those inside the range count, and the last one before
/// `start` sets the initial status (`todo` when there is none).
fn count_working_days(
    start: NaiveDate,
    end: NaiveDate,
    versions: &[(NaiveDate, TaskStatus)],
    counts: impl Fn(NaiveDate, TaskStatus) -> bool,
) -> u32 {
    let by_date: BTreeMap<NaiveDate, TaskStatus> = versions.iter().copied().collect();
    let mut status = by_date
        .range(..start)
        .next_back()
        .map(|(_, s)| *s)
        .unwrap_or(TaskStatus::Todo);
    let mut n = 0;
    for d in start.iter_days().take_while(|d| *d <= end) {
        if let Some(s) = by_date.get(&d) {
            status = *s;
        }
        if is_working_day(d) && counts(d, status) {
            n += 1;
        }
    }
    n
}

/// Working days spent: those in `start..=end` the task ended in progress, plus `end` itself, the day it
/// was completed (D-95). Days in to do or blocked don't count.
pub fn spent_working_days(
    start: NaiveDate,
    end: NaiveDate,
    versions: &[(NaiveDate, TaskStatus)],
) -> u32 {
    count_working_days(start, end, versions, |d, s| {
        d == end || s == TaskStatus::InProgress
    })
}

/// Working days in `start..=end` on which the task ended the day as blocked.
pub fn blocked_working_days(
    start: NaiveDate,
    end: NaiveDate,
    versions: &[(NaiveDate, TaskStatus)],
) -> u32 {
    count_working_days(start, end, versions, |_, s| s == TaskStatus::Blocked)
}
