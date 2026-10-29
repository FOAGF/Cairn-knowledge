//! External knowledge mode (D-72) with real folders: the synthetic test fixture
//! (`tests/fixtures/external-tests`) copied into a temporary folder for each test.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::scan::{self, Scope};
use super::watch::{FolderWatcher, WatchBatch};
use super::{Ext, set_test_trash};
use crate::categories::{self, CategoryChanges};
use crate::knowledge::mode::{self, KnowledgeStorage};
use crate::knowledge::*;
use crate::settings::KnowledgeMode;
use crate::tasks::NewTask;
use crate::{CoreError, Db, export, tasks};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/external-tests");

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

struct Env {
    _data: tempfile::TempDir,
    folder: tempfile::TempDir,
    trash: PathBuf,
    db: Db,
}

impl Env {
    fn root(&self) -> &Path {
        self.folder.path()
    }

    fn file(&self, rel: &str) -> PathBuf {
        Ext::new(self.root()).abs(rel).unwrap()
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.file(rel)).unwrap()
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.file(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn scan(&mut self) -> scan::ScanSummary {
        scan::scan(&mut self.db, Scope::All).unwrap()
    }

    fn page_at(&self, path: &str) -> Page {
        let id: String = self
            .db
            .conn()
            .query_row("SELECT id FROM pages WHERE path = ?1", [path], |r| r.get(0))
            .unwrap_or_else(|_| panic!("no page at {path}"));
        get_page(self.db.conn(), &id).unwrap()
    }

    fn category(&self, name: &str) -> categories::Category {
        categories::list(self.db.conn(), true)
            .unwrap()
            .into_iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no category {name}"))
    }

    fn group(&self, path: &str) -> Group {
        groups(self.db.conn(), None)
            .unwrap()
            .into_iter()
            .find(|g| g.path.as_deref() == Some(path))
            .unwrap_or_else(|| panic!("no group {path}"))
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn trashed(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.trash)
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }
}

/// A fresh data folder in external mode on a copy of the sample folder, scanned.
fn sample() -> Env {
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    copy_dir(Path::new(SAMPLE), folder.path());
    let trash = data.path().join("test-trash");
    set_test_trash(&trash);
    let mut db = Db::open_at(data.path()).unwrap();
    mode::choose_mode(&mut db, KnowledgeMode::External, Some(&folder.path().to_string_lossy())).unwrap();
    let mut env = Env {
        _data: data,
        folder,
        trash,
        db,
    };
    env.scan();
    env
}

fn patch() -> PagePatch {
    PagePatch::default()
}

// ---------------------------------------------------------------- reading the folder

/// FR-KN-EXT-2, -3, -5, -6, -7: categories, nested groups, titles, front matter, both link styles,
/// images, a Chinese page; a non-UTF-8 file, hidden folders and image-only folders are left out.
#[test]
fn the_sample_folder_is_indexed() {
    let e = sample();
    let names: BTreeSet<String> = categories::list(e.db.conn(), true)
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, BTreeSet::from(["General".into(), "Learning".into(), "Operations".into()]));
    assert_eq!(e.category("Operations").folder.as_deref(), Some("Operations"));
    // General has no folder yet: its page is in the root.
    assert_eq!(e.category("General").folder, None);

    let group_paths: BTreeSet<String> = groups(e.db.conn(), None)
        .unwrap()
        .into_iter()
        .filter_map(|g| g.path)
        .collect();
    assert_eq!(
        group_paths,
        BTreeSet::from([
            "Operations/Hoses".to_string(),
            "Operations/Pumps".to_string(),
            "Operations/Pumps/Seals".to_string()
        ])
    );
    let pumps = e.group("Operations/Pumps");
    assert_eq!(e.group("Operations/Pumps/Seals").parent_id.as_deref(), Some(pumps.id.as_str()));
    assert_eq!(pumps.parent_id, None);

