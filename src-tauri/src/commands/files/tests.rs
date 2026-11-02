//! The chat jobs end to end against the fake sidecar (`tests/fixtures/fake_sidecar.py`) with a
//! temporary data folder: no window (`Ctx.app` is `None`), no default model, so the `check` call
//! (`commands::checks::run_check`, written by the checks agent) is never reached here.

use std::path::PathBuf;
use std::time::Duration;

use cairn_core::conversations::NewConversation;
use cairn_core::providers::{AuthMode, NewModel, NewProvider, ProviderKind, RequestFormat};
use cairn_core::secrets::MemoryStore;
use cairn_core::settings::SettingsPatch;
use serde_json::{Value, json};

use super::*;
use crate::sidecar::SidecarConfig;

struct Env {
    _dir: tempfile::TempDir,
    state: AppState,
    sidecars: Sidecars,
    uploads: Uploads,
    deltas: Arc<Mutex<Vec<String>>>,
    conversation: String,
    category: String,
    chat_model: String,
    small_model: String,
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

async fn setup() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::open(DataPaths::new(dir.path()), Arc::new(MemoryStore::default()));
    let sidecars = Sidecars::new(fake_config(), Arc::new(|_| {}));
    let (conversation, category, chat_model, small_model) = state
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
            let mut model = |name: &str, label: &str, vision: bool| {
                cairn_core::providers::create_model(
                    db,
                    NewModel {
                        provider_id: provider.id.clone(),
                        name: name.into(),
                        label: Some(label.into()),
                        format: RequestFormat::ChatCompletions,
                        vision: Some(vision),
                        effort: None,
                        context_size: None,
                        web_search: None,
                    },
                )
            };
            let chat = model("chat-model", "Chat Model", false)?;
            let small = model("ctx-small", "Small Model", false)?;
            let ocr = model("ocr-model", "OCR Model", true)?;
            // Only the OCR model is a setting: there is no default model.
            cairn_core::settings::update(
                db,
                SettingsPatch {
                    ocr_model_id: Some(ocr.id),
                    ..SettingsPatch::default()
                },
            )?;
            let conversation = conversations::create(db, NewConversation::default())?;
            let category: String =
                db.conn()
                    .query_row("SELECT id FROM categories WHERE is_general = 1", [], |r| {
                        r.get(0)
                    })?;
            Ok((conversation.id, category, chat.id, small.id))
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

    fn uploads_dir(&self) -> PathBuf {
        self.state.paths.uploads_dir()
    }

    fn streamed(&self) -> String {
        self.deltas.lock().unwrap().concat()
    }

    /// Uploads in two chunks, like the composer does.
    fn upload(&self, name: &str, content: &[u8]) -> String {
        let id =
            begin_upload(&self.state.paths, &self.uploads, name, content.len() as u32).unwrap();
        let (a, b) = content.split_at(content.len() / 2);
        append_chunk(&self.uploads, &id, &STANDARD.encode(a)).unwrap();
        append_chunk(&self.uploads, &id, &STANDARD.encode(b)).unwrap();
        id
    }

    async fn file_draft(
        &self,
        name: &str,
        content: &str,
        instruction: &str,
        model: Option<&str>,
    ) -> ChatMessage {
        let upload_id = self.upload(name, content.as_bytes());
        let (job, started) = prepare_file_draft(
            &self.state,
            &self.uploads,
            FileDraftInput {
                conversation_id: self.conversation.clone(),
                upload_id,
                instruction: instruction.into(),
                model_id: model.map(str::to_string),
                category_id: self.category.clone(),
                effort: None,
            },
        )
        .await
        .unwrap();
        run_job(&self.ctx(), Job::File(job)).await;
        self.message(&started.assistant_message_id).await
    }

