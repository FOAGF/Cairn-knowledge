//! The glossary (spec §5.9): work terms with other names and a short description, global or per
//! category (FR-KN-GL-1/2), recognised in text by fixed rules (FR-KN-GL-4) and used by search
//! (FR-RAG-IX-3), "Used in" (UI-KN-8), spelling (FR-KN-GL-6) and, from M6, the assistant.
//!
//! Matching rules (FR-KN-GL-4; the shared cases in `app/src/lib/glossary-cases.json` must pass
//! here and in the UI's matcher):
//! - Whole words only; the boundary looks at Latin letters, digits and `_` only, so a Latin term
//!   right next to Chinese text still matches.
//! - A name of two or more capitals and no lowercase ("AN", "MMU") matches only in capitals;
//!   other names ignore (ASCII) case.
//! - Plurals and codes belong to the word: a trailing "s"/"es" and any "-<letters/digits>" runs
//!   ("MMUs", "MMU-07").
//! - Spaces inside a multi-word name match any run of white space.
//! - A name containing Han characters matches anywhere (Chinese has no word boundaries).
//! - Overlapping matches: the leftmost wins, then the longest.
//! - An item in category P uses the global entries plus P's; a P entry overrides a global entry
//!   with the same term (ignoring case) inside P.
//!
//! Upkeep:
//! - `hooks` call [`index_item`] on every page/task save (before the keyword and vector index, which
//!   read the item's terms), and `rebuild_all` calls [`rebuild`].
//! - Creating, changing or deleting an entry re-indexes only the items that may use it under its
//!   old or new names: the current `glossary_uses` rows plus keyword-index candidates for the names
//!   (search, vectors and uses are all refreshed for them).
//!
//! Owned by the M4 glossary-core agent.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::search::{self, SearchKind, is_han};
use crate::util::{new_id, now_iso};
use crate::vectors;
use crate::{CoreError, Db, Result};

mod matcher;
use matcher::{Found, Matcher};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryEntry {
    pub id: String,
    /// `None` = global (FR-KN-GL-2).
    pub category_id: Option<String>,
    pub term: String,
    /// e.g. the long form "Mobile Manufacturing Unit".
    pub other_names: Vec<String>,
    /// A few lines, plain text.
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    /// Computed (FR-KN-GL-3, D-38): entries named in this description or naming this term in
    /// theirs, one step, within what applies to this entry: a category entry sees its category's
    /// entries and the global ones; a global entry sees global ones only.
    pub related_ids: Vec<String>,
    /// Computed: for a category entry, the global entry with the same term it overrides.
    pub overrides_id: Option<String>,
    /// Computed: for a global entry, the categories that override it.
    pub overridden_in: Vec<String>,
    /// Computed: how many pages and tasks use it (`glossary_uses`).
    pub use_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewGlossaryEntry {
    /// Default: global.
    #[specta(optional)]
    pub category_id: Option<String>,
    pub term: String,
    #[specta(optional)]
    pub other_names: Option<Vec<String>>,
    #[specta(optional)]
    pub description: Option<String>,
}

/// Only the given fields change. `categoryId: ""` makes the entry global.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryPatch {
    #[specta(optional)]
    pub category_id: Option<String>,
    #[specta(optional)]
    pub term: Option<String>,
    #[specta(optional)]
    pub other_names: Option<Vec<String>>,
    #[specta(optional)]
    pub description: Option<String>,
}

/// One "Used in" line: an item and the text around its first match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageHit {
    /// `page` or `task`.
    pub kind: SearchKind,
    pub id: String,
    pub title: String,
    /// Task key for tasks.
    pub key: Option<String>,
    pub category_id: String,
    /// Plain text before the match (about 50 characters, "…" if cut), the matched text, and after.
    pub before: String,
    pub matched: String,
    pub after: String,
}

/// A page of "Used in" (UI-KN-8: 3 at first, then 10 more at a time).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryUsage {
    pub total: u32,
    /// Pages first, then tasks; newest first within each.
    pub items: Vec<UsageHit>,
}

/// A dev-seed entry with a fixed id (the sample glossary, like the other dev seeds).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GlossarySeed {
    pub id: String,
    pub category_id: Option<String>,
    pub term: String,
    pub other_names: Vec<String>,
    pub description: String,
}

/// A term an item uses, as it goes into the search indexes (FR-RAG-IX-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermText {
    pub term: String,
    pub other_names: Vec<String>,
    pub description: String,
}

impl TermText {
    /// "MMU (Mobile Manufacturing Unit): Truck that mixes …" — the form added to pieces and the
    /// keyword index's `extra` column.
    pub fn line(&self) -> String {
        let names = if self.other_names.is_empty() {
            self.term.clone()
        } else {
            format!("{} ({})", self.term, self.other_names.join(", "))
        };
        if self.description.is_empty() {
            names
        } else {
            format!("{names}: {}", self.description)
        }
    }
}

// ---------------------------------------------------------------- stored entries and scopes