    assert_eq!(e.count("SELECT COUNT(*) FROM pages"), 8);
    let welcome = e.page_at("Welcome.md");
    assert_eq!((welcome.title.as_str(), welcome.category_id.as_str()), ("Welcome", "p-general"));
    assert_eq!((welcome.origin, welcome.check_status), (PageOrigin::Folder, CheckStatus::Checked));
    let check = e.page_at("Operations/Pumps/Pump check.md");
    let overview = e.page_at("Operations/Overview.md");
    assert_eq!(check.group_id.as_deref(), Some(pumps.id.as_str()));
    assert_eq!(check.tags, ["pumps", "safety"]);
    assert!(check.body.contains(&format!("[overview](page:{})", overview.id)), "{}", check.body);
    assert!(check.body.contains(&format!("[Welcome](page:{})", welcome.id)), "{}", check.body);
    assert!(check.body.contains("![Pump](folder:Operations/attachments/pump.png)"), "{}", check.body);
    assert!(check.body.contains("[not a link](../Overview.md)"), "code stays: {}", check.body);
    assert!(welcome.body.contains(&format!("[pump check](page:{})", check.id)));
    assert_eq!(overview.source_info.as_deref(), Some("Site handbook, 2026 edition"));
    let hoses = e.page_at("Operations/Hoses/Hose guide.markdown");
    assert_eq!(hoses.title, "Hose guide");
    assert!(hoses.body.contains(&format!("[pump check](page:{})", check.id)), "{}", hoses.body);
    let chinese = e.page_at("Learning/中文笔记.md");
    assert_eq!(chinese.tags, ["中文", "笔记"]);
    assert_eq!(chinese.source_info.as_deref(), Some("课堂笔记"));
    assert!(e.page_at("Learning/Old notes.md").archived);
    // Search finds folder pages like any page.
    let hits = crate::search::search(
        e.db.conn(),
        &crate::search::SearchQuery {
            text: "所有者".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    // Backlinks come from the index's page links.
    let back = crate::mentions::backlinks(e.db.conn(), MentionTarget::Page, &check.id).unwrap();
    assert!(back.len() >= 3, "{back:?}");
    // The non-UTF-8 file is reported, the hidden folder is not read at all.
    let job = scan::prepare(e.db.conn(), Scope::All).unwrap();
    let plan = job.read(&mut |_| {}).unwrap();
    assert_eq!(plan.summary().skipped.len(), 1);
    assert_eq!(plan.summary().skipped[0].path, "Learning/legacy-gbk.md");
    assert_eq!(plan.summary().skipped[0].reason, "The file is not UTF-8 text.");
}

/// FR-KN-EXT-6: a second scan reads nothing when nothing changed; changed files are read,
/// renames keep the page (FR-KN-EXT-4), deleted files go, new folders become categories.
#[test]
fn scans_pick_up_changes_and_renames_made_outside() {
    let mut e = sample();
    let again = e.scan();
    assert!(!again.changed_anything(), "{again:?}");

    let check = e.page_at("Operations/Pumps/Pump check.md");
    let task = tasks::create(
        &mut e.db,
        NewTask {
            category_id: check.category_id.clone(),
            title: "Service pump".into(),
            ..Default::default()
        },
    )
    .unwrap();
    link_task_page(&mut e.db, &task.id, &check.id).unwrap();
    set_check_status(&mut e.db, &check.id, CheckStatus::NotChecked).unwrap();

    // Renamed and moved outside Cairn, text unchanged: the same page.
    std::fs::rename(e.file("Operations/Pumps/Pump check.md"), e.file("Operations/Hoses/Pump inspection.md")).unwrap();
    // Edited outside Cairn.
    e.write("Learning/Rust ownership.md", "---\ntags: [rust, memory]\n---\n\nBorrowing rules.\n");
    // Deleted outside Cairn.
    std::fs::remove_file(e.file("Learning/Old notes.md")).unwrap();
    // A new folder with a page.
    e.write("Safety/Permits/Hot work.md", "# Hot work\n\nSee [the inspection](../../Operations/Hoses/Pump%20inspection.md).\n");
    let s = e.scan();
    assert_eq!((s.renamed, s.changed, s.removed, s.added, s.categories_added), (1, 1, 1, 1, 1), "{s:?}");

    let moved = e.page_at("Operations/Hoses/Pump inspection.md");
    assert_eq!(moved.id, check.id);
    assert_eq!(moved.title, "Pump inspection");
    assert_eq!(moved.group_id.as_deref(), Some(e.group("Operations/Hoses").id.as_str()));
    assert_eq!(moved.check_status, CheckStatus::NotChecked, "Cairn-only data is kept");
    assert_eq!(task_page_links(e.db.conn(), Some(&task.id), None).unwrap().len(), 1);
    let rust = e.page_at("Learning/Rust ownership.md");
    assert_eq!((rust.body.as_str(), rust.tags.clone()), ("Borrowing rules.\n", vec!["memory".to_string(), "rust".to_string()]));
    assert_eq!(versions(e.db.conn(), &rust.id).unwrap().len(), 1, "a change from the folder writes today's version");
    assert_eq!(e.count("SELECT COUNT(*) FROM pages WHERE title = 'Old notes'"), 0);
    let safety = e.category("Safety");
    assert_eq!((safety.folder.as_deref(), safety.key.as_str()), (Some("Safety"), "SAF"));
    let hot = e.page_at("Safety/Permits/Hot work.md");
    assert_eq!(hot.group_id.as_deref(), Some(e.group("Safety/Permits").id.as_str()));
    assert!(hot.body.contains(&format!("(page:{})", check.id)), "{}", hot.body);
    assert!(!e.scan().changed_anything());

    // A watcher batch names only a new folder: its category, groups and pages come in.
    e.write("Projects/Q4/Plan/Kick-off.md", "# Kick-off\n");
    let s = scan::scan(&mut e.db, Scope::Paths(vec!["Projects".into()])).unwrap();
    assert_eq!((s.categories_added, s.groups_added, s.added), (1, 2, 1), "{s:?}");
    assert_eq!(e.group("Projects/Q4/Plan").parent_id.as_deref(), Some(e.group("Projects/Q4").id.as_str()));
    // And a batch naming a removed file removes its page.
    std::fs::remove_file(e.file("Projects/Q4/Plan/Kick-off.md")).unwrap();
    let s = scan::scan(&mut e.db, Scope::Paths(vec!["Projects/Q4/Plan/Kick-off.md".into()])).unwrap();
    assert_eq!(s.removed, 1);
}

/// FR-KN-EXT-5: a category's folder renamed outside Cairn renames the category (most of its files
/// reappeared); a group's folder renamed keeps the group's id and description. A folder going
/// missing never deletes its category.
#[test]
fn folder_renames_outside_keep_categories_and_groups() {
    let mut e = sample();
    let learning = e.category("Learning");
    let hoses = e.group("Operations/Hoses");
    update_group(
        &mut e.db,
        &hoses.id,
        GroupPatch {
            description: Some("Hydraulic hoses".into()),
            ..Default::default()
        },
    )
    .unwrap();
    std::fs::rename(e.file("Learning"), e.file("Study")).unwrap();
    std::fs::rename(e.file("Operations/Hoses"), e.file("Operations/Hose care")).unwrap();
    let s = e.scan();
    assert_eq!(s.categories_added, 0, "{s:?}");
    let study = e.category("Study");
    assert_eq!((study.id.as_str(), study.folder.as_deref()), (learning.id.as_str(), Some("Study")));
    let care = e.group("Operations/Hose care");
    assert_eq!((care.id.as_str(), care.name.as_str(), care.description.as_str()), (hoses.id.as_str(), "Hose care", "Hydraulic hoses"));
    assert_eq!(e.page_at("Study/Rust ownership.md").category_id, learning.id);

    // Gone without a replacement: the category stays, its pages go.
    std::fs::remove_dir_all(e.file("Study")).unwrap();
    e.scan();
    assert_eq!(e.category("Study").id, learning.id);
    assert_eq!(e.count(&format!("SELECT COUNT(*) FROM pages WHERE category_id = '{}'", learning.id)), 0);
}

// ---------------------------------------------------------------- writing

/// FR-KN-EXT-3, -10, -15: pages made and changed in Cairn are files; titles follow Windows' file
/// name rules; names that clash ignoring case are refused; renames update links in other files.
#[test]
fn pages_made_in_cairn_are_files() {
    let mut e = sample();
    let ops = e.category("Operations");
    let pumps = e.group("Operations/Pumps");
    let check = e.page_at("Operations/Pumps/Pump check.md");
    let p = create_page(
        &mut e.db,
        NewPage {
            category_id: ops.id.clone(),
            group_id: Some(pumps.id.clone()),
            title: "Valves: daily?".into(),
            body: format!("Do the [check](page:{}) first.\n", check.id),
            tags: Some(vec!["valves".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p.title, "Valves- daily-");
    assert_eq!(p.path.as_deref(), Some("Operations/Pumps/Valves- daily-.md"));
    assert_eq!(e.read("Operations/Pumps/Valves- daily-.md"), "---\ntags: [valves]\n---\n\nDo the [check](Pump%20check.md) first.\n");
    assert_eq!(p.body, format!("Do the [check](page:{}) first.\n", check.id));
    assert_eq!(p.file_hash.as_deref(), Some(super::hash_bytes(e.read("Operations/Pumps/Valves- daily-.md").as_bytes()).as_str()));
    // Same name in the same folder, other case: refused; in another folder: fine.
    let clash = NewPage {
        category_id: ops.id.clone(),
        group_id: Some(pumps.id.clone()),
        title: "pump CHECK".into(),
        body: String::new(),
        ..Default::default()
    };
    assert!(matches!(create_page(&mut e.db, clash.clone()), Err(CoreError::Conflict(_))));
    create_page(&mut e.db, NewPage { group_id: None, ..clash }).unwrap();
    // It is nearer to the hose guide than the page the guide's `[[Pump check]]` found: that link
    // now names its page's place, so it still finds it.
    let hose = e.read("Operations/Hoses/Hose guide.markdown");
    assert!(hose.contains("[[Operations/Pumps/Pump check|pump check]].\r\n"), "{hose}");
    let hose_body = e.page_at("Operations/Hoses/Hose guide.markdown").body;
    assert!(hose_body.contains(&format!("[pump check](page:{})", check.id)), "{hose_body}");
    // A new ungrouped General page goes into a General folder.
    let g = create_page(
        &mut e.db,
        NewPage {
            category_id: "p-general".into(),
            title: "Inbox".into(),
            body: "x".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(g.path.as_deref(), Some("General/Inbox.md"));
    assert_eq!(e.category("General").folder.as_deref(), Some("General"));

    // Rename: the file follows, and the links to it in other files (standard and wiki).
    let renamed = update_page(&mut e.db, &check.id, PagePatch { title: Some("Pump test".into()), ..patch() }).unwrap();
    assert_eq!(renamed.path.as_deref(), Some("Operations/Pumps/Pump test.md"));
    assert!(!e.file("Operations/Pumps/Pump check.md").exists());
    let welcome = e.read("Welcome.md");
    assert!(welcome.contains("[pump check](Operations/Pumps/Pump%20test.md)"), "{welcome}");
    let hose = e.read("Operations/Hoses/Hose guide.markdown");
    assert!(hose.contains("[[Operations/Pumps/Pump test|pump check]]"), "{hose}");
    assert!(hose.contains("\r\n"), "line endings kept");
    let seal = e.read("Operations/Pumps/Seals/Seal replacement.md");
    assert_eq!(seal, "# Seal replacement\n\nBefore you start, do the [pump check](../Pump%20test.md).\n");
    // The renamed file's own body is untouched (only the name changed).
    assert!(e.read("Operations/Pumps/Pump test.md").contains("Back to the [overview](../Overview.md). Read [[Welcome]] first."));
    // The index still links by id, and nothing needs a rescan.
    assert!(!e.scan().changed_anything());

    // Move to another category: the file moves and its own relative links follow.
    let learning = e.category("Learning");
    let moved = move_page(&mut e.db, &check.id, &learning.id).unwrap();
    assert_eq!(moved.path.as_deref(), Some("Learning/Pump test.md"));
    let text = e.read("Learning/Pump test.md");
    assert!(text.contains("[overview](../Operations/Overview.md)"), "{text}");
    assert!(text.contains("![Pump](../Operations/attachments/pump.png)"), "{text}");
    assert!(e.read("Welcome.md").contains("(Learning/Pump%20test.md)"));
    assert!(!e.scan().changed_anything());

    // Archive: front matter only; the body stays byte for byte.
    let before = e.read("Learning/Pump test.md");
    set_archived(&mut e.db, &check.id, true).unwrap();
    let after = e.read("Learning/Pump test.md");
    assert_eq!(after, before.replacen("  - safety\n", "  - safety\narchived: true\n", 1));

    // Delete: to the trash, with the image in `attachments` no other page uses (FR-KN-EXT-12).
    delete_page(&mut e.db, &check.id).unwrap();
    assert!(!e.file("Learning/Pump test.md").exists());
    assert!(!e.file("Operations/attachments/pump.png").exists());
    let trashed = e.trashed();
    assert_eq!(trashed.len(), 2, "{trashed:?}");
    assert!(trashed.iter().any(|n| n.ends_with("Pump test.md")) && trashed.iter().any(|n| n.ends_with("pump.png")));
    assert!(!e.scan().changed_anything());
}

/// D-72 contract: a write keeps unknown front matter keys and the user's text; a body changed in
/// Cairn is written whole from the index (`[[Page]]` links become standard links).
#[test]
fn writing_keeps_the_users_file() {
    let mut e = sample();
    let overview = e.page_at("Operations/Overview.md");
    update_page(&mut e.db, &overview.id, PagePatch { tags: Some(vec!["operations".into(), "plant".into()]), ..patch() }).unwrap();
    assert_eq!(
        e.read("Operations/Overview.md"),
        "---\nowner: Kim # kept as written\ntags: [operations, plant]\nsource: \"Site handbook, 2026 edition\"\n---\n\n# Operations\n\nDaily routines for the plant. See [Pump check](Pumps/Pump%20check.md) and\n[seal replacement](Pumps/Seals/Seal%20replacement.md).\n"
    );
    let hoses = e.page_at("Operations/Hoses/Hose guide.markdown");
    let body = format!("{}\nMore.\n", hoses.body);
    update_page(&mut e.db, &hoses.id, PagePatch { body: Some(body), ..patch() }).unwrap();
    assert_eq!(
        e.read("Operations/Hoses/Hose guide.markdown"),
        "# Hose guide\r\n\r\nCheck the hoses after the [pump check](../Pumps/Pump%20check.md).\r\nSee also [Seal replacement](../Pumps/Seals/Seal%20replacement.md).\r\n\r\nMore.\r\n"
    );
}

/// FR-KN-EXT-11: *Save* with the hash from when editing started is refused when the file changed;
/// *Keep mine* saves without it; *Take the disk version* reads the file in.
#[test]
fn a_file_changed_while_editing_is_a_conflict() {
    let mut e = sample();
    let page = e.page_at("Learning/Rust ownership.md");
    let started = page.file_hash.clone();
    e.write("Learning/Rust ownership.md", "Changed in another editor.\n");
    let mine = PagePatch {
        body: Some("My text.\n".into()),
        base_hash: started.clone(),
        ..patch()
    };
    let err = update_page(&mut e.db, &page.id, mine.clone()).unwrap_err();
    assert_eq!(err.kind(), "changed_on_disk");
    assert_eq!(e.read("Learning/Rust ownership.md"), "Changed in another editor.\n");
    let disk = page_on_disk(&e.db, &page.id).unwrap().unwrap();
    assert_eq!(disk.body, "Changed in another editor.\n");
    assert_eq!(get_page(e.db.conn(), &page.id).unwrap().body, page.body, "the index is not changed by looking");
    // Without a hash, text from an older version never overwrites the outside edit either (a
    // history restore, a merge).
    let restore = PagePatch { base_hash: None, ..mine.clone() };
    assert_eq!(update_page(&mut e.db, &page.id, restore).unwrap_err().kind(), "changed_on_disk");
    // Keep mine: the hash of the disk version the user saw.
    let keep_mine = PagePatch { base_hash: disk.file_hash.clone(), ..mine };
    let kept = update_page(&mut e.db, &page.id, keep_mine).unwrap();
    assert_eq!(e.read("Learning/Rust ownership.md"), "My text.\n");
    assert_eq!(kept.tags, Vec::<String>::new(), "the disk version (no tags) was read in first");
    // Take the disk version.
    e.write("Learning/Rust ownership.md", "Disk wins.\n");
    let taken = reload_page(&mut e.db, &page.id).unwrap();
    assert_eq!(taken.body, "Disk wins.\n");
    // Another change (archive) after an outside edit reads the file first: nothing is lost.
    e.write("Learning/Rust ownership.md", "Edited outside again.\n");
    set_archived(&mut e.db, &page.id, true).unwrap();
    assert_eq!(e.read("Learning/Rust ownership.md"), "---\narchived: true\n---\n\nEdited outside again.\n");
}

/// FR-KN-EXT-2, D-72 contract "Folders": groups hold groups in external mode only; a group can
/// move into another group; deleting one moves its contents up, refused when a name clashes.
#[test]
fn groups_nest_and_move_as_folders() {
    let mut e = sample();
    let ops = e.category("Operations");
    let pumps = e.group("Operations/Pumps");
    let hoses = e.group("Operations/Hoses");
    let filters = create_group(
        &mut e.db,
        NewGroup {
            category_id: ops.id.clone(),
            name: "Filters".into(),
            description: Some("Oil filters".into()),
            parent_id: Some(pumps.id.clone()),
        },
    )
    .unwrap();
    assert_eq!(filters.path.as_deref(), Some("Operations/Pumps/Filters"));
    assert!(e.file("Operations/Pumps/Filters").is_dir());
    // An empty group made in Cairn stays after a scan.
    assert!(!e.scan().changed_anything());
    assert_eq!(e.group("Operations/Pumps/Filters").id, filters.id);
    // Paths for the LLM.
    let labels: Vec<String> = groups_with_paths(e.db.conn(), &ops.id).unwrap().into_iter().map(|g| g.name).collect();
    assert!(labels.contains(&"Pumps / Filters".to_string()), "{labels:?}");

    // Move Pumps into Hoses: the folder moves with its pages and groups; links follow.
    let moved = update_group(
        &mut e.db,
        &pumps.id,
        GroupPatch {
            parent_id: Some(hoses.id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(moved.path.as_deref(), Some("Operations/Hoses/Pumps"));
    assert!(e.file("Operations/Hoses/Pumps/Seals/Seal replacement.md").is_file());
    assert_eq!(e.page_at("Operations/Hoses/Pumps/Pump check.md").group_id.as_deref(), Some(pumps.id.as_str()));
    assert_eq!(e.group("Operations/Hoses/Pumps/Filters").id, filters.id);
    assert!(e.read("Welcome.md").contains("(Operations/Hoses/Pumps/Pump%20check.md)"));
    let check = e.read("Operations/Hoses/Pumps/Pump check.md");
    assert!(check.contains("[overview](../../Overview.md)"), "{check}");
    assert!(check.contains("![Pump](../../attachments/pump.png)"), "{check}");
    // Not into itself.
    let err = update_group(&mut e.db, &hoses.id, GroupPatch { parent_id: Some(filters.id.clone()), ..Default::default() }).unwrap_err();
    assert_eq!(err.kind(), "invalid");
    assert!(!e.scan().changed_anything());

    // Deleting Pumps: its page and groups move up into Hoses.
    delete_group(&mut e.db, &pumps.id).unwrap();
    assert!(!e.file("Operations/Hoses/Pumps").exists());
    let check = e.page_at("Operations/Hoses/Pump check.md");
    assert_eq!(check.group_id.as_deref(), Some(hoses.id.as_str()));
    assert_eq!(e.group("Operations/Hoses/Seals").parent_id.as_deref(), Some(hoses.id.as_str()));
    assert!(!e.scan().changed_anything());

    // A clash refuses the whole delete.
    e.write("Operations/Hose guide.md", "Another file with the guide's name.\n");
    e.scan();
    let err = delete_group(&mut e.db, &hoses.id).unwrap_err();
    assert_eq!(err.kind(), "conflict");
    assert!(err.to_string().contains("Hose guide"), "{err}");
    assert!(e.file("Operations/Hoses/Pump check.md").is_file());
}

/// FR-KN-GR-1: internal mode keeps one level.
#[test]
fn internal_mode_refuses_nested_groups() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Db::open_at(dir.path()).unwrap();
    let g = create_group(&mut db, NewGroup { category_id: "p-general".into(), name: "A".into(), ..Default::default() }).unwrap();
    assert_eq!((g.parent_id.as_deref(), g.path.as_deref()), (None, None));
    let nested = NewGroup {
        category_id: "p-general".into(),
        name: "B".into(),
        parent_id: Some(g.id.clone()),
        ..Default::default()
    };
    assert_eq!(create_group(&mut db, nested).unwrap_err().kind(), "invalid");
    let err = update_group(&mut db, &g.id, GroupPatch { parent_id: Some(g.id.clone()), ..Default::default() }).unwrap_err();
    assert_eq!(err.kind(), "invalid");
    assert_eq!(plan_bulk_change(db.conn(), &[], &PageBulkChange::Archive).unwrap(), Vec::<NameClash>::new());
}

/// FR-KN-EXT-10: a bulk move that would give two files one name in a folder is listed by the plan
/// and refused as a whole.
#[test]
fn a_bulk_move_with_a_name_clash_is_refused_as_a_whole() {
    let mut e = sample();
    e.write("Learning/Overview.md", "Learning's own overview.\n");
    e.scan();
    let ops = e.category("Operations");
    let pumps = e.group("Operations/Pumps");
    let a = e.page_at("Learning/Overview.md");
    let b = e.page_at("Operations/Overview.md");
    let rust = e.page_at("Learning/Rust ownership.md");
    let change = PageBulkChange::Move {
        category_id: ops.id.clone(),
        group_id: Some(pumps.id.clone()),
    };
    let ids = vec![a.id.clone(), b.id.clone(), rust.id.clone()];
    let clashes = plan_bulk_change(e.db.conn(), &ids, &change).unwrap();
    assert_eq!(clashes.len(), 1);
    assert_eq!((clashes[0].folder.as_str(), clashes[0].name.as_str()), ("Operations/Pumps", "Overview.md"));
    assert_eq!(clashes[0].page_ids.len(), 2);
    let err = update_many(&mut e.db, &ids, change.clone()).unwrap_err();
    assert_eq!(err.kind(), "conflict");
    assert!(e.file("Learning/Rust ownership.md").is_file(), "nothing moved");
    // A name taken by a file already in the target folder clashes too.
    e.write("Operations/Pumps/Rust ownership.md", "x\n");
    e.scan();
    assert_eq!(plan_bulk_change(e.db.conn(), &[rust.id.clone()], &change).unwrap().len(), 1);
    std::fs::remove_file(e.file("Operations/Pumps/Rust ownership.md")).unwrap();
    e.scan();
    // Without clashes, one move straight into the group.
    let out = update_many(&mut e.db, &[rust.id.clone(), b.id.clone()], change).unwrap();
    assert_eq!((out.done, out.skipped.len()), (2, 0));
    assert_eq!(get_page(e.db.conn(), &rust.id).unwrap().path.as_deref(), Some("Operations/Pumps/Rust ownership.md"));
    assert_eq!(get_page(e.db.conn(), &b.id).unwrap().path.as_deref(), Some("Operations/Pumps/Overview.md"));
    assert!(!e.scan().changed_anything());
}

/// FR-KN-EXT-3, -12: pasted images go into the category's `attachments` folder; one no page uses
/// any more is moved to the trash, but only inside `attachments`.
#[test]
fn images_live_in_attachments_folders() {
    let mut e = sample();
    let learning = e.category("Learning");
    let png = std::fs::read(e.file("Operations/attachments/pump.png")).unwrap();
    let img = save_image(&mut e.db, &learning.id, &png, "image/png").unwrap();
    assert!(img.target.starts_with("folder:Learning/attachments/") && img.target.ends_with(".png"), "{}", img.target);
    assert!(Path::new(&img.file).is_file());
    assert_eq!(save_image(&mut e.db, &learning.id, b"<svg/>", "image/svg+xml").unwrap_err().kind(), "invalid");
    let rust = e.page_at("Learning/Rust ownership.md");
    let body = format!("{}\n![diagram]({})\n![flow](folder:Operations/diagrams/flow.png)\n", rust.body, img.target);
    update_page(&mut e.db, &rust.id, PagePatch { body: Some(body), ..patch() }).unwrap();
    assert!(e.read("Learning/Rust ownership.md").contains("![diagram](attachments/"));
    assert!(e.read("Learning/Rust ownership.md").contains("![flow](../Operations/diagrams/flow.png)"));
    // No page (or version: today's is replaced) uses them any more: the image in `attachments`
    // goes to the trash, the other one is never touched.
    update_page(&mut e.db, &rust.id, PagePatch { body: Some("No images.\n".into()), ..patch() }).unwrap();
    assert!(!Path::new(&img.file).exists());
    assert_eq!(e.trashed().len(), 1);
    assert!(e.file("Operations/diagrams/flow.png").is_file());
    // An image from Cairn's data folder (internal mode, a task) is copied in when written.
    let att = crate::attachments::save(&mut e.db, &png, "image/png").unwrap();
    let p = create_page(
        &mut e.db,
        NewPage {
            category_id: learning.id.clone(),
            title: "From a task".into(),
            body: format!("![shot](attachment:{})\n", att.name),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p.body, format!("![shot](folder:Learning/attachments/{})\n", att.name));
    assert!(e.file(&format!("Learning/attachments/{}", att.name)).is_file());
}

// ---------------------------------------------------------------- the watcher

fn collect(rx: &mpsc::Receiver<(Instant, WatchBatch)>, quiet: Duration) -> Vec<(Instant, WatchBatch)> {
    let mut out = Vec::new();
    while let Ok(b) = rx.recv_timeout(quiet) {
        out.push(b);
    }
    out
}

/// FR-KN-EXT-6: a burst of real file writes arrives in batches at most once a second, with every
/// path in one of them.
#[test]
fn the_watcher_batches_a_burst_at_most_once_a_second() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let watcher = FolderWatcher::start(dir.path(), move |b| {
        let _ = tx.send((Instant::now(), b));
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    std::fs::create_dir_all(dir.path().join("Burst")).unwrap();
    for i in 0..300 {
        std::fs::write(dir.path().join(format!("Burst/note {i}.md")), format!("# {i}\n")).unwrap();
        std::fs::write(dir.path().join(".git-like"), b"x").unwrap();
        if i % 20 == 0 {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let burst = started.elapsed();
    let batches = collect(&rx, Duration::from_millis(2500));
    watcher.stop();
    assert!(burst > Duration::from_millis(1000), "the burst should span over a second: {burst:?}");
    assert!(batches.len() >= 2, "{} batches", batches.len());
    assert!(batches.len() as u128 <= burst.as_millis() / 1000 + 2, "{} batches in {burst:?}", batches.len());
    for w in batches.windows(2) {
        let gap = w[1].0 - w[0].0;
        assert!(gap >= Duration::from_millis(990), "batches {gap:?} apart");
    }
    // A new folder's first files may come only as the folder (a scan of it reads them).
    let seen: BTreeSet<&String> = batches.iter().flat_map(|(_, b)| b.paths.iter()).collect();
    for i in 0..300 {
        let note = format!("Burst/note {i}.md");
        assert!(seen.contains(&note) || seen.contains(&"Burst".to_string()), "{note} missing");
    }
    assert!(!seen.iter().any(|p| p.starts_with('.')), "hidden paths are ignored");
}

/// Cairn's own writes are recognised by their hash and cause no batch; an outside change to the
/// same file right after does.
#[test]
fn the_watcher_ignores_cairns_own_writes() {
    let mut e = sample();
    let (tx, rx) = mpsc::channel();
    let watcher = FolderWatcher::start(e.root(), move |b| {
        let _ = tx.send((Instant::now(), b));
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let page = e.page_at("Learning/Rust ownership.md");
    update_page(&mut e.db, &page.id, PagePatch { body: Some("Written by Cairn.\n".into()), ..patch() }).unwrap();
    let echoes = collect(&rx, Duration::from_millis(1500));
    assert!(
        echoes.iter().all(|(_, b)| !b.paths.contains(&"Learning/Rust ownership.md".to_string())),
        "{echoes:?}"
    );
    // A scan of the paths an echo would name changes nothing either.
    let s = scan::scan(&mut e.db, Scope::Paths(vec!["Learning/Rust ownership.md".into()])).unwrap();
    assert!(!s.changed_anything());
    e.write("Learning/Rust ownership.md", "Written by someone else.\n");
    let outside = collect(&rx, Duration::from_millis(1500));
    watcher.stop();
    assert!(outside.iter().any(|(_, b)| b.paths.contains(&"Learning/Rust ownership.md".to_string())), "{outside:?}");
    let s = scan::scan(&mut e.db, Scope::Paths(vec!["Learning/Rust ownership.md".into()])).unwrap();
    assert_eq!(s.changed, 1);
    assert_eq!(get_page(e.db.conn(), &page.id).unwrap().body, "Written by someone else.\n");
}

// ---------------------------------------------------------------- modes

/// FR-KN-EXT-7: the choice is made once; external needs a usable folder outside the data folder.
#[test]
fn the_mode_is_chosen_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Db::open_at(dir.path()).unwrap();
    assert_eq!(mode::storage(db.conn()).unwrap(), KnowledgeStorage { mode: None, root: None, root_exists: false, watch_error: None });
    let inside = dir.path().join("notes");
    std::fs::create_dir_all(&inside).unwrap();
    for bad in ["", "relative/notes", "/no/such/folder", &inside.to_string_lossy()] {
        assert_eq!(mode::choose_mode(&mut db, KnowledgeMode::External, Some(bad)).unwrap_err().kind(), "invalid", "{bad}");
    }
    let folder = tempfile::tempdir().unwrap();
    // Pages made before the choice (development's demo data) keep it internal.
    let p = create_page(&mut db, NewPage { category_id: "p-general".into(), title: "Early".into(), body: String::new(), ..Default::default() }).unwrap();
    assert_eq!(mode::choose_mode(&mut db, KnowledgeMode::External, Some(&folder.path().to_string_lossy())).unwrap_err().kind(), "invalid");
    delete_page(&mut db, &p.id).unwrap();
    let s = mode::choose_mode(&mut db, KnowledgeMode::External, Some(&folder.path().to_string_lossy())).unwrap();
    assert_eq!((s.mode, s.root_exists), (Some(KnowledgeMode::External), true));
    assert!(mode::choose_mode(&mut db, KnowledgeMode::Internal, None).is_err());
    // Demo data and the Markdown folder import are for internal mode.
    seed_if_missing(&mut db, KnowledgeSeed::default()).unwrap();
    let import = FolderImportInput {
        folder: folder.path().to_string_lossy().into_owned(),
        category_id: "p-general".into(),
        ..Default::default()
    };
    assert_eq!(plan_folder_import(&db, &import).unwrap_err().kind(), "invalid");
    // The development preset only acts on a database without a mode.
    let other = tempfile::tempdir().unwrap();
    let mut fresh = Db::open_at(other.path()).unwrap();
    assert!(mode::dev_preset(&mut fresh, "internal").unwrap());
    assert!(!mode::dev_preset(&mut fresh, "internal").unwrap());
    assert_eq!(mode::storage(fresh.conn()).unwrap().mode, Some(KnowledgeMode::Internal));
    let third = tempfile::tempdir().unwrap();
    let mut fresh = Db::open_at(third.path()).unwrap();
    assert!(mode::dev_preset(&mut fresh, "nonsense").is_err());
    let target = folder.path().join("dev-notes");
    assert!(mode::dev_preset(&mut fresh, &format!("external={}", target.display())).unwrap());
    assert!(target.is_dir());
    // `external=sample`: a copy of the sample folder next to the data folder.
    let fourth = tempfile::tempdir().unwrap();
    let data = fourth.path().join("data");
    let mut fresh = Db::open_at(&data).unwrap();
    assert!(mode::dev_preset(&mut fresh, "external=sample").unwrap());
    let root = mode::storage(fresh.conn()).unwrap().root.unwrap();
    assert_eq!(PathBuf::from(&root), fourth.path().join("data-sample-knowledge"));
    assert!(Path::new(&root).join("Operations/Pumps/Pump check.md").is_file());
}

/// FR-KN-EXT-8: switching both ways, with kept switch backups; going back to a folder restores
/// its Cairn-only data by path; going internal loads the knowledge of a chosen backup.
#[test]
fn switching_both_ways_keeps_what_belongs_to_each_mode() {
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    copy_dir(Path::new(SAMPLE), folder.path());
    set_test_trash(&data.path().join("test-trash"));
    let mut db = Db::open_at(data.path()).unwrap();
    mode::choose_mode(&mut db, KnowledgeMode::Internal, None).unwrap();
    let task = tasks::create(&mut db, NewTask { category_id: "p-general".into(), title: "Write docs".into(), ..Default::default() }).unwrap();
    let internal = create_page(
        &mut db,
        NewPage {
            category_id: "p-general".into(),
            title: "Internal page".into(),
            body: "Kept in Cairn.".into(),
            ..Default::default()
        },
    )
    .unwrap();
    link_task_page(&mut db, &task.id, &internal.id).unwrap();
    let root = folder.path().to_string_lossy().into_owned();

    // Internal → external.
    let r = mode::switch_to_external(&mut db, &root).unwrap();
    assert_eq!((r.backup.mode, r.backup.pages, r.restored), (KnowledgeMode::Internal, 1, 0));
    assert!(r.backup.file_name.ends_with("-internal-1 page.zip"), "{}", r.backup.file_name);
    assert_eq!(r.scan.as_ref().unwrap().added, 8);
    assert!(get_page(db.conn(), &internal.id).is_err(), "the internal pages are packed away");
    assert_eq!(tasks::get(db.conn(), &task.id).unwrap().title, "Write docs", "tasks stay");
    assert!(task_page_links(db.conn(), Some(&task.id), None).unwrap().is_empty());
    let first_backup = r.backup.file_name.clone();
    // Cairn-only data on a folder page.
    let check_id: String = db.conn().query_row("SELECT id FROM pages WHERE path = 'Operations/Pumps/Pump check.md'", [], |r| r.get(0)).unwrap();
    set_check_status(&mut db, &check_id, CheckStatus::NotChecked).unwrap();
    link_task_page(&mut db, &task.id, &check_id).unwrap();
    // Every page waits to be embedded.
    let waiting: i64 = db.conn().query_row("SELECT COUNT(*) FROM chunks WHERE kind = 'page' AND vector IS NOT NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(waiting, 0);

    // External → internal, loading the first switch backup's knowledge.
    let r = mode::switch_to_internal(&mut db, Some(&first_backup)).unwrap();
    assert_eq!((r.backup.mode, r.backup.root.as_deref(), r.restored), (KnowledgeMode::External, Some(root.as_str()), 1));
    let back = get_page(db.conn(), &internal.id).unwrap();
    assert_eq!((back.title.as_str(), back.path.as_deref()), ("Internal page", None));
    assert_eq!(task_page_links(db.conn(), Some(&task.id), None).unwrap().len(), 1);
    assert!(folder.path().join("Operations/Pumps/Pump check.md").is_file(), "switching never changes the folder");
    assert_eq!(mode::storage(db.conn()).unwrap().mode, Some(KnowledgeMode::Internal));

    // Back to the same folder: its index comes back from its newest switch backup; a file changed
    // meanwhile is read again and keeps its page.
    std::fs::write(folder.path().join("Operations/Pumps/Pump check.md"), "Changed while internal.\n").unwrap();
    let r = mode::switch_to_external(&mut db, &root).unwrap();
    assert_eq!(r.restored, 8);
    let scanned = r.scan.unwrap();
    assert_eq!((scanned.changed, scanned.added, scanned.removed), (1, 0, 0), "{scanned:?}");
    let check = get_page(db.conn(), &check_id).unwrap();
    assert_eq!(check.check_status, CheckStatus::NotChecked);
    assert_eq!(check.body, "Changed while internal.\n");
    assert_eq!(task_page_links(db.conn(), Some(&task.id), Some(&check_id)).unwrap().len(), 1);

    // To internal with no pages.
    let r = mode::switch_to_internal(&mut db, None).unwrap();
    assert_eq!(r.restored, 0);
    let pages: i64 = db.conn().query_row("SELECT COUNT(*) FROM pages", [], |r| r.get(0)).unwrap();
    assert_eq!(pages, 0);

    // The switch backups: listed newest first; of the folder only the newest is kept (the two
    // internal ones stay); deleted on request.
    let list = mode::switch_backups(&db).unwrap();
    assert_eq!(list.backups.len(), 3);
    assert_eq!(list.backups.iter().filter(|b| b.mode == KnowledgeMode::Internal).count(), 2);
    assert!(list.backups.iter().all(|b| b.knowledge_only));
    assert_eq!(list.backups.iter().map(|b| b.size).sum::<u64>(), list.total_size);
    assert!(!list.over_warning);
    crate::settings::update(&mut db, crate::settings::SettingsPatch { switch_backup_warn_gb: Some(0.000_000_1), ..Default::default() }).unwrap();
    assert!(mode::switch_backups(&db).unwrap().over_warning);
    mode::delete_switch_backup(&db, &first_backup).unwrap();
    assert_eq!(mode::switch_backups(&db).unwrap().backups.len(), 2);
    assert_eq!(mode::delete_switch_backup(&db, "../cairn.sqlite").unwrap_err().kind(), "not_found");
}

/// The owner's choice: going internal can't load a switch backup made in a folder (external
/// mode); refused before anything is backed up or cleared.
#[test]
fn an_external_backup_is_refused_when_going_internal() {
    let mut e = sample();
    let root = e.root().to_string_lossy().into_owned();
    let external_backup = mode::switch_to_internal(&mut e.db, None).unwrap().backup.file_name;
    mode::switch_to_external(&mut e.db, &root).unwrap();
    let pages = e.count("SELECT COUNT(*) FROM pages");
    let backups = mode::switch_backups(&e.db).unwrap().backups.len();
    let err = mode::switch_to_internal(&mut e.db, Some(&external_backup)).unwrap_err();
    assert_eq!(err.kind(), "invalid");
    assert_eq!(err.to_string(), mode::EXTERNAL_BACKUP_REFUSED);
    assert_eq!(mode::check_internal_backup(&e.db, &external_backup).unwrap_err().to_string(), mode::EXTERNAL_BACKUP_REFUSED);
    // Nothing happened: still in the folder, the same pages, no new backup.
    assert_eq!(mode::storage(e.db.conn()).unwrap().mode, Some(KnowledgeMode::External));
    assert_eq!(e.count("SELECT COUNT(*) FROM pages"), pages);
    assert_eq!(mode::switch_backups(&e.db).unwrap().backups.len(), backups);
    // An older full switch backup made in external mode is refused the same way.
    let full = mode::switch_backups_dir(&e.db).join("2026-01-01-0000-external-8 pages.zip");
    export::export_to(&e.db, &full).unwrap();
    let name = full.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(mode::switch_to_internal(&mut e.db, Some(&name)).unwrap_err().to_string(), mode::EXTERNAL_BACKUP_REFUSED);
    assert_eq!(mode::storage(e.db.conn()).unwrap().mode, Some(KnowledgeMode::External));
}

/// FR-KN-EXT-14: an external backup imports with its folder, with another folder, or as tasks and
/// categories only.
#[test]
fn external_backups_import_with_a_folder_or_as_tasks_only() {
    let mut e = sample();
    let ops = e.category("Operations");
    let png = std::fs::read(e.file("Operations/attachments/pump.png")).unwrap();
    let att = crate::attachments::save(&mut e.db, &png, "image/png").unwrap();
    let task = tasks::create(
        &mut e.db,
        NewTask {
            category_id: ops.id.clone(),
            title: "Fix pump".into(),
            description: Some(format!("![shot](attachment:{})", att.name)),
            labels: Some(vec!["site".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    tasks::add_comment(&mut e.db, &task.id, "Done on Monday.").unwrap();
    let check = e.page_at("Operations/Pumps/Pump check.md");
    link_task_page(&mut e.db, &task.id, &check.id).unwrap();
    let zip = e._data.path().join("backup.zip");
    export::export_to(&e.db, &zip).unwrap();

    let info = mode::backup_info(&zip).unwrap();
    assert_eq!((info.mode, info.root_exists, info.pages, info.tasks), (Some(KnowledgeMode::External), true, 8, 1));
    assert_eq!(info.root.as_deref(), Some(e.root().to_string_lossy().as_ref()));

    // The folder is there: a plain import.
    let other = tempfile::tempdir().unwrap();
    let mut db = Db::open_at(other.path()).unwrap();
    export::import_from(&mut db, &zip).unwrap();
    assert_eq!(mode::storage(db.conn()).unwrap().root.as_deref(), Some(e.root().to_string_lossy().as_ref()));

    // The folder moved: a plain import is refused; with the new place, the index follows it.
    let moved = tempfile::tempdir().unwrap();
    let new_root = moved.path().join("Notes");
    copy_dir(e.root(), &new_root);
    std::fs::remove_dir_all(e.root().join("Operations")).unwrap();
    std::fs::rename(e.root(), e.root().with_extension("gone")).unwrap();
    let third = tempfile::tempdir().unwrap();
    let mut db = Db::open_at(third.path()).unwrap();
    assert!(!mode::backup_info(&zip).unwrap().root_exists);
    let err = export::import_from(&mut db, &zip).unwrap_err();
    assert!(err.to_string().contains("isn't on this computer"), "{err}");
    export::import_with_root(&mut db, &zip, &new_root.to_string_lossy()).unwrap();
    let s = scan::scan(&mut db, Scope::All).unwrap();
    assert!(!s.changed_anything(), "{s:?}");
    let same = get_page(db.conn(), &check.id).unwrap();
    assert_eq!(same.path.as_deref(), Some("Operations/Pumps/Pump check.md"));
    assert_eq!(task_page_links(db.conn(), Some(&task.id), None).unwrap().len(), 1);
    std::fs::rename(e.root().with_extension("gone"), e.root()).unwrap();

    // Tasks only: the tasks with comments, labels and images, and the categories; no pages.
    let fourth = tempfile::tempdir().unwrap();
    let mut db = Db::open_at(fourth.path()).unwrap();
    export::import_tasks_only(&mut db, &zip).unwrap();
    let t = tasks::get(db.conn(), &task.id).unwrap();
    assert_eq!(t.labels, ["site"]);
    assert_eq!(tasks::comments(db.conn(), &task.id).unwrap().len(), 1);
    assert!(db.paths().attachments_dir().join(&att.name).is_file());
    assert_eq!(categories::get(db.conn(), &ops.id).unwrap().folder, None);
    let count = |sql: &str| -> i64 { db.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!((count("SELECT COUNT(*) FROM pages"), count("SELECT COUNT(*) FROM groups"), count("SELECT COUNT(*) FROM task_page_links")), (0, 0, 0));
    let s = crate::settings::get(db.conn()).unwrap();
    assert_eq!((s.knowledge_mode, s.external_root), (Some(KnowledgeMode::Internal), None));
}

/// A category renamed in Cairn renames its folder (FR-KN-EXT-5), with the links into it.
#[test]
fn renaming_a_category_renames_its_folder() {
    let mut e = sample();
    let ops = e.category("Operations");
    categories::update(
        e.db.conn(),
        &ops.id,
        CategoryChanges {
            name: Some("Plant: operations".into()),
            description: None,
            color: None,
        },
    )
    .unwrap();
    assert_eq!(e.category("Plant: operations").folder.as_deref(), Some("Plant- operations"));
    assert!(e.file("Plant- operations/Pumps/Pump check.md").is_file());
    assert_eq!(e.group("Plant- operations/Pumps/Seals").name, "Seals");
    assert!(e.read("Welcome.md").contains("(Plant-%20operations/Pumps/Pump%20check.md)"));
    assert!(!e.scan().changed_anything());
    // A name another category's folder has is refused.
    let err = categories::update(
        e.db.conn(),
        &ops.id,
        CategoryChanges {
            name: Some("learning".into()),
            description: None,
            color: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, CoreError::Conflict(_)), "{err}");
}

/// FR-KN-EXT-15: files other tools named against Windows' rules are read as they are, and keep
/// their names until the user changes the title; Cairn's own names are clean.
#[cfg(unix)]
#[test]
fn names_other_tools_gave_stay_until_the_title_changes() {
    let mut e = sample();
    e.write("Learning/Notes: 2024?.md", "Old notes.\n");
    e.scan();
    let p = e.page_at("Learning/Notes: 2024?.md");
    assert_eq!(p.title, "Notes: 2024?");
    // The editor sends the title with every save: the file keeps its name.
    let saved = update_page(
        &mut e.db,
        &p.id,
        PagePatch {
            title: Some("Notes: 2024?".into()),
            body: Some("New notes.\n".into()),
            ..patch()
        },
    )
    .unwrap();
    assert_eq!(saved.path.as_deref(), Some("Learning/Notes: 2024?.md"));
    assert_eq!(e.read("Learning/Notes: 2024?.md"), "New notes.\n");
    let ops = e.category("Operations").id;
    let moved = move_page(&mut e.db, &p.id, &ops).unwrap();
    assert_eq!(moved.path.as_deref(), Some("Operations/Notes: 2024?.md"));
    // A new title is cleaned.
    let renamed = update_page(&mut e.db, &p.id, PagePatch { title: Some("Notes: 2025?".into()), ..patch() }).unwrap();
    assert_eq!((renamed.title.as_str(), renamed.path.as_deref()), ("Notes- 2025-", Some("Operations/Notes- 2025-.md")));
}

// ---------------------------------------------------------------- review fixes (2026-10-20)

/// Two names for one file (what a file system that ignores case does with `pump.md` and
/// `Pump.md`, made here with a hard link): the second name is never indexed as another page.
#[cfg(unix)]
#[test]
fn one_file_under_two_names_is_indexed_once() {
    let mut e = sample();
    let page = e.page_at("Learning/Rust ownership.md");
    std::fs::hard_link(e.file("Learning/Rust ownership.md"), e.file("Learning/rust ownership.md")).unwrap();
    let s = scan::scan(&mut e.db, Scope::Paths(vec!["Learning/rust ownership.md".into()])).unwrap();
    assert_eq!(s.added, 0, "{s:?}");
    let s = e.scan();
    assert_eq!(s.added, 0, "{s:?}");
    assert_eq!(e.page_at("Learning/Rust ownership.md").id, page.id);
    // An own-write note for a name that is gone counts only when that exact name is gone.
    let gone = e.file("Learning/RUST OWNERSHIP.md");
    super::note_own_write(&gone, None);
    assert!(super::is_own_write(&gone));
    super::note_own_write(&e.file("Learning/rust ownership.md"), None);
    assert!(!super::is_own_write(&e.file("Learning/rust ownership.md")), "the name is there");
}

/// FR-KN-EXT-4: a file or folder that disappears and comes back (a sync, a git checkout) gets its
/// id and Cairn-only data again, by path or by its text; after 30 days that data is dropped.
#[test]
fn files_that_come_back_get_their_data_again() {
    let mut e = sample();
    let page = e.page_at("Operations/Hoses/Hose guide.markdown");
    let hoses = e.group("Operations/Hoses");
    update_group(&mut e.db, &hoses.id, GroupPatch { description: Some("Hydraulics".into()), ..Default::default() }).unwrap();
    let task = tasks::create(&mut e.db, NewTask { category_id: page.category_id.clone(), title: "Hoses".into(), ..Default::default() }).unwrap();
    link_task_page(&mut e.db, &task.id, &page.id).unwrap();
    set_check_status(&mut e.db, &page.id, CheckStatus::NotChecked).unwrap();
    let before_versions = versions(e.db.conn(), &page.id).unwrap().len();
    assert!(before_versions >= 1);

    // The folder goes away for one pass.
    let away = e._data.path().join("away");
    std::fs::rename(e.file("Operations/Hoses"), &away).unwrap();
    let s = e.scan();
    assert_eq!((s.removed, s.groups_removed), (1, 1), "{s:?}");
    assert!(get_page(e.db.conn(), &page.id).is_err());
    assert_eq!(e.count("SELECT COUNT(*) FROM removed_pages"), 1);

    // It comes back: the same page and group, with their data.
    std::fs::rename(&away, e.file("Operations/Hoses")).unwrap();
    let s = e.scan();
    assert_eq!((s.added, s.groups_added), (1, 1), "{s:?}");
    let back = e.page_at("Operations/Hoses/Hose guide.markdown");
    assert_eq!(back.id, page.id);
    assert_eq!((back.check_status, back.created_at.as_str()), (CheckStatus::NotChecked, page.created_at.as_str()));
    assert_eq!(task_page_links(e.db.conn(), Some(&task.id), None).unwrap().len(), 1);
    assert!(versions(e.db.conn(), &page.id).unwrap().len() >= before_versions);
    let group = e.group("Operations/Hoses");
    assert_eq!((group.id.as_str(), group.description.as_str()), (hoses.id.as_str(), "Hydraulics"));
    assert_eq!(e.count("SELECT COUNT(*) FROM removed_pages") + e.count("SELECT COUNT(*) FROM removed_groups"), 0);

    // Removed in one pass, back elsewhere in a later one: found by its text.
    let text = std::fs::read(e.file("Learning/Rust ownership.md")).unwrap();
    let rust = e.page_at("Learning/Rust ownership.md");
    std::fs::remove_file(e.file("Learning/Rust ownership.md")).unwrap();
    e.scan();
    std::fs::write(e.file("Operations/Rust notes.md"), &text).unwrap();
    e.scan();
    assert_eq!(e.page_at("Operations/Rust notes.md").id, rust.id);

    // Kept 30 days only.
    std::fs::remove_file(e.file("Operations/Rust notes.md")).unwrap();
    e.scan();
    e.db.conn().execute("UPDATE removed_pages SET removed_at = '2020-01-01T00:00:00.000Z'", []).unwrap();
    e.write("Learning/unrelated.md", "x\n");
    e.scan();
    assert_eq!(e.count("SELECT COUNT(*) FROM removed_pages"), 0);
    std::fs::write(e.file("Operations/Rust notes.md"), &text).unwrap();
    e.scan();
    assert_ne!(e.page_at("Operations/Rust notes.md").id, rust.id);
}

/// A folder with no Markdown at all while the index has pages is unreachable (a drive not
/// mounted, an offline cloud folder): a full scan changes nothing and says so.
#[test]
fn an_empty_folder_is_not_taken_as_everything_deleted() {
    let mut e = sample();
    let pages = e.count("SELECT COUNT(*) FROM pages");
    let away = e._data.path().join("away");
    std::fs::create_dir_all(&away).unwrap();
    for entry in std::fs::read_dir(e.root()).unwrap() {
        let entry = entry.unwrap();
        std::fs::rename(entry.path(), away.join(entry.file_name())).unwrap();
    }
    let err = scan::scan(&mut e.db, Scope::All).unwrap_err();
    assert_eq!(err.kind(), "invalid");
    assert!(err.to_string().contains("changed nothing"), "{err}");
    assert_eq!(e.count("SELECT COUNT(*) FROM pages"), pages);
    for entry in std::fs::read_dir(&away).unwrap() {
        let entry = entry.unwrap();
        std::fs::rename(entry.path(), e.root().join(entry.file_name())).unwrap();
    }
    assert!(!e.scan().changed_anything());
}

/// Paths in links, embeds and the index never reach outside the root.
#[test]
fn paths_never_leave_the_root() {
    use super::convert::{folder_target, resolve};
    let ext = Ext::new("/notes");
    for bad in ["../x.md", "a/../../x", "a//b", "./a", "a\\b", "/abs", ""] {
        assert!(!super::valid_rel(bad), "{bad}");
    }
    assert!(ext.abs("a/../../etc/passwd").is_none());
    assert!(ext.abs("a\\..\\..\\x").is_none());
    assert_eq!(ext.abs(""), Some(PathBuf::from("/notes")));
    assert_eq!(ext.abs("Ops/a b.md"), Some(PathBuf::from("/notes/Ops/a b.md")));
    assert_eq!(folder_target("folder:a/..%2F..%2Fx.png"), None);
    assert_eq!(folder_target("folder:a/%5C..%5Cx.png"), None);
    assert_eq!(resolve("Ops/attachments", "..\\..\\..\\Users\\me\\x.png"), None);
    assert_eq!(resolve("Ops", "../../x.png"), None);
    assert_eq!(resolve("Ops", "sub/../x.png"), Some(("Ops/x.png".into(), None)));

    // An embed that climbs out of the root stays as written, and the image outside is untouched.
    let mut e = sample();
    let outside = e.root().parent().unwrap().join("outside.png");
    std::fs::write(&outside, b"\x89PNG not mine").unwrap();
    let name = outside.file_name().unwrap().to_string_lossy().into_owned();
    e.write("Operations/Escape.md", &format!("![[attachments/../../../{name}]] and ![x](../../{name})\n"));
    e.scan();
    let p = e.page_at("Operations/Escape.md");
    assert!(!p.body.contains("folder:"), "{}", p.body);
    update_page(&mut e.db, &p.id, PagePatch { body: Some("gone\n".into()), ..patch() }).unwrap();
    delete_page(&mut e.db, &p.id).unwrap();
    assert!(outside.is_file());
    std::fs::remove_file(outside).unwrap();
}

/// Links (symbolic links) below the root are never followed: not read, not watched, not written
/// through, and a category never takes one as its folder.
#[cfg(unix)]
#[test]
fn links_below_the_root_are_not_followed() {
    let mut e = sample();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("Secret.md"), "outside\n").unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), e.file("Linked")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), e.file("Operations/Pumps/Linked group")).unwrap();
    assert!(!e.scan().changed_anything());
    let s = scan::scan(&mut e.db, Scope::Paths(vec!["Linked/Secret.md".into(), "Operations/Pumps/Linked group".into()])).unwrap();
    assert!(!s.changed_anything(), "{s:?}");
    assert_eq!(e.count("SELECT COUNT(*) FROM pages WHERE title = 'Secret'"), 0);
    // A category named like the link gets a folder of its own.
    let c = categories::create(
        e.db.conn(),
        categories::NewCategory { name: "Linked".into(), key: "LNK".into(), description: String::new(), color: categories::CategoryColor::preset("blue") },
    )
    .unwrap();
    let p = create_page(&mut e.db, NewPage { category_id: c.id.clone(), title: "Mine".into(), body: "x".into(), ..Default::default() }).unwrap();
    assert_eq!(p.path.as_deref(), Some("Linked 2/Mine.md"));
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 1, "nothing written through the link");
    // A write through a link is refused.
    let ext = Ext::new(e.root());
    assert_eq!(ext.writable("Linked/New.md").unwrap_err().kind(), "invalid");
}

/// A change rolled back after it wrote its file: the watcher reads that file again (Cairn's note
/// on it is forgotten), so the index catches up with the folder. Images wait for the commit.
#[test]
fn a_rolled_back_change_is_read_again_from_the_folder() {
    let mut e = sample();
    let (tx, rx) = mpsc::channel();
    let watcher = FolderWatcher::start(e.root(), move |b| {
        let _ = tx.send((Instant::now(), b));
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let learning = e.category("Learning").id;
    let r: crate::Result<()> = e.db.tx(|tx| {
        create_page_in(tx, NewPage { category_id: learning.clone(), title: "Half done".into(), body: "Text\n".into(), ..Default::default() }, &crate::tasks::Stamp::now())?;
        Err(CoreError::Invalid("stopped".into()))
    });
    assert!(r.is_err());
    assert!(e.file("Learning/Half done.md").is_file(), "the file was written before the rollback");
    let batches = collect(&rx, Duration::from_millis(2000));
    watcher.stop();
    assert!(batches.iter().any(|(_, b)| b.paths.contains(&"Learning/Half done.md".to_string())), "{batches:?}");
    let s = scan::scan(&mut e.db, Scope::Paths(vec!["Learning/Half done.md".into()])).unwrap();
    assert_eq!(s.added, 1);

    // A body change that drops an image and is rolled back trashes nothing.
    let check = e.page_at("Operations/Pumps/Pump check.md");
    let r: crate::Result<()> = e.db.tx(|tx| {
        update_page_in(tx, &check.id, PagePatch { body: Some("No image.\n".into()), ..patch() }, &crate::tasks::Stamp::now())?;
        Err(CoreError::Invalid("stopped".into()))
    });
    assert!(r.is_err());
    assert!(e.file("Operations/attachments/pump.png").is_file());
}

/// FR-KN-EXT-11: restoring a history version (text from Cairn, no hash) after the file changed
/// outside is refused, not written over the outside edit.
#[test]
fn a_restore_never_overwrites_an_outside_edit() {
    let mut e = sample();
    let p = e.page_at("Learning/Rust ownership.md");
    update_page(&mut e.db, &p.id, PagePatch { body: Some("Version two.\n".into()), ..patch() }).unwrap();
    e.write("Learning/Rust ownership.md", "Edited outside.\n");
    let today = versions(e.db.conn(), &p.id).unwrap().last().unwrap().date.clone();
    assert_eq!(restore_version(&mut e.db, &p.id, &today).unwrap_err().kind(), "changed_on_disk");
    assert_eq!(e.read("Learning/Rust ownership.md"), "Edited outside.\n");
}

/// FR-KN-EXT-8: a switch backup of the folder that can't be read doesn't stop going external;
/// only the earlier Cairn-only data is missing.
#[test]
fn a_damaged_restore_source_does_not_stop_the_switch() {
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    copy_dir(Path::new(SAMPLE), folder.path());
    set_test_trash(&data.path().join("test-trash"));
    let mut db = Db::open_at(data.path()).unwrap();
    mode::choose_mode(&mut db, KnowledgeMode::Internal, None).unwrap();
    let root = folder.path().to_string_lossy().into_owned();
    // A "switch backup" of this root whose manifest reads but whose data is missing.
    let dir = mode::switch_backups_dir(&db);
    std::fs::create_dir_all(&dir).unwrap();
    {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(std::fs::File::create(dir.join("2026-01-01-0000-external-5 pages.zip")).unwrap());
        z.start_file("manifest.json", zip::write::SimpleFileOptions::default()).unwrap();
        let manifest = serde_json::json!({
            "appVersion": "0.13.0", "exportFormat": 1, "schemaVersion": crate::migrations::latest_version(),
            "exportedAt": "2026-01-01T00:00:00.000Z", "tables": {"pages": 5}, "attachments": 0,
            "knowledgeMode": "external", "externalRoot": root,
        });
        z.write_all(manifest.to_string().as_bytes()).unwrap();
        z.finish().unwrap();
    }
    let r = mode::switch_to_external(&mut db, &root).unwrap();
    assert_eq!((r.restored, r.scan.unwrap().added), (0, 8));
    assert_eq!(mode::storage(db.conn()).unwrap().mode, Some(KnowledgeMode::External));
}

/// The small ones: the top of a disk keeps its separator; names Cairn makes start with no dot;
/// a write keeps the file's permissions; a new file never replaces one that appeared meanwhile;
/// Cairn's stale temporary files are swept; `X.md` and `X.markdown` clash in a bulk move; an image
/// another file of the folder names is never trashed.
#[test]
fn small_review_fixes() {
    assert_eq!(super::clean_name(".git", "Category"), "git");
    assert_eq!(super::clean_name(" . .hidden notes", "page"), "hidden notes");
    assert_eq!(super::clean_name("...", "page"), "page");
    assert_eq!(super::clean_title(".CON"), "CON_");
    assert_eq!(super::clean_title("Plain"), "Plain");

    let mut e = sample();
    // A category named `.git` gets a visible folder; a page titled `.notes` a visible file.
    let c = categories::create(
        e.db.conn(),
        categories::NewCategory { name: ".git".into(), key: "GIT".into(), description: String::new(), color: categories::CategoryColor::preset("blue") },
    )
    .unwrap();
    let p = create_page(&mut e.db, NewPage { category_id: c.id.clone(), title: ".notes".into(), body: "x".into(), ..Default::default() }).unwrap();
    assert_eq!(p.path.as_deref(), Some("git/notes.md"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let f = e.file("Learning/Rust ownership.md");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
        let rust = e.page_at("Learning/Rust ownership.md");
        update_page(&mut e.db, &rust.id, PagePatch { body: Some("New.\n".into()), ..patch() }).unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o640);
    }

    // A new file is never written over one that is there.
    let taken = e.file("Learning/Taken.md");
    std::fs::write(&taken, "mine\n").unwrap();
    assert_eq!(super::write_new(&taken, b"cairn\n").unwrap_err().kind(), "conflict");
    assert_eq!(std::fs::read_to_string(&taken).unwrap(), "mine\n");

    // A stale temporary file is swept by a scan; a fresh one is left.
    let stale = e.file("Learning/.Old.md.cairn-tmp");
    let fresh = e.file("Learning/.New.md.cairn-tmp");
    std::fs::write(&stale, "x").unwrap();
    std::fs::write(&fresh, "x").unwrap();
    let old = std::time::SystemTime::now() - Duration::from_secs(3600);
    std::fs::File::options().write(true).open(&stale).unwrap().set_modified(old).unwrap();
    e.scan();
    assert!(!stale.exists() && fresh.exists());

    // `X.md` and `X.markdown` moved into one folder clash.
    e.write("Learning/Hose guide.md", "Another guide.\n");
    e.scan();
    let a = e.page_at("Learning/Hose guide.md");
    let b = e.page_at("Operations/Hoses/Hose guide.markdown");
    let change = PageBulkChange::Move { category_id: e.category("Operations").id, group_id: None };
    assert_eq!(plan_bulk_change(e.db.conn(), &[a.id.clone(), b.id.clone()], &change).unwrap().len(), 1);

    // An image only a file Cairn doesn't index uses (a canvas) stays.
    e.write("Operations/Board.canvas", r#"{"nodes":[{"file":"Operations/attachments/pump.png"}]}"#);
    let check = e.page_at("Operations/Pumps/Pump check.md");
    update_page(&mut e.db, &check.id, PagePatch { body: Some("No image.\n".into()), ..patch() }).unwrap();
    assert!(e.file("Operations/attachments/pump.png").is_file());
    std::fs::remove_file(e.file("Operations/Board.canvas")).unwrap();
}

/// FR-KN-EXT-14 (the owner's choice): a tasks-only import keeps this computer's settings and AI
/// providers; the backup brings only its tasks and categories; pages, glossary and conversations
/// end up empty.
#[test]
fn a_tasks_only_import_keeps_this_computers_settings_and_providers() {
    let mut e = sample();
    let ops = e.category("Operations");
    let task = tasks::create(&mut e.db, NewTask { category_id: ops.id.clone(), title: "Fix pump".into(), ..Default::default() }).unwrap();
    crate::settings::update(&mut e.db, crate::settings::SettingsPatch { user_name: Some("From the backup".into()), ..Default::default() }).unwrap();
    let zip = e._data.path().join("backup.zip");
    export::export_to(&e.db, &zip).unwrap();

    let here = tempfile::tempdir().unwrap();
    let mut db = Db::open_at(here.path()).unwrap();
    mode::choose_mode(&mut db, KnowledgeMode::Internal, None).unwrap();
    crate::settings::update(&mut db, crate::settings::SettingsPatch { user_name: Some("This computer".into()), ..Default::default() }).unwrap();
    db.conn()
        .execute_batch(
            "INSERT INTO providers (id, name, kind, endpoint, auth, created_at, updated_at)
             VALUES ('pv1', 'Work', 'azure_foundry', 'https://x/openai/v1', 'azure_cli', 'x', 'x');
             INSERT INTO models (id, provider_id, name, label, format, created_at, updated_at)
             VALUES ('m1', 'pv1', 'gpt', 'GPT', 'responses', 'x', 'x');
             INSERT INTO glossary_entries (id, term, created_at, updated_at) VALUES ('g1', 'Pump', 'x', 'x');",
        )
        .unwrap();
    let summary = export::import_tasks_only(&mut db, &zip).unwrap();
    // The summary names only what came from the backup.
    let reported: Vec<&str> = summary.tables.iter().map(|t| t.table.as_str()).collect();
    for absent in ["pages", "settings", "glossary_entries", "conversations", "providers"] {
        assert!(!reported.contains(&absent), "{reported:?}");
    }
    assert!(summary.tables.iter().any(|t| t.table == "tasks" && t.rows == 1), "{:?}", summary.tables);
    let s = crate::settings::get(db.conn()).unwrap();
    assert_eq!((s.user_name.as_str(), s.knowledge_mode), ("This computer", Some(KnowledgeMode::Internal)));
    let count = |sql: &str| -> i64 { db.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!((count("SELECT COUNT(*) FROM providers"), count("SELECT COUNT(*) FROM models")), (1, 1));
    assert_eq!(tasks::get(db.conn(), &task.id).unwrap().title, "Fix pump");
    assert_eq!(categories::get(db.conn(), &ops.id).unwrap().name, "Operations");
    assert_eq!(
        (count("SELECT COUNT(*) FROM pages"), count("SELECT COUNT(*) FROM glossary_entries"), count("SELECT COUNT(*) FROM conversations")),
        (0, 0, 0)
    );
}

/// A scan applies its plan in transactions of about `BATCH_TIME` each, so commands never wait
/// long behind a big first scan.
#[test]
fn a_big_scan_holds_the_database_only_briefly() {
    let mut e = sample();
    for i in 0..1000 {
        e.write(&format!("Bulk/Note {i}.md"), &format!("# Note {i}\n\nSome text.\n"));
    }
    let job = scan::prepare(e.db.conn(), Scope::All).unwrap();
    let mut plan = job.read(&mut |_| {}).unwrap();
    let started = Instant::now();
    let n = plan.apply_some(&mut e.db, usize::MAX).unwrap();
    let took = started.elapsed();
    assert!(n >= 1 && n < 1001, "{n} items in one transaction");
    assert!(took < Duration::from_millis(900), "one transaction took {took:?}");
    while plan.apply_some(&mut e.db, usize::MAX).unwrap() > 0 {}
    assert!(plan.is_done());
    assert_eq!(plan.summary().added, 1000);
}

/// A category made in Cairn (or by the demo data) while a scan was being planned takes the new
/// folder of its name instead of a second category appearing.
#[test]
fn a_category_made_during_a_scan_takes_its_folder() {
    let mut e = sample();
    e.write("Study/Notes.md", "x\n");
    let job = scan::prepare(e.db.conn(), Scope::All).unwrap();
    let mut plan = job.read(&mut |_| {}).unwrap();
    let made = categories::create(
        e.db.conn(),
        categories::NewCategory { name: "study".into(), key: "STU".into(), description: String::new(), color: categories::CategoryColor::preset("blue") },
    )
    .unwrap();
    while plan.apply_some(&mut e.db, scan::BATCH).unwrap() > 0 {}
    assert_eq!(plan.summary().categories_added, 0);
    assert_eq!(categories::get(e.db.conn(), &made.id).unwrap().folder.as_deref(), Some("Study"));
    assert_eq!(e.page_at("Study/Notes.md").category_id, made.id);
    assert_eq!(e.count("SELECT COUNT(*) FROM categories WHERE lower(name) = 'study'"), 1);
}

/// A title refused because of another file's name in the folder names that file's title.
#[test]
fn a_title_clash_names_the_page_that_is_there() {
    let mut e = sample();
    let learning = e.category("Learning").id;
    let err = create_page(&mut e.db, NewPage { category_id: learning, title: "rust OWNERSHIP".into(), body: String::new(), ..Default::default() })
        .unwrap_err();
    assert_eq!(err.kind(), "conflict");
    assert!(err.to_string().contains("a page named \"Rust ownership\""), "{err}");
    let chinese = e.page_at("Learning/中文笔记.md");
    let err = update_page(&mut e.db, &chinese.id, PagePatch { title: Some("RUST ownership".into()), ..patch() }).unwrap_err();
    assert!(err.to_string().contains("\"Rust ownership\""), "{err}");
}

/// Development only: in external mode the demo categories never add a second category next to
/// one the folder's scan made with the same name.
#[test]
fn demo_categories_do_not_double_the_folders_categories() {
    let e = sample();
    let seed = |id: &str, name: &str, key: &str| categories::CategorySeed {
        id: id.into(),
        name: name.into(),
        key: key.into(),
        description: String::new(),
        color: categories::CategoryColor::preset("green"),
        status: categories::CategoryStatus::Active,
        created_at: crate::util::now_iso(),
    };
    let n = categories::seed_if_missing(e.db.conn(), &[seed("p-learning", "Learning", "LRN"), seed("p-demo", "Demo", "DMO")]).unwrap();
    assert_eq!(n, 1);
    assert_eq!(e.count("SELECT COUNT(*) FROM categories WHERE name = 'Learning'"), 1);
}

/// Another knowledge folder while in external mode: one switch with one backup of the old
/// folder's index; going back to the old folder brings its Cairn-only data back by path; neither
/// folder's files change; tasks stay.
#[test]
fn changing_the_folder_is_one_switch() {
    let mut e = sample();
    let old_root = e.root().to_string_lossy().into_owned();
    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(other.path().join("Field")).unwrap();
    std::fs::write(other.path().join("Field/Survey.md"), "# Survey\n").unwrap();
    std::fs::write(other.path().join("Inbox.md"), "Loose note.\n").unwrap();
    let new_root = other.path().to_string_lossy().into_owned();

    // Cairn-only data on the old folder.
    let check = e.page_at("Operations/Pumps/Pump check.md");
    let task = tasks::create(&mut e.db, NewTask { category_id: check.category_id.clone(), title: "Service".into(), ..Default::default() }).unwrap();
    link_task_page(&mut e.db, &task.id, &check.id).unwrap();
    set_check_status(&mut e.db, &check.id, CheckStatus::NotChecked).unwrap();
    let history = versions(e.db.conn(), &check.id).unwrap().len();
    let old_bytes = std::fs::read(e.file("Operations/Pumps/Pump check.md")).unwrap();
    let backups_before = mode::switch_backups(&e.db).unwrap().backups.len();

    // Refused: the folder in use (also written another way), a folder that isn't there.
    assert_eq!(mode::switch_external_root(&mut e.db, &old_root).unwrap_err().kind(), "invalid");
    assert_eq!(mode::switch_external_root(&mut e.db, &format!("{old_root}/Operations/..")).unwrap_err().kind(), "invalid");
    assert_eq!(mode::switch_external_root(&mut e.db, "/no/such/folder").unwrap_err().kind(), "invalid");
    assert_eq!(mode::switch_backups(&e.db).unwrap().backups.len(), backups_before, "a refused change makes no backup");

    let r = mode::switch_external_root(&mut e.db, &new_root).unwrap();
    assert_eq!((r.backup.mode, r.backup.root.as_deref(), r.backup.pages), (KnowledgeMode::External, Some(old_root.as_str()), 8));
    assert_eq!((r.restored, r.scan.as_ref().unwrap().added), (0, 2));
    assert_eq!(mode::switch_backups(&e.db).unwrap().backups.len(), backups_before + 1, "one backup");
    assert_eq!(mode::storage(e.db.conn()).unwrap().root.as_deref(), Some(new_root.as_str()));
    assert!(get_page(e.db.conn(), &check.id).is_err(), "the old folder's pages are gone from the index");
    assert_eq!(e.count("SELECT COUNT(*) FROM pages"), 2);
    assert!(e.count("SELECT COUNT(*) FROM pages WHERE path = 'Field/Survey.md'") == 1);
    assert_eq!(tasks::get(e.db.conn(), &task.id).unwrap().title, "Service", "tasks stay");
    assert_eq!(std::fs::read(e.file("Operations/Pumps/Pump check.md")).unwrap(), old_bytes, "the old folder is not changed");
    let waiting: i64 = e.count("SELECT COUNT(*) FROM chunks WHERE kind = 'page' AND vector IS NOT NULL");
    assert_eq!(waiting, 0);

    // Back to the old folder: its Cairn-only data comes back by path.
    let r = mode::switch_external_root(&mut e.db, &old_root).unwrap();
    assert_eq!((r.backup.root.as_deref(), r.backup.pages, r.restored), (Some(new_root.as_str()), 2, 8));
    assert!(!r.scan.unwrap().changed_anything());
    let back = get_page(e.db.conn(), &check.id).unwrap();
    assert_eq!(back.check_status, CheckStatus::NotChecked);
    assert_eq!(task_page_links(e.db.conn(), Some(&task.id), Some(&check.id)).unwrap().len(), 1);
    assert_eq!(versions(e.db.conn(), &check.id).unwrap().len(), history);
    assert!(other.path().join("Field/Survey.md").is_file(), "the other folder is not changed");

    // Not in external mode: refused.
    mode::switch_to_internal(&mut e.db, None).unwrap();
    assert_eq!(mode::switch_external_root(&mut e.db, &new_root).unwrap_err().kind(), "invalid");
}

/// The switch backups (D-72, the owner's choices): knowledge only, loaded by switching but never
/// imported; one per folder (the newest), unless entering that folder failed to put its data
/// back; internal ones and other folders' are never deleted. An older full switch backup still
/// imports.
#[test]
fn switch_backups_hold_knowledge_only_and_one_per_folder() {
    let mut e = sample();
    let root = e.root().to_string_lossy().into_owned();
    let check = e.page_at("Operations/Pumps/Pump check.md");
    let task = tasks::create(&mut e.db, NewTask { category_id: check.category_id.clone(), title: "Service".into(), ..Default::default() }).unwrap();
    link_task_page(&mut e.db, &task.id, &check.id).unwrap();
    e.db.conn()
        .execute("INSERT INTO glossary_entries (id, term, created_at, updated_at) VALUES ('g1', 'Pump', 'x', 'x')", [])
        .unwrap();
    let history = versions(e.db.conn(), &check.id).unwrap().len();

    // Leaving the folder: a knowledge-only backup.
    let first = mode::switch_to_internal(&mut e.db, None).unwrap().backup;
    assert!(first.knowledge_only);
    let tables: Vec<String> = {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&first.path).unwrap()).unwrap();
        (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_string()).collect()
    };
    for kept in ["tables/pages.jsonl", "tables/page_versions.jsonl", "tables/task_page_links.jsonl", "tables/categories.jsonl"] {
        assert!(tables.iter().any(|t| t == kept), "{kept} missing: {tables:?}");
    }
    for left in ["tasks", "task_comments", "glossary_entries", "conversations", "settings", "providers", "activity"] {
        assert!(!tables.iter().any(|t| t == &format!("tables/{left}.jsonl")), "{left} in {tables:?}");
    }
    // Never imported.
    let other = tempfile::tempdir().unwrap();
    let mut fresh = Db::open_at(other.path()).unwrap();
    for err in [
        export::import_from(&mut fresh, Path::new(&first.path)).unwrap_err(),
        export::import_tasks_only(&mut fresh, Path::new(&first.path)).unwrap_err(),
        export::import_with_root(&mut fresh, Path::new(&first.path), &root).unwrap_err(),
        mode::backup_info(Path::new(&first.path)).unwrap_err(),
    ] {
        assert_eq!(err.to_string(), export::SWITCH_BACKUP_REFUSED);
    }
    // Loaded back on entering the folder again: pages, history, the task link (its task exists).
    let back = mode::switch_to_external(&mut e.db, &root).unwrap();
    assert_eq!(back.restored, 8);
    assert_eq!(versions(e.db.conn(), &check.id).unwrap().len(), history);
    assert_eq!(task_page_links(e.db.conn(), Some(&task.id), Some(&check.id)).unwrap().len(), 1);
    assert_eq!(e.count("SELECT COUNT(*) FROM glossary_entries"), 1, "the glossary stays as it is");
    let to_internal = mode::switch_to_internal(&mut e.db, None).unwrap();

    // One backup per folder: the folder was left twice, only the newest is kept; the internal
    // one stays.
    let list = mode::switch_backups(&e.db).unwrap().backups;
    let of_folder: Vec<&mode::SwitchBackup> = list.iter().filter(|b| b.mode == KnowledgeMode::External).collect();
    assert_eq!(of_folder.len(), 1);
    assert_eq!(of_folder[0].file_name, to_internal.backup.file_name);
    assert_eq!(list.iter().filter(|b| b.mode == KnowledgeMode::Internal).count(), 1);

    // Another folder's backups are untouched by leaving this one.
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("Else.md"), "x\n").unwrap();
    let elsewhere_root = elsewhere.path().to_string_lossy().into_owned();
    mode::switch_to_external(&mut e.db, &elsewhere_root).unwrap();
    mode::switch_external_root(&mut e.db, &root).unwrap();
    mode::switch_to_internal(&mut e.db, None).unwrap();
    let list = mode::switch_backups(&e.db).unwrap().backups;
    let of = |r: &str| list.iter().filter(|b| b.root.as_deref() == Some(r)).count();
    assert_eq!((of(&root), of(&elsewhere_root)), (1, 1));

    // Entering the folder failed to put its data back: its older backup is kept when leaving.
    let newest = list.iter().find(|b| b.root.as_deref() == Some(root.as_str())).unwrap().clone();
    {
        // Damaged: the manifest reads, the data doesn't.
        use std::io::Write;
        let manifest = {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(&newest.path).unwrap()).unwrap();
            let mut text = String::new();
            std::io::Read::read_to_string(&mut zip.by_name("manifest.json").unwrap(), &mut text).unwrap();
            text
        };
        let mut z = zip::ZipWriter::new(std::fs::File::create(&newest.path).unwrap());
        z.start_file("manifest.json", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(manifest.as_bytes()).unwrap();
        z.finish().unwrap();
    }
    assert_eq!(mode::switch_to_external(&mut e.db, &root).unwrap().restored, 0);
    mode::switch_to_internal(&mut e.db, None).unwrap();
    let list = mode::switch_backups(&e.db).unwrap().backups;
    assert_eq!(list.iter().filter(|b| b.root.as_deref() == Some(root.as_str())).count(), 2, "the older one is kept");
    let internal_backups = list.iter().filter(|b| b.mode == KnowledgeMode::Internal).count();
    assert_eq!(internal_backups, 3, "internal switch backups are never deleted");

    // An older full switch backup (a whole export) still imports.
    let full = mode::switch_backups_dir(&e.db).join("2026-01-01-0000-internal-8 pages.zip");
    export::export_to(&e.db, &full).unwrap();
    assert!(!mode::switch_backups(&e.db).unwrap().backups.iter().find(|b| b.path == full.to_string_lossy()).unwrap().knowledge_only);
    export::import_from(&mut fresh, &full).unwrap();
}

/// A knowledge-only switch backup carries the images its pages use (and only those), and loading
/// it brings them back with their rows.
#[test]
fn switch_backups_carry_the_images_pages_use() {
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    set_test_trash(&data.path().join("test-trash"));
    let mut db = Db::open_at(data.path()).unwrap();
    mode::choose_mode(&mut db, KnowledgeMode::Internal, None).unwrap();
    let png = std::fs::read(Path::new(SAMPLE).join("Operations/attachments/pump.png")).unwrap();
    let used = crate::attachments::save(&mut db, &png, "image/png").unwrap();
    let task_only = crate::attachments::save(&mut db, b"GIF89a task image", "image/gif").unwrap();
    tasks::create(&mut db, NewTask { category_id: "p-general".into(), title: "With image".into(), description: Some(format!("![t](attachment:{})", task_only.name)), ..Default::default() }).unwrap();
    let page = create_page(&mut db, NewPage { category_id: "p-general".into(), title: "Pictured".into(), body: format!("![p](attachment:{})\n", used.name), ..Default::default() }).unwrap();
    let r = mode::switch_to_external(&mut db, &folder.path().to_string_lossy()).unwrap();
    let names: Vec<String> = {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&r.backup.path).unwrap()).unwrap();
        (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_string()).collect()
    };
    assert!(names.contains(&format!("attachments/{}", used.name)), "{names:?}");
    assert!(!names.contains(&format!("attachments/{}", task_only.name)), "only the images pages use");
    // The page's image went with the cleared page; loading the backup brings it back.
    mode::switch_to_internal(&mut db, Some(&r.backup.file_name)).unwrap();
    assert_eq!(get_page(db.conn(), &page.id).unwrap().title, "Pictured");
    assert!(db.paths().attachments_dir().join(&used.name).is_file());
    let rows: i64 = db.conn().query_row("SELECT COUNT(*) FROM attachments WHERE name = ?1", [&used.name], |r| r.get(0)).unwrap();
    assert_eq!(rows, 1);
}
