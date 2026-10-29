//! Pasting a screenshot or a copied image file into an editor (FR-KN-ED-3). The webview hands a
//! pasted image to the page as a file on Windows (WebView2), but WebKitGTK on Linux may offer the
//! paste with no file and no text at all; then the editor asks here, and Rust reads the system
//! clipboard: an image (a screenshot tool's copy), else the first image file among copied files (a
//! file manager's *Copy*). Only called from a paste the user made, when it held no text or file.

use base64::Engine;
use serde::Serialize;
use specta::Type;
use tauri::AppHandle;

use crate::error::ApiResult;

/// An image read from the clipboard: a screenshot (as PNG) or a copied image file.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardImage {
    pub mime: String,
    pub data_base64: String,
}

/// The image on the system clipboard, or `None` (none there, or not needed on this system).
#[tauri::command]
#[specta::specta]
pub async fn read_clipboard_image(app: AppHandle) -> ApiResult<Option<ClipboardImage>> {
    Ok(read_image(&app).await.map(|(mime, bytes)| ClipboardImage {
        mime: mime.into(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }))
}

/// The types an editor stores (attachments, FR-KN-ED-3), by file extension.
#[cfg(target_os = "linux")]
fn image_mime(path: &std::path::Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// GTK's clipboard is read on its main thread; the reply comes back on a channel.
#[cfg(target_os = "linux")]
async fn read_image(app: &AppHandle) -> Option<(&'static str, Vec<u8>)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        let image = clipboard
            .wait_for_image()
            .and_then(|pixbuf| pixbuf.save_to_bufferv("png", &[]).ok())
            .map(|png| ("image/png", png));
        // Otherwise the first copied image file (the link is read here, the file below).
        let file = image.is_none().then(|| {
            clipboard.wait_for_uris().into_iter().find_map(|uri| {
                let path = url::Url::parse(&uri).ok()?.to_file_path().ok()?;
                image_mime(&path).map(|mime| (mime, path))
            })
        });
        let _ = tx.send((image, file.flatten()));
    })
    .ok()?;
    let (image, file) = match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
        Ok(Ok(found)) => found,
        _ => {
            tracing::warn!("reading the clipboard took too long");
            return None;
        }
    };
    if image.is_some() {
        return image;
    }
    let (mime, path) = file?;
    match tauri::async_runtime::spawn_blocking(move || std::fs::read(path)).await.ok()? {
        Ok(bytes) => Some((mime, bytes)),
        Err(e) => {
            tracing::warn!(error = %e, "could not read the copied image file");
            None
        }
    }
}

/// Other systems hand pasted images to the page itself.
#[cfg(not(target_os = "linux"))]
async fn read_image(_app: &AppHandle) -> Option<(&'static str, Vec<u8>)> {
    None
}
