//! Small, self-contained regressions: no developer-local sample folder is required.
use super::scan::{self, Scope};
use crate::{
    Db,
    knowledge::{self, NewPage, Page, PagePatch, mode},
    settings::KnowledgeMode,
};
use std::path::Path;

struct Env {
    _data: tempfile::TempDir,
    root: tempfile::TempDir,
    db: Db,
}
impl Env {
    fn new(files: &[(&str, &str)]) -> Self {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        for (path, text) in files {
            write(root.path(), path, text);
        }
        super::set_test_trash(&data.path().join("test-trash"));
        let mut db = Db::open_at(data.path()).unwrap();
        mode::choose_mode(
            &mut db,
            KnowledgeMode::External,
            Some(root.path().to_str().unwrap()),
        )
        .unwrap();
        scan::scan(&mut db, Scope::All).unwrap();
        Self {
            _data: data,
            root,
            db,
        }
    }
    fn page(&self, path: &str) -> Page {
        let id: String = self
            .db
            .conn()
            .query_row("SELECT id FROM pages WHERE path = ?1", [path], |r| r.get(0))
            .unwrap();
        knowledge::get_page(self.db.conn(), &id).unwrap()
    }
    fn rename(&mut self, path: &str, name: &str) {
        let id = self.page(path).id;
        knowledge::update_page(
            &mut self.db,
            &id,
            PagePatch {
                title: Some(name.into()),
                ..Default::default()
            },
        )
        .unwrap();
    }
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.path().join(path)).unwrap()
    }
    fn create(&mut self, category: &str, title: &str) -> Page {
        let category_id: String = self
            .db
            .conn()
            .query_row("SELECT id FROM categories WHERE name = ?1", [category], |r| r.get(0))
            .unwrap();
        knowledge::create_page(
            &mut self.db,
            NewPage {
                category_id,
                title: title.into(),
                body: "x".into(),
                ..Default::default()
            },
        )
        .unwrap()
    }
    /// `[text](page:<id>)` for the page at `path`.
    fn link(&self, text: &str, path: &str) -> String {
        format!("[{text}](page:{})", self.page(path).id)
    }
    fn unchanged_by_a_scan(&mut self) {
        let s = scan::scan(&mut self.db, Scope::All).unwrap();
        assert!(!s.changed_anything(), "{s:?}");
    }
}
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn rename_preserves_unscanned_frontmatter_of_referring_pages() {
    let mut e = Env::new(&[
        ("A.md", "Target\n"),
        (
            "B.md",
            "---\ntags: [old]\nsource: old-source\n---\n\n[A](A.md)\n",
        ),
    ]);
    write(
        e.root.path(),
        "B.md",
        "---\ntags: [new]\nsource: new-source\narchived: true\ncustom: keep-me\n---\n\n[A](A.md)\n",
    );
    e.rename("A.md", "Renamed");
    scan::scan(&mut e.db, Scope::All).unwrap();
    let b = e.page("B.md");
    assert_eq!(b.tags, ["new"]);
    assert_eq!(b.source_info.as_deref(), Some("new-source"));
    assert!(b.archived);
    knowledge::set_archived(&mut e.db, &b.id, false).unwrap();
    let disk = e.read("B.md");
    assert!(
        disk.contains("new-source") && disk.contains("new") && disk.contains("custom: keep-me"),
        "{disk}"
    );
    assert!(!disk.contains("old-source"));
}

#[test]
fn rename_resolves_qualified_wiki_links_before_rewriting() {
    let mut e = Env::new(&[
        ("C1/Same.md", "One\n"),
        ("C2/Same.md", "Two\n"),
        ("Links.md", "[[C1/Same]] and [[C2/Same]]\n"),
    ]);
    e.rename("C1/Same.md", "Renamed");
    assert_eq!(e.read("Links.md"), "[[C1/Renamed]] and [[C2/Same]]\n");
}

#[test]
fn rename_retargets_self_links_and_heading_links() {
    let mut e = Env::new(&[("A.md", "[Self](A.md)\n[Section](A.md#part)\n[[A]]\n")]);
    e.rename("A.md", "Renamed");
    assert_eq!(
        e.read("Renamed.md"),
        "[Self](Renamed.md)\n[Section](Renamed.md#part)\n[[Renamed]]\n"
    );
    let a = e.page("Renamed.md");
    let expected = format!(
        "[Self](page:{})\n[Section](folder:Renamed.md#part)\n[Renamed](page:{})\n",
        a.id, a.id
    );
    assert_eq!(a.body, expected);
    assert!(
        !scan::scan(&mut e.db, Scope::All)
            .unwrap()
            .changed_anything()
    );
    assert_eq!(e.page("Renamed.md").body, expected);
}

