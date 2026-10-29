//! The merge screen's starting point (FR-KN-DU-3) and *Suggest groups for ungrouped pages*
//! (FR-KN-GR-3).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use specta::Type;

use crate::checks::GroupPick;
use crate::knowledge::Group;

/// What a block of the new content starts as on the merge screen (FR-KN-DU-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockAction {
    /// Not on the existing page: add it.
    Add,
    /// Close to block `target` of the existing page: replace that block.
    Replace,
    /// (Nearly) the same as block `target`: leave it out.
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BlockMatch {
    pub action: BlockAction,
    /// Index of the closest existing block, for `replace` and `ignore`.
    pub target: Option<u32>,
    /// Similarity to that block, 0..1.
    pub score: f32,
}

/// Similarity at or above which a new block starts as *Ignore* (calibrated in M5).
pub const IGNORE_AT: f32 = 0.99;
/// Similarity at or above which a new block starts as *Replace* (calibrated in M5).
pub const REPLACE_AT: f32 = 0.7;

/// One [`BlockMatch`] per incoming block, from normalised vectors of the blocks (the caller embeds
/// them as documents). Headings and empty blocks are matched like any other block; the UI decides
/// how to show them. An existing block is the target of at most one *Replace*.
pub fn match_blocks(existing: &[Vec<f32>], incoming: &[Vec<f32>]) -> Vec<BlockMatch> {
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let mut out: Vec<BlockMatch> = incoming
        .iter()
        .map(|v| {
            let mut best: Option<(usize, f32)> = None;
            for (i, e) in existing.iter().enumerate() {
                let score = dot(e, v);
                if best.is_none_or(|(_, b)| score > b) {
                    best = Some((i, score));
                }
            }
            let Some((i, score)) = best else {
                return BlockMatch { action: BlockAction::Add, target: None, score: 0.0 };
            };
            let action = if score >= IGNORE_AT {
                BlockAction::Ignore
            } else if score >= REPLACE_AT {
                BlockAction::Replace
            } else {
                BlockAction::Add
            };
            let target = (action != BlockAction::Add).then_some(i as u32);
            BlockMatch { action, target, score }
        })
        .collect();
    one_replace_per_target(&mut out);
    out
}

/// The numbers in a text, in order: digit runs with `.` or `,` between digits (`0.982`, `2210`
/// of `HX-2210`).
fn numbers(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || (matches!(chars[i], '.' | ',')
                        && i + 1 < chars.len()
                        && chars[i + 1].is_ascii_digit()))
            {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
        } else {
            i += 1;
        }
    }
    out
}

/// Whether two texts contain the same sequence of numbers.
pub fn same_numbers(a: &str, b: &str) -> bool {
    numbers(a) == numbers(b)
}

/// A block is never *Ignore* when its numbers differ from its target's: a changed number must
/// not be dropped quietly. `existing` and `incoming` are the block texts `matches` was made from.
pub fn apply_number_rule(existing: &[String], incoming: &[String], matches: &mut [BlockMatch]) {
    for (m, text) in matches.iter_mut().zip(incoming) {
        if let (BlockAction::Ignore, Some(t)) = (m.action, m.target)
            && existing.get(t as usize).is_some_and(|e| !same_numbers(e, text))
        {
            m.action = BlockAction::Replace;
        }
    }
    one_replace_per_target(matches);
}

/// One *Replace* per existing block: the closest incoming block keeps it, the others are added.
fn one_replace_per_target(out: &mut [BlockMatch]) {
    let mut winner: HashMap<u32, usize> = HashMap::new();
    for (k, m) in out.iter().enumerate() {
        if let (BlockAction::Replace, Some(t)) = (m.action, m.target) {
            match winner.get(&t) {
                Some(&w) if out[w].score >= m.score => {}
                _ => {
                    winner.insert(t, k);
                }
            }
        }
    }
    for (k, m) in out.iter_mut().enumerate() {
        if let (BlockAction::Replace, Some(t)) = (m.action, m.target)
            && winner.get(&t) != Some(&k)
        {
            m.action = BlockAction::Add;
            m.target = None;
        }
    }
}

/// One page's suggestion from *Suggest groups* (FR-KN-GR-3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupProposal {
    pub page_id: String,
    pub page_title: String,
    pub group: GroupPick,
}

/// A new group proposed by the run, with the description the model gave it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewGroupName {
    pub name: String,
    pub description: String,
}