    async fn revise(&self, text: &str) -> ChatMessage {
        let (job, started) = prepare_revise(
            &self.state,
            ReviseInput {
                conversation_id: self.conversation.clone(),
                text: text.into(),
                model_id: Some(self.chat_model.clone()),
                effort: None,
            },
        )
        .await
        .unwrap();
        run_job(&self.ctx(), Job::Revise(job)).await;
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

    async fn chat_file_text(&self, id: &str) -> Option<String> {
        let id = id.to_string();
        self.state
            .with_db(move |db| Ok(conversations::chat_file(db.conn(), &id)?.text))
            .await
            .unwrap()
    }

    /// What the fake sidecar was asked, in order.
    async fn calls(&self) -> Vec<Value> {
        let r = self
            .sidecars
            .call(Worker::Ai, "calls", json!({}))
            .await
            .unwrap();
        r["calls"].as_array().unwrap().clone()
    }

    fn assert_uploads_gone(&self) {
        let left: Vec<_> = std::fs::read_dir(self.uploads_dir())
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "left in uploads: {left:?}");
    }
}

fn kinds(calls: &[Value]) -> Vec<String> {
    calls
        .iter()
        .map(|c| {
            c["kind"]
                .as_str()
                .or(c["method"].as_str())
                .unwrap()
                .to_string()
        })
        .collect()
}

fn labels(m: &ChatMessage) -> Vec<&str> {
    m.steps.iter().map(|s| s.label.as_str()).collect()
}

