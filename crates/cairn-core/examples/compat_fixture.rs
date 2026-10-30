//! A compatibility fixture (NFR-REL-6): a small data folder and its export, with rows in every
//! table this version keeps, all made through the core's own save paths.
//!
//! ```text
//! cargo run -p cairn-core --example compat_fixture -- <empty dir> [external]
//! ```
//!
//! Writes `<dir>/data/` (the data folder: `cairn.sqlite`, `attachments/`) and `<dir>/export.zip`.
//! The fixtures in `tests/fixtures/compat/` were made with this file at each version's own commit;
//! `tests/compat.rs` opens and imports them, and checks the rows made here.
//!
//! Knowledge is internal, so there are no nested groups, and the external-mode tables
//! (`removed_pages`, `removed_groups`) stay empty. With `external` (from 0.13.0, D-72), the
//! knowledge is in external mode in `<dir>/notes/`, with a nested group, a page written outside
//! Cairn, and a group folder deleted outside Cairn, whose data waits in those two tables.

use std::path::{Path, PathBuf};

use cairn_core::categories::{self, CategoryColor, NewCategory};
use cairn_core::conversations::{self, ChatRole, FileKind, NewConversation, NewMessage};
use cairn_core::glossary::{self, NewGlossaryEntry};
use cairn_core::knowledge::external::scan::{self, Scope};
use cairn_core::knowledge::{self, NewGroup, NewPage, PagePatch};
use cairn_core::providers::{self, AuthMode, NewModel, NewProvider, ProviderKind, RequestFormat};
use cairn_core::secrets::MemoryStore;
use cairn_core::settings::{self, SettingsPatch};
use cairn_core::tasks::{self, NewTask, Priority, TaskLinkType, TaskPatch, TaskStatus, TaskType};
use cairn_core::vectors::{self, IndexModel};
use cairn_core::{Db, attachments, drafts, export};