fn late_target(scoped: bool) {
    let mut e = Env::new(&[("A.md", "[Future](B.md)\n")]);
    write(e.root.path(), "B.md", "Target\n");
    let scope = if scoped {
        Scope::Paths(vec!["B.md".into()])
    } else {
        Scope::All
    };
    let s = scan::scan(&mut e.db, scope.clone()).unwrap();
    assert_eq!((s.added, s.relinked), (1, 1), "{s:?}");
    let b = e.page("B.md");
    assert_eq!(e.page("A.md").body, format!("[Future](page:{})\n", b.id));
    assert_eq!(
        e.read("A.md"),
        "[Future](B.md)\n",
        "index refresh must never rewrite files"
    );
    std::fs::remove_file(e.root.path().join("B.md")).unwrap();
    scan::scan(&mut e.db, scope).unwrap();
    assert_eq!(e.page("A.md").body, "[Future](B.md)\n");
}
#[test]
fn full_scan_refreshes_references_when_targets_change() {
    late_target(false);
}
#[test]
fn scoped_scan_refreshes_references_when_targets_change() {
    late_target(true);
}

#[test]
fn rename_does_not_rewrite_an_unqualified_wiki_link_to_a_nearer_unmoved_page() {
    let mut e = Env::new(&[
        ("C1/Same.md", "One"),
        ("C2/Same.md", "Two"),
        ("C2/Links.md", "[[Same]]\n"),
    ]);
    e.rename("C1/Same.md", "Renamed");
    assert_eq!(e.read("C2/Links.md"), "[[Same]]\n");
    assert_eq!(
        e.page("C2/Links.md").body,
        format!("[Same](page:{})\n", e.page("C2/Same.md").id)
    );
}

#[test]
fn a_missing_qualified_wiki_target_does_not_bind_to_another_folder() {
    let e = Env::new(&[("C1/Same.md", "One"), ("Links.md", "[[Missing/Same]]\n")]);
    assert_eq!(e.page("Links.md").body, "[[Missing/Same]]\n");
}

