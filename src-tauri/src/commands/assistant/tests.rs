//! The assistant job end to end against the fake sidecar (`tests/fixtures/fake_sidecar.py`, its
//! M6 block) with a temporary data folder: no window (`Ctx.app` is `None`). The fake answers by
//! markers in the message (FINDME, GENERALQ, WEBSEARCHED, …).

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cairn_core::DataPaths;
use cairn_core::assistant::parts::SourceKind;
use cairn_core::conversations::{ConversationPatch, NewConversation, StepState};
use cairn_core::knowledge::{self, NewPage};
use cairn_core::providers::{
    AuthMode, EffortConfig, ModelPatch, NewModel, NewProvider, ProviderKind, RequestFormat, create_model, create_provider,
};
use cairn_core::secrets::MemoryStore;
use cairn_core::settings::SettingsPatch;
use cairn_core::tasks::{self, NewTask, TaskStatus};
use serde_json::{Value, json};

use super::*;
use crate::sidecar::{SidecarConfig, Worker};

struct Env {
    _dir: tempfile::TempDir,
    state: AppState,
    sidecars: Sidecars,
    uploads: Uploads,
    deltas: Arc<std::sync::Mutex<Vec<String>>>,
    conversation: String,
    category: String,
    chat_model: String,
    small_model: String,
    responses_model: String,
}

fn fake_config() -> SidecarConfig {
    SidecarConfig {
        python: Some(PathBuf::from("python3")),
        script_args: vec![
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_sidecar.py"
            )
            .into(),
        ],
        model_dir: Some(PathBuf::from("/nonexistent/model")),
        idle_timeout: Duration::from_secs(60),
        embed_threads: 1,
        start_timeout: Duration::from_secs(10),
    }
}

