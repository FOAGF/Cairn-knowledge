//! Working-day arithmetic for the completion summary (FR-TASK-SUM-1..4).
//! Rules: date-based, inclusive, Monday to Friday, no public holidays. A working day is blocked when the
//! task's status at the end of that day was `blocked`; the status carries forward over days without a version.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate, Weekday};

use super::model::TaskStatus;

fn is_working_day(d: NaiveDate) -> bool {
    !matches!(d.weekday(), Weekday::Sat | Weekday::Sun)
}

/// Working days from `start` to `end`, both inclusive. 0 when `end` is before `start`.
pub fn working_days_between(start: NaiveDate, end: NaiveDate) -> u32 {
    start
        .iter_days()
        .take_while(|d| *d <= end)
        .filter(|d| is_working_day(*d))
        .count() as u32
}

/// Working days in `start..=end` on which the task ended the day as blocked.
/// `versions` (date, status) may come in any order; those inside the range count, and the last one before
/// `start` sets the initial status (`todo` when there is none).
pub fn blocked_working_days(
    start: NaiveDate,
    end: NaiveDate,
    versions: &[(NaiveDate, TaskStatus)],
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
        if status == TaskStatus::Blocked && is_working_day(d) {
            n += 1;
        }
    }
    n
}