#[test]
fn rename_and_body_edit_keep_self_links_resolvable() {
    let mut e = Env::new(&[("A.md", "Original")]);
    let a = e.page("A.md");
    let body = format!("Edited [Self](page:{})\n", a.id);
    knowledge::update_page(
        &mut e.db,
        &a.id,
        PagePatch {
            title: Some("Renamed".into()),
            body: Some(body.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(e.read("Renamed.md"), "Edited [Self](Renamed.md)\n");
    assert_eq!(e.page("Renamed.md").body, body);
}

#[test]
fn stale_reference_refresh_never_overwrites_a_newer_edit() {
    let mut e = Env::new(&[("A.md", "[Future](B.md)\n")]);
    write(e.root.path(), "B.md", "Target");
    let mut plan = scan::prepare(e.db.conn(), Scope::Paths(vec!["B.md".into()]))
        .unwrap()
        .read(&mut |_| {})
        .unwrap();
    let a = e.page("A.md");
    knowledge::update_page(
        &mut e.db,
        &a.id,
        PagePatch {
            body: Some("Newer body\n".into()),
            tags: Some(vec!["new".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    while plan.apply_some(&mut e.db, 1).unwrap() > 0 {}
    let saved = e.page("A.md");
    assert_eq!(saved.body, "Newer body\n");
    assert_eq!(saved.tags, ["new"]);
    assert!(e.read("A.md").contains("Newer body"));
}

#[test]
fn qualified_wiki_resolution_is_independent_of_candidate_order() {
    use std::collections::HashMap;
    for candidates in [
        vec!["C/Same.markdown", "C/Same.md"],
        vec!["C/Same.md", "C/Same.markdown"],
    ] {
        let stems = HashMap::from([(
            "same".to_string(),
            candidates.into_iter().map(str::to_string).collect(),
        )]);
        assert_eq!(
            super::store::wiki_page_path(&stems, "C/Same", "Links.md").as_deref(),
            Some("C/Same.md")
        );
        assert_eq!(
            super::store::wiki_page_path(&stems, "Same.md", "Links.md").as_deref(),
            Some("C/Same.md")
        );
        assert_eq!(
            super::store::wiki_page_path(&stems, "Same.markdown", "Links.md").as_deref(),
            Some("C/Same.markdown")
        );
    }
}

#[test]
fn renaming_preserves_explicit_wiki_extensions() {
    let mut e = Env::new(&[
        ("C/Same.md", "One"),
        ("C/Links/Renamed.markdown", "Two"),
        ("C/Links/Links.md", "[[Same.md]] and [[C/Same.md]]\n"),
    ]);
    let same = e.page("C/Same.md");
    e.rename("C/Same.md", "Renamed");
    assert_eq!(
        e.read("C/Links/Links.md"),
        "[[Renamed.md]] and [[C/Renamed.md]]\n"
    );
    assert_eq!(
        e.page("C/Links/Links.md").body,
        format!(
            "[Renamed.md](page:{}) and [Renamed.md](page:{})\n",
            same.id, same.id
        )
    );
}

fn unrelated_target_does_not_reindex_other_pages(scoped: bool, remove: bool) {
    let mut e = Env::new(&[("Anchor.md", "Target"), ("Keep.md", "[[Anchor|Alias]]\n")]);
    write(e.root.path(), "Unrelated.md", "New note");
    if remove {
        scan::scan(&mut e.db, Scope::All).unwrap();
        std::fs::remove_file(e.root.path().join("Unrelated.md")).unwrap();
    }
    let scope = if scoped {
        Scope::Paths(vec!["Unrelated.md".into()])
    } else {
        Scope::All
    };
    let mut plan = scan::prepare(e.db.conn(), scope)
        .unwrap()
        .read(&mut |_| {})
        .unwrap();
    assert_eq!(
        plan.total(),
        1,
        "an unrelated target must not schedule referrer reads"
    );
    while plan.apply_some(&mut e.db, scan::BATCH).unwrap() > 0 {}
    assert_eq!(
        e.page("Keep.md").body,
        format!("[Alias](page:{})\n", e.page("Anchor.md").id)
    );
}

#[test]
fn watcher_add_does_not_reindex_unrelated_pages() {
    unrelated_target_does_not_reindex_other_pages(true, false);
}
#[test]
fn watcher_remove_does_not_reindex_unrelated_pages() {
    unrelated_target_does_not_reindex_other_pages(true, true);
}
#[test]
fn full_add_does_not_reindex_unrelated_pages() {
    unrelated_target_does_not_reindex_other_pages(false, false);
}
#[test]
fn full_remove_does_not_reindex_unrelated_pages() {
    unrelated_target_does_not_reindex_other_pages(false, true);
}

#[test]
fn topology_refresh_follows_aliased_wiki_links_to_nearer_same_named_pages() {
    let mut e = Env::new(&[
        ("Far/Guide.md", "Far"),
        ("Near/Links.md", "[[gUiDe|Alias]]\n"),
    ]);
    let far = e.page("Far/Guide.md");
    assert_eq!(
        e.page("Near/Links.md").body,
        format!("[Alias](page:{})\n", far.id)
    );
    write(e.root.path(), "Near/Guide.md", "Near");
    let scope = Scope::Paths(vec!["Near/Guide.md".into()]);
    scan::scan(&mut e.db, scope.clone()).unwrap();
    assert_eq!(
        e.page("Near/Links.md").body,
        format!("[Alias](page:{})\n", e.page("Near/Guide.md").id)
    );
    std::fs::remove_file(e.root.path().join("Near/Guide.md")).unwrap();
    scan::scan(&mut e.db, scope).unwrap();
    assert_eq!(
        e.page("Near/Links.md").body,
        format!("[Alias](page:{})\n", far.id)
    );
}

#[test]
fn topology_refresh_handles_encoded_paths_fragments_and_literal_percent_text() {
    let mut e = Env::new(&[(
        "Links.md",
        "Literal %FF and 100%.\n[Alias](Guide%20book.md)\n[Section](Guide%20book.md#part)\n",
    )]);
    write(e.root.path(), "Guide book.md", "Target");
    let scope = Scope::Paths(vec!["Guide book.md".into()]);
    scan::scan(&mut e.db, scope.clone()).unwrap();
    let expected = format!(
        "Literal %FF and 100%.\n[Alias](page:{})\n[Section](folder:Guide%20book.md#part)\n",
        e.page("Guide book.md").id
    );
    assert_eq!(e.page("Links.md").body, expected);
    std::fs::remove_file(e.root.path().join("Guide book.md")).unwrap();
    scan::scan(&mut e.db, scope).unwrap();
    assert_eq!(e.page("Links.md").body, e.read("Links.md"));
}

#[test]
fn topology_refresh_handles_unicode_names_and_fully_encoded_relative_targets() {
    let mut e = Env::new(&[(
        "Links.md",
        "[[指南|Alias]]\n[Encoded](%E6%8C%87%E5%8D%97.md)\n[[ÄPFEL|Unicode case]]\n",
    )]);
    write(e.root.path(), "指南.md", "Target");
    write(e.root.path(), "Äpfel.md", "Other");
    let scope = Scope::Paths(vec!["指南.md".into(), "Äpfel.md".into()]);
    scan::scan(&mut e.db, scope.clone()).unwrap();
    assert_eq!(
        e.page("Links.md").body,
        format!(
            "[Alias](page:{0})\n[Encoded](page:{0})\n[Unicode case](page:{1})\n",
            e.page("指南.md").id,
            e.page("Äpfel.md").id
        )
    );
    for path in ["指南.md", "Äpfel.md"] {
        std::fs::remove_file(e.root.path().join(path)).unwrap();
    }
    scan::scan(&mut e.db, scope).unwrap();
    assert_eq!(e.page("Links.md").body, e.read("Links.md"));
}

#[test]
fn reference_refresh_uses_current_namespace_after_another_scan_between_batches() {
    let mut e = Env::new(&[
        ("Far/Same.md", "Far"),
        ("Near/L1.md", "[[Same|Alias]]\n"),
        ("Near/L2.md", "[[Same|Alias]]\n"),
    ]);
    write(e.root.path(), "Near/Same.md", "Near");
    let scope = Scope::Paths(vec!["Near/Same.md".into()]);
    let mut plan = scan::prepare(e.db.conn(), scope.clone())
        .unwrap()
        .read(&mut |_| {})
        .unwrap();
    // One item per transaction: the nearer target, then until the first referrer found it.
    assert_eq!(plan.apply_some(&mut e.db, 1).unwrap(), 1);
    let near = |e: &Env| format!("[Alias](page:{})\n", e.page("Near/Same.md").id);
    while e.page("Near/L1.md").body != near(&e) && e.page("Near/L2.md").body != near(&e) {
        assert!(plan.apply_some(&mut e.db, 1).unwrap() > 0, "no referrer was read again");
    }
    std::fs::remove_file(e.root.path().join("Near/Same.md")).unwrap();
    scan::scan(&mut e.db, scope).unwrap();
    while plan.apply_some(&mut e.db, 1).unwrap() > 0 {}
    let expected = format!("[Alias](page:{})\n", e.page("Far/Same.md").id);
    for path in ["Near/L1.md", "Near/L2.md"] {
        assert_eq!(e.page(path).body, expected);
    }
}

#[test]
fn reference_refresh_loads_the_namespace_at_most_once_per_batch() {
    let mut e = Env::new(&[("Far/Guide.md", "Far")]);
    for i in 0..16 {
        write(
            e.root.path(),
            &format!("Near/Link{i:02}.md"),
            "[[Guide|Alias]]\n",
        );
    }
    scan::scan(&mut e.db, Scope::All).unwrap();
    write(e.root.path(), "Near/Guide.md", "Near");
    let mut plan = scan::prepare(e.db.conn(), Scope::Paths(vec!["Near/Guide.md".into()]))
        .unwrap()
        .read(&mut |_| {})
        .unwrap();
    loop {
        super::store::STEM_LOADS.with(|loads| loads.set(0));
        let applied = plan.apply_some(&mut e.db, scan::BATCH).unwrap();
        let loads = super::store::STEM_LOADS.with(|loads| loads.get());
        assert!(
            loads <= 1,
            "one batch loaded the full namespace {loads} times"
        );
        if applied == 0 {
            break;
        }
    }
    let expected = format!("[Alias](page:{})\n", e.page("Near/Guide.md").id);
    for i in 0..16 {
        assert_eq!(e.page(&format!("Near/Link{i:02}.md")).body, expected);
    }
}

#[test]
fn topology_refresh_matches_contextual_unicode_case_in_raw_and_encoded_targets() {
    let mut e = Env::new(&[
        ("Raw.md", "[Alias](ΟΣ.md)\n"),
        ("Encoded.md", "[Alias](%CE%9F%CE%A3.md)\n"),
        ("Wiki.md", "[[ΟΣ.md|Alias]]\n"),
        ("Lower.md", "[[ος|Alias]]\n"),
    ]);
    write(e.root.path(), "ΟΣ.md", "Target");
    let scope = Scope::Paths(vec!["ΟΣ.md".into()]);
    scan::scan(&mut e.db, scope.clone()).unwrap();
    let expected = format!("[Alias](page:{})\n", e.page("ΟΣ.md").id);
    for path in ["Raw.md", "Encoded.md", "Wiki.md", "Lower.md"] {
        assert_eq!(e.page(path).body, expected, "{path}");
    }
    std::fs::remove_file(e.root.path().join("ΟΣ.md")).unwrap();
    scan::scan(&mut e.db, scope).unwrap();
    for path in ["Raw.md", "Encoded.md", "Wiki.md", "Lower.md"] {
        assert_eq!(e.page(path).body, e.read(path), "{path}");
    }
}

/// FR-KN-EXT-16: a page made or renamed in Cairn that a `[[Name]]` link would now find instead of
/// the page it found makes that link name its page's place; file and index keep agreeing.
#[test]
fn a_new_nearer_page_does_not_take_wiki_links_from_the_page_they_found() {
    let mut e = Env::new(&[
        ("Far/Guide.md", "Far\n"),
        ("Near/Links.md", "[[Guide|Alias]]\n"),
        ("Later.md", "[[Manual]]\n"),
    ]);
    let far = e.link("Alias", "Far/Guide.md");
    e.create("Near", "Guide");
    assert_eq!(e.read("Near/Links.md"), "[[Far/Guide|Alias]]\n");
    assert_eq!(e.page("Near/Links.md").body, format!("{far}\n"));
    e.unchanged_by_a_scan();
    e.rename("Far/Guide.md", "Handbook");
    assert_eq!(e.read("Near/Links.md"), "[[Far/Handbook|Alias]]\n");
    assert_eq!(e.page("Near/Links.md").body, format!("{far}\n"));
    e.unchanged_by_a_scan();
    // A link that found nothing finds a page made with its name (the file stays as it is).
    e.create("General", "Manual");
    assert_eq!(e.page("Later.md").body, format!("{}\n", e.link("Manual", "General/Manual.md")));
    assert_eq!(e.read("Later.md"), "[[Manual]]\n");
    e.unchanged_by_a_scan();
}

/// A page moved in Cairn keeps its own `[[…]]` links on the pages they found.
#[test]
fn a_moved_page_keeps_its_own_wiki_links() {
    let mut e = Env::new(&[("C1/Same.md", "One\n"), ("C2/Same.md", "Two\n"), ("C1/Mover.md", "[[Same]]\n")]);
    let one = e.link("Same", "C1/Same.md");
    let mover = e.page("C1/Mover.md").id;
    let c2: String = e.db.conn().query_row("SELECT id FROM categories WHERE name = 'C2'", [], |r| r.get(0)).unwrap();
    knowledge::move_page(&mut e.db, &mover, &c2).unwrap();
    assert_eq!(e.read("C2/Mover.md"), "[[C1/Same]]\n");
    assert_eq!(e.page("C2/Mover.md").body, format!("{one}\n"));
    e.unchanged_by_a_scan();
}

/// `[[…]]` links with a path: from the page's folder (`../`), from the root, or the end of a path;
/// case is ignored, also in the extension; a path that names no page finds nothing.
#[test]
fn wiki_links_with_paths_and_any_case_find_their_pages() {
    let e = Env::new(&[
        ("Ops/Overview.md", "O\n"),
        (
            "Ops/Pumps/Check.md",
            "[[../Overview|Rel]] [[ops/overview|Case]] [[Pumps/Check|Partial]] [[Ops/Overview|Exact]] [[Missing/Overview|Missing]]\n",
        ),
        ("Äpfel.md", "x\n"),
        ("C/Same.md", "y\n"),
        ("Links.md", "[[äpfel]] [[äpfel.md|A2]] [[C/Same.MD|S1]] [[c/same|S2]] [[same.MD|S3]]\n"),
    ]);
    let o = |t: &str| e.link(t, "Ops/Overview.md");
    assert_eq!(
        e.page("Ops/Pumps/Check.md").body,
        format!(
            "{} {} {} {} [[Missing/Overview|Missing]]\n",
            o("Rel"),
            o("Case"),
            e.link("Partial", "Ops/Pumps/Check.md"),
            o("Exact")
        )
    );
    let s = |t: &str| e.link(t, "C/Same.md");
    assert_eq!(
        e.page("Links.md").body,
        format!("{} {} {} {} {}\n", e.link("äpfel", "Äpfel.md"), e.link("A2", "Äpfel.md"), s("S1"), s("S2"), s("S3"))
    );
}

/// `![[image]]` finds an image anywhere in the folder, also when a page is read again after
/// another page arrived, after an outside edit (a watcher batch), and after a rename in Cairn.
#[test]
fn embeds_found_anywhere_in_the_folder_stay_found() {
    let mut e = Env::new(&[("Ops/diagrams/flow.png", "png"), ("Ops/Pumps/Check.md", "![[flow.png]] and [[Guide]]\n")]);
    let image = "![flow.png](folder:Ops/diagrams/flow.png)";
    assert_eq!(e.page("Ops/Pumps/Check.md").body, format!("{image} and [[Guide]]\n"));
    write(e.root.path(), "Ops/Guide.md", "Guide\n");
    scan::scan(&mut e.db, Scope::All).unwrap();
    assert_eq!(e.page("Ops/Pumps/Check.md").body, format!("{image} and {}\n", e.link("Guide", "Ops/Guide.md")));
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(e.root.path(), "Ops/Pumps/Check.md", "![[flow.png]] edited outside\n");
    scan::scan(&mut e.db, Scope::Paths(vec!["Ops/Pumps/Check.md".into()])).unwrap();
    assert_eq!(e.page("Ops/Pumps/Check.md").body, format!("{image} edited outside\n"));
    e.rename("Ops/Pumps/Check.md", "Checked");
    assert_eq!(e.read("Ops/Pumps/Checked.md"), "![[flow.png]] edited outside\n");
    assert_eq!(e.page("Ops/Pumps/Checked.md").body, format!("{image} edited outside\n"));
    e.unchanged_by_a_scan();
}

/// FR-KN-EXT-16: a page renamed outside Cairn keeps its links, also when the linking page is read
/// again for another reason and its `[[A]]` would now find another page; saving that page's text
/// in Cairn writes the new name.
#[test]
fn a_page_renamed_outside_keeps_its_links_until_the_linking_page_is_saved() {
    let mut e = Env::new(&[
        ("A.md", "Target\n"),
        ("Elsewhere/A.md", "Another A\n"),
        ("Ref.md", "[x](A.md) and [[A]] and [[Later]]\n"),
    ]);
    let a = e.page("A.md").id;
    let kept = format!("[x](page:{a}) and [A](page:{a})");
    assert_eq!(e.page("Ref.md").body, format!("{kept} and [[Later]]\n"));
    std::fs::rename(e.root.path().join("A.md"), e.root.path().join("B.md")).unwrap();
    let s = scan::scan(&mut e.db, Scope::All).unwrap();
    assert_eq!((s.renamed, s.removed, s.added), (1, 0, 0), "{s:?}");
    assert_eq!(e.page("B.md").id, a);
    assert_eq!(e.page("Ref.md").body, format!("{kept} and [[Later]]\n"));
    // A page that the third link names arrives: Ref is read again, and keeps its links to B.
    write(e.root.path(), "Later.md", "Later\n");
    let s = scan::scan(&mut e.db, Scope::Paths(vec!["Later.md".into()])).unwrap();
    assert_eq!(s.relinked, 1, "{s:?}");
    assert_eq!(e.page("Ref.md").body, format!("{kept} and {}\n", e.link("Later", "Later.md")));
    let r = e.page("Ref.md");
    knowledge::update_page(
        &mut e.db,
        &r.id,
        PagePatch {
            body: Some(format!("{}more\n", r.body)),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(e.read("Ref.md"), "[x](B.md) and [A](B.md) and [Later](Later.md)\nmore\n");
}

/// A page deleted in Cairn: the pages linking to it find what their files say now.
#[test]
fn deleting_a_page_reads_the_pages_linking_to_it_again() {
    let mut e = Env::new(&[("A.md", "Target\n"), ("Other/A.md", "Other\n"), ("Ref.md", "[x](A.md) and [[A]]\n")]);
    let a = e.page("A.md").id;
    assert_eq!(e.page("Ref.md").body, format!("[x](page:{a}) and [A](page:{a})\n"));
    knowledge::delete_page(&mut e.db, &a).unwrap();
    assert_eq!(e.page("Ref.md").body, format!("[x](A.md) and {}\n", e.link("A", "Other/A.md")));
    assert_eq!(e.read("Ref.md"), "[x](A.md) and [[A]]\n");
    e.unchanged_by_a_scan();
}

/// A new file changed again before its watcher batch was applied: the add waits for the next
/// batch, and so do the links to it (not a link to a plain file meanwhile).
#[test]
fn links_wait_for_a_new_page_whose_add_was_skipped() {
    let mut e = Env::new(&[("A.md", "[F](X.md)\n")]);
    write(e.root.path(), "X.md", "outside\n");
    let scope = Scope::Paths(vec!["X.md".into()]);
    let mut plan = scan::prepare(e.db.conn(), scope.clone()).unwrap().read(&mut |_| {}).unwrap();
    write(e.root.path(), "X.md", "outside, saved again with more text\n");
    while plan.apply_some(&mut e.db, scan::BATCH).unwrap() > 0 {}
    assert!(!plan.summary().changed_anything(), "{:?}", plan.summary());
    assert_eq!(e.page("A.md").body, "[F](X.md)\n");
    let s = scan::scan(&mut e.db, scope).unwrap();
    assert_eq!((s.added, s.relinked), (1, 1), "{s:?}");
    assert_eq!(e.page("A.md").body, format!("{}\n", e.link("F", "X.md")));
}

/// A `%` that isn't an escape doesn't make a page a candidate for every link refresh.
#[test]
fn percent_signs_in_text_do_not_cause_rereads() {
    let mut e = Env::new(&[("Anchor.md", "A\n"), ("Keep.md", "50% done [[Missing]] [[Anchor|Alias]]\n")]);
    write(e.root.path(), "Unrelated.md", "U\n");
    scan::REREADS.with(|n| n.set(0));
    let s = scan::scan(&mut e.db, Scope::Paths(vec!["Unrelated.md".into()])).unwrap();
    assert_eq!((s.added, s.relinked), (1, 0), "{s:?}");
    assert_eq!(scan::REREADS.with(|n| n.get()), 0);
}

/// A page removed outside, or deleted in Cairn: pages linking only to one of its headings
/// (`folder:` links in the index) find what their files say now.
#[test]
fn a_removed_page_unlinks_the_links_to_its_headings() {
    let mut e = Env::new(&[
        ("A.md", "# Part\nTarget\n"),
        ("B.md", "# Part\nOther\n"),
        ("Ref.md", "[Section](A.md#part)\n"),
        ("Ref2.md", "[Section](B.md#part)\n"),
    ]);
    assert_eq!(e.page("Ref.md").body, "[Section](folder:A.md#part)\n");
    std::fs::remove_file(e.root.path().join("A.md")).unwrap();
    scan::scan(&mut e.db, Scope::Paths(vec!["A.md".into()])).unwrap();
    assert_eq!(e.page("Ref.md").body, "[Section](A.md#part)\n");
    let b = e.page("B.md").id;
    knowledge::delete_page(&mut e.db, &b).unwrap();
    assert_eq!(e.page("Ref2.md").body, "[Section](B.md#part)\n");
}

/// A link whose whole path is percent-encoded finds a page that arrives later.
#[test]
fn a_fully_encoded_link_finds_a_page_that_arrives() {
    let mut e = Env::new(&[("Ref.md", "[A](%41%2E%6D%64)\n")]);
    write(e.root.path(), "A.md", "Target\n");
    scan::scan(&mut e.db, Scope::Paths(vec!["A.md".into()])).unwrap();
    assert_eq!(e.page("Ref.md").body, format!("{}\n", e.link("A", "A.md")));
}

/// A linking page edited outside (its front matter) and not yet scanned when Cairn renames the page
/// it links to: its link is rewritten from what the file found, and the index takes the file whole.
#[test]
fn renaming_reads_in_an_outside_edit_of_a_linking_page_whole() {
    let mut e = Env::new(&[("A.md", "Target\n"), ("Other/A.md", "Other\n"), ("Ref.md", "[[A]]\n")]);
    let target = e.page("A.md").id;
    write(e.root.path(), "Ref.md", "---\ntags: [changed]\n---\n\n[[A]]\n");
    e.rename("A.md", "B");
    assert_eq!(e.read("Ref.md"), "---\ntags: [changed]\n---\n\n[[B]]\n");
    let r = e.page("Ref.md");
    assert_eq!(r.body, format!("[B](page:{target})\n"));
    assert_eq!(r.tags, ["changed"]);
    e.unchanged_by_a_scan();
}

/// A page moved outside keeps its links although another page with its title is nearer to them
/// now: only a page that arrives takes a link.
#[test]
fn a_page_moved_outside_keeps_its_links_from_a_page_with_its_title() {
    let mut e = Env::new(&[("A/Guide.md", "unique original\n"), ("B/Guide.md", "other\n"), ("A/Ref.md", "[[Guide]]\n")]);
    let target = e.page("A/Guide.md").id;
    std::fs::create_dir_all(e.root.path().join("Z")).unwrap();
    std::fs::rename(e.root.path().join("A/Guide.md"), e.root.path().join("Z/Guide.md")).unwrap();
    scan::scan(&mut e.db, Scope::All).unwrap();
    assert_eq!(e.page("Z/Guide.md").id, target);
    assert_eq!(e.page("A/Ref.md").body, format!("[Guide](page:{target})\n"));
}

/// A page in the root keeps its links when a page with its title is made nearer to them: they
/// name it from their folder, as the root has no other path for it.
#[test]
fn a_root_page_keeps_its_links_when_a_nearer_page_is_made() {
    let mut e = Env::new(&[("Guide.md", "Original\n"), ("Near/Ref.md", "[[Guide]]\n")]);
    let original = e.link("Guide", "Guide.md");
    e.create("Near", "Guide");
    assert_eq!(e.read("Near/Ref.md"), "[[../Guide]]\n");
    assert_eq!(e.page("Near/Ref.md").body, format!("{original}\n"));
    e.unchanged_by_a_scan();
}

/// A link kept after an outside rename stays on its page when Cairn renames that page back.
#[test]
fn renaming_back_in_cairn_keeps_a_link_kept_after_an_outside_rename() {
    let mut e = Env::new(&[("A.md", "Target\n"), ("Other/A.md", "Other\n"), ("Ref.md", "[[A]]\n")]);
    let original = format!("[A](page:{})\n", e.page("A.md").id);
    std::fs::rename(e.root.path().join("A.md"), e.root.path().join("B.md")).unwrap();
    scan::scan(&mut e.db, Scope::All).unwrap();
    assert_eq!(e.page("Ref.md").body, original);
    e.rename("B.md", "A");
    assert_eq!(e.read("Ref.md"), "[[A]]\n");
    assert_eq!(e.page("Ref.md").body, original);
    e.unchanged_by_a_scan();
}

/// A page moved outside in the same scan as a page with a title it links to arrives nearer to it:
/// its link follows the page that arrived, as any page's does.
#[test]
fn a_page_moved_outside_follows_a_nearer_page_added_in_the_same_scan() {
    let mut e = Env::new(&[("Far/Guide.md", "Far\n"), ("Old/Ref.md", "[[Guide]]\n")]);
    assert_eq!(e.page("Old/Ref.md").body, format!("{}\n", e.link("Guide", "Far/Guide.md")));
    std::fs::create_dir_all(e.root.path().join("Near")).unwrap();
    std::fs::rename(e.root.path().join("Old/Ref.md"), e.root.path().join("Near/Ref.md")).unwrap();
    write(e.root.path(), "Near/Guide.md", "Near\n");
    let s = scan::scan(&mut e.db, Scope::All).unwrap();
    assert_eq!((s.renamed, s.added), (1, 1), "{s:?}");
    assert_eq!(e.page("Near/Ref.md").body, format!("{}\n", e.link("Guide", "Near/Guide.md")));
    e.unchanged_by_a_scan();
}

/// `![[…]]` finds its file ignoring case, as `[[…]]` finds pages: in a full scan, a watcher batch
/// and a rename in Cairn.
#[test]
fn embeds_find_their_file_whatever_its_case() {
    let mut e = Env::new(&[("Ops/diagrams/flow.png", "png"), ("Ops/Pumps/Check.md", "![[Flow.PNG]]\n")]);
    let image = "![Flow.PNG](folder:Ops/diagrams/flow.png)";
    assert_eq!(e.page("Ops/Pumps/Check.md").body, format!("{image}\n"));
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(e.root.path(), "Ops/Pumps/Check.md", "![[Flow.PNG]] edited\n");
    scan::scan(&mut e.db, Scope::Paths(vec!["Ops/Pumps/Check.md".into()])).unwrap();
    assert_eq!(e.page("Ops/Pumps/Check.md").body, format!("{image} edited\n"));
    e.rename("Ops/Pumps/Check.md", "Checked");
    assert_eq!(e.page("Ops/Pumps/Checked.md").body, format!("{image} edited\n"));
    e.unchanged_by_a_scan();
}