/// `default` sets the default model (needed for `doc-terms`).
async fn setup(default: bool) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::open(DataPaths::new(dir.path()), Arc::new(MemoryStore::default()));
    let sidecars = Sidecars::new(fake_config(), Arc::new(|_| {}));
    let (conversation, category, chat_model, small_model, responses_model) = state
        .with_db(move |db| {
            let secrets = MemoryStore::default();
            let mut provider = |name: &str, kind| {
                create_provider(
                    db,
                    &secrets,
                    NewProvider {
                        name: name.into(),
                        kind,
                        endpoint: Some("http://localhost:1/v1".into()),
                        api_version: None,
                        auth: AuthMode::None,
                    },
                )
            };
            let local = provider("Local", ProviderKind::Local)?;
            let remote = provider("Remote", ProviderKind::OpenaiCompatible)?;
            let mut model = |provider_id: &str, name: &str, label: &str, format| {
                create_model(
                    db,
                    NewModel {
                        provider_id: provider_id.into(),
                        name: name.into(),
                        label: Some(label.into()),
                        format,
                        vision: None,
                        effort: None,
                        context_size: None,
                        web_search: Some(true),
                    },
                )
            };
            let chat = model(&local.id, "chat-model", "Chat Model", RequestFormat::ChatCompletions)?;
            let small = model(&local.id, "ctx-small", "Small Model", RequestFormat::ChatCompletions)?;
            let responses = model(&remote.id, "resp-model", "Responses Model", RequestFormat::Responses)?;
            if default {
                cairn_core::settings::update(
                    db,
                    SettingsPatch {
                        default_model_id: Some(chat.id.clone()),
                        ..SettingsPatch::default()
                    },
                )?;
            }
            let conversation = conversations::create(db, NewConversation::default())?;
            let category: String =
                db.conn()
                    .query_row("SELECT id FROM categories WHERE is_general = 1", [], |r| {
                        r.get(0)
                    })?;
            Ok((conversation.id, category, chat.id, small.id, responses.id))
        })
        .await
        .unwrap();
    Env {
        _dir: dir,
        state,
        sidecars,
        uploads: Uploads::default(),
        deltas: Arc::default(),
        conversation,
        category,
        chat_model,
        small_model,
        responses_model,
    }
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

    async fn page(&self, category: &str, title: &str, body: &str) -> String {
        let (category, title, body) = (category.to_string(), title.to_string(), body.to_string());
        self.state
            .with_db(move |db| {
                Ok(knowledge::create_page(
                    db,
                    NewPage {
                        category_id: category,
                        title,
                        body,
                        group_id: None,
                        tags: None,
                        source_info: None,
                        origin: None,
                        origin_task_id: None,
                        check_status: None,
                    },
                )?
                .id)
            })
            .await
            .unwrap()
    }

    async fn task(&self, title: &str, description: &str, status: TaskStatus) -> String {
        let (category, title, description) =
            (self.category.clone(), title.to_string(), description.to_string());
        self.state
            .with_db(move |db| {
                Ok(tasks::create(
                    db,
                    NewTask {
                        category_id: category,
                        title,
                        description: Some(description),
                        status: Some(status),
                        ..NewTask::default()
                    },
                )?
                .id)
            })
            .await
            .unwrap()
    }

    async fn category_named(&self, name: &str, key: &str) -> String {
        let (name, key) = (name.to_string(), key.to_string());
        self.state
            .with_db(move |db| {
                Ok(cairn_core::categories::create(
                    db.conn(),
                    cairn_core::categories::NewCategory {
                        name,
                        key,
                        description: String::new(),
                        color: cairn_core::categories::CategoryColor::preset("blue"),
                    },
                )?
                .id)
            })
            .await
            .unwrap()
    }

    fn input(&self, text: &str) -> SendInput {
        SendInput {
            conversation_id: self.conversation.clone(),
            text: text.into(),
            model_id: Some(self.chat_model.clone()),
            effort: Some("high".into()),
            upload_id: None,
            web: false,
        }
    }

    /// Sends a message and runs its job to the end; returns the assistant message.
    async fn ask(&self, input: SendInput) -> ChatMessage {
        let (job, started) = prepare_send(&self.state, &self.uploads, input).await.unwrap();
        files::run_job(&self.ctx(), Job::Ask(job)).await;
        self.message(&started.assistant_message_id).await
    }

    async fn message(&self, id: &str) -> ChatMessage {
        let id = id.to_string();
        self.state
            .with_db(move |db| conversations::get_message(db.conn(), &id))
            .await
            .unwrap()
    }

    async fn messages(&self) -> Vec<ChatMessage> {
        let id = self.conversation.clone();
        self.state
            .with_db(move |db| conversations::messages(db.conn(), &id))
            .await
            .unwrap()
    }

    async fn calls(&self) -> Vec<Value> {
        let r = self
            .sidecars
            .call(Worker::Ai, "calls", json!({}))
            .await
            .unwrap();
        r["calls"].as_array().unwrap().clone()
    }

    async fn call_kinds(&self) -> Vec<String> {
        self.calls()
            .await
            .iter()
            .filter(|c| c["method"] == "llm.complete")
            .map(|c| c["kind"].as_str().unwrap().to_string())
            .collect()
    }

    fn upload(&self, name: &str, content: &[u8]) -> String {
        let id = files::begin_upload(&self.state.paths, &self.uploads, name, content.len() as u32)
            .unwrap();
        let (a, b) = content.split_at(content.len() / 2);
        files::append_chunk(&self.uploads, &id, &STANDARD.encode(a)).unwrap();
        files::append_chunk(&self.uploads, &id, &STANDARD.encode(b)).unwrap();
        id
    }

    async fn model_has_web_search(&self, id: &str) -> bool {
        let id = id.to_string();
        self.state
            .with_db(move |db| Ok(providers::get_model(db.conn(), &id)?.web_search))
            .await
            .unwrap()
    }
}

fn labels(m: &ChatMessage) -> Vec<&str> {
    m.steps.iter().map(|s| s.label.as_str()).collect()
}

fn assert_finished(m: &ChatMessage) {
    assert!(m.error.is_none(), "{:?}", m.error);
    assert!(
        m.steps.iter().all(|s| s.state == StepState::Done),
        "{:?}",
        m.steps
    );
}

// ---------------------------------------------------------------- questions

