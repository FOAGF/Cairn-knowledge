//! Cairn core: everything that touches the data (docs/development.md, "Layout").
//!
//! The core is plain synchronous Rust with no Tauri dependency, so it can be tested
//! with `cargo test`. The Tauri layer calls it on a blocking thread (spec TC-15).

pub mod db;
pub mod error;
pub mod migrations;
pub mod paths;
pub mod util;

// Feature areas.
pub mod assistant;
pub mod attachments;
pub mod backup;
pub mod bulk;
pub mod checks;
pub mod chunking;
pub mod conversations;
pub mod drafts;
pub mod export;
pub mod glossary;
pub mod home;
pub mod hooks;
pub mod hybrid;
pub mod knowledge;
pub mod lint;
pub mod mentions;
pub mod merging;
pub mod categories;
pub mod prompts;
pub mod providers;
pub mod replies;
pub mod search;
pub mod search_model;
pub mod secrets;
pub mod settings;
pub mod sources;
pub mod startup;
pub mod tasks;
pub mod vectors;

pub use db::Db;
pub use error::{CoreError, Result};
pub use paths::DataPaths;
