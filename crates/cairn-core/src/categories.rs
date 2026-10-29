//! Categories (spec §3, DM-1..4, DM-6, DM-7; FR-PRJ-*).
//!
//! Rules chosen for v1:
//! - A task key is 2-6 characters, `A-Z` and `0-9`, starting with a letter. It is stored in upper case
//!   (input is trimmed and upper-cased), is unique ignoring case, and never changes (FR-PRJ-6).
//! - Categories are archived, never deleted (DM-6); the built-in General category (DM-4) cannot be archived.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{CoreError, Result};
use crate::util::{new_id, now_iso};

pub const KEY_MIN_LEN: usize = 2;
pub const KEY_MAX_LEN: usize = 6;

/// A category's colour (FR-PRJ-6, D-40): one of the preset names below, or any custom colour
/// written as `#rrggbb`. The UI shows presets with the theme's tuned shades and derives the
/// shades of a custom colour from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct CategoryColor(String);

impl CategoryColor {
    /// The preset colours, in the order the colour picker shows them.
    pub const PRESETS: [&'static str; 6] = ["blue", "green", "purple", "amber", "rose", "slate"];

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A preset by name (for code and tests). Unknown names give blue.
    pub fn preset(name: &str) -> CategoryColor {
        CategoryColor::parse(name)
    }

    /// A value read from the database; anything unrecognised falls back to blue.
    fn parse(s: &str) -> CategoryColor {
        CategoryColor::checked(s).unwrap_or_default()
    }

    /// A preset name or `#rgb` / `#rrggbb` (normalised to lowercase `#rrggbb`), else `None`.
    fn checked(s: &str) -> Option<CategoryColor> {
        let s = s.trim().to_ascii_lowercase();
        if CategoryColor::PRESETS.contains(&s.as_str()) {
            return Some(CategoryColor(s));
        }
        let hex = s.strip_prefix('#')?;
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        match hex.len() {
            6 => Some(CategoryColor(s)),
            3 => Some(CategoryColor(format!(
                "#{}",
                hex.chars().flat_map(|c| [c, c]).collect::<String>()
            ))),
            _ => None,
        }
    }

    /// Checks a colour the user chose.
    fn validated(&self) -> Result<CategoryColor> {
        CategoryColor::checked(&self.0).ok_or_else(|| {
            CoreError::Invalid(format!(
                "“{}” is not a colour. Pick one of the colours or enter a hex colour such as #3b82f6.",
                self.0
            ))
        })
    }
}

impl Default for CategoryColor {
    fn default() -> Self {
        CategoryColor("blue".into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum CategoryStatus {
    Active,
    Archived,
}

impl CategoryStatus {
    fn as_str(self) -> &'static str {
        match self {
            CategoryStatus::Active => "active",
            CategoryStatus::Archived => "archived",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    /// Task-key prefix, e.g. `PRX` for `PRX-12`.
    pub key: String,
    /// Short description, given to the LLM as context (DM-7).
    pub description: String,
    pub color: CategoryColor,
    pub status: CategoryStatus,
    pub is_general: bool,
    pub created_at: String,
    pub updated_at: String,
    /// External mode (FR-KN-EXT-5): the category's folder in the root once it is made or found;
    /// `None` until then, and in internal mode.
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewCategory {
    pub name: String,
    pub key: String,
    #[serde(default)]
    pub description: String,
    pub color: CategoryColor,
}

/// Changes to name, description and colour. The key is deliberately not here (FR-PRJ-6).
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CategoryChanges {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<CategoryColor>,
}

const COLUMNS: &str =
    "id, name, key, description, color, status, is_general, created_at, updated_at, folder";

fn from_row(r: &Row) -> rusqlite::Result<Category> {
    let color: String = r.get(4)?;
    let status: String = r.get(5)?;
    Ok(Category {
        id: r.get(0)?,
        name: r.get(1)?,
        key: r.get(2)?,
        description: r.get(3)?,
        color: CategoryColor::parse(&color),
        status: if status == "archived" {
            CategoryStatus::Archived
        } else {
            CategoryStatus::Active
        },
        is_general: r.get::<_, i64>(6)? != 0,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        folder: r.get(9)?,
    })
}

/// Categories ordered by creation (General first). Archived ones only on request.
pub fn list(conn: &Connection, include_archived: bool) -> Result<Vec<Category>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM categories {} ORDER BY is_general DESC, created_at, name",
        if include_archived {
            ""
        } else {
            "WHERE status = 'active'"
        }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: &str) -> Result<Category> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM categories WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| CoreError::NotFound {
        entity: "category",
        id: id.to_string(),
    })
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::Invalid("Give the category a name.".into()));
    }
    Ok(name.to_string())
}

/// Trims, upper-cases and validates a task key (FR-PRJ-6).
pub fn normalize_key(key: &str) -> Result<String> {
    let key = key.trim().to_uppercase();
    let ok_len = (KEY_MIN_LEN..=KEY_MAX_LEN).contains(&key.chars().count());
    let ok_chars = key
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && key.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    if !ok_len || !ok_chars {
        return Err(CoreError::Invalid(format!(
            "The task key must be {KEY_MIN_LEN}-{KEY_MAX_LEN} letters or digits and start with a letter (for example PRX)."
        )));
    }
    Ok(key)
}

fn key_taken(conn: &Connection, key: &str) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM categories WHERE key = ?1", [key], |_| Ok(()))
        .optional()?
        .is_some())
}

