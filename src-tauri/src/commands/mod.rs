//! Tauri commands, one module per area. Every command is `async` (runs off the UI thread)
//! and reaches the database through `AppState::with_db`.

pub mod actions;
pub mod app;
pub mod assistant;
pub mod backup;
pub mod checks;
pub mod clipboard;
pub mod conversations;
pub mod creation;
pub mod data;
pub mod files;
pub mod glossary;
pub mod home;
pub mod knowledge;
pub mod logs;
pub mod categories;
pub mod providers;
pub mod search;
pub mod search_model;
pub mod settings;
pub mod storage;
pub mod tasks;
