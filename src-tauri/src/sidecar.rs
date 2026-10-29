//! The two sidecar processes (TC-12) and the JSON-RPC link to them (`sidecar/PROTOCOL.md`).
//!
//! - **Start on demand:** the first `call` for a worker starts its process (state `starting`, then
//!   `ready` after a `ping`). The UI never waits for a start it didn't ask for (NFR-PERF-2).
//! - **Idle exit:** after `idle_timeout` with no request in flight, stdin is closed and the process
//!   exits by itself; memory goes back to the OS (FR-RAG-EM-1a). The next call starts it again.
//! - **Crashes:** a process that dies is marked `failed` with a plain-language detail; calls in
//!   flight get an `unavailable` error; the next call starts it again.
//! - **Status** changes are sent to the UI as the `SidecarStatus` event.
//! - **Where Python is:** `CAIRN_SIDECAR_PYTHON`, else in debug builds `<repo>/sidecar/.venv/bin/python`
//!   (set up by `scripts/setup-sidecar-dev.sh`; `.venv/Scripts/python.exe` on Windows), else the bundled interpreter
//!   (`<resources>/sidecar/python/bin/python3`; on Windows `<resources>\sidecar\python\python.exe`, where the
//!   resource folder is the exe's folder). The embedding
//!   model folder: `CAIRN_EMBED_MODEL_DIR` (empty: none, as in a lite package), else `<resources>/models/embeddinggemma-300m`, else in debug builds `<repo>/sidecar/models/embeddinggemma-300m`,
//!   else a model added later in the data folder (`<data>/models/embeddinggemma-300m`, FR-RAG-EM-3, D-82). The lite
//!   packages have none until one is added: `embed.*` calls then fail with the kind [`NOT_INSTALLED`].
//! - The child's environment follows PROTOCOL.md ("Environment"); its stderr goes to our log.
//! - **Windows (M8):** the workers get no console window (`CREATE_NO_WINDOW`; the `az` commands they
//!   start inherit that hidden console), and each is put into a Job Object that ends it when Cairn
//!   ends, even after a crash (`winjob.rs`).
//!
//! - Status changes go through a callback (`StatusCallback`), so the link is tested without a Tauri
//!   `AppHandle` (`src-tauri/tests/sidecar_link.rs` runs it against a fake sidecar).
//!
//! Owned by the M4 sidecar-link agent.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, Notify, oneshot};

use crate::error::ApiError;

/// Where the release build keeps Python and the model, below the app's resource folder
/// (`packaging/build-sidecar.sh`, `packaging/tauri.release.conf.json`).
#[cfg(not(windows))]
const BUNDLED_PYTHON: &str = "sidecar/python/bin/python3";
/// python-build-standalone's Windows `install_only` layout has the interpreter at the top.
#[cfg(windows)]
const BUNDLED_PYTHON: &str = "sidecar/python/python.exe";
/// The development virtual environment's interpreter, below `<repo>/sidecar`.
#[cfg(not(windows))]
const DEV_PYTHON: &str = ".venv/bin/python";
#[cfg(windows)]
const DEV_PYTHON: &str = ".venv/Scripts/python.exe";
/// Windows process creation flag: no console window for a console program started by a GUI program.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BUNDLED_MODEL_DIR: &str = "models/embeddinggemma-300m";

/// The embedding model and the vector size the index uses (FR-RAG-EM-1, M0-b: q4 at 256 dims).
pub const EMBED_MODEL_ID: &str = "embeddinggemma-300m-q4";
pub const EMBED_DIMS: u32 = 256;
/// The error kind of an `embed.*` call while no search model is installed (D-82). Not a fault: the
/// callers use words only and say nothing about it.
pub const NOT_INSTALLED: &str = "not_installed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Worker {
    /// LLM calls (LiteLLM), Azure sign-in check, file extraction (M5).
    Ai,
    /// The search model (EmbeddingGemma), at low priority.
    Embed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    Stopped,
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkerStatus {
    pub state: WorkerState,
    /// Plain-language reason when `failed` (e.g. "Python for the AI worker was not found at …").
    pub detail: Option<String>,
}

/// Both workers. Returned by `sidecar_status` and sent as an event whenever either changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct SidecarStatus {
    pub ai: WorkerStatus,
    pub embed: WorkerStatus,
}