/// `n` paragraphs of about 350 characters.
fn long_text(n: usize) -> String {
    (0..n)
        .map(|i| {
            format!(
                "Paragraph {i}: {}",
                "calibrate the pump and log the pressure. ".repeat(8)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

async fn eventually<T>(what: &str, mut probe: impl AsyncFnMut() -> Option<T>) -> T {
    for _ in 0..200 {
        if let Some(v) = probe().await {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for: {what}");
}

// ---------------------------------------------------------------- uploads

#[tokio::test]
async fn uploads_are_checked_and_leftovers_are_swept_once() {
    let env = setup().await;
    let paths = &env.state.paths;
    std::fs::create_dir_all(paths.uploads_dir().join("old")).unwrap();
    std::fs::write(paths.uploads_dir().join("old/p1.jpg"), b"x").unwrap();
    std::fs::write(paths.uploads_dir().join("left.pdf"), b"x").unwrap();

    let e = begin_upload(paths, &env.uploads, "notes.zip", 10).unwrap_err();
    assert_eq!(e.kind, "invalid");
    assert!(
        e.message.contains(".docx") && e.message.contains("notes.zip"),
        "{}",
        e.message
    );
    // The first real upload removes what an earlier run left behind.
    let id = begin_upload(paths, &env.uploads, "Notes.PDF", 4).unwrap();
    assert_eq!(std::fs::read_dir(paths.uploads_dir()).unwrap().count(), 1);
    append_chunk(&env.uploads, &id, &STANDARD.encode(b"abcd")).unwrap();
    assert!(
        append_chunk(&env.uploads, &id, &STANDARD.encode(b"e")).is_err(),
        "larger than announced"
    );
    assert!(
        append_chunk(&env.uploads, &id, "***").is_err(),
        "not base64"
    );
    assert!(append_chunk(&env.uploads, "nope", "").is_err());
    // A second upload does not sweep the first.
    let second = begin_upload(paths, &env.uploads, "a.md", 1).unwrap();
    assert_eq!(std::fs::read_dir(paths.uploads_dir()).unwrap().count(), 2);
    // An upload that is not complete can't start a draft.
    let (a, b) = (
        env.uploads.lock().get(&id).map(|u| u.path.clone()).unwrap(),
        second,
    );
    let started = prepare_file_draft(
        &env.state,
        &env.uploads,
        FileDraftInput {
            conversation_id: env.conversation.clone(),
            upload_id: b.clone(),
            instruction: String::new(),
            model_id: None,
            category_id: env.category.clone(),
            effort: None,
        },
    )
    .await;
    assert_eq!(started.err().unwrap().kind, "invalid");
    assert!(env.messages().await.is_empty());
    // Cancel removes the file.
    let up = env.uploads.take(&id).unwrap();
    remove_path(&up.path);
    assert!(!a.exists());
}

// ---------------------------------------------------------------- file drafts

#[tokio::test]
async fn a_markdown_file_without_an_instruction_becomes_the_card_as_it_is() {
    let env = setup().await;
    let text = "# Pump notes\n\nBody with 中文.\n";
    let m = env.file_draft("notes.md", text, "", None).await;
    assert!(m.error.is_none(), "{:?}", m.error);
    let card = m.draft.clone().unwrap();
    assert!(card.as_is);
    assert_eq!(card.title, "Pump notes");
    assert_eq!(card.body, text.trim());
    assert_eq!(card.file_name.as_deref(), Some("notes.md"));
    assert_eq!(card.source_info.as_deref(), Some("notes.md"));
    assert_eq!(card.category_id, env.category);
    assert_eq!(card.state, DraftState::Draft);
    assert!(
        m.steps.iter().all(|s| s.state == StepState::Done),
        "{:?}",
        m.steps
    );
    assert_eq!(labels(&m)[0], "Reading notes.md");
    // The LLM was never called; the text is kept with the conversation (FR-KN-UP-6).
    assert_eq!(kinds(&env.calls().await), ["file.extract"]);
    assert_eq!(
        env.chat_file_text(card.chat_file_id.as_deref().unwrap())
            .await
            .as_deref(),
        Some(text.trim())
    );
    let all = env.messages().await;
    let attachment = all[0].attachment.as_ref().unwrap();
    assert_eq!(
        (attachment.name.as_str(), attachment.kind),
        ("notes.md", FileKind::Text)
    );
    assert_eq!(attachment.chat_file_id, card.chat_file_id);
    env.assert_uploads_gone();

    // No heading: the title is the file name.
    let m = env
        .file_draft("Pump log.txt", "plain words", "", None)
        .await;
    let card = m.draft.unwrap();
    assert_eq!(
        (card.title.as_str(), card.body.as_str()),
        ("Pump log", "# Pump log\n\nplain words")
    );
}

#[tokio::test]
async fn a_scanned_page_goes_through_ocr_then_the_conversation_model_drafts_and_streams() {
    let env = setup().await;
    let spec =
        json!({"parts": [{"page": 1, "text": "Typed page one."}, {"page": 2, "image": true}]});
    let m = env
        .file_draft("scan.pdf", &spec.to_string(), "", Some(&env.chat_model))
        .await;
    assert!(m.error.is_none(), "{:?}", m.error);
    let card = m.draft.clone().unwrap();
    assert!(!card.as_is && !card.incomplete);
    assert_eq!(card.title, "Draft of scan.pdf");
    // The stored text has the typed page and the OCR text, in page order.
    let text = env
        .chat_file_text(card.chat_file_id.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(text, "Typed page one.\n\nScanned text of p0002.jpg");
    assert_eq!(m.model_label.as_deref(), Some("Chat Model"));
    assert_eq!(
        labels(&m),
        [
            "Reading scan.pdf",
            "Reading page 1 of 1 with OCR Model",
            "Drafting the page with Chat Model"
        ]
    );
    // One OCR call (not streamed), one draft (streamed as it was written).
    let calls = env.calls().await;
    assert_eq!(kinds(&calls), ["file.extract", "ocr", "draft"]);
    assert_eq!(calls[1]["model"], "ocr-model");
    assert_eq!(calls[1]["stream"], false);
    assert_eq!(
        (calls[2]["model"].as_str(), calls[2]["stream"].as_bool()),
        (Some("chat-model"), Some(true))
    );
    assert_eq!(env.streamed(), card.body);
    // The upload and the page image are gone (FR-KN-UP-6).
    env.assert_uploads_gone();
}

#[tokio::test]
async fn several_scanned_pages_are_read_a_few_at_a_time_in_page_order() {
    let env = setup().await;
    let parts: Vec<Value> = (0..7).map(|_| json!({"image": true})).collect();
    let spec = json!({ "parts": parts });
    let m = env
        .file_draft("scan.pdf", &spec.to_string(), "", Some(&env.chat_model))
        .await;
    assert!(m.error.is_none(), "{:?}", m.error);
    let text = env
        .chat_file_text(m.draft.unwrap().chat_file_id.as_deref().unwrap())
        .await
        .unwrap();
    let expected: Vec<String> = (1..=7)
        .map(|i| format!("Scanned text of p{i:04}.jpg"))
        .collect();
    assert_eq!(text, expected.join("\n\n"));
    assert_eq!(
        kinds(&env.calls().await)
            .iter()
            .filter(|k| *k == "ocr")
            .count(),
        7
    );
    assert!(labels_contain(
        &m_steps(&env).await,
        "Reading page 7 of 7 with OCR Model"
    ));
}

async fn m_steps(env: &Env) -> Vec<String> {
    let all = env.messages().await;
    all.last()
        .unwrap()
        .steps
        .iter()
        .map(|s| s.label.clone())
        .collect()
}

fn labels_contain(steps: &[String], label: &str) -> bool {
    steps.iter().any(|s| s == label)
}

#[tokio::test]
async fn a_revision_that_is_a_page_becomes_a_new_card_and_the_old_one_is_superseded() {
    let env = setup().await;
    let first = env
        .file_draft(
            "doc.docx",
            &json!({"parts": [{"text": "Alpha."}]}).to_string(),
            "",
            Some(&env.chat_model),
        )
        .await;
    let old = first.draft.clone().unwrap();
    let revised = env.revise("keep only alpha").await;
    assert!(revised.error.is_none(), "{:?}", revised.error);
    let card = revised.draft.clone().unwrap();
    assert_eq!(card.title, "Revised: keep only alpha");
    assert_eq!(card.chat_file_id, old.chat_file_id);
    assert_eq!(card.file_name.as_deref(), Some("doc.docx"));
    assert_eq!(card.category_id, env.category);
    assert_eq!(card.state, DraftState::Draft);
    assert!(!card.superseded);
    assert_eq!(labels(&revised), ["Revising the draft with Chat Model"]);
    // The streamed reply is the new card's text.
    assert!(env.streamed().ends_with(&card.body));
    // The first card is "Earlier version"; the file's text is still kept for the next request.
    let first_now = env.message(&first.id).await;
    assert!(first_now.draft.unwrap().superseded);
    assert!(
        env.chat_file_text(card.chat_file_id.as_deref().unwrap())
            .await
            .is_some()
    );
    let all = env.messages().await;
    assert_eq!(active_card(&all).unwrap().0.id, revised.id);

    // The revision prompt held the source, the current draft and the request.
    let calls = env.calls().await;
    assert_eq!(kinds(&calls).last().unwrap(), "revise");
}

#[tokio::test]
async fn a_revision_reads_referenced_pages_and_tasks_as_sources() {
    let env = setup().await;
    env.file_draft("notes.md", "Original source.", "", Some(&env.chat_model))
        .await;
    let category = env.category.clone();
    let (page_id, task_key) = env
        .state
        .with_db(move |db| {
            let page = cairn_core::knowledge::create_page(
                db,
                cairn_core::knowledge::NewPage {
                    category_id: category.clone(),
                    title: "Writing guide".into(),
                    body: "Use a concrete example in every section.".into(),
                    ..Default::default()
                },
            )?;
            let task = cairn_core::tasks::create(
                db,
                cairn_core::tasks::NewTask {
                    category_id: category,
                    title: "Review the guide".into(),
                    description: Some("Explain the rollback steps.".into()),
                    ..Default::default()
                },
            )?;
            cairn_core::tasks::add_comment(db, &task.id, "Use the tested rollback command.")?;
            Ok((page.id, task.key))
        })
        .await
        .unwrap();
    let request = format!("Follow [Old title](page:{page_id}) and #{task_key}.");
    let revised = env.revise(&request).await;
    assert!(revised.error.is_none(), "{:?}", revised.error);
    let calls = env.calls().await;
    let messages = calls.iter().rev().find(|c| c["kind"] == "revise").unwrap()["messages"]
        .as_array()
        .unwrap();
    let source = messages[1]["content"].as_str().unwrap();
    assert!(source.contains("Original source."), "{source}");
    assert!(
        source.contains("Use a concrete example in every section."),
        "{source}"
    );
    assert!(source.contains("Explain the rollback steps."), "{source}");
    assert!(
        source.contains("Use the tested rollback command."),
        "{source}"
    );
    assert_eq!(
        messages.last().unwrap()["content"],
        format!("Follow [[Writing guide]] and #{task_key}.")
    );
    let all = env.messages().await;
    assert!(
        all.iter()
            .any(|m| m.role == ChatRole::User && m.text == request)
    );
    assert!(revised.draft.is_some());
}

#[tokio::test]
async fn long_references_in_a_revision_share_the_source_budget() {
    let env = setup().await;
    env.file_draft("notes.md", "Original source.", "", Some(&env.chat_model))
        .await;
    let category = env.category.clone();
    let page = env
        .state
        .with_db(move |db| {
            cairn_core::knowledge::create_page(
                db,
                cairn_core::knowledge::NewPage {
                    category_id: category,
                    title: "Calibration guide".into(),
                    body: long_text(40),
                    ..Default::default()
                },
            )
        })
        .await
        .unwrap();
    let revised = env
        .revise(&format!("Add calibration from [Guide](page:{})", page.id))
        .await;
    assert!(revised.error.is_none(), "{:?}", revised.error);
    assert!(revised.draft.is_some());
    let calls = env.calls().await;
    let revisions: Vec<_> = calls.iter().filter(|c| c["kind"] == "revise").collect();
    assert!(revisions.len() >= 2, "{calls:?}");
    assert!(revisions[0]["chars"].as_u64().unwrap() > 10_000);
    assert!(revisions.last().unwrap()["chars"].as_u64().unwrap() <= 10_000);
}

#[tokio::test]
async fn a_revision_that_is_not_a_page_is_a_plain_answer_and_the_draft_stays() {
    let env = setup().await;
    let first = env
        .file_draft(
            "doc.docx",
            &json!({"parts": [{"text": "Alpha."}]}).to_string(),
            "",
            Some(&env.chat_model),
        )
        .await;
    let answer = env.revise("ANSWERONLY what is in it?").await;
    assert!(answer.draft.is_none() && answer.error.is_none());
    assert!(
        answer.text.starts_with("Sure, the draft covers"),
        "{}",
        answer.text
    );
    let all = env.messages().await;
    let (card_message, card) = active_card(&all).unwrap();
    assert_eq!(card_message.id, first.id);
    assert!(!card.superseded);
    // The next request sees that exchange as earlier turns: 4 messages + source/draft framing.
    let again = env.revise("ANSWERONLY and now?").await;
    assert!(again.text.contains("messages so far"), "{}", again.text);
    let n: usize = again
        .text
        .split_whitespace()
        .rev()
        .nth(3)
        .unwrap()
        .parse()
        .unwrap();
    // system, user (source), assistant (draft), user + assistant (earlier turn), user (request).
    assert_eq!(n, 6);
}

#[tokio::test]
async fn a_revision_without_a_draft_is_refused() {
    let env = setup().await;
    let e = prepare_revise(
        &env.state,
        ReviseInput {
            conversation_id: env.conversation.clone(),
            text: "hi".into(),
            model_id: None,
            effort: None,
        },
    )
    .await;
    assert_eq!(e.err().unwrap().kind, "invalid");
    assert!(env.messages().await.is_empty());
}

#[tokio::test]
async fn stop_ends_the_job_and_the_message_says_so() {
    let env = setup().await;
    let upload_id = env.upload("slow.md", b"# T\n\nSome text.");
    let (job, started) = prepare_file_draft(
        &env.state,
        &env.uploads,
        FileDraftInput {
            conversation_id: env.conversation.clone(),
            upload_id,
            instruction: "tidy this up SLOWSTREAM".into(),
            model_id: Some(env.chat_model.clone()),
            category_id: env.category.clone(),
            effort: None,
        },
    )
    .await
    .unwrap();
    // Dropping the future is what `cancel_job` does to the spawned task.
    let ctx = env.ctx();
    let run = run_job(&ctx, Job::File(job));
    assert!(
        tokio::time::timeout(Duration::from_millis(600), run)
            .await
            .is_err(),
        "should still be running"
    );
    let m = eventually("the stop to be recorded", async || {
        let m = env.message(&started.assistant_message_id).await;
        m.error.is_some().then_some(m)
    })
    .await;
    let error = m.error.unwrap();
    assert_eq!(
        (error.kind.as_str(), error.message.as_str()),
        ("cancelled", "Stopped.")
    );
    assert!(
        error.can_use_as_is,
        "the text was read before the draft started"
    );
    assert!(m.steps.iter().all(|s| s.state != StepState::Active));
    assert_eq!(m.steps.last().unwrap().state, StepState::Failed);
    assert!(m.draft.is_none());
    assert!(!env.streamed().is_empty(), "some text had streamed");
    env.assert_uploads_gone();
    // The sidecar was told to stop the stream: the worker is still usable.
    assert!(
        env.sidecars
            .call(Worker::Ai, "ping", json!({}))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn a_failed_draft_can_be_retried_or_used_as_it_is() {
    let env = setup().await;
    // No model is picked and there is no default model: the draft step can't run.
    let content = json!({"parts": [{"text": "Alpha beta."}]}).to_string();
    let failed = env.file_draft("doc.docx", &content, "", None).await;
    let error = failed.error.clone().unwrap();
    assert_eq!(error.kind, "llm_unavailable");
    assert!(error.can_use_as_is);
    assert!(failed.draft.is_none());
    assert_eq!(failed.steps.last().unwrap().state, StepState::Failed);
    env.assert_uploads_gone();

    // *Use the extracted text as is* (FR-KN-UP-10).
    let id = failed.id.clone();
    let m = env
        .state
        .with_db(move |db| use_text_as_is(db, &id))
        .await
        .unwrap();
    let card = m.draft.unwrap();
    assert!(card.as_is && m.error.is_none());
    assert_eq!(
        (card.title.as_str(), card.body.as_str()),
        ("doc", "# doc\n\nAlpha beta.")
    );
    assert_eq!(card.category_id, env.category);
    assert!(m.steps.iter().all(|s| s.state == StepState::Done));

    // *Retry*: clear it, set a default model, and the stored text is drafted.
    let id = failed.id.clone();
    let default = env.chat_model.clone();
    env.state
        .with_db(move |db| {
            cairn_core::settings::update(
                db,
                SettingsPatch {
                    default_model_id: Some(default),
                    ..SettingsPatch::default()
                },
            )?;
            // The card from "as is" is not the subject of the retry.
            conversations::update_message(
                db,
                &id,
                MessagePatch {
                    error: Some(Some(MessageError {
                        kind: "llm_unavailable".into(),
                        message: "x".into(),
                        can_use_as_is: true,
                    })),
                    ..MessagePatch::default()
                },
            )
        })
        .await
        .unwrap();
    let id = failed.id.clone();
    let (job, assistant) = env
        .state
        .with_db(move |db| plan_retry(db, &id, None))
        .await
        .unwrap();
    assert_eq!(assistant, failed.id);
    let reset = env.message(&assistant).await;
    assert!(reset.error.is_none() && reset.steps.is_empty());
    let Job::File(ref file_job) = job else {
        panic!("a file job")
    };
    assert!(file_job.upload.is_none() && file_job.chat_file_id.is_some());
    run_job(&env.ctx(), job).await;
    let m = env.message(&assistant).await;
    assert!(m.error.is_none(), "{:?}", m.error);
    assert_eq!(m.draft.unwrap().title, "Draft of doc.docx");
    // Retry for a message whose text was never read is refused with a clear message.
    let upload_id = env.upload("bad.pdf", b"BADFILE");
    let (job, started) = prepare_file_draft(
        &env.state,
        &env.uploads,
        FileDraftInput {
            conversation_id: env.conversation.clone(),
            upload_id,
            instruction: String::new(),
            model_id: None,
            category_id: env.category.clone(),
            effort: None,
        },
    )
    .await
    .unwrap();
    run_job(&env.ctx(), Job::File(job)).await;
    let bad = env.message(&started.assistant_message_id).await;
    let error = bad.error.unwrap();
    assert_eq!(error.kind, "ai_bad_file");
    assert!(!error.can_use_as_is);
    env.assert_uploads_gone();
    let id = bad.id.clone();
    let e = env
        .state
        .with_db(move |db| plan_retry(db, &id, None))
        .await
        .err()
        .unwrap();
    assert!(e.message.contains("Attach the file again"), "{}", e.message);
    let id = bad.id.clone();
    assert!(
        env.state
            .with_db(move |db| use_text_as_is(db, &id))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_reply_cut_off_by_the_token_limit_marks_the_card_incomplete() {
    let env = setup().await;
    let m = env
        .file_draft(
            "doc.md",
            "# T\n\nbody",
            "rewrite this CUTOFF",
            Some(&env.chat_model),
        )
        .await;
    assert!(m.draft.unwrap().incomplete);
}

// ---------------------------------------------------------------- long text (FR-KN-UP-4a)

#[tokio::test]
async fn a_model_that_refuses_a_long_text_gets_excerpts_when_there_is_an_instruction() {
    let env = setup().await;
    let text = long_text(40);
    assert!(text.len() > 13_000);
    let m = env
        .file_draft(
            "long.md",
            &text,
            "only the calibration",
            Some(&env.chat_model),
        )
        .await;
    assert!(m.error.is_none(), "{:?}", m.error);
    let card = m.draft.unwrap();
    assert_eq!(card.title, "Draft of long.md");
    // The first call sent everything and was refused; the last sent less and was accepted.
    let calls = env.calls().await;
    let drafts: Vec<&Value> = calls.iter().filter(|c| c["kind"] == "draft").collect();
    assert!(drafts.len() >= 2, "{calls:?}");
    assert!(drafts[0]["chars"].as_u64().unwrap() > 10_000);
    assert!(drafts.last().unwrap()["chars"].as_u64().unwrap() <= 10_000);
    // The whole text stays stored for later requests.
    assert_eq!(
        env.chat_file_text(card.chat_file_id.as_deref().unwrap())
            .await
            .unwrap(),
        text.trim()
    );
}

#[tokio::test]
async fn a_long_text_without_an_instruction_is_drafted_in_parts_and_combined() {
    let env = setup().await;
    let content = json!({"parts": [{"text": long_text(60)}]}).to_string();
    // "ctx-small" accepts 3000 tokens: the budget is the floor (1000 tokens) per part.
    let m = env
        .file_draft("long.docx", &content, "", Some(&env.small_model))
        .await;
    assert!(m.error.is_none(), "{:?}", m.error);
    let calls = env.calls().await;
    let k = kinds(&calls);
    let parts = k.iter().filter(|x| *x == "part").count();
    assert!(parts >= 4, "{k:?}");
    assert_eq!(k.last().unwrap(), "combine");
    assert_eq!(k.iter().filter(|x| *x == "combine").count(), 1);
    assert_eq!(
        calls.last().unwrap()["stream"],
        true,
        "only the combined page streams"
    );
    let card = m.draft.clone().unwrap();
    assert_eq!(card.title, "Combined long.docx");
    assert!(
        card.body.contains(&format!("{parts} parts combined")),
        "{}",
        card.body
    );
    assert_eq!(env.streamed(), card.body);
    assert!(
        labels(&m).contains(&"Combining the parts with Small Model"),
        "{:?}",
        m.steps
    );
    // The fake's combined page is far shorter than the parts: the card says details may be missing.
    assert!(
        m.steps.last().unwrap().label.starts_with("Warning:"),
        "{:?}",
        m.steps
    );
}

#[test]
fn a_file_dropped_as_a_link_is_read_when_cairn_takes_its_type() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("my photo.JPG");
    std::fs::write(&photo, b"\xff\xd8 not really a jpeg").unwrap();
    let uri = url::Url::from_file_path(&photo).unwrap().to_string();
    assert!(uri.contains("my%20photo"), "{uri}");
    let f = read_dropped(&format!("{uri}\r\n")).unwrap();
    assert_eq!(f.name, "my photo.JPG");
    assert_eq!(STANDARD.decode(f.data_base64).unwrap(), b"\xff\xd8 not really a jpeg");

    let other = dir.path().join("notes.odt");
    std::fs::write(&other, b"x").unwrap();
    let e = read_dropped(url::Url::from_file_path(&other).unwrap().as_str()).unwrap_err();
    assert!(e.message.contains("\"notes.odt\" is not one of them"), "{}", e.message);
    // A folder named like a file, a missing file, and links that are not local files.
    std::fs::create_dir(dir.path().join("pics.png")).unwrap();
    for uri in [
        url::Url::from_file_path(dir.path().join("pics.png")).unwrap().to_string(),
        url::Url::from_file_path(dir.path().join("gone.pdf")).unwrap().to_string(),
        "https://example.com/a.png".to_string(),
        "not a link".to_string(),
    ] {
        assert_eq!(read_dropped(&uri).unwrap_err().kind, "invalid", "{uri}");
    }
}