/// Reads one `suggest-groups` reply (README "Local checks" › `suggest-groups`). `batch` maps the
/// batch ids to (batch id, page id, page title) (`p1`, …); `proposed` holds the new names of earlier
/// batches of the same run, so a repeated name is spelled the same. The new groups returned are
/// those this batch uses, an earlier batch's included (the caller keeps each name once).
pub fn read_group_suggestions(
    reply: &Map<String, Value>,
    batch: &[(String, String, String)],
    groups: &[Group],
    proposed: &[NewGroupName],
) -> (Vec<GroupProposal>, Vec<NewGroupName>) {
    let key = |s: &str| s.trim().to_lowercase();
    // New names of this run so far, in first-seen spelling: earlier batches, then this one.
    let mut names: Vec<NewGroupName> = proposed.to_vec();
    let described: HashMap<String, String> = reply
        .get("new_groups")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|g| {
                    let name = g.get("name")?.as_str()?;
                    let description = g.get("description").and_then(Value::as_str).unwrap_or("");
                    Some((key(name), description.trim().to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut seen_ids: HashSet<&str> = HashSet::new();
    let mut proposals = Vec::new();
    let mut used: Vec<String> = Vec::new();
    for entry in reply.get("pages").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = entry.get("id").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        let Some((_, page_id, title)) = batch.iter().find(|(b, _, _)| b == id) else {
            continue;
        };
        if !seen_ids.insert(id) {
            continue;
        }
        let Some(name) = entry
            .get("group")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|n| !n.is_empty())
        else {
            continue;
        };
        let pick = if let Some(g) = groups.iter().find(|g| key(&g.name) == key(name)) {
            GroupPick::Existing { group_id: g.id.clone(), name: g.name.clone() }
        } else {
            let spelled = match names.iter().find(|n| key(&n.name) == key(name)) {
                Some(n) => n.name.clone(),
                None => {
                    names.push(NewGroupName {
                        name: name.to_string(),
                        description: described.get(&key(name)).cloned().unwrap_or_default(),
                    });
                    name.to_string()
                }
            };
            if !used.iter().any(|u| key(u) == key(&spelled)) {
                used.push(spelled.clone());
            }
            GroupPick::New { name: spelled }
        };
        proposals.push(GroupProposal {
            page_id: page_id.clone(),
            page_title: title.clone(),
            group: pick,
        });
    }
    let new_groups = used
        .iter()
        .filter_map(|u| names.iter().find(|n| key(&n.name) == key(u)).cloned())
        .map(|mut n| {
            // An earlier batch's proposal without a description may get one here.
            if n.description.is_empty() {
                n.description = described.get(&key(&n.name)).cloned().unwrap_or_default();
            }
            n
        })
        .collect();
    (proposals, new_groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn unit(angle: f32) -> Vec<f32> {
        vec![angle.cos(), angle.sin()]
    }

    #[test]
    fn blocks_start_as_add_replace_or_ignore() {
        let existing = vec![unit(0.0), unit(1.5)];
        let incoming = vec![unit(0.0), unit(0.55), unit(0.9), unit(-1.5)];
        let m = match_blocks(&existing, &incoming);
        assert_eq!((m[0].action, m[0].target), (BlockAction::Ignore, Some(0)));
        assert_eq!((m[1].action, m[1].target), (BlockAction::Replace, Some(0)));
        // 0.6 rad from block 1 (cos 0.825): a Replace of block 1.
        assert_eq!((m[2].action, m[2].target), (BlockAction::Replace, Some(1)));
        assert_eq!((m[3].action, m[3].target), (BlockAction::Add, None));
    }

    #[test]
    fn a_block_is_replaced_once() {
        let existing = vec![unit(0.0)];
        let incoming = vec![unit(0.5), unit(0.3), unit(-0.45)];
        let m = match_blocks(&existing, &incoming);
        assert_eq!(m[1].action, BlockAction::Replace);
        assert_eq!(m[0].action, BlockAction::Add);
        assert_eq!(m[2].action, BlockAction::Add);
        assert_eq!(m[0].target, None);
        assert!(m[0].score > REPLACE_AT, "the score is still reported");
    }

    #[test]
    fn numbers_are_compared_in_order() {
        assert!(same_numbers("K-factor 0.982 for HX-2210.", "The K-factor is 0.982 (HX-2210)"));
        assert!(!same_numbers("Wait 5 minutes", "Wait 10 min"));
        assert!(!same_numbers("HX-2210", "HX-2211"));
        assert!(!same_numbers("1 then 2", "2 then 1"));
        assert!(same_numbers("Step 3.", "Step 3"));
        assert!(same_numbers("no numbers", "none either"));
        assert!(!same_numbers("1,5 bar", "1.5 bar"));
    }

    #[test]
    fn a_changed_number_is_never_ignored() {
        let existing = vec!["Wait 5 minutes.".to_string(), "Rinse well.".to_string()];
        let incoming = vec!["Wait 10 minutes.".to_string(), "Rinse well.".to_string()];
        let mut m = match_blocks(&[unit(0.0), unit(1.5)], &[unit(0.0), unit(1.5)]);
        assert_eq!((m[0].action, m[1].action), (BlockAction::Ignore, BlockAction::Ignore));
        apply_number_rule(&existing, &incoming, &mut m);
        assert_eq!((m[0].action, m[0].target), (BlockAction::Replace, Some(0)));
        assert_eq!(m[1].action, BlockAction::Ignore);
    }

    #[test]
    fn nothing_to_match_is_added() {
        let m = match_blocks(&[], &[unit(0.0)]);
        assert_eq!(m[0].action, BlockAction::Add);
        assert!(match_blocks(&[unit(0.0)], &[]).is_empty());
    }

    fn group(id: &str, name: &str) -> Group {
        Group {
            id: id.into(),
            category_id: "p".into(),
            parent_id: None,
            name: name.into(),
            description: String::new(),
            path: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn batch() -> Vec<(String, String, String)> {
        ["p1", "p2", "p3", "p4"]
            .iter()
            .map(|b| (b.to_string(), format!("page-{b}"), format!("Title {b}")))
            .collect()
    }

    fn reply(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn group_suggestions_are_mapped_and_cleaned() {
        let groups = vec![group("g1", "Calibration")];
        let proposed = vec![NewGroupName { name: "Safety".into(), description: "Rules".into() }];
        let r = reply(json!({
            "pages": [
                {"id": "p1", "group": "calibration", "existing": false},
                {"id": "p2", "group": "safety", "existing": false},
                {"id": "p9", "group": "Calibration", "existing": true},
                {"id": "p3", "group": " Pumps ", "existing": false},
                {"id": "p4", "group": null, "existing": false},
                {"id": "p1", "group": "Pumps", "existing": false}
            ],
            "new_groups": [
                {"name": "Pumps", "description": " Pump pages "},
                {"name": "Unused", "description": "x"}
            ]
        }));
        let (proposals, new_groups) = read_group_suggestions(&r, &batch(), &groups, &proposed);
        assert_eq!(proposals.len(), 3, "unknown id, null and a repeated id give nothing");
        assert_eq!(
            proposals[0].group,
            GroupPick::Existing { group_id: "g1".into(), name: "Calibration".into() }
        );
        assert_eq!(proposals[0].page_id, "page-p1");
        assert_eq!(proposals[0].page_title, "Title p1");
        assert_eq!(proposals[1].group, GroupPick::New { name: "Safety".into() });
        assert_eq!(proposals[2].group, GroupPick::New { name: "Pumps".into() });
        assert_eq!(
            new_groups,
            vec![
                NewGroupName { name: "Safety".into(), description: "Rules".into() },
                NewGroupName { name: "Pumps".into(), description: "Pump pages".into() },
            ]
        );
    }

    #[test]
    fn a_new_name_is_spelled_alike_within_a_batch() {
        let r = reply(json!({
            "pages": [
                {"id": "p1", "group": "Pumps", "existing": false},
                {"id": "p2", "group": "pumps", "existing": true}
            ],
            "new_groups": [{"name": "Pumps", "description": "d"}]
        }));
        let (proposals, new_groups) = read_group_suggestions(&r, &batch(), &[], &[]);
        assert_eq!(proposals[1].group, GroupPick::New { name: "Pumps".into() });
        assert_eq!(new_groups.len(), 1);
    }

    #[test]
    fn a_reply_without_pages_gives_nothing() {
        let (p, n) = read_group_suggestions(&reply(json!({"pages": "x"})), &batch(), &[], &[]);
        assert!(p.is_empty() && n.is_empty());
    }
}