/// A failed sidecar call. `kind` is one of PROTOCOL.md's error kinds, or `unavailable` when the
/// process could not be started or died.
#[derive(Debug, Clone)]
pub struct SidecarError {
    pub kind: String,
    pub message: String,
    pub hint: Option<String>,
}

impl From<SidecarError> for ApiError {
    fn from(e: SidecarError) -> Self {
        let message = match e.hint {
            Some(h) if !h.is_empty() => format!("{} ({h})", e.message),
            _ => e.message,
        };
        ApiError {
            kind: format!("ai_{}", e.kind),
            message,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SidecarConfig {
    /// The Python interpreter; `None` = not found (calls fail with `unavailable`).
    pub python: Option<PathBuf>,
    /// Arguments between the interpreter and `--role <role>`. Default: `-I -m cairn_sidecar`.
    /// Tests put a fake sidecar script here.
    pub script_args: Vec<String>,
    /// The embedding model folder (PROTOCOL.md `model_dir`).
    pub model_dir: Option<PathBuf>,
    /// Settings › Search "idle timeout" (`embedding_idle_timeout_min`), for both workers.
    pub idle_timeout: Duration,
    /// ONNX Runtime threads for the embed worker: `min(4, max(1, cores / 2))` (FR-RAG-IX-5).
    pub embed_threads: u32,
    /// How long the first `ping` of a new process may take (Python start, antivirus scans, …).
    pub start_timeout: Duration,
}

impl SidecarConfig {
    /// Resolves paths in this order: the environment, then the release resource folder, then (debug
    /// builds only) the repo's dev setup (see the module comment).
    /// `resource_dir` is the app's resource folder (inside the AppImage, where the release build
    /// bundles Python and the model; NFR-INS-1). The layout is `packaging/build-sidecar.sh`'s:
    /// `<resource_dir>/sidecar/python/bin/python3` and `<resource_dir>/models/embeddinggemma-300m`.
    /// On Windows (portable zip, no bundle) Tauri's resource folder is the exe's folder, and
    /// Python is `<resource_dir>\sidecar\python\python.exe`.
    pub fn resolve(idle_timeout_min: u32, resource_dir: Option<PathBuf>) -> Self {
        // Debug builds run from the repo: <repo>/sidecar holds the venv and the model (dev setup).
        let dev_root = cfg!(debug_assertions)
            .then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar"));
        let bundled = |rel: &str| {
            resource_dir
                .as_ref()
                .map(|r| r.join(rel))
                .filter(|p| p.exists())
        };
        let python = std::env::var_os("CAIRN_SIDECAR_PYTHON")
            .map(PathBuf::from)
            .or_else(|| bundled(BUNDLED_PYTHON))
            .or_else(|| {
                dev_root
                    .as_ref()
                    .map(|r| r.join(DEV_PYTHON))
                    .filter(|p| p.exists())
            });
        let model_dir = match std::env::var_os("CAIRN_EMBED_MODEL_DIR") {
            // Empty: no model at all, as in a lite package (D-82; for tests).
            Some(dir) if dir.is_empty() => None,
            Some(dir) => Some(PathBuf::from(dir)),
            None => bundled(BUNDLED_MODEL_DIR).or_else(|| {
                dev_root
                    .as_ref()
                    .map(|r| r.join("models/embeddinggemma-300m"))
                    .filter(|p| p.exists())
            }),
        };
        let cores = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(2);
        Self {
            python,
            script_args: vec!["-I".into(), "-m".into(), "cairn_sidecar".into()],
            model_dir,
            idle_timeout: Duration::from_secs(u64::from(idle_timeout_min.max(1)) * 60),
            embed_threads: (cores / 2).clamp(1, 4),
            start_timeout: Duration::from_secs(20),
        }
    }

    /// Without a bundled model, uses one added to the data folder when its files are all there
    /// (FR-RAG-EM-3, D-82).
    pub fn with_added_model(mut self, paths: &cairn_core::DataPaths) -> Self {
        if self.model_dir.is_none() {
            let dir = cairn_core::search_model::added_dir(paths);
            if cairn_core::search_model::is_complete(&dir) {
                self.model_dir = Some(dir);
            }
        }
        self
    }
}

/// Called with the new status of both workers whenever either changes.
pub type StatusCallback = Arc<dyn Fn(SidecarStatus) + Send + Sync>;

/// Receives the text of each `stream` notification.
type DeltaFn = Arc<dyn Fn(String) + Send + Sync>;

fn unavailable(message: impl Into<String>) -> SidecarError {
    SidecarError {
        kind: "unavailable".into(),
        message: message.into(),
        hint: None,
    }
}

/// A poisoned lock only means another thread panicked; the data is still usable here.
fn lock<T>(m: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Worker {
    fn role(self) -> &'static str {
        match self {
            Worker::Ai => "ai",
            Worker::Embed => "embed",
        }
    }

    /// How the worker is named in messages for the user.
    fn label(self) -> &'static str {
        match self {
            Worker::Ai => "AI worker",
            Worker::Embed => "search model worker",
        }
    }
}

fn state(state: WorkerState, detail: Option<String>) -> WorkerStatus {
    WorkerStatus { state, detail }
}

/// What the manager and the process tasks share.
struct Shared {
    config: StdMutex<SidecarConfig>,
    status: StdMutex<SidecarStatus>,
    on_status: StatusCallback,
}

impl Shared {
    fn set_status(&self, worker: Worker, new: WorkerStatus) {
        let snapshot = {
            let mut all = lock(&self.status);
            let slot = match worker {
                Worker::Ai => &mut all.ai,
                Worker::Embed => &mut all.embed,
            };
            if *slot == new {
                return;
            }
            *slot = new;
            all.clone()
        };
        (self.on_status)(snapshot);
    }
}

/// A request waiting for its answer.
struct Pending {
    tx: oneshot::Sender<Result<Value, SidecarError>>,
    on_delta: Option<DeltaFn>,
}

type Answer = oneshot::Receiver<Result<Value, SidecarError>>;

/// One running process and its pipes.
struct Conn {
    worker: Worker,
    /// Which start this is; a late event of an old process must not change the status of a new one.
    generation: u64,
    stdin: Mutex<Option<ChildStdin>>,
    pending: StdMutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    in_flight: AtomicUsize,
    last_activity: StdMutex<Instant>,
    /// The process is gone (or going): no new requests.
    closed: AtomicBool,
    /// We closed stdin on purpose (idle exit): report `stopped`, not `failed`.
    stopping: AtomicBool,
    /// The start failed and was already reported: say nothing more when the process exits.
    discard: AtomicBool,
    /// Asks the supervisor to kill the process.
    kill: Notify,
}

impl Conn {
    fn touch(&self) {
        *lock(&self.last_activity) = Instant::now();
    }

    fn register(&self, on_delta: Option<DeltaFn>) -> (u64, Answer) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, Pending { tx, on_delta });
        (id, rx)
    }