pub fn create(conn: &Connection, input: NewCategory) -> Result<Category> {
    let name = clean_name(&input.name)?;
    let key = normalize_key(&input.key)?;
    if key_taken(conn, &key)? {
        return Err(CoreError::Conflict(format!(
            "The task key {key} is already used by another category. Choose a different key."
        )));
    }
    let id = new_id();
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id, name, key, description, color, status, is_general, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'active', 0, ?6, ?6)",
        params![id, name, key, input.description.trim(), input.color.validated()?.as_str(), now],
    )?;
    get(conn, &id)
}

/// Changes name, description and/or colour (DM-6, DM-7). Works on archived categories too.
/// External mode (FR-KN-EXT-5): a new name renames the category's folder too, when it has one.
pub fn update(conn: &Connection, id: &str, changes: CategoryChanges) -> Result<Category> {
    let current = get(conn, id)?;
    let name = match changes.name {
        Some(n) => clean_name(&n)?,
        None => current.name.clone(),
    };
    if name != current.name
        && let Some(ext) = crate::knowledge::external::active(conn)?
    {
        crate::knowledge::external::store::rename_category(conn, &ext, id, &name)?;
    }
    let description = changes
        .description
        .map(|d| d.trim().to_string())
        .unwrap_or(current.description);
    let color = match changes.color {
        Some(c) => c.validated()?,
        None => current.color,
    };
    conn.execute(
        "UPDATE categories SET name = ?2, description = ?3, color = ?4, updated_at = ?5 WHERE id = ?1",
        params![id, name, description, color.as_str(), now_iso()],
    )?;
    get(conn, id)
}

/// Archive or restore a category (DM-6). General cannot be archived (DM-4).
pub fn set_archived(conn: &Connection, id: &str, archived: bool) -> Result<Category> {
    let current = get(conn, id)?;
    if archived && current.is_general {
        return Err(CoreError::Invalid(
            "The General category cannot be archived.".into(),
        ));
    }
    let status = if archived {
        CategoryStatus::Archived
    } else {
        CategoryStatus::Active
    };
    conn.execute(
        "UPDATE categories SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status.as_str(), now_iso()],
    )?;
    get(conn, id)
}

/// Suggests a free task key for a category name (FR-PRJ-6): the initials of several words, or the
/// first letters of a single word. A number is added when the key is taken.
pub fn suggest_key(conn: &Connection, name: &str) -> Result<String> {
    let base = key_base(name);
    if !key_taken(conn, &base)? {
        return Ok(base);
    }
    for n in 2..1000u32 {
        let suffix = n.to_string();
        let keep = KEY_MAX_LEN - suffix.len();
        let candidate: String = base.chars().take(keep).collect::<String>() + &suffix;
        if !key_taken(conn, &candidate)? {
            return Ok(candidate);
        }
    }
    Ok(String::new())
}

fn key_base(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.starts_with(|c: char| c.is_ascii_alphabetic()))
        .map(|w| w.to_uppercase())
        .collect();
    let mut key: String = if words.len() >= 2 {
        words
            .iter()
            .filter_map(|w| w.chars().next())
            .take(4)
            .collect()
    } else {
        words
            .first()
            .map(|w| w.chars().take(3).collect())
            .unwrap_or_default()
    };
    // Top up a too-short key from the first word.
    if key.chars().count() < KEY_MIN_LEN {
        if let Some(first) = words.first() {
            key = first.chars().take(3).collect();
        }
    }
    // A key must start with a letter and have the minimum length.
    let key = key
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .to_string();
    if key.chars().count() < KEY_MIN_LEN {
        return "PRJ".into();
    }
    key
}

