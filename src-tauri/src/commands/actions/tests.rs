//! The action branch end to end against the fake sidecar (`tests/fixtures/fake_sidecar.py`, its
//! "M6 actions block") with a temporary data folder: no window, so no `check` call after a new page.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cairn_core::DataPaths;
use cairn_core::assistant::formats::PiecePart;
use cairn_core::assistant::parts::{LinkKind, Proposal};
use cairn_core::assistant::plan::{PlanFilters, RequestKind};
use cairn_core::conversations::{self, ChatRole, NewConversation, NewMessage};
use cairn_core::knowledge::{NewPage, PagePatch};
use cairn_core::categories::{NewCategory, CategoryColor};
use cairn_core::providers::{AuthMode, NewModel, NewProvider, ProviderKind, RequestFormat};
use cairn_core::secrets::MemoryStore;
use cairn_core::tasks::{NewTask, Priority, TaskStatus};

use super::*;
use crate::sidecar::{Sidecars, SidecarConfig};

struct Env {
    _dir: tempfile::TempDir,
    state: AppState,
    sidecars: Sidecars,
    deltas: Arc<Mutex<Vec<String>>>,
    conversation: String,
    model: String,
    category: String,
    /// t1, t2, t3.
    tasks: Vec<cairn_core::tasks::Task>,
    page: cairn_core::knowledge::Page,
}

fn fake_config() -> SidecarConfig {
    SidecarConfig {
        python: Some(PathBuf::from("python3")),
        script_args: vec![
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake_sidecar.py").into(),
        ],
        model_dir: Some(PathBuf::from("/nonexistent/model")),
        idle_timeout: Duration::from_secs(60),
        embed_threads: 1,
        start_timeout: Duration::from_secs(10),
    }
}

async fn setup() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::open(DataPaths::new(dir.path()), Arc::new(MemoryStore::default()));
    let sidecars = Sidecars::new(fake_config(), Arc::new(|_| {}));
    let (conversation, model, category, tasks, page) = state
        .with_db(|db| {
            let secrets = MemoryStore::default();
            let provider = cairn_core::providers::create_provider(
                db,
                &secrets,
                NewProvider {
                    name: "Local".into(),
                    kind: ProviderKind::Local,
                    endpoint: Some("http://localhost:1/v1".into()),
                    api_version: None,
                    auth: AuthMode::None,
                },
            )?;
            let model = cairn_core::providers::create_model(
                db,
                NewModel {
                    provider_id: provider.id,
                    name: "chat-model".into(),
                    label: Some("Chat Model".into()),
                    format: RequestFormat::ChatCompletions,
                    vision: Some(false),
                    effort: None,
                    context_size: None,
                    web_search: None,
                },
            )?;
            let category = categories::create(
                db.conn(),
                NewCategory {
                    name: "Alpha".into(),
                    key: "AAA".into(),
                    description: String::new(),
                    color: CategoryColor::preset("blue"),
                },
            )?
            .id;
            let mut tasks = Vec::new();
            for title in ["Auger hose wear", "Pump seal", "Grease the rollers"] {
                tasks.push(cairn_core::tasks::create(
                    db,
                    NewTask {
                        category_id: category.clone(),
                        title: title.into(),
                        due_date: Some("2026-10-20".into()),
                        ..NewTask::default()
                    },
                )?);
            }
            let page = knowledge::create_page(
                db,
                NewPage {
                    category_id: category.clone(),
                    title: "Pre-start".into(),
                    body: "# Pre-start\n\nCheck hoses.".into(),
                    ..NewPage::default()
                },
            )?;
            let conversation = conversations::create(db, NewConversation::default())?;
            Ok((conversation.id, model.id, category, tasks, page))
        })
        .await
        .unwrap();
    Env {
        _dir: dir,
        state,
        sidecars,
        deltas: Arc::default(),
        conversation,
        model,
        category,
        tasks,
        page,
    }
}

/// What `run` returned, with the stored assistant message.
struct Ran {
    done: Result<Done, JobError>,
    message: ChatMessage,
    progress: Progress,
}

