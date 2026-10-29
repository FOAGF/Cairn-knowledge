// Prevents an extra console window on Windows in release builds. Do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK's DMA-BUF renderer draws text inside scrolling areas with greyscale, slightly
    // offset anti-aliasing on this machine's GPU, so it looks doubled. The shared-memory path keeps
    // GPU compositing and renders text normally. A value set by the user wins.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: runs first in `main`, before any other thread exists.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
    // NFR-INS-1: the release check starts the app in self-test mode: no window, one line of JSON.
    if cairn_lib::selftest::requested() {
        std::process::exit(cairn_lib::selftest::run());
    }
    cairn_lib::run();
}