    /// One writer at a time, so lines never interleave.
    async fn write_line(&self, value: &Value) -> Result<(), SidecarError> {
        let mut line = value.to_string();
        line.push('\n');
        let mut guard = self.stdin.lock().await;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| unavailable(format!("The {} is not running.", self.worker.label())))?;
        let write = async {
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await
        };
        write.await.map_err(|e| {
            unavailable(format!(
                "The {} could not be reached: {e}",
                self.worker.label()
            ))
        })
    }

    /// Sends a request without waiting; the answer arrives through the receiver from `register`.
    async fn send_request(&self, id: u64, method: &str, params: Value) -> Result<(), SidecarError> {
        self.write_line(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await
    }

    /// Handles one line from stdout: an answer, or a `stream` notification.
    fn dispatch(&self, line: &str) {
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            tracing::debug!(worker = ?self.worker, "ignored a non-JSON line from the sidecar");
            return;
        };
        if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            let pending = lock(&self.pending).remove(&id);
            if let Some(p) = pending {
                let result = match msg.get("error") {
                    Some(err) => Err(parse_error(err)),
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = p.tx.send(result);
            }
        } else if msg.get("method").and_then(Value::as_str) == Some("stream") {
            let params = msg.get("params");
            let id = params.and_then(|p| p.get("id")).and_then(Value::as_u64);
            let delta = params.and_then(|p| p.get("delta")).and_then(Value::as_str);
            if let (Some(id), Some(delta)) = (id, delta) {
                let callback = lock(&self.pending)
                    .get(&id)
                    .and_then(|p| p.on_delta.clone());
                if let Some(callback) = callback {
                    callback(delta.to_string());
                }
            }
        }
    }

    /// Every waiting request gets the same error (the process died).
    fn fail_all(&self, message: &str) {
        let all: Vec<Pending> = lock(&self.pending).drain().map(|(_, p)| p).collect();
        for p in all {
            let _ = p.tx.send(Err(unavailable(message)));
        }
    }
}

