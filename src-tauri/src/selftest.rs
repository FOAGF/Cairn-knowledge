//! The release check (NFR-INS-1, D-46; M7 part C): the release command starts the built app once
//! in self-test mode, which checks that the sidecar answers and embedding works, prints the result
//! and exits.
//!
//! - **Start:** `Cairn.AppImage --self-test` (or the environment variable `CAIRN_SELF_TEST=1`).
//!   `main` checks `requested()` before anything else, so no window opens and no display is needed.
//! - **What it does:** finds Python and the model the way the app does (`SidecarConfig::resolve`),
//!   starts both workers, pings the AI worker and embeds one sentence, which must give a vector of
//!   `EMBED_DIMS` numbers. A lite package (no model bundled, NFR-INS-1b, D-82) skips the embedding:
//!   `model_dir`, `dims` and `embed_ms` are null, and the release script expects that only there. It never opens the data folder (the release script still sets a
//!   throw-away `CAIRN_DATA_DIR`, so nothing can touch real data).
//! - **Result:** one line of JSON on stdout (`{"ok":true,...}` or `{"ok":false,"error":"..."}`) and
//!   exit code 0 or 1. If `CAIRN_SELF_TEST_OUT=<file>` is set, the same line is also written to that
//!   file: a Windows GUI-subsystem exe has no usable stdout when PowerShell starts it (M8).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::sidecar::{EMBED_DIMS, SidecarConfig, Sidecars, Worker};

/// How long the whole check may take (a cold start reads the 200 MB model from the AppImage).
const TIMEOUT: Duration = Duration::from_secs(180);
const SENTENCE: &str = "Cairn keeps notes and tasks in one place.";

/// Whether this run is the self-test.
pub fn requested() -> bool {
    std::env::args().skip(1).any(|a| a == "--self-test")
        || std::env::var_os("CAIRN_SELF_TEST").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Runs the check, prints the one-line result and returns the process exit code.
pub fn run() -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => return report(Err(format!("could not start the async runtime: {e}"))),
    };
    let result = runtime.block_on(async {
        match tokio::time::timeout(TIMEOUT, check()).await {
            Ok(r) => r,
            Err(_) => Err(format!("no answer within {} seconds", TIMEOUT.as_secs())),
        }
    });
    report(result)
}

fn report(result: Result<Value, String>) -> i32 {
    let (line, code) = match result {
        Ok(mut v) => {
            v["ok"] = json!(true);
            (v.to_string(), 0)
        }
        Err(error) => (json!({"ok": false, "error": error}).to_string(), 1),
    };
    print_line(&line);
    if let Some(file) = std::env::var_os("CAIRN_SELF_TEST_OUT").filter(|f| !f.is_empty()) {
        write_line(Path::new(&file), &line);
    }
    code
}

/// `println!` panics when stdout is closed (a GUI exe without a console), so write quietly.
fn print_line(line: &str) {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// The result line plus a newline, in UTF-8 without a byte order mark.
fn write_line(file: &Path, line: &str) {
    if let Err(e) = std::fs::write(file, format!("{line}\n")) {
        eprintln!("could not write {}: {e}", file.display());
    }
}

async fn check() -> Result<Value, String> {
    let config = SidecarConfig::resolve(1, resource_dir());
    let python = config.python.clone();
    let model_dir = config.model_dir.clone();
    let sidecars = Sidecars::new(config, Arc::new(|_| {}));

    let started = Instant::now();
    let pong = sidecars
        .call(Worker::Ai, "ping", json!({}))
        .await
        .map_err(|e| format!("the AI worker did not answer: {}", e.message))?;
    let ping_ms = started.elapsed().as_millis() as u64;

    // The LLM library loads: its local model list knows a well-known model (no network call). Its
    // import once failed only in the compiled-only Windows bundle (M8: LiteLLM's `.env` lookup).
    let started = Instant::now();
    let info = sidecars
        .call(
            Worker::Ai,
            "llm.model_info",
            json!({"target": {"provider_kind": "openai_compatible", "model": "gpt-4o"}}),
        )
        .await
        .map_err(|e| format!("the AI worker's LLM library did not answer: {}", e.message))?;
    if !info["max_input_tokens"].as_u64().is_some_and(|n| n > 0) {
        return Err("the AI worker could not load its LLM library (LiteLLM)".into());
    }
    let llm_ms = started.elapsed().as_millis() as u64;

    if model_dir.is_none() {
        return Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "python": pong.get("python"),
            "python_path": python,
            "model_dir": null,
            "ping_ms": ping_ms,
            "llm_ms": llm_ms,
            "embed_ms": null,
            "dims": null,
        }));
    }
    let started = Instant::now();
    let vector = sidecars
        .embed_query(SENTENCE)
        .await
        .map_err(|e| format!("embedding failed: {}", e.message))?;
    let embed_ms = started.elapsed().as_millis() as u64;
    if vector.len() != EMBED_DIMS as usize {
        return Err(format!(
            "the embedding has {} numbers, expected {EMBED_DIMS}",
            vector.len()
        ));
    }
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !norm.is_finite() || (norm - 1.0).abs() > 0.05 {
        return Err(format!("the embedding is not normalised (length {norm})"));
    }
    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "python": pong.get("python"),
        "python_path": python,
        "model_dir": model_dir,
        "ping_ms": ping_ms,
        "llm_ms": llm_ms,
        "embed_ms": embed_ms,
        "dims": vector.len(),
    }))
}

/// The folder Tauri bundles resources into. The AppImage has the app at `usr/bin/<exe>` and the
/// resources at `usr/lib/Cairn`; a build run from `target/` keeps them next to the executable; on
/// Windows they are always next to the exe (Tauri does the same).
/// (Tauri's own lookup needs an app handle, which would need a display.)
fn resource_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    if cfg!(windows) {
        return Some(dir.to_path_buf());
    }
    dir.join("../lib/Cairn")
        .canonicalize()
        .ok()
        .or_else(|| Some(dir.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_result_line_is_written_to_the_file_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("out.json");
        write_line(&file, r#"{"ok":true}"#);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{\"ok\":true}\n");
    }
}