/// D-65: the reply records the effort level its calls sent, and the reasoning tokens reported.
#[tokio::test]
async fn the_reply_records_the_effort_sent_and_the_reasoning_reported() {
    let env = setup(false).await;
    env.page(&env.category, "Hose guide", "hose replacement steps").await;
    // Effort is off for the model: nothing is sent, whatever was picked.
    let m = env.ask(env.input("replace hose")).await;
    assert_eq!((m.effort.as_deref(), m.sent_effort.as_deref()), (Some("high"), Some("")));
    assert_eq!(m.reasoning_tokens, None);
    // On, with a default level: the picked level is sent, and the reasoning is added up.
    let id = env.chat_model.clone();
    env.state
        .with_db(move |db| {
            cairn_core::providers::update_model(
                db,
                &id,
                ModelPatch {
                    effort: Some(EffortConfig {
                        enabled: true,
                        default_level: Some("low".into()),
                        ..EffortConfig::default()
                    }),
                    ..ModelPatch::default()
                },
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let m = env.ask(env.input("replace hose")).await;
    assert_eq!(m.sent_effort.as_deref(), Some("high"));
    // The fake worker reports 10 per call that was sent a level; the plan step and the answer.
    assert!(m.reasoning_tokens.unwrap_or(0) >= 20, "{:?}", m.reasoning_tokens);
    // Nothing picked: the model's default level.
    let mut input = env.input("replace hose");
    input.effort = None;
    let m = env.ask(input).await;
    assert_eq!((m.effort.as_deref(), m.sent_effort.as_deref()), (None, Some("low")));
}

#[tokio::test]
async fn a_question_is_answered_from_the_sources_with_its_markers_read() {
    let env = setup(false).await;
    let page = env.page(&env.category, "Hose guide", "hose replacement steps").await;
    env.page(&env.category, "Fuel", "nothing to do with it").await;
    let m = env.ask(env.input("replace hose")).await;
    assert_finished(&m);
    let answer = m.answer.clone().unwrap();
    // The invalid marker [99] is gone, [1] stays; the cited list says so.
    assert!(m.text.contains("[1]") && !m.text.contains("[99]"), "{}", m.text);
    assert!(m.text.contains("Sources: 1."), "{}", m.text);
    assert_eq!(answer.basis, AnswerBasis::Notes);
    assert_eq!(answer.cited, vec![1]);
    assert_eq!(answer.sources.len(), 1);
    assert_eq!(answer.sources[0].id.as_deref(), Some(page.as_str()));
    assert_eq!(answer.sources[0].kind, SourceKind::Page);
    assert!(answer.sources[0].snippet.contains("hose replacement"));
    assert_eq!(
        labels(&m),
        [
            "Reading your question",
            "Found 1 item",
            "Writing the answer with Chat Model"
        ]
    );
    assert_eq!(m.effort.as_deref(), Some("high"));
    // The text was streamed, and the effort went with the calls.
    assert!(env.deltas.lock().unwrap().concat().contains("Sources: 1"));
    let calls = env.calls().await;
    let answer_call = calls.iter().find(|c| c["kind"] == "ask-answer").unwrap();
    assert_eq!(answer_call["effort"], "high");
    assert!(answer_call["last"].as_str().unwrap().contains("Hose guide"));
    // The conversation is titled by its first message; the user message is stored as typed.
    let all = env.messages().await;
    assert_eq!(all[0].text, "replace hose");
    let title = env
        .state
        .with_db({
            let id = env.conversation.clone();
            move |db| Ok(conversations::get(db.conn(), &id)?.title)
        })
        .await
        .unwrap();
    assert_eq!(title, "replace hose");
}

#[tokio::test]
async fn nothing_found_is_not_from_your_notes_and_a_general_question_is_labelled() {
    let env = setup(false).await;
    env.page(&env.category, "Hose guide", "hose replacement steps").await;
    let m = env.ask(env.input("what about zzzqqq")).await;
    let a = m.answer.unwrap();
    assert_eq!(a.basis, AnswerBasis::NotInNotes);
    assert!(a.sources.is_empty() && a.cited.is_empty());
    assert!(m.text.starts_with("Your notes don't cover this."));

    let m = env.ask(env.input("GENERALQ why is the sky blue")).await;
    assert_eq!(m.answer.unwrap().basis, AnswerBasis::General);
    assert!(m.text.contains("General knowledge, not from your notes"));
    // No search for a general question: the plan, then the answer.
    let kinds = env.call_kinds().await;
    assert_eq!(kinds.len(), 4, "{kinds:?}");
}

#[tokio::test]
async fn an_unreadable_plan_is_asked_again_once_then_the_message_is_searched_as_it_is() {
    let env = setup(false).await;
    env.page(&env.category, "Hose guide", "hose replacement steps").await;
    // Unreadable first, fine on the retry.
    let m = env.ask(env.input("PLANRETRY hose")).await;
    assert_eq!(env.call_kinds().await, ["ask-plan", "ask-plan", "ask-answer"]);
    assert_eq!(m.answer.unwrap().sources.len(), 1);
    // Unreadable twice: the fallback searches the message's own words (any word finds the page).
    let m = env.ask(env.input("BADPLAN hose")).await;
    assert_finished(&m);
    assert_eq!(m.answer.unwrap().sources.len(), 1);
}

#[tokio::test]
async fn the_earlier_turns_go_with_the_question_within_the_history_budget() {
    let env = setup(false).await;
    env.page(&env.category, "Hose guide", "hose replacement steps").await;
    env.ask(env.input("hose")).await;
    let m = env.ask(env.input("hose again")).await;
    assert!(m.text.contains("Earlier turns: 2."), "{}", m.text);
    // The cited page of the first answer is sent again as a source (pinned) even for words
    // that don't find it.
    let m = env.ask(env.input("zzzqqq")).await;
    assert!(m.text.contains("Sources: 1."), "{}", m.text);
    // A tiny budget leaves the history out.
    let id = env.conversation.clone();
    env.state
        .with_db(move |db| {
            conversations::update(
                db,
                &id,
                ConversationPatch {
                    history_budget: Some(1),
                    ..ConversationPatch::default()
                },
            )
        })
        .await
        .unwrap();
    // The model's own limit may be lower than the budget; the small model has 3000 tokens.
    let m = env.ask(env.input("hose once more")).await;
    assert!(m.text.contains("Earlier turns: 0."), "{}", m.text);
}

#[tokio::test]
async fn the_history_numbers_follow_the_model_limit() {
    let env = setup(false).await;
    let (used, b, _) = history_numbers(&env.state, &env.sidecars, &env.conversation, Some(env.small_model.clone()))
        .await
        .unwrap();
    assert_eq!(used, 0);
    // 3000 tokens of context leave nothing after the reserve: the floors apply.
    assert_eq!(b.history_limit, Some(settings::MIN_BUDGET_TOKENS));
    assert_eq!(b.history, settings::MIN_BUDGET_TOKENS);
    assert_eq!(b.source, settings::MIN_BUDGET_TOKENS as usize);
    // An unknown size takes the settings.
    let (_, b, _) = history_numbers(&env.state, &env.sidecars, &env.conversation, Some(env.chat_model.clone()))
        .await
        .unwrap();
    assert_eq!((b.history, b.history_limit, b.source), (30_000, None, 8_000));
}

// ---------------------------------------------------------------- find

#[tokio::test]
async fn a_find_lists_every_match_under_the_intro() {
    let env = setup(false).await;
    let open = env.task("Hose open", "hose is leaking", TaskStatus::Todo).await;
    env.task("Hose closed", "hose was replaced", TaskStatus::Done).await;
    env.task("Pump", "pump only", TaskStatus::Todo).await;
    env.page(&env.category, "Hose guide", "hose replacement steps").await;
    // Open tasks about hoses: a status filter means tasks only.
    let m = env.ask(env.input("FINDME hose STATUS_TODO")).await;
    assert_finished(&m);
    let find = m.find.clone().unwrap();
    assert_eq!(find.intro, "Here is what I found:");
    assert_eq!(m.text, "Here is what I found:");
    assert_eq!(find.items.len(), 1);
    assert_eq!(find.items[0].id, open);
    assert_eq!(find.items[0].key.as_deref(), Some("GEN-1"));
    assert!(labels(&m).contains(&"Found 1 item"));
    assert!(m.answer.is_none());
    // Without words: all open tasks, by due date, priority and key.
    let m = env.ask(env.input("FINDME STATUS_TODO")).await;
    let keys: Vec<_> = m.find.unwrap().items.iter().map(|i| i.key.clone().unwrap()).collect();
    assert_eq!(keys, ["GEN-1", "GEN-3"]);
    // The find went through the plan only: no answer call.
    assert!(!env.call_kinds().await.contains(&"ask-answer".to_string()));
}

// ---------------------------------------------------------------- 🌐

#[tokio::test]
async fn a_web_message_sends_only_what_was_typed_and_labels_a_real_search() {
    let env = setup(false).await;
    env.page(&env.category, "Secret notes", "hose replacement steps").await;
    let mut input = env.input("WEBSEARCHED what is new");
    input.web = true;
    let m = env.ask(input).await;
    assert_finished(&m);
    let a = m.answer.clone().unwrap();
    assert_eq!(a.basis, AnswerBasis::Web);
    assert_eq!(a.cited, vec![1, 2]);
    assert_eq!(a.sources[0].kind, SourceKind::Web);
    assert_eq!(a.sources[0].url.as_deref(), Some("https://example.com/a"));
    assert_eq!(a.sources[0].snippet, "the quoted text");
    assert_eq!(m.text, "Web answer.");
    assert_eq!(labels(&m), ["Searching the web with Chat Model"]);
    assert!(a.web_unticked.is_none());
    assert!(env.model_has_web_search(&env.chat_model).await);
    // Only ask-web ran, with the setting's searches, and nothing from the notes in it.
    let calls = env.calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["kind"], "ask-web");
    assert_eq!(calls[0]["web_search"], json!({"max_uses": 5}));
    assert_eq!(calls[0]["n_messages"], 2);
    assert!(!calls[0]["last"].as_str().unwrap().contains("Secret"));
    // The user message remembers the switch.
    let user = &env.messages().await[0];
    assert_eq!(user.extra.as_deref(), Some(r#"{"web":true}"#));
}

#[tokio::test]
async fn a_web_reply_without_a_search_is_labelled_and_unticks_chat_completions_only() {
    let env = setup(false).await;
    let mut input = env.input("what is new");
    input.web = true;
    let m = env.ask(input.clone()).await;
    let a = m.answer.unwrap();
    assert_eq!(a.basis, AnswerBasis::NoWebSearch);
    assert!(a.sources.is_empty() && a.cited.is_empty());
    assert!(a.web_unticked.unwrap().contains("Chat Model ignored the web search request"));
    assert!(!env.model_has_web_search(&env.chat_model).await);

    // The Responses API has the tool: the model chose not to search, and the tick stays.
    input.model_id = Some(env.responses_model.clone());
    let m = env.ask(input).await;
    let a = m.answer.unwrap();
    assert_eq!(a.basis, AnswerBasis::NoWebSearch);
    assert!(a.web_unticked.is_none());
    assert!(env.model_has_web_search(&env.responses_model).await);
}

#[tokio::test]
async fn a_refused_web_search_unticks_with_the_reason_and_answers_again_without_it() {
    let env = setup(false).await;
    let mut input = env.input("WEBREFUSE what is new");
    input.web = true;
    input.model_id = Some(env.responses_model.clone());
    let m = env.ask(input).await;
    assert_finished(&m);
    let a = m.answer.unwrap();
    assert_eq!(a.basis, AnswerBasis::NoWebSearch);
    assert!(a.web_unticked.unwrap().contains("web_search_options"));
    assert!(!env.model_has_web_search(&env.responses_model).await);
    assert_eq!(m.text, "Web answer.");
    let calls = env.calls().await;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["web_search"], json!({"max_uses": 5}));
    assert!(calls[1]["web_search"].is_null());
}

// ---------------------------------------------------------------- files

#[tokio::test]
async fn a_question_about_an_attached_file_uses_its_parts_and_suggests_terms_once() {
    let env = setup(true).await;
    let upload = env.upload("units.md", b"# Units\n\nThe Zorp is 5 units.");
    let mut input = env.input("What is a Zorp?");
    input.upload_id = Some(upload);
    let m = env.ask(input).await;
    assert_finished(&m);
    let a = m.answer.clone().unwrap();
    assert_eq!(a.sources.len(), 1);
    assert_eq!(a.sources[0].kind, SourceKind::File);
    assert_eq!(a.sources[0].title, "units.md");
    assert_eq!(a.sources[0].part.as_deref(), Some("Part 1 of 1"));
    assert_eq!(a.basis, AnswerBasis::Notes);
    // doc-terms: the term in the text is kept, the one that is not is dropped.
    let terms: Vec<_> = a.terms.iter().map(|t| t.term.as_str()).collect();
    assert_eq!(terms, ["Zorp"]);
    assert_eq!(a.shown_terms, ["Zorp"]);
    assert_eq!(a.terms_category_id.as_deref(), Some(env.category.as_str()));
    assert!(labels(&m).iter().any(|l| l.starts_with("Reading units.md")));
    assert!(labels(&m).iter().any(|l| l.contains("glossary terms")));
    // The file is a chat file now, named by the user message.
    let user = &env.messages().await[0];
    let id = user.attachment.as_ref().unwrap().chat_file_id.clone().unwrap();
    let text = env
        .state
        .with_db(move |db| Ok(conversations::chat_file(db.conn(), &id)?.text))
        .await
        .unwrap();
    assert!(text.unwrap().contains("Zorp"));
    let kinds = env.call_kinds().await;
    assert_eq!(kinds, ["ask-plan", "ask-answer", "doc-terms"]);
    let calls = env.calls().await;
    assert!(
        calls.iter().find(|c| c["kind"] == "ask-answer").unwrap()["last"].as_str().unwrap().contains("File \"units.md\" (attached to this message; part 1 of 1)")
    );
}

#[tokio::test]
async fn bad_document_terms_are_skipped_quietly_and_so_is_a_missing_default_model() {
    let env = setup(true).await;
    let upload = env.upload("units.md", b"The Zorp is 5 units. BADTERMS");
    let mut input = env.input("What is a Zorp?");
    input.upload_id = Some(upload);
    let m = env.ask(input).await;
    assert_finished(&m);
    assert!(m.answer.unwrap().terms.is_empty());

    let env = setup(false).await;
    let upload = env.upload("units.md", b"The Zorp is 5 units.");
    let mut input = env.input("What is a Zorp?");
    input.upload_id = Some(upload);
    let m = env.ask(input).await;
    assert_finished(&m);
    assert!(m.answer.unwrap().terms.is_empty());
    assert!(!env.call_kinds().await.contains(&"doc-terms".to_string()));
}

// ---------------------------------------------------------------- scope, retry

#[tokio::test]
async fn search_all_categories_answers_the_same_message_again_over_every_category() {
    let env = setup(false).await;
    let other = env.category_named("Other", "OTH").await;
    env.page(&other, "Hose in other", "hose replacement steps").await;
    let id = env.conversation.clone();
    let general = env.category.clone();
    env.state
        .with_db(move |db| {
            conversations::update(
                db,
                &id,
                ConversationPatch {
                    scope_category_ids: Some(vec![general]),
                    ..ConversationPatch::default()
                },
            )
        })
        .await
        .unwrap();
    let m = env.ask(env.input("OUTSIDE:Other. hose")).await;
    let a = m.answer.clone().unwrap();
    assert!(a.sources.is_empty());
    assert_eq!(a.outside_scope, ["Other"]);
    let calls = env.calls().await;
    let first = calls.iter().find(|c| c["kind"] == "ask-answer").unwrap();
    assert!(first["last"].as_str().unwrap().contains("Not searched, because outside"));

    let mid = m.id.clone();
    let job = env
        .state
        .with_db(move |db| widen_search(db, &mid))
        .await
        .unwrap();
    assert!(job.all_categories);
    files::run_job(&env.ctx(), Job::Ask(job)).await;
    let again = env.message(&m.id).await;
    assert_finished(&again);
    let a = again.answer.unwrap();
    assert_eq!(a.sources.len(), 1);
    assert_eq!(a.sources[0].title, "Hose in other");
    assert!(a.outside_scope.is_empty());
    // Same message: the answer replaced, not added.
    assert_eq!(env.messages().await.len(), 2);
}

#[tokio::test]
async fn a_failed_message_is_retried_from_the_stored_messages() {
    let env = setup(false).await;
    env.page(&env.category, "Hose guide", "hose replacement steps").await;
    // No model is picked and there is no default: the job fails with llm_unavailable.
    let mut input = env.input("hose");
    input.model_id = None;
    let m = env.ask(input).await;
    assert_eq!(m.error.as_ref().unwrap().kind, "llm_unavailable");
    assert!(m.answer.is_none());
    assert_eq!(labels(&m), ["Reading your question"]);
    assert_eq!(m.steps[0].state, StepState::Failed);

    // Retry with a model.
    let (id, model) = (m.id.clone(), env.chat_model.clone());
    let (job, assistant) = env
        .state
        .with_db(move |db| files::plan_retry(db, &id, Some(model)))
        .await
        .unwrap();
    assert_eq!(assistant, m.id);
    let reset = env.message(&m.id).await;
    assert!(reset.error.is_none() && reset.steps.is_empty() && reset.text.is_empty());
    files::run_job(&env.ctx(), job).await;
    let done = env.message(&m.id).await;
    assert_finished(&done);
    assert_eq!(done.answer.unwrap().sources.len(), 1);
}

#[tokio::test]
async fn a_message_needs_text_or_a_file_and_a_web_message_carries_no_file() {
    let env = setup(false).await;
    let e = prepare_send(&env.state, &env.uploads, env.input("  ")).await.err().unwrap();
    assert_eq!(e.kind, "invalid");
    let upload = env.upload("a.md", b"text");
    let mut input = env.input("look this up");
    input.web = true;
    input.upload_id = Some(upload);
    let e = prepare_send(&env.state, &env.uploads, input).await.err().unwrap();
    assert_eq!(e.kind, "invalid");
    assert!(env.messages().await.is_empty());
    assert_eq!(title_of("  First line\nsecond", None), "First line");
    assert_eq!(title_of(&"x".repeat(60), None), format!("{}…", "x".repeat(48)));
    assert_eq!(title_of("", Some("torque.pdf")), "torque.pdf");
}