fn parse_error(err: &Value) -> SidecarError {
    let data = err.get("data");
    let text = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
    SidecarError {
        kind: text(data.and_then(|d| d.get("kind"))).unwrap_or_else(|| "internal".into()),
        message: text(err.get("message")).unwrap_or_else(|| "The worker reported an error.".into()),
        hint: text(data.and_then(|d| d.get("hint"))),
    }
}

/// Counts a request as in flight (for the idle timer) and cancels it if dropped half-way.
struct Lease {
    conn: Arc<Conn>,
    /// `(request id, streaming)` once registered. If the answer already came, the pending entry
    /// is gone and dropping the lease does nothing more.
    request: Option<(u64, bool)>,
}

impl Lease {
    fn new(conn: Arc<Conn>) -> Self {
        conn.in_flight.fetch_add(1, Ordering::SeqCst);
        Self {
            conn,
            request: None,
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some((id, streaming)) = self.request.take() {
            let still_waiting = lock(&self.conn.pending).remove(&id).is_some();
            // Dropped before the answer: tell a streaming worker to stop (M6 chat "Stop").
            if still_waiting
                && streaming
                && let Ok(handle) = tokio::runtime::Handle::try_current()
            {
                let conn = self.conn.clone();
                handle.spawn(async move {
                    let _ = conn
                        .write_line(
                            &json!({"jsonrpc": "2.0", "method": "cancel", "params": {"id": id}}),
                        )
                        .await;
                });
            }
        }
        self.conn.touch();
        self.conn.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The process of one worker: `current` is the live connection, if any.
struct Slot {
    current: Mutex<Option<Arc<Conn>>>,
    generation: AtomicU64,
}

impl Slot {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            current: Mutex::new(None),
            generation: AtomicU64::new(0),
        })
    }
}

/// Managed Tauri state: `app.state::<Sidecars>()`.
pub struct Sidecars {
    shared: Arc<Shared>,
    ai: Arc<Slot>,
    embed: Arc<Slot>,
}

impl Sidecars {
    /// Creates the manager; no process is started until the first call. `on_status` is called
    /// (from any thread) with the new status of both workers whenever one changes.
    pub fn new(config: SidecarConfig, on_status: StatusCallback) -> Self {
        Self {
            shared: Arc::new(Shared {
                config: StdMutex::new(config),
                status: StdMutex::new(SidecarStatus {
                    ai: state(WorkerState::Stopped, None),
                    embed: state(WorkerState::Stopped, None),
                }),
                on_status,
            }),
            ai: Slot::new(),
            embed: Slot::new(),
        }
    }

    fn slot(&self, worker: Worker) -> &Arc<Slot> {
        match worker {
            Worker::Ai => &self.ai,
            Worker::Embed => &self.embed,
        }
    }

    pub fn status(&self) -> SidecarStatus {
        lock(&self.shared.status).clone()
    }

    /// The search model folder in use, if any (D-82).
    pub fn model_dir(&self) -> Option<PathBuf> {
        lock(&self.shared.config).model_dir.clone()
    }

    pub fn model_installed(&self) -> bool {
        self.model_dir().is_some()
    }

    /// A search model was added or removed (FR-RAG-EM-3). The next `embed.*` call uses it.
    pub fn set_model_dir(&self, dir: Option<PathBuf>) {
        lock(&self.shared.config).model_dir = dir;
    }