/// One stored entry, without the computed fields.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub id: String,
    pub category_id: Option<String>,
    pub term: String,
    pub other_names: Vec<String>,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Entry {
    /// The term, then the other names.
    fn names(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.term).chain(self.other_names.iter())
    }

    fn text(&self) -> TermText {
        TermText {
            term: self.term.clone(),
            other_names: self.other_names.clone(),
            description: self.description.clone(),
        }
    }

    /// The key for "same term in the same scope" (ignoring case, as the unique index does).
    fn term_key(&self) -> String {
        self.term.to_ascii_lowercase()
    }
}

const ENTRY_COLUMNS: &str =
    "id, category_id, term, other_names, description, created_at, updated_at";

fn entry_from_row(r: &rusqlite::Row) -> rusqlite::Result<Entry> {
    let names: String = r.get(3)?;
    Ok(Entry {
        id: r.get(0)?,
        category_id: r.get(1)?,
        term: r.get(2)?,
        // A damaged list only loses the other names, never the entry.
        other_names: serde_json::from_str(&names).unwrap_or_default(),
        description: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

/// Every entry, sorted by term (ignoring case), then id: also the order that decides which entry
/// owns a name that two entries share.
fn load_entries(conn: &Connection) -> Result<Vec<Rc<Entry>>> {
    let mut entries = conn
        .prepare_cached(&format!("SELECT {ENTRY_COLUMNS} FROM glossary_entries"))?
        .query_map([], entry_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by(|a, b| {
        a.term
            .to_lowercase()
            .cmp(&b.term.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(entries.into_iter().map(Rc::new).collect())
}

fn load_entry(conn: &Connection, id: &str) -> Result<Entry> {
    conn.prepare_cached(&format!(
        "SELECT {ENTRY_COLUMNS} FROM glossary_entries WHERE id = ?1"
    ))?
    .query_row([id], entry_from_row)
    .optional()?
    .ok_or_else(|| CoreError::NotFound {
        entity: "glossary entry",
        id: id.to_string(),
    })
}

/// The entries that apply in `category` ("" = outside any category glossary): the global ones plus
/// the category's, where a category entry hides the global entry with the same term (FR-KN-GL-2).
fn applicable(all: &[Rc<Entry>], category: &str) -> Vec<Rc<Entry>> {
    let own: HashSet<String> = all
        .iter()
        .filter(|e| e.category_id.as_deref() == Some(category))
        .map(|e| e.term_key())
        .collect();
    all.iter()
        .filter(|e| match e.category_id.as_deref() {
            Some(p) => p == category,
            None => !own.contains(&e.term_key()),
        })
        .cloned()
        .collect()
}

/// The entries that apply in one category and a matcher for their names.
pub(crate) struct Scope {
    entries: Vec<Rc<Entry>>,
    matcher: Matcher,
}

impl Scope {
    fn new(entries: Vec<Rc<Entry>>) -> Scope {
        let names: Vec<(usize, String)> = entries
            .iter()
            .enumerate()
            .flat_map(|(i, e)| e.names().map(move |n| (i, n.clone())))
            .collect();
        Scope {
            matcher: Matcher::new(&names),
            entries,
        }
    }

    /// Matches in text order, with the entry each belongs to.
    fn find(&self, text: &str) -> Vec<(&Rc<Entry>, Found)> {
        self.matcher
            .find(text)
            .into_iter()
            .map(|f| (&self.entries[f.entry], f))
            .collect()
    }
}

/// All entries, loaded once for a batch of work, with a matcher per category made on demand.
pub(crate) struct Glossary {
    all: Vec<Rc<Entry>>,
    categories_with_entries: HashSet<String>,
    scopes: HashMap<String, Rc<Scope>>,
}

impl Glossary {
    pub(crate) fn load(conn: &Connection) -> Result<Glossary> {
        Ok(Glossary::new(load_entries(conn)?))
    }

    fn new(all: Vec<Rc<Entry>>) -> Glossary {
        let categories_with_entries = all.iter().filter_map(|e| e.category_id.clone()).collect();
        Glossary {
            all,
            categories_with_entries,
            scopes: HashMap::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    /// The scope for items of `category`; categories without entries of their own share the global one.
    fn scope(&mut self, category: &str) -> Rc<Scope> {
        let key = if self.categories_with_entries.contains(category) {
            category
        } else {
            ""
        };
        if let Some(s) = self.scopes.get(key) {
            return Rc::clone(s);
        }
        let scope = Rc::new(Scope::new(applicable(&self.all, key)));
        self.scopes.insert(key.to_string(), Rc::clone(&scope));
        scope
    }

    /// The entries of `category` found in `text`, in first-match order, once each.
    pub(crate) fn terms_in(&mut self, category: &str, text: &str) -> Vec<TermText> {
        if self.all.is_empty() {
            return Vec::new();
        }
        let scope = self.scope(category);
        let mut seen = HashSet::new();
        scope
            .find(text)
            .into_iter()
            .filter(|(e, _)| seen.insert(e.id.clone()))
            .map(|(e, _)| e.text())
            .collect()
    }
}

// ---------------------------------------------------------------- computed fields

/// For every entry the ids of its related entries (FR-KN-GL-3, D-38): entries named in its
/// description or naming its term in theirs, one step, within the entries that apply to it.
fn related_map(all: &[Rc<Entry>]) -> HashMap<String, HashSet<String>> {
    let mut related: HashMap<String, HashSet<String>> = HashMap::new();
    // One pass per scope: the global one, then every category that has entries.
    let mut scopes: Vec<Option<&str>> = vec![None];
    let mut categories: Vec<&str> = all
        .iter()
        .filter_map(|e| e.category_id.as_deref())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    categories.sort_unstable();
    scopes.extend(categories.into_iter().map(Some));

    for scope_category in scopes {
        let scope = Scope::new(applicable(all, scope_category.unwrap_or("")));
        for from in &scope.entries {
            for (to, _) in scope.find(&from.description) {
                if to.id == from.id {
                    continue;
                }
                // The link counts for the entries that belong to this scope.
                if from.category_id.as_deref() == scope_category {
                    related
                        .entry(from.id.clone())
                        .or_default()
                        .insert(to.id.clone());
                }
                if to.category_id.as_deref() == scope_category {
                    related
                        .entry(to.id.clone())
                        .or_default()
                        .insert(from.id.clone());
                }
            }
        }
    }
    related
}

fn use_counts(conn: &Connection) -> Result<HashMap<String, u32>> {
    Ok(conn
        .prepare_cached("SELECT entry_id, COUNT(*) FROM glossary_uses GROUP BY entry_id")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?
        .collect::<std::result::Result<HashMap<_, _>, _>>()?)
}

/// The entries with `related_ids`, `overrides_id`, `overridden_in` and `use_count` filled in.
fn with_computed(conn: &Connection, all: &[Rc<Entry>]) -> Result<Vec<GlossaryEntry>> {
    let related = related_map(all);
    let counts = use_counts(conn)?;
    let global_by_term: HashMap<String, &str> = all
        .iter()
        .filter(|e| e.category_id.is_none())
        .map(|e| (e.term_key(), e.id.as_str()))
        .collect();
    let mut overridden_in: HashMap<&str, Vec<String>> = HashMap::new();
    for e in all {
        if let (Some(p), Some(g)) = (&e.category_id, global_by_term.get(&e.term_key())) {
            overridden_in.entry(g).or_default().push(p.clone());
        }
    }
    Ok(all
        .iter()
        .map(|e| GlossaryEntry {
            id: e.id.clone(),
            category_id: e.category_id.clone(),
            term: e.term.clone(),
            other_names: e.other_names.clone(),
            description: e.description.clone(),
            created_at: e.created_at.clone(),
            updated_at: e.updated_at.clone(),
            related_ids: related.get(&e.id).map_or_else(Vec::new, |set| {
                all.iter()
                    .filter(|o| set.contains(&o.id))
                    .map(|o| o.id.clone())
                    .collect()
            }),
            overrides_id: e
                .category_id
                .as_ref()
                .and_then(|_| global_by_term.get(&e.term_key()))
                .map(|id| (*id).to_string()),
            overridden_in: overridden_in
                .remove(e.id.as_str())
                .map_or_else(Vec::new, |mut v| {
                    v.sort();
                    v.dedup();
                    v
                }),
            use_count: counts.get(&e.id).copied().unwrap_or(0),
        })
        .collect())
}

// ---------------------------------------------------------------- entries

/// Every entry, sorted by term (case-insensitive), with the computed fields filled in.
pub fn list(conn: &Connection) -> Result<Vec<GlossaryEntry>> {
    with_computed(conn, &load_entries(conn)?)
}

pub fn get(conn: &Connection, id: &str) -> Result<GlossaryEntry> {
    list(conn)?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| CoreError::NotFound {
            entity: "glossary entry",
            id: id.to_string(),
        })
}

/// Trimmed, empty names removed, duplicates (and the term itself) dropped, order kept.
fn clean_names(term: &str, names: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let n = n.trim().to_string();
        if !n.is_empty() && n != term && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

fn clean_term(term: &str) -> Result<String> {
    let term = term.trim();
    if term.is_empty() {
        return Err(CoreError::Invalid("Give the term a name.".into()));
    }
    Ok(term.to_string())
}

/// The category must exist and not be archived (FR-KN-GL-2).
fn check_category(conn: &Connection, category_id: &str) -> Result<()> {
    let archived: Option<bool> = conn
        .query_row(
            "SELECT status = 'archived' FROM categories WHERE id = ?1",
            [category_id],
            |r| r.get(0),
        )
        .optional()?;
    match archived {
        None => Err(CoreError::NotFound {
            entity: "category",
            id: category_id.to_string(),
        }),
        Some(true) => Err(CoreError::Invalid(
            "That category is archived; restore it to add glossary entries.".into(),
        )),
        Some(false) => Ok(()),
    }
}

/// One term per scope, ignoring case (the unique index would refuse it too, with a worse message).
fn check_unique(conn: &Connection, category: Option<&str>, term: &str, except: &str) -> Result<()> {
    let taken: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM glossary_entries
             WHERE COALESCE(category_id, '') = ?1 AND term = ?2 COLLATE NOCASE AND id != ?3",
            params![category.unwrap_or(""), term, except],
            |r| r.get(0),
        )
        .optional()?;
    if taken.is_none() {
        return Ok(());
    }
    Err(CoreError::Conflict(match category {
        None => format!("“{term}” is already in the global glossary"),
        Some(p) => {
            let name: String =
                conn.query_row("SELECT name FROM categories WHERE id = ?1", [p], |r| r.get(0))?;
            format!("“{term}” is already in the glossary of category {name}")
        }
    }))
}

fn insert_entry(conn: &Connection, e: &Entry) -> Result<()> {
    conn.execute(
        "INSERT INTO glossary_entries (id, category_id, term, other_names, description, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            e.id,
            e.category_id,
            e.term,
            serde_json::to_string(&e.other_names)?,
            e.description,
            e.created_at,
            e.updated_at
        ],
    )?;
    Ok(())
}

/// Validates (term required; unique per scope ignoring case; the category must exist), saves,
/// and re-indexes the items that may use it.
pub fn create(db: &mut Db, input: NewGlossaryEntry) -> Result<GlossaryEntry> {
    let id = db.tx(|tx| {
        let term = clean_term(&input.term)?;
        let category_id = input.category_id.filter(|p| !p.is_empty());
        if let Some(p) = &category_id {
            check_category(tx, p)?;
        }
        check_unique(tx, category_id.as_deref(), &term, "")?;
        let now = now_iso();
        let entry = Entry {
            id: new_id(),
            category_id,
            other_names: clean_names(&term, input.other_names.unwrap_or_default()),
            term,
            description: input.description.unwrap_or_default().trim().to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        insert_entry(tx, &entry)?;
        let mut items = HashSet::new();
        collect_affected(tx, &entry, &mut items)?;
        reindex_after_change(tx, &items, Some(&entry.id))?;
        Ok(entry.id)
    })?;
    get(db.conn(), &id)
}

pub fn update(db: &mut Db, id: &str, patch: GlossaryPatch) -> Result<GlossaryEntry> {
    db.tx(|tx| {
        let old = load_entry(tx, id)?;
        let mut new = old.clone();
        if let Some(term) = &patch.term {
            new.term = clean_term(term)?;
        }
        if let Some(p) = &patch.category_id {
            new.category_id = Some(p.clone()).filter(|p| !p.is_empty());
            if new.category_id != old.category_id
                && let Some(p) = &new.category_id
            {
                check_category(tx, p)?;
            }
        }
        if let Some(d) = &patch.description {
            new.description = d.trim().to_string();
        }
        new.other_names = clean_names(
            &new.term,
            patch
                .other_names
                .clone()
                .unwrap_or_else(|| old.other_names.clone()),
        );
        check_unique(tx, new.category_id.as_deref(), &new.term, id)?;
        new.updated_at = now_iso();

        let mut items = HashSet::new();
        collect_affected(tx, &old, &mut items)?;
        tx.execute(
            "UPDATE glossary_entries SET category_id = ?1, term = ?2, other_names = ?3,
                 description = ?4, updated_at = ?5 WHERE id = ?6",
            params![
                new.category_id,
                new.term,
                serde_json::to_string(&new.other_names)?,
                new.description,
                new.updated_at,
                id
            ],
        )?;
        collect_affected(tx, &new, &mut items)?;
        reindex_after_change(tx, &items, Some(id))
    })?;
    get(db.conn(), id)
}

/// Deletes the entry and re-indexes the items that used it.
pub fn delete(db: &mut Db, id: &str) -> Result<()> {
    db.tx(|tx| {
        let old = load_entry(tx, id)?;
        let mut items = HashSet::new();
        collect_affected(tx, &old, &mut items)?;
        // `glossary_uses` rows go with the entry (ON DELETE CASCADE).
        tx.execute("DELETE FROM glossary_entries WHERE id = ?1", [id])?;
        reindex_after_change(tx, &items, None)?;
        search::remove(tx, SearchKind::Term, id)?;
        vectors::remove_item(tx, SearchKind::Term, id)
    })
}

/// Words of every term and other name, for the spelling check (FR-KN-GL-6). Names with Chinese
/// characters are left out (the check skips Chinese anyway).
pub fn spelling_words(conn: &Connection) -> Result<Vec<String>> {
    let mut words: BTreeSet<String> = BTreeSet::new();
    for e in load_entries(conn)? {
        for name in e.names().filter(|n| !n.chars().any(is_han)) {
            for w in name.split(|c: char| !(c.is_alphanumeric() || c == '\'')) {
                if w.chars().any(char::is_alphabetic) {
                    words.insert(w.to_string());
                }
            }
        }
    }
    Ok(words.into_iter().collect())
}

/// Inserts sample entries whose ids are missing (development only), then re-indexes everything.
pub fn dev_seed(db: &mut Db, seed: Vec<GlossarySeed>) -> Result<()> {
    db.tx(|tx| {
        for s in seed {
            let category_id = s.category_id.filter(|p| !p.is_empty());
            if let Some(p) = &category_id
                && check_category(tx, p).is_err()
            {
                continue;
            }
            let now = now_iso();
            let term = s.term.trim().to_string();
            let entry = Entry {
                id: s.id,
                category_id,
                other_names: clean_names(&term, s.other_names),
                term,
                description: s.description,
                created_at: now.clone(),
                updated_at: now,
            };
            // Same id, or the same term in the same scope, is left alone.
            if load_entry(tx, &entry.id).is_ok()
                || check_unique(tx, entry.category_id.as_deref(), &entry.term, "").is_err()
            {
                continue;
            }
            insert_entry(tx, &entry)?;
        }
        crate::hooks::rebuild_all(tx)
    })
}

// ---------------------------------------------------------------- re-indexing after an entry change

type Items = HashSet<(SearchKind, String)>;

/// Adds the items that may use (or have used, or will use) `entry` to `items`: the ones recorded in
/// `glossary_uses`, the ones using the global entry it hides, and keyword-index candidates for
/// each of its names (limited to its category for a category entry).
fn collect_affected(conn: &Connection, entry: &Entry, items: &mut Items) -> Result<()> {
    let kind_of = |s: &str| {
        if s == "page" {
            SearchKind::Page
        } else {
            SearchKind::Task
        }
    };
    let mut add_rows = |sql: &str, arg: &str| -> Result<()> {
        let rows = conn
            .prepare_cached(sql)?
            .query_map([arg], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        items.extend(rows.into_iter().map(|(k, id)| (kind_of(&k), id)));
        Ok(())
    };
    add_rows(
        "SELECT kind, ref_id FROM glossary_uses WHERE entry_id = ?1",
        &entry.id,
    )?;
    if entry.category_id.is_some() {
        add_rows(
            "SELECT u.kind, u.ref_id FROM glossary_uses u
             JOIN glossary_entries g ON g.id = u.entry_id
             WHERE g.category_id IS NULL AND g.term = ?1 COLLATE NOCASE",
            &entry.term,
        )?;
    }
    for name in entry.names() {
        let Some(m) = name_query(name) else { continue };
        let found = conn
            .prepare_cached(
                "WITH h AS MATERIALIZED (SELECT rowid AS rid FROM search_index WHERE search_index MATCH ?1)
                 SELECT r.kind, r.ref_id FROM h JOIN search_ref r ON r.rowid = h.rid
                 WHERE r.kind IN ('page', 'task')",
            )?
            .query_map([m], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (k, id) in found {
            let kind = kind_of(&k);
            let keep = match &entry.category_id {
                None => true,
                Some(p) => item_category(conn, kind, &id)?.as_deref() == Some(p.as_str()),
            };
            if keep {
                items.insert((kind, id));
            }
        }
    }
    Ok(())
}

/// A keyword query that finds every text that may contain `name`: its words as one phrase, the
/// last word as a prefix (so "MMUs" and "MMU-07" are found too). `None` if the name has no words.
fn name_query(name: &str) -> Option<String> {
    let spaced = search::spaced(name);
    if !spaced.chars().any(char::is_alphanumeric) {
        return None;
    }
    Some(format!("\"{}\"*", spaced.replace('"', "\"\"")))
}

/// Refreshes uses, keyword row and pieces of `items`, then the entry itself (`entry_id`).
fn reindex_after_change(conn: &Connection, items: &Items, entry_id: Option<&str>) -> Result<()> {
    let mut g = Glossary::load(conn)?;
    for (kind, id) in items {
        reindex_with(conn, &mut g, *kind, id)?;
    }
    if let Some(id) = entry_id {
        search::index_term(conn, id)?;
        vectors::index_item(conn, SearchKind::Term, id)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- uses

/// The category and the text the glossary reads for a page (title and body) or task (title,
/// description and comments); `None` if the item doesn't exist.
fn item_text(conn: &Connection, kind: SearchKind, id: &str) -> Result<Option<(String, String)>> {
    match kind {
        SearchKind::Page => Ok(conn
            .prepare_cached(
                "SELECT category_id, title || char(10) || body FROM pages WHERE id = ?1",
            )?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?),
        SearchKind::Task => {
            let row: Option<(String, String)> = conn
                .prepare_cached(
                    "SELECT category_id, title || char(10) || description FROM tasks WHERE id = ?1",
                )?
                .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            let Some((category, mut text)) = row else {
                return Ok(None);
            };
            let comments = conn
                .prepare_cached(
                    "SELECT text FROM task_comments WHERE task_id = ?1 ORDER BY created_at, id",
                )?
                .query_map([id], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for c in comments {
                text.push('\n');
                text.push_str(&c);
            }
            Ok(Some((category, text)))
        }
        SearchKind::Term => Ok(None),
    }
}

fn item_category(conn: &Connection, kind: SearchKind, id: &str) -> Result<Option<String>> {
    let sql = match kind {
        SearchKind::Page => "SELECT category_id FROM pages WHERE id = ?1",
        SearchKind::Task => "SELECT category_id FROM tasks WHERE id = ?1",
        SearchKind::Term => return Ok(None),
    };
    Ok(conn
        .prepare_cached(sql)?
        .query_row([id], |r| r.get(0))
        .optional()?)
}

/// Recomputes which entries one page or task uses (`glossary_uses`). `hooks` use [`reindex`],
/// which does this first; this one alone is for callers that don't touch the other indexes.
#[allow(dead_code)]
pub(crate) fn index_item(conn: &Connection, kind: SearchKind, id: &str) -> Result<()> {
    index_item_with(conn, &mut Glossary::load(conn)?, kind, id)
}

pub(crate) fn index_item_with(
    conn: &Connection,
    g: &mut Glossary,
    kind: SearchKind,
    id: &str,
) -> Result<()> {
    remove_item(conn, kind, id)?;
    if g.is_empty() {
        return Ok(());
    }
    let Some((category, text)) = item_text(conn, kind, id)? else {
        return Ok(());
    };
    let scope = g.scope(&category);
    let mut insert = conn.prepare_cached(
        "INSERT OR IGNORE INTO glossary_uses (entry_id, kind, ref_id) VALUES (?1, ?2, ?3)",
    )?;
    for (entry, _) in scope.find(&text) {
        insert.execute(params![entry.id, kind.as_str(), id])?;
    }
    Ok(())
}

/// Forgets what an item used (it was deleted, or is about to be recomputed).
pub(crate) fn remove_item(conn: &Connection, kind: SearchKind, id: &str) -> Result<()> {
    conn.prepare_cached("DELETE FROM glossary_uses WHERE kind = ?1 AND ref_id = ?2")?
        .execute(params![kind.as_str(), id])?;
    Ok(())
}

/// Recomputes every item's uses (after import, schema upgrade, *Rebuild index*).
pub(crate) fn rebuild(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM glossary_uses", [])?;
    let mut g = Glossary::load(conn)?;
    if g.is_empty() {
        return Ok(());
    }
    for (kind, table) in [(SearchKind::Page, "pages"), (SearchKind::Task, "tasks")] {
        let ids = conn
            .prepare(&format!("SELECT id FROM {table}"))?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for id in ids {
            index_item_with(conn, &mut g, kind, &id)?;
        }
    }
    Ok(())
}

/// Saves of a page or task, in the order the glossary needs: uses first (the keyword and vector
/// index read them), then the keyword row, then the pieces. Called from `hooks`.
pub(crate) fn reindex(conn: &Connection, kind: SearchKind, id: &str) -> Result<()> {
    reindex_with(conn, &mut Glossary::load(conn)?, kind, id)
}

fn reindex_with(conn: &Connection, g: &mut Glossary, kind: SearchKind, id: &str) -> Result<()> {
    index_item_with(conn, g, kind, id)?;
    match kind {
        SearchKind::Page => search::index_page(conn, id)?,
        SearchKind::Task => search::index_task(conn, id)?,
        SearchKind::Term => {}
    }
    vectors::index_item_with(conn, g, kind, id)
}

/// The terms an item uses, as indexed (FR-RAG-IX-3), sorted by term. Read by the keyword index.
pub(crate) fn used_lines(conn: &Connection, kind: SearchKind, id: &str) -> Result<Vec<String>> {
    let rows = conn
        .prepare_cached(
            "SELECT e.term, e.other_names, e.description FROM glossary_uses u
             JOIN glossary_entries e ON e.id = u.entry_id
             WHERE u.kind = ?1 AND u.ref_id = ?2
             ORDER BY e.term COLLATE NOCASE, e.id",
        )?
        .query_map(params![kind.as_str(), id], |r| {
            let names: String = r.get(1)?;
            Ok(TermText {
                term: r.get(0)?,
                other_names: serde_json::from_str(&names).unwrap_or_default(),
                description: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows.iter().map(TermText::line).collect())
}

/// What the keyword and vector index hold for one entry: (title, other names, description).
pub(crate) fn entry_parts(conn: &Connection, id: &str) -> Result<Option<(String, String, String)>> {
    Ok(conn
        .prepare_cached(
            "SELECT term, other_names, description FROM glossary_entries WHERE id = ?1",
        )?
        .query_row([id], |r| {
            let names: Vec<String> =
                serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or_default();
            Ok((r.get(0)?, names.join(", "), r.get(2)?))
        })
        .optional()?)
}

/// The entries that apply in `category_id` and appear in `text`, in first-match order, once each
/// (FR-RAG-IX-3: for a piece's embedded text and an item's keyword `extra`).
pub fn terms_in(conn: &Connection, category_id: &str, text: &str) -> Result<Vec<TermText>> {
    Ok(Glossary::load(conn)?.terms_in(category_id, text))
}

// ---------------------------------------------------------------- "Used in"

/// Characters shown on each side of the match (UI-KN-8).
const CONTEXT_CHARS: usize = 50;

/// Plain words of a Markdown fragment: link targets, emphasis, code and heading marks removed,
/// white space collapsed to single spaces (one kept at an end where there was white space).
///
/// `at_line_start`: the fragment starts at the beginning of a line (heading and quote marks there
/// are dropped); otherwise it starts mid-line.
fn plain(fragment: &str, at_line_start: bool) -> String {
    let stripped = search::strip_markdown(fragment);
    let mut marks_out = String::with_capacity(stripped.len());
    for (i, line) in stripped.split('\n').enumerate() {
        let line = if i == 0 && !at_line_start {
            line
        } else {
            line.trim_start().trim_start_matches(['#', '>'])
        };
        marks_out.extend(line.chars().map(|c| if c == '|' { ' ' } else { c }));
        marks_out.push('\n');
    }
    marks_out.pop();
    let marks_out: String = marks_out
        .chars()
        .filter(|c| !matches!(c, '*' | '`' | '~'))
        .collect();
    let mut out = marks_out.split_whitespace().collect::<Vec<_>>().join(" ");
    if marks_out.starts_with(char::is_whitespace) && !out.is_empty() {
        out.insert(0, ' ');
    }
    if marks_out.ends_with(char::is_whitespace) && !out.is_empty() {
        out.push(' ');
    }
    out
}

/// The last `n` characters of `s`; "…" in front if it was longer.
fn tail_chars(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.trim_start().to_string();
    }
    let cut: String = s.chars().skip(count - n).collect();
    format!("…{}", cut.trim_start())
}

/// The first `n` characters of `s`; "…" after it if it was longer.
fn head_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.trim_end().to_string();
    }
    let cut: String = s.chars().take(n).collect();
    format!("{}…", cut.trim_end())
}

/// Windows of raw text read around a match before cleaning (bigger than what is shown, because
/// cleaning shortens it).
const WINDOW_BYTES: usize = 400;

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Excerpt (before, matched, after) around byte range `start..end` of `text`.
fn excerpt(text: &str, start: usize, end: usize) -> (String, String, String) {
    let from = floor_boundary(text, start.saturating_sub(WINDOW_BYTES));
    let to = ceil_boundary(text, (end + WINDOW_BYTES).min(text.len()));
    let before = plain(
        &text[from..start],
        from == 0 || text.as_bytes()[from - 1] == b'\n',
    );
    let after = plain(&text[end..to], false);
    let matched = text[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let before = if from > 0 && before.chars().count() <= CONTEXT_CHARS {
        format!("…{}", before.trim_start())
    } else {
        tail_chars(&before, CONTEXT_CHARS)
    };
    let after = if to < text.len() && after.chars().count() <= CONTEXT_CHARS {
        format!("{}…", after.trim_end())
    } else {
        head_chars(&after, CONTEXT_CHARS)
    };
    (before, matched, after)
}

/// "Used in" (UI-KN-8): `limit` items from `offset`, with excerpts around the first match.
pub fn usage(conn: &Connection, id: &str, offset: u32, limit: u32) -> Result<GlossaryUsage> {
    load_entry(conn, id)?; // NotFound for a missing entry
    const FROM: &str = "FROM glossary_uses u
         LEFT JOIN pages p ON u.kind = 'page' AND p.id = u.ref_id
         LEFT JOIN tasks t ON u.kind = 'task' AND t.id = u.ref_id
         WHERE u.entry_id = ?1 AND (p.id IS NOT NULL OR t.id IS NOT NULL)";
    let total: u32 = conn.query_row(&format!("SELECT COUNT(*) {FROM}"), [id], |r| r.get(0))?;
    let rows = conn
        .prepare(&format!(
            "SELECT u.kind, u.ref_id, COALESCE(p.title, t.title), t.key,
                    COALESCE(p.category_id, t.category_id) {FROM}
             ORDER BY u.kind = 'task', COALESCE(p.updated_at, t.updated_at) DESC, u.ref_id
             LIMIT ?2 OFFSET ?3"
        ))?
        .query_map(params![id, limit, offset], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut g = Glossary::load(conn)?;
    let mut items = Vec::with_capacity(rows.len());
    for (kind, ref_id, title, key, category_id) in rows {
        let kind = if kind == "page" {
            SearchKind::Page
        } else {
            SearchKind::Task
        };
        let (mut before, mut matched, mut after) = Default::default();
        if let Some((category, text)) = item_text(conn, kind, &ref_id)?
            && let Some((_, f)) = g
                .scope(&category)
                .find(&text)
                .into_iter()
                .find(|(e, _)| e.id == id)
        {
            (before, matched, after) = excerpt(&text, f.start, f.end);
        }
        items.push(UsageHit {
            kind,
            id: ref_id,
            title,
            key,
            category_id,
            before,
            matched,
            after,
        });
    }
    Ok(GlossaryUsage { total, items })
}

// ---------------------------------------------------------------- the glossary in prompts (M5)

/// One entry as an LLM prompt receives it (FR-LLM-AS-8), with its scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptEntry {
    pub term: String,
    pub other_names: Vec<String>,
    pub description: String,
    /// `None` = global; otherwise the name of the entry's category.
    pub category_name: Option<String>,
}

/// The entries for a prompt about text of category `category_id` (FR-LLM-AS-8): those whose term or
/// other name occurs in any of `texts` (FR-KN-GL-4), in first-match order, then the entries
/// related to those, one step (FR-KN-GL-3), when they apply in the category too. Once each.
/// Format them with [`crate::prompts::glossary_lines`].
pub fn prompt_entries(conn: &Connection, category_id: &str, texts: &[&str]) -> Result<Vec<PromptEntry>> {
    let all = load_entries(conn)?;
    if all.is_empty() {
        return Ok(Vec::new());
    }
    let mut glossary = Glossary::new(all.clone());
    let scope = glossary.scope(category_id);
    let mut seen: HashSet<String> = HashSet::new();
    let mut picked: Vec<Rc<Entry>> = Vec::new();
    for text in texts {
        for (e, _) in scope.find(text) {
            if seen.insert(e.id.clone()) {
                picked.push(Rc::clone(e));
            }
        }
    }
    let related = related_map(&all);
    let applicable: HashMap<&str, &Rc<Entry>> =
        scope.entries.iter().map(|e| (e.id.as_str(), e)).collect();
    for hit in picked.clone() {
        let mut ids: Vec<&String> = related.get(&hit.id).into_iter().flatten().collect();
        ids.sort();
        for id in ids {
            if let Some(e) = applicable.get(id.as_str())
                && seen.insert(id.clone())
            {
                picked.push(Rc::clone(e));
            }
        }
    }
    let names: HashMap<String, String> = conn
        .prepare_cached("SELECT id, name FROM categories")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(picked
        .into_iter()
        .map(|e| PromptEntry {
            term: e.term.clone(),
            other_names: e.other_names.clone(),
            description: e.description.clone(),
            category_name: e
                .category_id
                .as_ref()
                .map(|p| names.get(p).cloned().unwrap_or_else(|| p.clone())),
        })
        .collect())
}

/// [`prompt_entries`] for text of several categories (the assistant's sources come from more than
/// one, FR-LLM-AS-8), plus the entries `term_ids` name (found by the search, FR-KN-GL-5) whatever
/// their scope. Entries are listed once each (an entry that applies in several of the categories
/// once), in the order the categories are given.
pub fn prompt_entries_in(
    conn: &Connection,
    category_ids: &[String],
    texts: &[&str],
    term_ids: &[String],
) -> Result<Vec<PromptEntry>> {
    let mut out: Vec<PromptEntry> = Vec::new();
    for category_id in category_ids {
        for e in prompt_entries(conn, category_id, texts)? {
            if !out.contains(&e) {
                out.push(e);
            }
        }
    }
    if !term_ids.is_empty() {
        let names: HashMap<String, String> = conn
            .prepare_cached("SELECT id, name FROM categories")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        for id in term_ids {
            let Ok(e) = get(conn, id) else { continue };
            let entry = PromptEntry {
                term: e.term,
                other_names: e.other_names,
                description: e.description,
                category_name: e
                    .category_id
                    .map(|p| names.get(&p).cloned().unwrap_or(p)),
            };
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
    }
    Ok(out)
}

/// Every name (term and other names) of the entries that apply in `category_id` (FR-KN-GL-2), for
/// the term filter of `checks`.
pub(crate) fn applicable_names(conn: &Connection, category_id: &str) -> Result<Vec<String>> {
    let all = load_entries(conn)?;
    Ok(applicable(&all, category_id)
        .iter()
        .flat_map(|e| e.names().cloned().collect::<Vec<_>>())
        .collect())
}

/// Whether any of `names` occurs in `text` by the FR-KN-GL-4 rules (whole words, capitals only in
/// capitals, plurals and codes, Chinese anywhere).
pub(crate) fn any_name_in(names: &[String], text: &str) -> bool {
    if names.is_empty() {
        return false;
    }
    let names: Vec<(usize, String)> = names.iter().cloned().enumerate().collect();
    !matcher::Matcher::new(&names).find(text).is_empty()
}

/// FR-KN-GL-7: *Ignore* on a suggested term; it is never suggested in `category_id` again.
pub fn ignore_term(db: &mut Db, category_id: &str, term: &str) -> Result<()> {
    let term = term.trim();
    if term.is_empty() {
        return Err(CoreError::Invalid("The term is empty.".into()));
    }
    db.conn().execute(
        "INSERT INTO glossary_ignored (category_id, term, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT DO NOTHING",
        params![category_id, term, now_iso()],
    )?;
    Ok(())
}

/// The terms ignored in `category_id` (FR-KN-GL-7), sorted.
pub fn ignored_terms(conn: &Connection, category_id: &str) -> Result<Vec<String>> {
    Ok(conn
        .prepare_cached("SELECT term FROM glossary_ignored WHERE category_id = ?1 ORDER BY term")?
        .query_map([category_id], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?)
}
