//! The earlier conversation sent with a question, and the history budget (FR-LLM-AS-10, D-44).
//!
//! - [`turns`]: the messages before a given one, as the README's "What the app appends" renders
//!   them: a user turn is the message as typed (a file is not repeated); an assistant turn is its
//!   text without `[n]` markers; a find answer is its intro line and one row per item
//!   (`- Task KEY: title`, `- Page: title`); a proposal is its reply and one line per row result
//!   ("Created OPS-48", "Marked OPS-41 as done", "Rejected: …"); a draft card is "Draft page:
//!   title". Messages with an error, and still-running ones, are left out.
//! - [`fit`]: the newest turns that fit the budget (`chunking::estimate_tokens`); older ones drop.
//! - The budget: the conversation's own (`Conversation::history_budget`) or the setting, never
//!   more than the model can take after the sources and the reply (`model_limit`).
//! - [`usage`] for the composer: % left; amber below 25%; *Widen* below 10% doubles the
//!   conversation's budget, up to the model's limit ([`widened`]).
//! - [`shown_items`]: the items shown under the latest assistant message (its cited chips in
//!   order, or its find rows), for `cited` / `refs` and as pinned sources.
//!
//! Owned by the M6 answers agent.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::assistant::actions;
use crate::assistant::formats::{Turn, strip_markers};
use crate::assistant::items::ItemRef;
use crate::assistant::parts::SourceKind;
use crate::chunking::estimate_tokens;
use crate::conversations::{ChatMessage, ChatRole, StepState};
use crate::prompts::Role;

/// Percent left below which the composer shows the amber warning.
pub const WARN_BELOW_PERCENT: u8 = 25;
/// Percent left below which *Widen for this conversation* is offered.
pub const WIDEN_BELOW_PERCENT: u8 = 10;

/// A message whose job is still going: a running step and no error.
fn is_running(m: &ChatMessage) -> bool {
    m.error.is_none() && m.steps.iter().any(|s| s.state == StepState::Active)
}

