//! The sidecar link against a fake sidecar (`tests/fixtures/fake_sidecar.py`, plain `python3`).
//! Covers: start + ping, concurrent calls, streaming order, idle exit, crash and restart, cancel,
//! environment, embedding vectors, and a missing Python.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cairn_lib::sidecar::{EMBED_DIMS, SidecarConfig, SidecarStatus, Sidecars, Worker, WorkerState};
use serde_json::json;

type Log = Arc<Mutex<Vec<SidecarStatus>>>;

fn fake_config(idle: Duration) -> SidecarConfig {
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
        idle_timeout: idle,
        embed_threads: 1,
        start_timeout: Duration::from_secs(10),
    }
}

fn make(idle: Duration) -> (Sidecars, Log) {
    make_with(fake_config(idle))
}

fn make_with(config: SidecarConfig) -> (Sidecars, Log) {
    let log: Log = Arc::default();
    let sink = log.clone();
    let sidecars = Sidecars::new(config, Arc::new(move |s| sink.lock().unwrap().push(s)));
    (sidecars, log)
}

async fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..300 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn pid(v: &serde_json::Value) -> u64 {
    v["pid"].as_u64().unwrap()
}

#[tokio::test]
async fn first_call_starts_the_worker_and_reports_status() {
    let (sc, log) = make(Duration::from_secs(60));
    assert_eq!(sc.status().ai.state, WorkerState::Stopped);
    let r = sc
        .call(Worker::Ai, "echo", json!({"text": "hi"}))
        .await
        .unwrap();
    assert_eq!(r["text"], "hi");
    assert_eq!(sc.status().ai.state, WorkerState::Ready);
    assert_eq!(sc.status().embed.state, WorkerState::Stopped);
    let states: Vec<_> = log.lock().unwrap().iter().map(|s| s.ai.state).collect();
    assert_eq!(states, vec![WorkerState::Starting, WorkerState::Ready]);
}

#[tokio::test]
async fn concurrent_first_calls_start_one_process() {
    let (sc, _) = make(Duration::from_secs(60));
    let sc = Arc::new(sc);
    let mut tasks = Vec::new();
    for i in 0..8 {
        let sc = sc.clone();
        tasks.push(tokio::spawn(async move {
            sc.call(Worker::Ai, "echo", json!({"text": i}))
                .await
                .unwrap()
        }));
    }
    let mut pids = Vec::new();
    for (i, t) in tasks.into_iter().enumerate() {
        let r = t.await.unwrap();
        assert_eq!(r["text"], i);
        pids.push(pid(&r));
    }
    pids.dedup();
    assert_eq!(pids.len(), 1, "all calls must share one process");
}