/// A sample category to insert in development builds (see [`seed_if_missing`]).
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CategorySeed {
    pub id: String,
    pub name: String,
    pub key: String,
    pub description: String,
    pub color: CategoryColor,
    pub status: CategoryStatus,
    pub created_at: String,
}

/// Inserts each category unless its id or key already exists. Never changes existing rows.
/// Returns how many were inserted. Used only for the dev-mode demo data. In external mode a seed
/// is also skipped when a category of that name exists already (one the folder's scan made), so
/// the demo data never adds a second "Learning" next to the folder's.
pub fn seed_if_missing(conn: &Connection, seeds: &[CategorySeed]) -> Result<usize> {
    let external = crate::knowledge::external::is_external(conn)?;
    let mut inserted = 0;
    for s in seeds {
        if external {
            let named = list(conn, true)?
                .iter()
                .any(|c| c.id != s.id && c.name.to_lowercase() == s.name.trim().to_lowercase());
            if named {
                continue;
            }
        }
        inserted += conn.execute(
            "INSERT OR IGNORE INTO categories (id, name, key, description, color, status, is_general, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7)",
            params![
                s.id,
                s.name,
                s.key.to_uppercase(),
                s.description,
                CategoryColor::parse(s.color.as_str()).as_str(),
                s.status.as_str(),
                s.created_at
            ],
        )?;
    }
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_at(dir.path()).unwrap();
        (dir, db)
    }

    fn new(name: &str, key: &str) -> NewCategory {
        NewCategory {
            name: name.into(),
            key: key.into(),
            description: String::new(),
            color: CategoryColor::preset("blue"),
        }
    }

    fn no_changes() -> CategoryChanges {
        CategoryChanges {
            name: None,
            description: None,
            color: None,
        }
    }

    #[test]
    fn general_exists_and_lists_first() {
        let (_d, db) = db();
        create(db.conn(), new("Alpha", "alp")).unwrap();
        let all = list(db.conn(), false).unwrap();
        assert_eq!(all[0].id, "p-general");
        assert!(all[0].is_general);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn create_normalises_and_validates() {
        let (_d, db) = db();
        let input = NewCategory {
            description: "  ctx ".into(),
            ..new("  Alpha ", " alp1 ")
        };
        let p = create(db.conn(), input).unwrap();
        assert_eq!(
            (p.name.as_str(), p.key.as_str(), p.description.as_str()),
            ("Alpha", "ALP1", "ctx")
        );
        assert_eq!(p.status, CategoryStatus::Active);
        assert!(!p.is_general);
        for bad in ["", "A", "1AB", "ABCDEFG", "AB-C", "AB C", "ÄBC"] {
            assert!(
                matches!(create(db.conn(), new("X", bad)), Err(CoreError::Invalid(_))),
                "{bad}"
            );
        }
        assert!(matches!(
            create(db.conn(), new("   ", "ZZZ")),
            Err(CoreError::Invalid(_))
        ));
    }

    #[test]
    fn duplicate_key_is_a_conflict_ignoring_case() {
        let (_d, db) = db();
        create(db.conn(), new("Alpha", "ALP")).unwrap();
        assert!(matches!(
            create(db.conn(), new("Other", "alp")),
            Err(CoreError::Conflict(_))
        ));
        assert!(matches!(
            create(db.conn(), new("Other", "gen")),
            Err(CoreError::Conflict(_))
        ));
    }

    /// D-40: any colour, not only the presets.
    #[test]
    fn custom_colours_are_accepted_and_normalised() {
        let (_d, db) = db();
        let mut input = new("Alpha", "ALP");
        input.color = CategoryColor("#E4572E".into());
        let p = create(db.conn(), input).unwrap();
        assert_eq!(p.color.as_str(), "#e4572e");
        let short = CategoryChanges {
            color: Some(CategoryColor("#AbC".into())),
            ..no_changes()
        };
        assert_eq!(
            update(db.conn(), &p.id, short).unwrap().color.as_str(),
            "#aabbcc"
        );
        let preset = CategoryChanges {
            color: Some(CategoryColor(" Rose ".into())),
            ..no_changes()
        };
        assert_eq!(
            update(db.conn(), &p.id, preset).unwrap().color.as_str(),
            "rose"
        );
        for bad in ["red", "#12345", "#ggg000", ""] {
            let changes = CategoryChanges {
                color: Some(CategoryColor(bad.into())),
                ..no_changes()
            };
            assert!(
                matches!(
                    update(db.conn(), &p.id, changes),
                    Err(CoreError::Invalid(_))
                ),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn update_changes_name_description_color_but_not_key() {
        let (_d, db) = db();
        let p = create(db.conn(), new("Alpha", "ALP")).unwrap();
        let changes = CategoryChanges {
            name: Some(" Beta ".into()),
            description: Some("d".into()),
            color: Some(CategoryColor::preset("rose")),
        };
        let u = update(db.conn(), &p.id, changes).unwrap();
        assert_eq!(
            (
                u.name.as_str(),
                u.description.as_str(),
                u.color,
                u.key.as_str()
            ),
            ("Beta", "d", CategoryColor::preset("rose"), "ALP")
        );
        assert_eq!(update(db.conn(), &p.id, no_changes()).unwrap().name, "Beta");
        let blank = CategoryChanges {
            name: Some(" ".into()),
            ..no_changes()
        };
        assert!(matches!(
            update(db.conn(), &p.id, blank),
            Err(CoreError::Invalid(_))
        ));
        assert!(matches!(
            update(db.conn(), "nope", no_changes()),
            Err(CoreError::NotFound { .. })
        ));
    }

    #[test]
    fn archive_and_restore_but_not_general() {
        let (_d, db) = db();
        let p = create(db.conn(), new("Alpha", "ALP")).unwrap();
        assert_eq!(
            set_archived(db.conn(), &p.id, true).unwrap().status,
            CategoryStatus::Archived
        );
        assert_eq!(list(db.conn(), false).unwrap().len(), 1);
        assert_eq!(list(db.conn(), true).unwrap().len(), 2);
        assert_eq!(
            set_archived(db.conn(), &p.id, false).unwrap().status,
            CategoryStatus::Active
        );
        assert!(matches!(
            set_archived(db.conn(), "p-general", true),
            Err(CoreError::Invalid(_))
        ));
        assert_eq!(
            get(db.conn(), "p-general").unwrap().status,
            CategoryStatus::Active
        );
    }

    #[test]
    fn suggested_keys() {
        let (_d, db) = db();
        let s = |n: &str| suggest_key(db.conn(), n).unwrap();
        assert_eq!(s("Data Pipeline"), "DP");
        assert_eq!(s("Customer Portal Redesign Phase Two"), "CPRP");
        assert_eq!(s("Sensors"), "SEN");
        assert_eq!(s("x"), "PRJ");
        assert_eq!(s("数据管道"), "PRJ");
        assert_eq!(s("  "), "PRJ");
        assert_eq!(s("2024 Plan"), "PLA");
        // General uses GEN, so a name that would give GEN gets a number.
        assert_eq!(s("General"), "GEN2");
        create(db.conn(), new("a", "GEN2")).unwrap();
        assert_eq!(s("General"), "GEN3");
        for n in ["Data Pipeline", "Sensors", "Web Application Server Cluster"] {
            assert!(normalize_key(&s(n)).is_ok());
        }
    }

    #[test]
    fn seed_inserts_missing_only() {
        let (_d, db) = db();
        let seed = |id: &str, key: &str, name: &str| CategorySeed {
            id: id.into(),
            name: name.into(),
            key: key.into(),
            description: String::new(),
            color: CategoryColor::preset("green"),
            status: CategoryStatus::Active,
            created_at: now_iso(),
        };
        let seeds = vec![
            seed("p-general", "GEN", "General!"),
            seed("p-a", "AAA", "A"),
            seed("p-b", "BBB", "B"),
        ];
        assert_eq!(seed_if_missing(db.conn(), &seeds).unwrap(), 2);
        let rename = CategoryChanges {
            name: Some("Renamed".into()),
            ..no_changes()
        };
        update(db.conn(), "p-a", rename).unwrap();
        assert_eq!(seed_if_missing(db.conn(), &seeds).unwrap(), 0);
        assert_eq!(get(db.conn(), "p-a").unwrap().name, "Renamed");
        assert_eq!(get(db.conn(), "p-general").unwrap().name, "General");
        // A different id with a key already in use is skipped, not an error.
        assert_eq!(
            seed_if_missing(db.conn(), &[seed("p-c", "AAA", "C")]).unwrap(),
            0
        );
    }
}