/// A 1×1 PNG, for a pasted image.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn main() -> cairn_core::Result<()> {
    let usage = || -> ! {
        eprintln!("usage: compat_fixture <empty dir> [external]");
        std::process::exit(2)
    };
    let out = std::path::absolute(PathBuf::from(
        std::env::args().nth(1).unwrap_or_else(|| usage()),
    ))?;
    let notes = match std::env::args().nth(2).as_deref() {
        None => None,
        Some("external") => Some(out.join("notes")),
        Some(_) => usage(),
    };
    let mut db = Db::open_at(&out.join("data"))?;
    match &notes {
        None => knowledge::mode::choose_mode(&mut db, settings::KnowledgeMode::Internal, None)?,
        Some(notes) => {
            std::fs::create_dir_all(notes)?;
            knowledge::mode::choose_mode(
                &mut db,
                settings::KnowledgeMode::External,
                Some(notes.to_string_lossy().as_ref()),
            )?
        }
    };
    settings::update(
        &mut db,
        SettingsPatch {
            user_name: Some("Fixture".into()),
            backup_enabled: Some(false),
            ..Default::default()
        },
    )?;

    // Providers: one without a key, one of the kind added last (no key is stored: FR-DATA-5).
    let secrets = MemoryStore::default();
    let local = providers::create_provider(
        &mut db,
        &secrets,
        NewProvider {
            name: "Local".into(),
            kind: ProviderKind::Local,
            endpoint: Some("http://127.0.0.1:5299/v1".into()),
            api_version: None,
            auth: AuthMode::None,
        },
    )?;
    providers::create_provider(
        &mut db,
        &secrets,
        NewProvider {
            name: "DeepSeek".into(),
            kind: ProviderKind::DeepSeek,
            endpoint: None,
            api_version: None,
            auth: AuthMode::ApiKey,
        },
    )?;
    let model = providers::create_model(
        &mut db,
        NewModel {
            provider_id: local.id,
            name: "fixture-model".into(),
            label: Some("Fixture model".into()),
            format: RequestFormat::ChatCompletions,
            vision: None,
            effort: None,
            context_size: None,
            web_search: None,
        },
    )?;
    settings::update(
        &mut db,
        SettingsPatch {
            default_model_id: Some(model.id.clone()),
            ..Default::default()
        },
    )?;

    // Categories: tasks live in CMP; one starts in OLD and moves, which leaves the key alias OLD-1.
    let category = |name: &str, key: &str| NewCategory {
        name: name.into(),
        key: key.into(),
        description: format!("{name}, for the compatibility fixture."),
        color: CategoryColor::preset(CategoryColor::PRESETS[0]),
    };
    let cmp = categories::create(db.conn(), category("Compat", "CMP"))?;
    let old = categories::create(db.conn(), category("Old", "OLD"))?;

    let group = knowledge::create_group(
        &mut db,
        NewGroup {
            category_id: cmp.id.clone(),
            name: "Fixture group".into(),
            description: Some("Pages of the fixture.".into()),
            parent_id: None,
        },
    )?;
    glossary::create(
        &mut db,
        NewGlossaryEntry {
            category_id: Some(cmp.id.clone()),
            term: "pipeline".into(),
            other_names: Some(vec!["data pipeline".into()]),
            description: Some("Moves data from sensors to storage.".into()),
        },
    )?;
    glossary::ignore_term(&mut db, &cmp.id, "sensor")?;

    // Tasks: CMP-1 with labels, a due date, a comment, a sub-task, a related task and a change.
    let task = |title: &str, category_id: &str| NewTask {
        category_id: category_id.into(),
        title: title.into(),
        ..Default::default()
    };
    let t1 = tasks::create(
        &mut db,
        NewTask {
            task_type: Some(TaskType::Feature),
            description: Some("Build the pipeline. See [[Fixture page]]. 中文描述。".into()),
            status: Some(TaskStatus::Todo),
            priority: Some(Priority::High),
            labels: Some(vec!["backend".into(), "数据".into()]),
            due_date: Some("2026-12-01".into()),
            ..task("Fixture task", &cmp.id)
        },
    )?;
    let t2 = tasks::create(&mut db, task("Sub-task", &cmp.id))?;
    let t3 = tasks::create(
        &mut db,
        NewTask {
            status: Some(TaskStatus::Done),
            ..task("Done task", &cmp.id)
        },
    )?;
    tasks::add_link(&mut db, TaskLinkType::Parent, &t1.id, &t2.id)?;
    tasks::add_link(&mut db, TaskLinkType::Relates, &t1.id, &t3.id)?;
    tasks::update(
        &mut db,
        &t1.id,
        TaskPatch {
            status: Some(TaskStatus::InProgress),
            ..Default::default()
        },
    )?;
    let moved = tasks::create(&mut db, task("Moved task", &old.id))?;
    tasks::move_to_category(&mut db, &moved.id, &cmp.id)?;
    tasks::add_comment(&mut db, &t1.id, "First comment, 第一条评论. Moved: #OLD-1.")?;

    // Pages: one with tags, a pasted image and mentions of a page and a task, changed once.
    let image = attachments::save(&mut db, PNG, "image/png")?;
    let p2 = knowledge::create_page(
        &mut db,
        NewPage {
            category_id: cmp.id.clone(),
            title: "Second page".into(),
            body: "The second page.".into(),
            ..Default::default()
        },
    )?;
    let body = |change: &str| {
        format!(
            "A page about the pipeline{change}, see [Second page](page:{}) and #CMP-1.\n\n中文段落。\n\n![dot](attachment:{})\n",
            p2.id, image.name
        )
    };
    let p1 = knowledge::create_page(
        &mut db,
        NewPage {
            group_id: Some(group.id.clone()),
            tags: Some(vec!["alpha".into(), "标签".into()]),
            ..NewPage {
                category_id: cmp.id.clone(),
                title: "Fixture page".into(),
                body: body(""),
                ..Default::default()
            }
        },
    )?;
    knowledge::update_page(
        &mut db,
        &p1.id,
        PagePatch {
            body: Some(body(", changed")),
            ..Default::default()
        },
    )?;
    knowledge::link_task_page(&mut db, &t1.id, &p1.id)?;
    drafts::save(
        db.conn(),
        "page",
        &p1.id,
        "Unsaved text of the fixture page.",
    )?;
    if let Some(notes) = &notes {
        external(&mut db, notes, &cmp.id, &group.id, &t1.id, &p1.id)?;
    }

    // Conversations: one with the model, effort, an answer and a file; one archived.
    let message = |role: ChatRole, text: &str, model_id: Option<String>| NewMessage {
        role,
        text: text.into(),
        model_id,
        model_label: None,
        effort: Some("high".into()),
        attachment: None,
        steps: None,
        draft: None,
        answer: None,
        find: None,
        proposal: None,
        extra: None,
    };
    let chat = conversations::create(
        &mut db,
        NewConversation {
            title: "Fixture conversation".into(),
            scope_category_ids: Some(vec![cmp.id.clone()]),
            model_id: Some(model.id.clone()),
            effort: Some("high".into()),
        },
    )?;
    let question = conversations::add_message(
        &mut db,
        &chat.id,
        message(ChatRole::User, "What is the pipeline?", None),
    )?;
    conversations::add_chat_file(
        &mut db,
        &chat.id,
        &question.id,
        "notes.txt",
        FileKind::Text,
        23,
        1,
        "Notes about the pipeline",
    )?;
    conversations::add_message(
        &mut db,
        &chat.id,
        message(
            ChatRole::Assistant,
            "It moves data from sensors to storage.",
            Some(model.id.clone()),
        ),
    )?;
    let archived = conversations::create(
        &mut db,
        NewConversation {
            title: "Archived conversation".into(),
            scope_category_ids: None,
            model_id: None,
            effort: None,
        },
    )?;
    conversations::archive(&mut db, &archived.id)?;

    // The search index's pieces get stand-in vectors, so vector_meta records a model.
    let conn = db.conn();
    let waiting: Vec<i64> = conn
        .prepare("SELECT id FROM chunks WHERE vector IS NULL")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for (i, id) in waiting.iter().enumerate() {
        let mut v = vec![0.0f32; 256];
        v[i % 256] = 1.0;
        conn.execute(
            "UPDATE chunks SET vector = ?1 WHERE id = ?2",
            rusqlite::params![vectors::to_blob(&v), id],
        )?;
    }
    vectors::ensure_model(
        conn,
        &IndexModel {
            id: "embeddinggemma-300m-q4".into(),
            dims: 256,
        },
    )?;

    export::export_to(&db, &out.join("export.zip"))?;

    // Row counts, to see that every table has rows.
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND sql NOT LIKE 'CREATE VIRTUAL%' AND name NOT LIKE 'search_index_%' ORDER BY name")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    println!("schema {version}");
    for t in tables {
        let n: i64 = conn.query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0))?;
        println!("{n:>4}  {t}");
    }
    Ok(())
}