impl Env {
    fn ctx(&self) -> Ctx<'_> {
        let sink = self.deltas.clone();
        Ctx {
            app: None,
            state: &self.state,
            sidecars: &self.sidecars,
            sink: Some(Arc::new(move |t| sink.lock().unwrap().push(t))),
            tally: Default::default(),
        }
    }

    async fn run(&self, request: &str) -> Ran {
        self.run_with(request, vec![]).await
    }

    async fn run_with(&self, request: &str, sources: Vec<SourcePiece>) -> Ran {
        let (conversation, request_text) = (self.conversation.clone(), request.to_string());
        let (user, assistant) = self
            .state
            .with_db(move |db| {
                let new = |role, text: String| NewMessage {
                    role,
                    text,
                    model_id: None,
                    model_label: None,
                    effort: None,
                    attachment: None,
                    steps: Some(vec![]),
                    draft: None,
                    answer: None,
                    find: None,
                    proposal: None,
                    extra: None,
                };
                let u = conversations::add_message(db, &conversation, new(ChatRole::User, request_text))?;
                let a = conversations::add_message(db, &conversation, new(ChatRole::Assistant, String::new()))?;
                Ok((u.id, a.id))
            })
            .await
            .unwrap();
        let job = AskJob {
            conversation_id: self.conversation.clone(),
            user_message_id: user,
            assistant_id: assistant.clone(),
            text: request.to_string(),
            refs: Vec::new(),
            model_id: Some(self.model.clone()),
            effort: None,
            web: false,
            all_categories: false,
            upload: None,
            chat_file_id: None,
        };
        let plan = Plan {
            kind: RequestKind::Change,
            search: String::new(),
            filters: PlanFilters::default(),
            refs: vec![],
            outside_scope: vec![],
            intro: String::new(),
        };
        let mut candidates: Vec<ItemRef> = self.tasks.iter().map(|t| ItemRef::task(&t.id)).collect();
        candidates.push(ItemRef::page(&self.page.id));
        let input = ActionInput {
            candidates,
            sources,
            history: vec![],
            categories: vec![PlanCategory { id: self.category.clone(), name: "Alpha".into(), in_scope: true }],
            today: "2026-10-15 (Thursday)".into(),
            glossary: String::new(),
        };
        let mut progress = Progress::new(&assistant);
        let done = run(&self.ctx(), &job, &plan, input, &mut progress).await;
        let message = self.message(&assistant).await;
        Ran { done, message, progress }
    }

    async fn message(&self, id: &str) -> ChatMessage {
        let id = id.to_string();
        self.state
            .with_db(move |db| conversations::get_message(db.conn(), &id))
            .await
            .unwrap()
    }

    async fn confirm(&self, id: &str) -> (ChatMessage, Touched) {
        let id = id.to_string();
        self.state.with_db(move |db| confirm(db, &id)).await.unwrap()
    }

    async fn task(&self, id: &str) -> cairn_core::tasks::Task {
        let id = id.to_string();
        self.state
            .with_db(move |db| cairn_core::tasks::get(db.conn(), &id))
            .await
            .unwrap()
    }
}

fn proposal(m: &ChatMessage) -> &Proposal {
    m.proposal.as_ref().expect("a proposal")
}

#[tokio::test]
async fn a_create_becomes_a_card_with_terms_and_confirms() {
    let e = setup().await;
    let ran = e.run("CREATE a task to order a hose").await;
    let done = ran.done.ok().expect("done");
    assert_eq!(done.text, "Here is what I would change.");
    assert!(done.card.is_none());
    assert_eq!(ran.progress.steps[0].label, "Working out the changes with Chat Model");
    let p = proposal(&ran.message);
    assert_eq!(p.rows.len(), 1);
    assert!(p.rows[0].ticked && p.rows[0].state == RowState::Pending);
    let ProposedChange::CreateTask { category_id, title, priority, links, .. } = &p.rows[0].change else {
        panic!("not a create");
    };
    assert_eq!((category_id, title.as_str(), *priority), (&e.category, "Order a new auger hose", Priority::High));
    assert_eq!(links.len(), 2);
    assert_eq!(links[1].kind, LinkKind::Page);
    assert_eq!(p.terms.len(), 1);
    assert_eq!(p.terms[0].term, "MMU");
    assert_eq!(p.terms_category_id.as_ref(), Some(&e.category));

    let (m, touched) = e.confirm(&ran.message.id).await;
    assert_eq!(touched, Touched { tasks: true, knowledge: true });
    let row = &proposal(&m).rows[0];
    assert_eq!((row.state, row.result.as_deref()), (RowState::Done, Some("Created AAA-4")));
    let t = e.task(row.result_id.as_ref().unwrap()).await;
    assert_eq!((t.title.as_str(), t.priority), ("Order a new auger hose", Priority::High));
    // A second click applies nothing again.
    let (_, touched) = e.confirm(&ran.message.id).await;
    assert_eq!(touched, Touched::default());
}

