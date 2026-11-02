//! References in an Assistant message (FR-LLM-AS-14, D-81): the tasks and pages a message points at
//! with the mention syntax of DM-MN-3, `#KEY` and `[Title](page:<id>)`, chosen from the composer's
//! suggestions or typed.
//!
//! A referenced item that exists is pinned: it goes to the model as a source on top of the search,
//! and to the change step as a candidate, so "add a comment to #PRX-12" names its target. The
//! model reads a page link as `[[Title]]` (the page's current title), never its id; task keys are
//! left as they are. Mentions inside code don't count, as in pages.

use rusqlite::Connection;

use crate::assistant::formats::ItemInfo;
use crate::assistant::items::{self, ItemRef};
use crate::{Result, knowledge, mentions, tasks};

/// The items `text` refers to that exist, in order of first appearance, pages before tasks; none twice.
pub fn items(conn: &Connection, text: &str) -> Result<Vec<ItemRef>> {
    let (page_ids, keys) = mentions::parse(text);
    let mut out: Vec<ItemRef> = Vec::new();
    for id in page_ids {
        if knowledge::get_page(conn, &id).is_ok() {
            push(&mut out, ItemRef::page(id));
        }
    }
    for key in keys {
        // A key the task had before a move (an alias) still finds it.
        if let Ok(t) = tasks::get(conn, &key) {
            push(&mut out, ItemRef::task(t.id));
        }
    }
    Ok(out)
}

fn push(out: &mut Vec<ItemRef>, r: ItemRef) {
    if !out.contains(&r) {
        out.push(r);
    }
}

/// `text` as the model reads it: each page link becomes `[[Title]]`, with the page's current title
/// (the stored one when the page is gone).
pub fn for_model(conn: &Connection, text: &str) -> String {
    mentions::to_readable(
        text,
        &mut |id| knowledge::get_page(conn, id).ok().map(|p| format!("[[{}]]", p.title)),
        &mut |_| None,
    )
}

/// The referenced items as source text for a draft revision. The caller fits this together with
/// the original source, so references share its context budget rather than bypassing it.
pub fn source_text(conn: &Connection, text: &str) -> Result<String> {
    let mut blocks = Vec::new();
    for item in items::load(conn, &items(conn, text)?)? {
        let mut body = match &item {
            ItemInfo::Page(p) => p.body.clone(),
            ItemInfo::Task(t) => {
                let mut body = t.description.clone();
                for comment in tasks::comments(conn, &t.id)? {
                    body.push_str(&format!("\n\nComment {}:\n{}", comment.created_at, comment.text));
                }
                body
            }
        };
        body = for_model(conn, &body);
        blocks.push(format!("{}\n{body}", item.head()));
    }
    Ok(blocks.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use crate::categories::{self, CategoryColor, NewCategory};
    use crate::knowledge::NewPage;
    use crate::tasks::NewTask;

    struct Env {
        _dir: tempfile::TempDir,
        db: Db,
        task: tasks::Task,
        page: knowledge::Page,
    }

    fn env() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_at(dir.path()).unwrap();
        let category = categories::create(
            db.conn(),
            NewCategory {
                name: "Alpha".into(),
                key: "AAA".into(),
                description: String::new(),
                color: CategoryColor::preset("blue"),
            },
        )
        .unwrap()
        .id;
        let task = tasks::create(
            &mut db,
            NewTask { category_id: category.clone(), title: "Pump seal".into(), ..NewTask::default() },
        )
        .unwrap();
        let page = knowledge::create_page(
            &mut db,
            NewPage { category_id: category, title: "VPN setup".into(), body: "Steps.".into(), ..NewPage::default() },
        )
        .unwrap();
        Env { _dir: dir, db, task, page }
    }

    #[test]
    fn plain_text_has_no_references() {
        let e = env();
        assert!(items(e.db.conn(), "What do my notes say about VPNs?").unwrap().is_empty());
        assert_eq!(for_model(e.db.conn(), "Plain [link](https://x.org)"), "Plain [link](https://x.org)");
    }

    #[test]
    fn existing_items_are_referenced_once_pages_first() {
        let e = env();
        let text = format!(
            "Comment on #{} about [Old name](page:{}), then #{} again",
            e.task.key, e.page.id, e.task.key
        );
        assert_eq!(
            items(e.db.conn(), &text).unwrap(),
            vec![ItemRef::page(e.page.id.clone()), ItemRef::task(e.task.id.clone())]
        );
    }

    #[test]
    fn unknown_items_and_code_are_left_out() {
        let e = env();
        let text = format!("#ZZ-9, [Gone](page:nope) and `#{}`", e.task.key);
        assert!(items(e.db.conn(), &text).unwrap().is_empty());
        assert!(source_text(e.db.conn(), &text).unwrap().is_empty());
    }

    #[test]
    fn the_model_reads_page_titles_not_ids() {
        let e = env();
        let text = format!("Add a line to [Old name](page:{}) for #{}; see [Gone](page:nope)", e.page.id, e.task.key);
        assert_eq!(
            for_model(e.db.conn(), &text),
            format!("Add a line to [[VPN setup]] for #{}; see Gone", e.task.key)
        );
    }
}
