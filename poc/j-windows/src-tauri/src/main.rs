// M8 feasibility (plan §12): what the real Windows build depends on, checked from inside a Tauri
// app built on Linux. Each check returns one line; the page shows them for the owner to paste back.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(PathBuf::from)
}

/// The portable layout keeps data next to the exe (D-56): can this folder be written?
fn check_write() -> String {
    let Some(dir) = exe_dir() else { return "FAIL write: no exe folder".into() };
    let data = dir.join("data");
    let file = data.join("write-test.txt");
    match std::fs::create_dir_all(&data).and_then(|_| std::fs::write(&file, "ok")) {
        Ok(()) => {
            let _ = std::fs::remove_file(&file);
            let _ = std::fs::remove_dir(&data);
            format!("PASS write next to the exe: {}", dir.display())
        }
        Err(e) => format!("FAIL write next to the exe ({}): {e}", dir.display()),
    }
}

/// API keys go to Credential Manager (NFR-SEC-1, D-57): save, read and delete a throw-away entry.
fn check_keychain() -> String {
    let entry = match keyring::Entry::new("cairn-wintest", "throwaway") {
        Ok(e) => e,
        Err(e) => return format!("FAIL keychain open: {e}"),
    };
    let result = entry
        .set_password("feasibility")
        .and_then(|_| entry.get_password())
        .and_then(|p| entry.delete_credential().map(|_| p));
    match result {
        Ok(p) if p == "feasibility" => "PASS Credential Manager: saved, read and deleted a test entry".into(),
        Ok(p) => format!("FAIL Credential Manager read back {p:?}"),
        Err(e) => format!("FAIL Credential Manager: {e}"),
    }
}

#[tauri::command]
fn checks() -> Vec<String> {
    vec![
        format!("INFO os {} {}", std::env::consts::OS, std::env::consts::ARCH),
        format!("INFO exe {}", std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()),
        check_write(),
        check_keychain(),
    ]
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![checks])
        .run(tauri::generate_context!())
        .expect("error while running the Windows test");
}