#[tokio::test]
async fn three_updates_in_one_card_and_one_unticked() {
    let e = setup().await;
    let ran = e.run("THREE update them").await;
    let id = ran.message.id.clone();
    assert_eq!(proposal(&ran.message).rows.len(), 3);
    assert_eq!(ran.done.ok().unwrap().text, "Updating three tasks.");

    let sid = id.clone();
    let m = e.state.with_db(move |db| set_row(db, &sid, 1, false)).await.unwrap();
    assert!(!proposal(&m).rows[1].ticked);
    let bad = e.state.with_db({
        let id = id.clone();
        move |db| set_row(db, &id, 9, false)
    });
    assert!(bad.await.is_err());

    let (m, _) = e.confirm(&id).await;
    let rows = &proposal(&m).rows;
    assert_eq!(
        rows.iter().map(|r| (r.state, r.result.as_deref())).collect::<Vec<_>>(),
        [
            (RowState::Done, Some("Marked AAA-1 as done")),
            (RowState::Rejected, None),
            (RowState::Done, Some("Updated AAA-3")),
        ]
    );
    assert_eq!(e.task(&e.tasks[0].id).await.status, TaskStatus::Done);
    assert_eq!(e.task(&e.tasks[1].id).await.priority, Priority::Medium, "the unticked row changed nothing");
    let t3 = e.task(&e.tasks[2].id).await;
    assert_eq!((t3.due_date.as_deref(), t3.labels.as_slice()), (Some("2026-11-02"), ["site".to_string()].as_slice()));
    // A ticked row can't be changed after the confirmation.
    let again = e.state.with_db(move |db| set_row(db, &id, 0, false)).await;
    assert!(again.is_err());
}

#[tokio::test]
async fn the_change_step_reads_the_sources() {
    // D-83: a referenced task's comment reaches ask-action, so a task can be made from it.
    let e = setup().await;
    let id = e.tasks[0].id.clone();
    let info = e
        .state
        .with_db(move |db| cairn_core::assistant::items::load(db.conn(), &[ItemRef::task(&id)]))
        .await
        .unwrap()
        .remove(0);
    let comment = SourcePiece {
        item: Some(info),
        part: PiecePart::Comment("2026-10-27".into()),
        text: "The wifi drops every hour near the gate.".into(),
    };
    let ran = e.run_with("FROMSOURCE: a task from #AAA-1's latest comment", vec![comment]).await;
    let ProposedChange::CreateTask { description, .. } = &proposal(&ran.message).rows[0].change else {
        panic!("{:?}", proposal(&ran.message).rows[0].change)
    };
    assert_eq!(description, "The wifi drops every hour near the gate.");
}

#[tokio::test]
async fn a_comment_is_added() {
    let e = setup().await;
    let ran = e.run("COMMENT on the hose task").await;
    let (m, touched) = e.confirm(&ran.message.id).await;
    assert!(touched.tasks);
    assert_eq!(proposal(&m).rows[0].result.as_deref(), Some("Commented on AAA-1"));
    let id = e.tasks[0].id.clone();
    let comments = e
        .state
        .with_db(move |db| cairn_core::tasks::comments(db.conn(), &id))
        .await
        .unwrap();
    assert_eq!(comments[0].text, "Replaced the hose on site.");
}

#[tokio::test]
async fn an_ambiguous_request_asks_and_a_delete_is_declined() {
    let e = setup().await;
    let ran = e.run("AMBIGUOUS mark it done").await;
    assert_eq!(ran.done.ok().unwrap().text, "Which task do you mean?");
    assert!(ran.message.proposal.is_none());
    let ran = e.run("DELETE the pump task").await;
    assert!(ran.done.ok().unwrap().text.starts_with("The chat cannot delete"));
    assert!(ran.message.proposal.is_none());
}

#[tokio::test]
async fn a_proposal_whose_changes_all_drop_says_so_instead_of_the_models_reply() {
    let e = setup().await;
    let ran = e.run("NOOP set the hose task to to do").await;
    let text = ran.done.ok().unwrap().text;
    assert!(text.starts_with("Nothing needed changing"), "{text}");
    assert!(ran.message.proposal.is_none());
}