/// The turns before `before_id` (all messages when it isn't found), oldest first. `conn` is for
/// the proposal rows' result lines (`actions::row_line`).
pub fn turns(conn: &Connection, messages: &[ChatMessage], before_id: &str) -> Vec<Turn> {
    let end = messages
        .iter()
        .position(|m| m.id == before_id)
        .unwrap_or(messages.len());
    let mut out = Vec::new();
    for m in &messages[..end] {
        if m.error.is_some() || is_running(m) {
            continue;
        }
        let turn = match m.role {
            ChatRole::User => {
                let typed = m.text.trim();
                let text = match (&m.attachment, typed.is_empty()) {
                    (Some(a), true) => format!("(attached {})", a.name),
                    // Page links read as `[[Title]]`, as in the message itself (FR-LLM-AS-14).
                    _ => crate::assistant::references::for_model(conn, typed),
                };
                Turn { role: Role::User, text }
            }
            ChatRole::Assistant => {
                let text = if let Some(find) = &m.find {
                    let mut lines = vec![find.intro.trim().to_string()];
                    for it in &find.items {
                        lines.push(match (&it.kind, &it.key) {
                            (SourceKind::Task, Some(key)) => format!("- Task {key}: {}", it.title),
                            (SourceKind::Task, None) => format!("- Task: {}", it.title),
                            _ => format!("- Page: {}", it.title),
                        });
                    }
                    lines.retain(|l| !l.is_empty());
                    lines.join("\n")
                } else if let Some(p) = &m.proposal {
                    let mut lines = vec![m.text.trim().to_string()];
                    lines.extend(p.rows.iter().map(|r| actions::row_line(conn, r)));
                    lines.retain(|l| !l.is_empty());
                    lines.join("\n")
                } else if let Some(d) = &m.draft {
                    format!("Draft page: {}", d.title)
                } else {
                    strip_markers(m.text.trim())
                };
                Turn { role: Role::Assistant, text }
            }
        };
        if !turn.text.trim().is_empty() {
            out.push(turn);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fitted {
    /// The newest turns that fit, oldest first.
    pub turns: Vec<Turn>,
    pub used_tokens: u32,
    /// How many older turns were left out.
    pub dropped: usize,
}

/// The newest turns that fit; the history is never cut in the middle, so the first turn (from
/// the newest) that does not fit ends it.
pub fn fit(turns: Vec<Turn>, budget_tokens: u32) -> Fitted {
    let total = turns.len();
    let mut used = 0usize;
    let mut keep = 0usize;
    for t in turns.iter().rev() {
        let cost = estimate_tokens(&t.text);
        if used + cost > budget_tokens as usize {
            break;
        }
        used += cost;
        keep += 1;
    }
    let kept = turns.into_iter().skip(total - keep).collect();
    Fitted {
        turns: kept,
        used_tokens: used as u32,
        dropped: total - keep,
    }
}

/// What the composer shows (UI-AS-3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryUsage {
    /// Tokens the conversation so far takes.
    pub used: u32,
    /// The budget in force: the conversation's or the setting, capped by the model.
    pub budget: u32,
    /// What the model takes for history (its context minus sources and reply); `None` = unknown.
    pub model_limit: Option<u32>,
    /// 0..=100.
    pub percent_left: u8,
    /// Below [`WIDEN_BELOW_PERCENT`] and the budget is below the model's limit.
    pub can_widen: bool,
    /// The budget equals the model's limit: the oldest turns are no longer sent.
    pub at_model_limit: bool,
}

pub fn usage(used: u32, budget: u32, model_limit: Option<u32>) -> HistoryUsage {
    let percent_left = if budget == 0 {
        0
    } else {
        (u64::from(budget.saturating_sub(used)) * 100 / u64::from(budget)) as u8
    };
    HistoryUsage {
        used,
        budget,
        model_limit,
        percent_left,
        can_widen: percent_left < WIDEN_BELOW_PERCENT && model_limit.is_none_or(|l| budget < l),
        at_model_limit: model_limit.is_some_and(|l| budget >= l),
    }
}

/// The budget after *Widen*: doubled, capped at the model's limit.
pub fn widened(budget: u32, model_limit: Option<u32>) -> u32 {
    let doubled = budget.saturating_mul(2);
    match model_limit {
        Some(l) => doubled.min(l.max(budget)),
        None => doubled,
    }
}

/// The items shown under the latest assistant message among `messages`: its cited chips in order,
/// or its find rows. Files and web pages are not items.
pub fn shown_items(messages: &[ChatMessage]) -> Vec<ItemRef> {
    let Some(m) = messages
        .iter()
        .rev()
        .find(|m| m.role == ChatRole::Assistant && m.error.is_none() && !is_running(m))
    else {
        return Vec::new();
    };
    let mut out: Vec<ItemRef> = Vec::new();
    let mut add = |r: ItemRef| {
        if !out.contains(&r) {
            out.push(r);
        }
    };
    if let Some(find) = &m.find {
        for it in &find.items {
            add(ItemRef {
                kind: it.kind,
                id: it.id.clone(),
            });
        }
    } else if let Some(a) = &m.answer {
        for n in &a.cited {
            if let Some(r) = a
                .sources
                .get((*n as usize).wrapping_sub(1))
                .and_then(ItemRef::of_source)
            {
                add(r);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::parts::{AnswerPart, FindPart, FoundItem, SourceRef};
    use crate::conversations::{ChatAttachment, ChatStep, FileKind, MessageError};

    fn msg(id: &str, role: ChatRole, text: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            conversation_id: "c".into(),
            role,
            text: text.into(),
            model_id: None,
            model_label: None,
            effort: None,
            sent_effort: None,
            reasoning_tokens: None,
            created_at: String::new(),
            attachment: None,
            steps: Vec::new(),
            draft: None,
            error: None,
            answer: None,
            find: None,
            proposal: None,
            extra: None,
        }
    }

    fn conn() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    fn source(kind: SourceKind, id: Option<&str>, title: &str) -> SourceRef {
        SourceRef {
            kind,
            id: id.map(str::to_string),
            title: title.into(),
            key: None,
            category_id: None,
            part: None,
            snippet: String::new(),
            url: None,
        }
    }

    #[test]
    fn turns_are_rendered_and_failed_or_running_ones_left_out() {
        let mut all = vec![
            msg("u1", ChatRole::User, " What is the K-factor? "),
            msg("a1", ChatRole::Assistant, "It is 0.982 [1]."),
            msg("u2", ChatRole::User, "hoses"),
        ];
        let mut find = msg("a2", ChatRole::Assistant, "");
        find.find = Some(FindPart {
            intro: "Hose tasks:".into(),
            items: vec![
                FoundItem {
                    kind: SourceKind::Task,
                    id: "t".into(),
                    title: "Auger hose".into(),
                    key: Some("OPS-41".into()),
                    category_id: "p".into(),
                },
                FoundItem {
                    kind: SourceKind::Page,
                    id: "g".into(),
                    title: "Hose guide".into(),
                    key: None,
                    category_id: "p".into(),
                },
            ],
            outside_scope: vec![],
        });
        all.push(find);
        let mut failed = msg("a3", ChatRole::Assistant, "");
        failed.error = Some(MessageError {
            kind: "x".into(),
            message: "x".into(),
            can_use_as_is: false,
        });
        all.push(failed);
        let mut running = msg("a4", ChatRole::Assistant, "partial");
        running.steps = vec![ChatStep {
            label: "Searching".into(),
            state: StepState::Active,
        }];
        all.push(running);
        let mut file = msg("u3", ChatRole::User, "");
        file.attachment = Some(ChatAttachment {
            name: "torque.pdf".into(),
            size: 1,
            kind: FileKind::Pdf,
            chat_file_id: None,
        });
        all.push(file);
        all.push(msg("a5", ChatRole::Assistant, "now"));

        let t = turns(&conn(), &all, "a5");
        let text: Vec<(Role, &str)> = t.iter().map(|t| (t.role, t.text.as_str())).collect();
        assert_eq!(
            text,
            vec![
                (Role::User, "What is the K-factor?"),
                (Role::Assistant, "It is 0.982."),
                (Role::User, "hoses"),
                (
                    Role::Assistant,
                    "Hose tasks:\n- Task OPS-41: Auger hose\n- Page: Hose guide"
                ),
                (Role::User, "(attached torque.pdf)"),
            ]
        );
        // An unknown id means every message.
        assert_eq!(turns(&conn(), &all[..2], "nope").len(), 2);
    }

    #[test]
    fn the_newest_turns_that_fit_are_kept() {
        let turn = |role, n: usize| Turn {
            role,
            text: "x".repeat(n * 4),
        };
        let turns = vec![
            turn(Role::User, 10),
            turn(Role::Assistant, 10),
            turn(Role::User, 10),
            turn(Role::Assistant, 10),
        ];
        let f = fit(turns.clone(), 25);
        assert_eq!((f.turns.len(), f.used_tokens, f.dropped), (2, 20, 2));
        assert_eq!(f.turns[0], turns[2]);
        let all = fit(turns.clone(), 1000);
        assert_eq!((all.turns.len(), all.dropped), (4, 0));
        // The newest turn alone is too big: nothing is sent.
        let none = fit(turns, 5);
        assert!(none.turns.is_empty() && none.dropped == 4 && none.used_tokens == 0);
        assert_eq!(fit(vec![], 10).turns.len(), 0);
    }

    #[test]
    fn usage_and_widening() {
        let u = usage(7_500, 10_000, Some(40_000));
        assert_eq!((u.percent_left, u.can_widen, u.at_model_limit), (25, false, false));
        let u = usage(9_500, 10_000, Some(40_000));
        assert_eq!((u.percent_left, u.can_widen), (5, true));
        // At the model's limit there is nothing to widen to.
        let u = usage(39_000, 40_000, Some(40_000));
        assert_eq!((u.percent_left, u.can_widen, u.at_model_limit), (2, false, true));
        // Over budget and a zero budget read as nothing left.
        assert_eq!(usage(50, 10, None).percent_left, 0);
        assert_eq!(usage(1, 0, None).percent_left, 0);
        assert!(usage(95, 100, None).can_widen);
        assert_eq!(widened(30_000, Some(50_000)), 50_000);
        assert_eq!(widened(10_000, Some(50_000)), 20_000);
        assert_eq!(widened(10_000, None), 20_000);
        assert_eq!(widened(60_000, Some(50_000)), 60_000);
    }

    #[test]
    fn shown_items_are_the_cited_chips_or_the_find_rows() {
        let mut a = msg("a1", ChatRole::Assistant, "text");
        a.answer = Some(AnswerPart {
            sources: vec![
                source(SourceKind::Page, Some("pg"), "Page"),
                source(SourceKind::File, None, "f.pdf"),
                source(SourceKind::Task, Some("tk"), "Task"),
                source(SourceKind::Page, Some("pg"), "Page again"),
            ],
            cited: vec![3, 2, 1, 4, 9],
            ..AnswerPart::default()
        });
        let all = vec![
            msg("u", ChatRole::User, "q"),
            a,
            msg("u2", ChatRole::User, "next"),
        ];
        assert_eq!(shown_items(&all), vec![ItemRef::task("tk"), ItemRef::page("pg")]);
        assert!(shown_items(&all[..1]).is_empty());
        assert!(shown_items(&[]).is_empty());
    }
}