/// External mode: a group inside the fixture group with a page, a page written outside Cairn
/// (with an Obsidian link and tags), and a group whose folder is deleted outside Cairn, with a
/// page linked to a task; the scan keeps their data in `removed_groups` and `removed_pages`.
fn external(
    db: &mut Db,
    notes: &Path,
    category_id: &str,
    group_id: &str,
    task_id: &str,
    page_id: &str,
) -> cairn_core::Result<()> {
    let group = |name: &str, parent_id: Option<&str>| NewGroup {
        category_id: category_id.into(),
        name: name.into(),
        description: Some(format!("{name}, for the compatibility fixture.")),
        parent_id: parent_id.map(Into::into),
    };
    let page = |title: &str, body: String, group_id: &str| NewPage {
        category_id: category_id.into(),
        title: title.into(),
        body,
        group_id: Some(group_id.into()),
        ..Default::default()
    };
    let path_of = |db: &Db, id: &str| -> cairn_core::Result<String> {
        Ok(db
            .conn()
            .query_row("SELECT path FROM groups WHERE id = ?1", [id], |r| r.get(0))?)
    };

    let inner = knowledge::create_group(db, group("Inner group", Some(group_id)))?;
    knowledge::create_page(
        db,
        page(
            "Nested page",
            format!("Two groups down, see [Fixture page](page:{page_id}).\n"),
            &inner.id,
        ),
    )?;
    let dir = path_of(db, group_id)?;
    std::fs::write(
        notes.join(&dir).join("From outside.md"),
        "---\ntags: [outside]\n---\nWritten outside Cairn, about [[Fixture page]] and #CMP-1.\n",
    )?;

    let removed = knowledge::create_group(db, group("Removed group", None))?;
    let gone = knowledge::create_page(
        db,
        page(
            "Removed page",
            "Deleted outside Cairn with its folder.\n".into(),
            &removed.id,
        ),
    )?;
    knowledge::link_task_page(db, task_id, &gone.id)?;
    std::fs::remove_dir_all(notes.join(path_of(db, &removed.id)?))?;

    let summary = scan::scan(db, Scope::All)?;
    println!(
        "scan: {} added, {} removed, {} groups removed",
        summary.added, summary.removed, summary.groups_removed
    );
    Ok(())
}