    /// Ends `worker`'s process now, as the idle timeout would: stdin is closed, and the process is
    /// killed if it hasn't ended after a few seconds. Requests in flight fail with `unavailable`.
    /// The next call starts it again. Used to let go of a search model's files (D-82).
    pub async fn stop(&self, worker: Worker) {
        let slot = self.slot(worker).clone();
        let Some(conn) = slot.current.lock().await.take() else {
            return;
        };
        conn.stopping.store(true, Ordering::SeqCst);
        conn.closed.store(true, Ordering::SeqCst);
        conn.stdin.lock().await.take();
        for _ in 0..50 {
            if self.status_of(worker) != WorkerState::Ready {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        conn.kill.notify_one();
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    fn status_of(&self, worker: Worker) -> WorkerState {
        let status = self.status();
        match worker {
            Worker::Ai => status.ai.state,
            Worker::Embed => status.embed.state,
        }
    }

    /// Settings changed the idle timeout; applies to both workers from now on.
    pub fn set_idle_timeout(&self, timeout: Duration) {
        lock(&self.shared.config).idle_timeout = timeout;
    }

    /// One request; starts the worker if needed.
    pub async fn call(
        &self,
        worker: Worker,
        method: &str,
        params: Value,
    ) -> Result<Value, SidecarError> {
        self.run(worker, method, params, None).await
    }

    /// A request whose text arrives in `stream` notifications; `on_delta` gets each piece of text.
    /// Dropping the returned future sends `cancel` (M6 uses this for chat).
    pub async fn call_streaming(
        &self,
        worker: Worker,
        method: &str,
        params: Value,
        on_delta: Box<dyn Fn(String) + Send + Sync>,
    ) -> Result<Value, SidecarError> {
        self.run(worker, method, params, Some(Arc::from(on_delta)))
            .await
    }

    async fn run(
        &self,
        worker: Worker,
        method: &str,
        params: Value,
        on_delta: Option<DeltaFn>,
    ) -> Result<Value, SidecarError> {
        let mut lease = self.acquire(worker).await?;
        let streaming = on_delta.is_some();
        let (id, answer) = lease.conn.register(on_delta);
        lease.request = Some((id, streaming));
        lease.conn.send_request(id, method, params).await?;
        // The sender is dropped without an answer only if the process task ended abnormally.
        answer.await.unwrap_or_else(|_| {
            Err(unavailable(format!(
                "The {} stopped answering.",
                worker.label()
            )))
        })
    }

    /// The live connection of `worker`, starting the process first if needed. Only one caller
    /// starts it; the others wait on the lock and then use the same process.
    async fn acquire(&self, worker: Worker) -> Result<Lease, SidecarError> {
        let slot = self.slot(worker).clone();
        let mut current = slot.current.lock().await;
        if let Some(conn) = current.as_ref()
            && !conn.closed.load(Ordering::SeqCst)
        {
            // Counted while the lock is held, so the idle timer can't close it in between.
            return Ok(Lease::new(conn.clone()));
        }
        let conn = self.start(worker, &slot).await?;
        let lease = Lease::new(conn.clone());
        *current = Some(conn);
        Ok(lease)
    }

    /// Starts a process and waits for its first `ping`. Sets the status (`starting`, then
    /// `ready` or `failed`).
    async fn start(&self, worker: Worker, slot: &Arc<Slot>) -> Result<Arc<Conn>, SidecarError> {
        let config = lock(&self.shared.config).clone();
        let generation = slot.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.shared
            .set_status(worker, state(WorkerState::Starting, None));
        let fail = |message: String| {
            self.shared
                .set_status(worker, state(WorkerState::Failed, Some(message.clone())));
            unavailable(message)
        };

        let Some(python) = config.python.clone() else {
            return Err(fail(if cfg!(debug_assertions) {
                format!(
                    "Python for the {} was not found. Run scripts/setup-sidecar-dev.sh (or set CAIRN_SIDECAR_PYTHON).",
                    worker.label()
                )
            } else {
                format!(
                    "Cairn's installation is incomplete: the bundled Python for the {} is missing. Reinstalling Cairn should fix this.",
                    worker.label()
                )
            }));
        };

        let mut command = Command::new(&python);
        command
            .args(&config.script_args)
            .arg("--role")
            .arg(worker.role())
            // PROTOCOL.md "Environment": LiteLLM must not prefer an API key from the environment.
            .env_remove("AZURE_OPENAI_API_KEY")
            .env_remove("AZURE_API_KEY")
            .env_remove("AZURE_AI_API_KEY")
            .env("LITELLM_LOCAL_MODEL_COST_MAP", "True")
            .env("LITELLM_LOG", "ERROR")
            // D-82: the bundle leaves out LiteLLM's Rust extension, so every route stays on Python.
            .env("LITELLM_RUST", "false")
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // A GUI program starting a console program would flash a console window for each worker.
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        if std::env::var_os("APPIMAGE").is_some() {
            // M0-d: the AppImage's launcher points these at its own copies of GTK and WebKit
            // libraries; Python's packages (and the `az` command the sidecar may start) need the system's.
            for name in ["LD_LIBRARY_PATH", "LD_PRELOAD", "PYTHONHOME", "PYTHONPATH"] {
                command.env_remove(name);
            }
        }
        let mut child = command.spawn().map_err(|e| {
            tracing::warn!(worker = ?worker, error = %e, "could not start the sidecar");
            fail(format!("The {} could not be started: {e}", worker.label()))
        })?;
        // The worker must end with Cairn, even after a crash (a killed Cairn closes no stdin that
        // a worker busy in a call would notice).
        #[cfg(windows)]
        match child.raw_handle() {
            Some(handle) => {
                if let Err(e) = crate::winjob::assign(handle) {
                    tracing::warn!(worker = ?worker, error = %e, "could not put the sidecar into the job object");
                }
            }
            None => {
                tracing::warn!(worker = ?worker, "the sidecar ended before it joined the job object");
            }
        }
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(fail(format!(
                "The {} could not be connected.",
                worker.label()
            )));
        };

        let conn = Arc::new(Conn {
            worker,
            generation,
            stdin: Mutex::new(Some(stdin)),
            pending: StdMutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            in_flight: AtomicUsize::new(0),
            last_activity: StdMutex::new(Instant::now()),
            closed: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            discard: AtomicBool::new(false),
            kill: Notify::new(),
        });
        tokio::spawn(supervise(
            self.shared.clone(),
            slot.clone(),
            conn.clone(),
            child,
            stdout,
            stderr,
        ));

        // The first request tells us the process is alive (generous: the first start is slow).
        let (id, answer) = conn.register(None);
        let ping = async {
            conn.send_request(id, "ping", json!({})).await?;
            answer
                .await
                .unwrap_or_else(|_| Err(unavailable("The worker stopped before answering.")))
        };
        let reply = match tokio::time::timeout(config.start_timeout, ping).await {
            Ok(r) => r,
            Err(_) => Err(unavailable(format!(
                "The {} did not answer within {} seconds.",
                worker.label(),
                config.start_timeout.as_secs()
            ))),
        };
        if let Err(e) = reply {
            lock(&conn.pending).remove(&id);
            conn.discard.store(true, Ordering::SeqCst);
            conn.closed.store(true, Ordering::SeqCst);
            conn.kill.notify_one();
            return Err(fail(e.message));
        }
        conn.touch();
        self.shared
            .set_status(worker, state(WorkerState::Ready, None));
        tokio::spawn(idle_monitor(
            self.shared.clone(),
            slot.clone(),
            conn.clone(),
        ));
        Ok(conn)
    }

    /// The query vector for a search (query prompt), normalised, `EMBED_DIMS` long.
    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>, SidecarError> {
        let mut params = self.embed_params()?;
        params["text"] = json!(text);
        let result = self.call(Worker::Embed, "embed.query", params).await?;
        decode_vector(result.get("vector"))
    }

    /// Vectors for document pieces `(title, text)`, in the same order. Callers send small batches
    /// (at most 8) so a search never waits long behind indexing (PROTOCOL.md "Concurrency").
    pub async fn embed_documents(
        &self,
        items: Vec<(String, String)>,
    ) -> Result<Vec<Vec<f32>>, SidecarError> {
        let mut params = self.embed_params()?;
        params["items"] = items
            .iter()
            .map(|(title, text)| json!({"title": title, "text": text}))
            .collect();
        let result = self.call(Worker::Embed, "embed.documents", params).await?;
        let vectors = result
            .get("vectors")
            .and_then(Value::as_array)
            .ok_or_else(|| bad_reply("no vectors"))?;
        if vectors.len() != items.len() {
            return Err(bad_reply("wrong number of vectors"));
        }
        vectors.iter().map(|v| decode_vector(Some(v))).collect()
    }

    /// Loads the model in `dir` and embeds a test sentence, which must give `EMBED_DIMS` finite
    /// numbers of length 1 (FR-RAG-EM-3: the last check before an added model is used).
    pub async fn check_model(&self, dir: &std::path::Path) -> Result<(), SidecarError> {
        let threads = lock(&self.shared.config).embed_threads;
        let params = json!({
            "model_dir": dir,
            "dims": EMBED_DIMS,
            "threads": threads,
            "text": "Cairn keeps notes and tasks in one place.",
        });
        let result = self.call(Worker::Embed, "embed.query", params).await?;
        let vector = decode_vector(result.get("vector"))?;
        let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if vector.iter().all(|x| x.is_finite()) && (norm - 1.0).abs() <= 0.05 {
            Ok(())
        } else {
            Err(bad_reply("a test sentence gave an unusable vector"))
        }
    }

    /// `model_dir`, `dims` and `threads` for every `embed.*` request.
    fn embed_params(&self) -> Result<Value, SidecarError> {
        let config = lock(&self.shared.config);
        let model_dir = config.model_dir.as_ref().ok_or_else(|| SidecarError {
            kind: NOT_INSTALLED.into(),
            message: if cfg!(debug_assertions) {
                "The search model isn't installed. Run scripts/setup-sidecar-dev.sh, set CAIRN_EMBED_MODEL_DIR, or add it in Settings › Search."
            } else {
                "Search by meaning isn't installed. It can be added in Settings › Search."
            }
            .into(),
            hint: None,
        })?;
        Ok(json!({
            "model_dir": model_dir,
            "dims": EMBED_DIMS,
            "threads": config.embed_threads,
        }))
    }
}

fn bad_reply(what: &str) -> SidecarError {
    SidecarError {
        kind: "internal".into(),
        message: format!("The search model worker sent an unexpected reply ({what})."),
        hint: None,
    }
}

/// Base64 of little-endian f32 (PROTOCOL.md "embed methods"), `EMBED_DIMS` numbers.
fn decode_vector(value: Option<&Value>) -> Result<Vec<f32>, SidecarError> {
    use base64::Engine as _;
    let text = value
        .and_then(Value::as_str)
        .ok_or_else(|| bad_reply("no vector"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|_| bad_reply("vector is not base64"))?;
    if bytes.len() != EMBED_DIMS as usize * 4 {
        return Err(bad_reply("vector has the wrong size"));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// NFR-SEC-3, D-47: a stderr line of the sidecar goes to our log by its level. The sidecar writes
/// `[sidecar] LEVEL text` (`rpc.log`): ERROR and WARNING keep their level, INFO and DEBUG become
/// debug lines. Anything else (a traceback, a library's warning) is kept as a warning, because it
/// may be the only trace of a crash.
fn log_stderr_line(worker: Worker, line: &str) {
    let (level, text) = parse_stderr_line(line);
    let text = text.chars().take(1000).collect::<String>();
    match level {
        StderrLevel::Error => tracing::error!(target: "sidecar", worker = ?worker, "{text}"),
        StderrLevel::Warn => tracing::warn!(target: "sidecar", worker = ?worker, "{text}"),
        StderrLevel::Debug => tracing::debug!(target: "sidecar", worker = ?worker, "{text}"),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum StderrLevel {
    Error,
    Warn,
    Debug,
}

fn parse_stderr_line(line: &str) -> (StderrLevel, &str) {
    let Some(rest) = line.strip_prefix("[sidecar] ") else {
        return (StderrLevel::Warn, line);
    };
    let (level, text) = rest.split_once(' ').unwrap_or((rest, ""));
    match level {
        "ERROR" | "CRITICAL" => (StderrLevel::Error, text),
        "WARNING" | "WARN" => (StderrLevel::Warn, text),
        "INFO" | "DEBUG" => (StderrLevel::Debug, text),
        // An old or unknown format: keep it.
        _ => (StderrLevel::Warn, rest),
    }
}

/// Owns the child process: reads its output, copies its stderr to our log, and reports its end.
async fn supervise(
    shared: Arc<Shared>,
    slot: Arc<Slot>,
    conn: Arc<Conn>,
    mut child: Child,
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
) {
    let worker = conn.worker;
    // NFR-SEC-3: the sidecar never writes content or keys to stderr; here it only goes to the log.
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log_stderr_line(worker, &line);
        }
    });

    let mut lines = BufReader::new(stdout).lines();
    loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => conn.dispatch(&line),
                _ => break,
            },
            _ = conn.kill.notified() => {
                let _ = child.start_kill();
                break;
            }
        }
    }

    conn.closed.store(true, Ordering::SeqCst);
    let exit = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(status) => status.ok(),
        Err(_) => {
            let _ = child.kill().await;
            None
        }
    };
    let message = match exit.and_then(|s| s.code()) {
        Some(code) => format!(
            "The {} stopped unexpectedly (exit code {code}). It starts again on the next use.",
            worker.label()
        ),
        None => format!(
            "The {} was stopped unexpectedly. It starts again on the next use.",
            worker.label()
        ),
    };
    conn.fail_all(&message);