#[tokio::test]
async fn a_page_change_carries_a_diff_and_applies() {
    let e = setup().await;
    let ran = e.run("PAGEEDIT add a step").await;
    assert_eq!(ran.done.as_ref().ok().unwrap().text, "Changing the page.");
    assert!(ran.progress.steps.iter().any(|s| s.label == "Writing the change to Pre-start"));
    let p = proposal(&ran.message);
    let ProposedChange::UpdatePage { old_body, new_body, base_updated_at, title, .. } = &p.rows[0].change
    else {
        panic!("not a page change");
    };
    assert_eq!(title, "Pre-start");
    assert_eq!(old_body, &e.page.body);
    assert_eq!(new_body, "# Pre-start\n\nCheck hoses.\n\n- Add a step: check the seals.");
    assert_eq!(base_updated_at, &e.page.updated_at);

    let (m, touched) = e.confirm(&ran.message.id).await;
    assert!(touched.knowledge && !touched.tasks);
    let row = &proposal(&m).rows[0];
    assert_eq!(row.result.as_deref(), Some("Updated page “Pre-start”"));
    assert_eq!(row.result_id.as_deref(), Some(e.page.id.as_str()));
    let id = e.page.id.clone();
    let page = e.state.with_db(move |db| knowledge::get_page(db.conn(), &id)).await.unwrap();
    assert!(page.body.contains("check the seals"));
}

#[tokio::test]
async fn a_stale_page_change_fails_on_confirm() {
    let e = setup().await;
    let ran = e.run("PAGEEDIT add a step").await;
    let id = e.page.id.clone();
    e.state
        .with_db(move |db| {
            std::thread::sleep(Duration::from_millis(5));
            knowledge::update_page(
                db,
                &id,
                PagePatch { body: Some("# Pre-start\n\nEdited by hand.".into()), ..PagePatch::default() },
            )
        })
        .await
        .unwrap();
    let (m, touched) = e.confirm(&ran.message.id).await;
    assert!(!touched.knowledge);
    let row = &proposal(&m).rows[0];
    assert_eq!(row.state, RowState::Failed);
    assert_eq!(row.result.as_deref(), Some("The page changed since this was proposed. Ask again."));
}

#[tokio::test]
async fn a_page_change_that_cannot_be_made_or_changes_nothing_is_only_text() {
    let e = setup().await;
    let ran = e.run("PAGEEDIT CANNOT do it").await;
    assert!(ran.message.proposal.is_none());
    assert_eq!(
        ran.done.ok().unwrap().text,
        "Changing the page.\n\nThe page does not say what is missing."
    );
    let ran = e.run("PAGEEDIT SAME thing").await;
    assert!(ran.message.proposal.is_none());
    assert_eq!(ran.done.ok().unwrap().text, "Changing the page.\n\nNo change was made to “Pre-start”.");
}

#[tokio::test]
async fn a_new_page_streams_into_a_draft_card() {
    let e = setup().await;
    let ran = e.run("NEWPAGE write a guide").await;
    let done = ran.done.ok().unwrap();
    assert_eq!(done.text, "Writing a page.");
    assert!(ran.message.proposal.is_none(), "a page is a draft card, not a row");
    let card = done.card.expect("a draft card");
    assert_eq!(card.title, "Hose guide");
    assert_eq!(card.body, "# Hose guide\n\nFrom the sources.");
    assert_eq!((card.category_id.as_str(), card.state), (e.category.as_str(), DraftState::Draft));
    assert!(card.file_name.is_none() && card.chat_file_id.is_none());
    assert_eq!(e.deltas.lock().unwrap().concat(), "# Hose guide\n\nFrom the sources. [1]");
    assert!(ran.progress.steps.iter().any(|s| s.label == "Writing the page"));
}

#[tokio::test]
async fn a_page_and_a_page_change_in_one_message() {
    let e = setup().await;
    let ran = e.run("MIXED NEWPAGE and PAGEEDIT").await;
    let done = ran.done.ok().unwrap();
    assert!(done.card.is_some());
    assert_eq!(proposal(&ran.message).rows.len(), 1);
}

#[tokio::test]
async fn an_unreadable_reply_is_retried_once() {
    let e = setup().await;
    let ran = e.run("RETRYJSON COMMENT").await;
    assert_eq!(proposal(&ran.message).rows.len(), 1);
    let ran = e.run("UNREADABLE").await;
    let err = ran.done.err().expect("an error");
    assert_eq!(err.error.kind, "ai_unreadable");
}

#[tokio::test]
async fn reject_rejects_what_is_pending() {
    let e = setup().await;
    let ran = e.run("THREE update them").await;
    let id = ran.message.id.clone();
    let m = e.state.with_db(move |db| reject(db, &id)).await.unwrap();
    assert!(proposal(&m).rows.iter().all(|r| r.state == RowState::Rejected));
    assert_eq!(e.task(&e.tasks[0].id).await.status, TaskStatus::Todo);
    let (m, touched) = e.confirm(&ran.message.id).await;
    assert_eq!(touched, Touched::default());
    assert!(proposal(&m).rows.iter().all(|r| r.state == RowState::Rejected));
}