#[tokio::test]
async fn slow_call_does_not_block_a_fast_one() {
    let (sc, _) = make(Duration::from_secs(60));
    let sc = Arc::new(sc);
    sc.call(Worker::Ai, "ping", json!({})).await.unwrap();
    let slow = {
        let sc = sc.clone();
        tokio::spawn(async move { sc.call(Worker::Ai, "slow", json!({"ms": 600})).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    let t = std::time::Instant::now();
    sc.call(Worker::Ai, "echo", json!({"text": "x"}))
        .await
        .unwrap();
    assert!(t.elapsed() < Duration::from_millis(400));
    slow.await.unwrap().unwrap();
}

#[tokio::test]
async fn streaming_deltas_arrive_in_order_before_the_result() {
    let (sc, _) = make(Duration::from_secs(60));
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = seen.clone();
    let r = sc
        .call_streaming(
            Worker::Ai,
            "stream",
            json!({"parts": ["a", "b", "c", "d"], "delay_ms": 5}),
            Box::new(move |d| sink.lock().unwrap().push(d)),
        )
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), ["a", "b", "c", "d"]);
    assert_eq!(r["text"], "abcd");
}

#[tokio::test]
async fn errors_keep_kind_and_hint() {
    let (sc, _) = make(Duration::from_secs(60));
    let e = sc
        .call(Worker::Ai, "no_such_method", json!({}))
        .await
        .unwrap_err();
    assert_eq!(e.kind, "internal"); // method-not-found carries no data.kind
    let e = sc
        .call(Worker::Embed, "embed.query", json!({}))
        .await
        .unwrap_err();
    assert_eq!(e.kind, "bad_request");
    assert_eq!(e.hint.as_deref(), Some("check params"));
}

#[tokio::test]
async fn idle_worker_exits_and_next_call_restarts_it() {
    let (sc, log) = make(Duration::from_millis(300));
    let first = pid(&sc.call(Worker::Embed, "echo", json!({})).await.unwrap());
    eventually("worker stopped", || {
        sc.status().embed.state == WorkerState::Stopped
    })
    .await;
    let states: Vec<_> = log.lock().unwrap().iter().map(|s| s.embed.state).collect();
    assert_eq!(
        states,
        vec![
            WorkerState::Starting,
            WorkerState::Ready,
            WorkerState::Stopped
        ]
    );
    let second = pid(&sc.call(Worker::Embed, "echo", json!({})).await.unwrap());
    assert_ne!(first, second);
    assert_eq!(sc.status().embed.state, WorkerState::Ready);
}

#[tokio::test]
async fn a_call_in_flight_keeps_the_worker_alive() {
    let (sc, _) = make(Duration::from_millis(200));
    // The call lasts longer than the idle timeout.
    let r = sc
        .call(Worker::Ai, "slow", json!({"ms": 900}))
        .await
        .unwrap();
    assert_eq!(r["cancelled"], false);
    assert_eq!(sc.status().ai.state, WorkerState::Ready);
}

#[tokio::test]
async fn set_idle_timeout_applies_to_a_running_worker() {
    let (sc, _) = make(Duration::from_secs(3600));
    sc.call(Worker::Ai, "ping", json!({})).await.unwrap();
    sc.set_idle_timeout(Duration::from_millis(100));
    eventually("worker stopped", || {
        sc.status().ai.state == WorkerState::Stopped
    })
    .await;
}

#[tokio::test]
async fn crash_marks_failed_fails_calls_in_flight_and_restarts_on_next_call() {
    let (sc, log) = make(Duration::from_secs(60));
    let sc = Arc::new(sc);
    let before = pid(&sc.call(Worker::Ai, "echo", json!({})).await.unwrap());
    let waiting = {
        let sc = sc.clone();
        tokio::spawn(async move { sc.call(Worker::Ai, "slow", json!({"ms": 5000})).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let crashed = sc.call(Worker::Ai, "crash", json!({})).await.unwrap_err();
    assert_eq!(crashed.kind, "unavailable");
    assert!(
        crashed.message.contains("exit code 3"),
        "{}",
        crashed.message
    );
    assert_eq!(waiting.await.unwrap().unwrap_err().kind, "unavailable");
    let status = sc.status().ai;
    assert_eq!(status.state, WorkerState::Failed);
    assert!(status.detail.unwrap().contains("stopped unexpectedly"));
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|s| s.ai.state == WorkerState::Failed)
    );

    let after = pid(&sc.call(Worker::Ai, "echo", json!({})).await.unwrap());
    assert_ne!(before, after);
    assert_eq!(sc.status().ai.state, WorkerState::Ready);
    assert_eq!(sc.status().ai.detail, None);
}

#[tokio::test]
async fn dropping_a_streaming_call_sends_cancel() {
    let (sc, _) = make(Duration::from_secs(60));
    let parts: Vec<String> = (0..200).map(|i| i.to_string()).collect();
    let call = sc.call_streaming(
        Worker::Ai,
        "stream",
        json!({"parts": parts, "delay_ms": 20}),
        Box::new(|_| {}),
    );
    // Give up after the first few deltas, like the chat "Stop" button.
    assert!(
        tokio::time::timeout(Duration::from_millis(150), call)
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    let r = sc.call(Worker::Ai, "cancelled", json!({})).await.unwrap();
    assert_eq!(r["ids"].as_array().unwrap().len(), 1, "{r}");
    // The worker is still usable.
    assert_eq!(
        sc.call(Worker::Ai, "echo", json!({"text": 1}))
            .await
            .unwrap()["text"],
        1
    );
}

#[tokio::test]
async fn the_environment_follows_the_protocol() {
    // Set before any child is spawned by this test; the link must remove it.
    unsafe { std::env::set_var("AZURE_OPENAI_API_KEY", "should-not-reach-the-child") };
    let (sc, _) = make(Duration::from_secs(60));
    let r = sc.call(Worker::Ai, "env", json!({})).await.unwrap();
    assert_eq!(r["azure_present"], json!([]));
    assert_eq!(r["cost_map"], "True");
    assert_eq!(r["log"], "ERROR");
    assert_eq!(r["path_set"], true);
}

#[tokio::test]
async fn embedding_vectors_are_decoded() {
    let (sc, _) = make(Duration::from_secs(60));
    let q = sc.embed_query("hello").await.unwrap();
    assert_eq!(q.len(), EMBED_DIMS as usize);
    let norm: f32 = q.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-4);
    assert_eq!(q, sc.embed_query("hello").await.unwrap());
    let docs = sc
        .embed_documents(vec![("T".into(), "one".into()), ("T".into(), "two".into())])
        .await
        .unwrap();
    assert_eq!(docs.len(), 2);
    assert_ne!(docs[0], docs[1]);
    assert_eq!(docs[0].len(), EMBED_DIMS as usize);
}

#[tokio::test]
async fn missing_python_or_model_is_unavailable_with_a_hint() {
    let mut config = fake_config(Duration::from_secs(60));
    config.python = None;
    let (sc, _) = make_with(config);
    let e = sc.call(Worker::Ai, "ping", json!({})).await.unwrap_err();
    assert_eq!(e.kind, "unavailable");
    assert!(e.message.contains("setup-sidecar-dev.sh"), "{}", e.message);
    assert_eq!(sc.status().ai.state, WorkerState::Failed);

    let mut config = fake_config(Duration::from_secs(60));
    config.model_dir = None;
    let (sc, _) = make_with(config);
    // No model is "not installed" (a lite package, D-82), not a fault; no process is started.
    let e = sc.embed_query("x").await.unwrap_err();
    assert_eq!(e.kind, cairn_lib::sidecar::NOT_INSTALLED);
    assert!(e.message.contains("Settings › Search"), "{}", e.message);
    assert_eq!(sc.status().embed.state, WorkerState::Stopped);

    let mut config = fake_config(Duration::from_secs(60));
    config.python = Some(PathBuf::from("/definitely/not/python"));
    let (sc, _) = make_with(config);
    let e = sc.call(Worker::Ai, "ping", json!({})).await.unwrap_err();
    assert_eq!(e.kind, "unavailable");
}