    let current_start = slot.generation.load(Ordering::SeqCst) == conn.generation;
    if !conn.discard.load(Ordering::SeqCst) && current_start {
        let status = if conn.stopping.load(Ordering::SeqCst) {
            state(WorkerState::Stopped, None)
        } else {
            tracing::warn!(worker = ?worker, "the sidecar process ended unexpectedly");
            state(WorkerState::Failed, Some(message))
        };
        shared.set_status(worker, status);
    }
    let mut current = slot.current.lock().await;
    if current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &conn)) {
        *current = None;
    }
}

/// Closes the process's stdin when nothing has been requested for `idle_timeout` (FR-RAG-EM-1a).
async fn idle_monitor(shared: Arc<Shared>, slot: Arc<Slot>, conn: Arc<Conn>) {
    loop {
        let timeout = lock(&shared.config).idle_timeout;
        let poll = (timeout / 4).clamp(Duration::from_millis(10), Duration::from_secs(5));
        tokio::time::sleep(poll).await;
        if conn.closed.load(Ordering::SeqCst) {
            return;
        }
        let idle = |c: &Conn| {
            c.in_flight.load(Ordering::SeqCst) == 0 && lock(&c.last_activity).elapsed() >= timeout
        };
        if !idle(&conn) {
            continue;
        }
        // Check again with the slot locked: new requests are counted under the same lock.
        let mut current = slot.current.lock().await;
        if conn.closed.load(Ordering::SeqCst) || !idle(&conn) {
            continue;
        }
        conn.stopping.store(true, Ordering::SeqCst);
        conn.closed.store(true, Ordering::SeqCst);
        if current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &conn)) {
            *current = None;
        }
        drop(current);
        // Closing stdin is the polite request to exit (PROTOCOL.md "Lifetime"); kill if ignored.
        conn.stdin.lock().await.take();
        tokio::time::sleep(Duration::from_secs(5)).await;
        conn.kill.notify_one();
        return;
    }
}

