//! Helpers shared by the unit tests.

use rusqlite::Connection;

use crate::categories::{self, CategoryColor, NewCategory};

/// A blue category with no description; returns its id.
pub(crate) fn category(conn: &Connection, name: &str, key: &str) -> String {
    categories::create(
        conn,
        NewCategory {
            name: name.into(),
            key: key.into(),
            description: String::new(),
            color: CategoryColor::preset("blue"),
        },
    )
    .unwrap()
    .id
}