#[cfg(test)]
mod stderr_tests {
    use super::*;

    /// Windows: a Python that writes `\r\n` must still parse (M8). `supervise` reads with
    /// `lines()`, which drops both characters; and JSON ignores trailing white space anyway.
    #[tokio::test]
    async fn crlf_lines_from_the_sidecar_are_read() {
        let mut lines = BufReader::new(&b"{\"id\":1}\r\n[sidecar] INFO x\r\n"[..]).lines();
        let first = lines.next_line().await.unwrap().unwrap();
        assert_eq!(first, "{\"id\":1}");
        assert!(serde_json::from_str::<Value>("{\"id\":1}\r").is_ok());
        let second = lines.next_line().await.unwrap().unwrap();
        assert_eq!(parse_stderr_line(&second), (StderrLevel::Debug, "x"));
    }

    #[test]
    fn stderr_lines_are_mapped_by_level() {
        use StderrLevel::*;
        assert_eq!(parse_stderr_line("[sidecar] ERROR llm.complete: KeyError"), (Error, "llm.complete: KeyError"));
        assert_eq!(parse_stderr_line("[sidecar] WARNING slow"), (Warn, "slow"));
        assert_eq!(parse_stderr_line("[sidecar] INFO ai started, pid 4"), (Debug, "ai started, pid 4"));
        assert_eq!(parse_stderr_line("[sidecar] DEBUG x"), (Debug, "x"));
        // No format at all (a traceback, a library warning): kept.
        assert_eq!(parse_stderr_line("Traceback (most recent call last):"), (Warn, "Traceback (most recent call last):"));
        assert_eq!(parse_stderr_line("[sidecar] ai started"), (Warn, "ai started"));
    }
}
