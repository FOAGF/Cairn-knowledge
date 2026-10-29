//! The search model added after installing (FR-RAG-EM-3, D-82). The lite packages leave the
//! embedding model out; it can be downloaded or added from a folder or a `.zip` later, into
//! `<data>/models/embeddinggemma-300m`.
//!
//! - **Pins:** the files, their sizes and SHA-256, and the Hugging Face revision come from
//!   `packaging/model-pins.sh`, the file the release scripts use ([`pins`]).
//! - **Checks:** only the three pinned files are taken from a source; each must have its pinned
//!   size and hash. From a zip, only those entries are read, never more than the pinned size, and
//!   an entry with an absolute path or `..` is refused. The hash is taken on the bytes as they are
//!   written into a temporary folder in `models/` ([`stage`]), so what is checked is what is kept.
//!   The Tauri layer then has the search worker load the folder, and only after that does
//!   [`activate`] rename it into place. A failure removes the temporary folder.
//! - **State** (`search-model.json` in the data folder, like `backup-state.json`): whether a
//!   download is under way (it resumes at the next start), whether the Gemma terms were accepted,
//!   and whether the Home notice was dismissed. It is local to this data folder and never exported.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::paths::DataPaths;
use crate::{CoreError, Result};

/// The folder name of the model, inside `models/` (and inside the app's resources when bundled).
pub const MODEL_FOLDER: &str = "embeddinggemma-300m";
/// Where a download keeps its files until they are checked.
pub const DOWNLOAD_FOLDER: &str = ".download";
const STATE_FILE: &str = "search-model.json";
const PINS_SOURCE: &str = include_str!("../../../packaging/model-pins.sh");
const BUFFER: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedFile {
    /// Relative path inside the model folder, e.g. `onnx/model_q4.onnx`.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

impl PinnedFile {
    /// The file name without its folder, e.g. `model_q4.onnx`.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pins {
    pub repo: String,
    pub revision: String,
    pub files: Vec<PinnedFile>,
}

impl Pins {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// The download address of `file` at the pinned revision, below `base` (Hugging Face's
    /// `https://huggingface.co`, or a test server).
    pub fn url(&self, base: &str, file: &PinnedFile) -> String {
        format!("{}/{}/resolve/{}/{}", base.trim_end_matches('/'), self.repo, self.revision, file.path)
    }
}

/// The model's pins, read from `packaging/model-pins.sh` when the app was compiled.
pub fn pins() -> &'static Pins {
    static PINS: OnceLock<Pins> = OnceLock::new();
    PINS.get_or_init(|| parse_pins(PINS_SOURCE).expect("packaging/model-pins.sh can't be read"))
}

/// Reads `MODEL_REPO=`, `MODEL_REV=` and the `MODEL_PINS` ("sha256  path") and `MODEL_SIZES`
/// ("size  path") lists of the shell file.
fn parse_pins(text: &str) -> Option<Pins> {
    let value = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{name}=")))
            .map(|v| v.trim().trim_matches('"').to_string())
    };
    // A quoted list: from `NAME="` to the closing quote.
    let list = |name: &str| -> Option<Vec<(String, String)>> {
        let start = text.find(&format!("{name}=\""))? + name.len() + 2;
        let end = start + text[start..].find('"')?;
        text[start..end]
            .lines()
            .map(|l| {
                let (a, b) = l.trim().split_once("  ")?;
                Some((a.to_string(), b.trim().to_string()))
            })
            .collect()
    };
    let hashes = list("MODEL_PINS")?;
    let sizes = list("MODEL_SIZES")?;
    let files = hashes
        .into_iter()
        .map(|(sha256, path)| {
            let size = sizes.iter().find(|(_, p)| *p == path)?.0.parse().ok()?;
            Some(PinnedFile { path, size, sha256 })
        })
        .collect::<Option<Vec<_>>>()?;
    (!files.is_empty()).then_some(Pins { repo: value("MODEL_REPO")?, revision: value("MODEL_REV")?, files })
}

/// The added model's folder.
pub fn added_dir(paths: &DataPaths) -> PathBuf {
    paths.models_dir().join(MODEL_FOLDER)
}

pub fn download_dir(paths: &DataPaths) -> PathBuf {
    paths.models_dir().join(DOWNLOAD_FOLDER)
}

/// True when `dir` holds every pinned file with its pinned size (cheap: no hashing; the hashes
/// were checked when the model was added).
pub fn is_complete(dir: &Path) -> bool {
    is_complete_with(pins(), dir)
}

fn is_complete_with(pins: &Pins, dir: &Path) -> bool {
    pins.files
        .iter()
        .all(|f| fs::metadata(dir.join(&f.path)).is_ok_and(|m| m.is_file() && m.len() == f.size))
}

// ---------------------------------------------------------------- state

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelState {
    /// A download was started and has neither finished nor been cancelled: it resumes at start.
    pub downloading: bool,
    /// The Gemma Terms of Use were accepted before a download.
    pub terms_accepted: bool,
    /// The Home notice for a copy with data but no model was dismissed for good.
    pub notice_dismissed: bool,
}

pub fn load_state(paths: &DataPaths) -> ModelState {
    fs::read_to_string(paths.root.join(STATE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_state(paths: &DataPaths, state: &ModelState) -> Result<()> {
    let file = paths.root.join(STATE_FILE);
    let part = paths.root.join(format!("{STATE_FILE}.part"));
    fs::write(&part, serde_json::to_vec_pretty(state)?)?;
    fs::rename(part, file)?;
    Ok(())
}

// ---------------------------------------------------------------- adding

/// Copies the pinned files from `source` (a folder, or a `.zip` file) into a new temporary folder
/// in `models/`, checking each file's size and hash on the bytes written. With `move_files` (a
/// finished download, whose folder Cairn owns) a folder's files are hashed where they are and then
/// moved, not copied. `progress` gets the bytes done so far; setting `cancel` stops the work.
/// Returns the temporary folder; on any error it is removed again.
pub fn stage(
    paths: &DataPaths,
    source: &Path,
    move_files: bool,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    stage_with(pins(), paths, source, move_files, progress, cancel)
}

fn stage_with(
    pins: &Pins,
    paths: &DataPaths,
    source: &Path,
    move_files: bool,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    fs::create_dir_all(paths.models_dir())?;
    let staging = paths.models_dir().join(format!(".incoming-{}", uuid::Uuid::now_v7().simple()));
    fs::create_dir(&staging)?;
    let result = if source.is_dir() {
        stage_folder(pins, source, &staging, move_files, progress, cancel)
    } else {
        stage_zip(pins, source, &staging, progress, cancel)
    };
    match result {
        Ok(()) => Ok(staging),
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

fn invalid(message: String) -> CoreError {
    CoreError::Invalid(message)
}

fn cancelled() -> CoreError {
    invalid("Cancelled.".into())
}

/// A file of the folder: at its pinned path (the Hugging Face layout, `onnx/…`) or at the top.
fn find_in_folder(source: &Path, file: &PinnedFile) -> Option<PathBuf> {
    [source.join(&file.path), source.join(file.name())]
        .into_iter()
        .find(|p| p.is_file())
}

fn stage_folder(
    pins: &Pins,
    source: &Path,
    staging: &Path,
    move_files: bool,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<()> {
    // Every file is found, with the right size, before anything is read.
    let mut found = Vec::new();
    for file in &pins.files {
        let path = find_in_folder(source, file)
            .ok_or_else(|| invalid(format!("{} wasn't found in {}.", file.name(), source.display())))?;
        check_size(file, fs::metadata(&path)?.len())?;
        found.push((file, path));
    }
    let mut done = 0;
    for (file, path) in found {
        let target = staging.join(&file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if move_files {
            // Cairn's own download folder: hash in place, then move (no second copy of 200 MB).
            let mut reader = File::open(&path)?;
            let digest = copy_hashing(file, &mut reader, &mut std::io::sink(), &mut done, progress, cancel)?;
            check_hash(file, &digest)?;
            if fs::rename(&path, &target).is_err() {
                copy_checked(file, &mut File::open(&path)?, &target, &mut 0, &mut |_| {}, cancel)?;
            }
        } else {
            copy_checked(file, &mut File::open(&path)?, &target, &mut done, progress, cancel)?;
        }
    }
    Ok(())
}

fn stage_zip(
    pins: &Pins,
    source: &Path,
    staging: &Path,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<()> {
    let open_error = |e: zip::result::ZipError| invalid(format!("{} can't be read as a zip file: {e}", source.display()));
    let mut archive = zip::ZipArchive::new(File::open(source)?).map_err(open_error)?;
    // Each pinned file's entry: at its pinned path or by its name, at the top or in one folder.
    let mut entries = Vec::new();
    for file in &pins.files {
        let mut matches = Vec::new();
        for i in 0..archive.len() {
            let entry = archive.by_index_raw(i).map_err(open_error)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().replace('\\', "/");
            let fits = name == file.path
                || name.ends_with(&format!("/{}", file.path))
                || name == file.name()
                || name.ends_with(&format!("/{}", file.name()));
            if fits {
                // Absolute paths and `..` are refused, even though nothing is unpacked by its name.
                if entry.enclosed_name().is_none() {
                    return Err(invalid(format!("The zip file has an unsafe path: {name}")));
                }
                matches.push((i, entry.size()));
            }
        }
        match matches.as_slice() {
            [] => return Err(invalid(format!("{} wasn't found in the zip file.", file.name()))),
            [(i, size)] => {
                check_size(file, *size)?;
                entries.push((file, *i));
            }
            _ => return Err(invalid(format!("The zip file holds more than one {}.", file.name()))),
        }
    }
    let mut done = 0;
    for (file, i) in entries {
        let entry = archive.by_index(i).map_err(open_error)?;
        let target = staging.join(&file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // Never more than the pinned size, whatever the entry's header says.
        copy_checked(file, &mut entry.take(file.size + 1), &target, &mut done, progress, cancel)?;
    }
    Ok(())
}

fn check_size(file: &PinnedFile, size: u64) -> Result<()> {
    if size == file.size {
        return Ok(());
    }
    Err(invalid(format!(
        "{} is {size} bytes, but the expected file is {} bytes. It may be from a different model version.",
        file.name(),
        file.size
    )))
}

fn check_hash(file: &PinnedFile, digest: &str) -> Result<()> {
    if digest.eq_ignore_ascii_case(&file.sha256) {
        return Ok(());
    }
    Err(invalid(format!(
        "{} doesn't match the expected file. It may be from a different model version, or damaged.",
        file.name()
    )))
}

/// Writes `reader` to `target` and checks the bytes written against the pins.
fn copy_checked(
    file: &PinnedFile,
    reader: &mut dyn Read,
    target: &Path,
    done: &mut u64,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<()> {
    let mut out = File::create(target)?;
    let digest = copy_hashing(file, reader, &mut out, done, progress, cancel)?;
    out.sync_all()?;
    check_hash(file, &digest)
}

/// Copies at most the pinned size plus one byte (to notice a longer file), hashing on the way.
fn copy_hashing(
    file: &PinnedFile,
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    done: &mut u64,
    progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; BUFFER];
    let mut written = 0u64;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(cancelled());
        }
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        written += n as u64;
        if written > file.size {
            return Err(invalid(format!("{} is longer than the expected file.", file.name())));
        }
        hasher.update(&buffer[..n]);
        writer.write_all(&buffer[..n])?;
        *done += n as u64;
        progress(*done);
    }
    check_size(file, written)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Renames a checked temporary folder (from [`stage`]) into place as the added model, replacing
/// one that is there. Returns the model folder.
pub fn activate(paths: &DataPaths, staging: &Path) -> Result<PathBuf> {
    let dir = added_dir(paths);
    if dir.exists() {
        remove(paths)?;
    }
    fs::rename(staging, &dir)?;
    Ok(dir)
}

/// Deletes the added model: renamed first, so a half-deleted folder is never taken for a model.
pub fn remove(paths: &DataPaths) -> Result<()> {
    let dir = added_dir(paths);
    if !dir.exists() {
        return Ok(());
    }
    let doomed = paths.models_dir().join(format!(".removing-{}", uuid::Uuid::now_v7().simple()));
    fs::rename(&dir, &doomed)?;
    fs::remove_dir_all(&doomed)?;
    Ok(())
}

/// Removes temporary folders an earlier run left behind (a check or a removal cut short). The
/// download folder stays: its finished files let a download resume.
pub fn clean_leftovers(paths: &DataPaths) {
    let Ok(entries) = fs::read_dir(paths.models_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".incoming-") || name.starts_with(".removing-") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: [(&str, &[u8]); 3] = [
        ("onnx/model_q4.onnx", b"graph"),
        ("onnx/model_q4.onnx_data", b"weights weights weights"),
        ("tokenizer.json", b"{\"vocab\":{}}"),
    ];

    fn sha(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn test_pins() -> Pins {
        Pins {
            repo: "org/model".into(),
            revision: "abc".into(),
            files: FILES
                .iter()
                .map(|(p, b)| PinnedFile { path: (*p).into(), size: b.len() as u64, sha256: sha(b) })
                .collect(),
        }
    }

    struct Env {
        _dir: tempfile::TempDir,
        paths: DataPaths,
        source: PathBuf,
    }

    fn env() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path().join("data"));
        fs::create_dir_all(&paths.root).unwrap();
        let source = dir.path().join("source");
        fs::create_dir_all(&source).unwrap();
        Env { _dir: dir, paths, source }
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn stage_from(e: &Env, source: &Path, move_files: bool) -> Result<PathBuf> {
        stage_with(&test_pins(), &e.paths, source, move_files, &mut |_| {}, &AtomicBool::new(false))
    }

    /// Nothing but the model folder (if any) is left in `models/`.
    fn no_leftovers(e: &Env) {
        let left: Vec<String> = fs::read_dir(e.paths.models_dir())
            .unwrap()
            .map(|d| d.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != MODEL_FOLDER)
            .collect();
        assert!(left.is_empty(), "left behind: {left:?}");
    }

    #[test]
    fn the_shipped_pins_are_read() {
        let p = pins();
        assert_eq!(p.repo, "onnx-community/embeddinggemma-300m-ONNX");
        assert_eq!(p.files.len(), 3);
        assert!(p.files.iter().all(|f| f.size > 0 && f.sha256.len() == 64));
        assert!(p.total_size() > 200_000_000);
        assert!(p.url("https://huggingface.co/", &p.files[0]).starts_with(
            "https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX/resolve/"
        ));
    }

    #[test]
    fn a_folder_in_either_layout_is_staged_and_activated() {
        let e = env();
        // Hugging Face layout
        for (p, b) in FILES {
            write(&e.source.join(p), b);
        }
        let staging = stage_from(&e, &e.source, false).unwrap();
        let dir = activate(&e.paths, &staging).unwrap();
        assert!(is_complete_with(&test_pins(), &dir));
        assert!(e.source.join("tokenizer.json").exists(), "a copy leaves the source alone");
        // Flat layout, replacing the model that is there
        let flat = e.source.join("flat");
        for (p, b) in FILES {
            write(&flat.join(p.rsplit('/').next().unwrap()), b);
        }
        let staging = stage_from(&e, &flat, false).unwrap();
        activate(&e.paths, &staging).unwrap();
        assert!(is_complete_with(&test_pins(), &added_dir(&e.paths)));
        no_leftovers(&e);
    }

    #[test]
    fn a_download_is_moved_not_copied() {
        let e = env();
        let download = download_dir(&e.paths);
        for (p, b) in FILES {
            write(&download.join(p), b);
        }
        let staging = stage_from(&e, &download, true).unwrap();
        assert!(!download.join("tokenizer.json").exists());
        activate(&e.paths, &staging).unwrap();
        assert!(is_complete_with(&test_pins(), &added_dir(&e.paths)));
    }

    #[test]
    fn wrong_or_missing_files_are_refused_and_nothing_is_left() {
        let e = env();
        write(&e.source.join("onnx/model_q4.onnx"), b"graph");
        let err = stage_from(&e, &e.source, false).unwrap_err().to_string();
        assert!(err.contains("model_q4.onnx_data wasn't found"), "{err}");

        write(&e.source.join("onnx/model_q4.onnx_data"), b"weights weights weightz"); // same size, other bytes
        write(&e.source.join("tokenizer.json"), b"{\"vocab\":{}}");
        let err = stage_from(&e, &e.source, false).unwrap_err().to_string();
        assert!(err.contains("model_q4.onnx_data doesn't match"), "{err}");

        write(&e.source.join("onnx/model_q4.onnx_data"), b"short");
        let err = stage_from(&e, &e.source, false).unwrap_err().to_string();
        assert!(err.contains("is 5 bytes"), "{err}");
        no_leftovers(&e);
        assert!(!added_dir(&e.paths).exists());
    }

    #[test]
    fn a_zip_is_read_entry_by_entry() {
        let e = env();
        let zip = e.source.join("model.zip");
        let mut entries: Vec<(&str, &[u8])> = vec![
            ("embeddinggemma-300m-ONNX/onnx/model_q4.onnx", FILES[0].1),
            ("embeddinggemma-300m-ONNX/onnx/model_q4.onnx_data", FILES[1].1),
            ("embeddinggemma-300m-ONNX/tokenizer.json", FILES[2].1),
            ("embeddinggemma-300m-ONNX/onnx/model_fp16.onnx", b"ignored"),
        ];
        zip_of(&zip, &entries);
        let staging = stage_from(&e, &zip, false).unwrap();
        activate(&e.paths, &staging).unwrap();
        assert!(is_complete_with(&test_pins(), &added_dir(&e.paths)));
        assert!(!added_dir(&e.paths).join("onnx/model_fp16.onnx").exists());

        // A second copy of a file is ambiguous.
        entries.push(("other/tokenizer.json", FILES[2].1));
        zip_of(&zip, &entries);
        let err = stage_from(&e, &zip, false).unwrap_err().to_string();
        assert!(err.contains("more than one tokenizer.json"), "{err}");
        no_leftovers(&e);
    }

    #[test]
    fn unsafe_or_oversized_zip_entries_are_refused() {
        let e = env();
        let zip = e.source.join("bad.zip");
        zip_of(&zip, &[("../onnx/model_q4.onnx", FILES[0].1), ("onnx/model_q4.onnx_data", FILES[1].1), ("tokenizer.json", FILES[2].1)]);
        let err = stage_from(&e, &zip, false).unwrap_err().to_string();
        assert!(err.contains("unsafe path"), "{err}");

        zip_of(&zip, &[("onnx/model_q4.onnx", b"graph and more"), ("onnx/model_q4.onnx_data", FILES[1].1), ("tokenizer.json", FILES[2].1)]);
        let err = stage_from(&e, &zip, false).unwrap_err().to_string();
        assert!(err.contains("is 14 bytes"), "{err}");

        fs::write(&zip, b"not a zip").unwrap();
        let err = stage_from(&e, &zip, false).unwrap_err().to_string();
        assert!(err.contains("can't be read as a zip file"), "{err}");
        no_leftovers(&e);
    }

    #[test]
    fn cancelling_stops_and_cleans_up() {
        let e = env();
        for (p, b) in FILES {
            write(&e.source.join(p), b);
        }
        let err = stage_with(&test_pins(), &e.paths, &e.source, false, &mut |_| {}, &AtomicBool::new(true)).unwrap_err();
        assert_eq!(err.to_string(), "Cancelled.");
        no_leftovers(&e);
    }

    #[test]
    fn progress_counts_every_byte() {
        let e = env();
        for (p, b) in FILES {
            write(&e.source.join(p), b);
        }
        let mut last = 0;
        stage_with(&test_pins(), &e.paths, &e.source, false, &mut |d| last = d, &AtomicBool::new(false)).unwrap();
        assert_eq!(last, test_pins().total_size());
    }

    #[test]
    fn removing_and_leftovers() {
        let e = env();
        for (p, b) in FILES {
            write(&e.source.join(p), b);
        }
        let staging = stage_from(&e, &e.source, false).unwrap();
        activate(&e.paths, &staging).unwrap();
        fs::create_dir_all(e.paths.models_dir().join(".incoming-old")).unwrap();
        fs::create_dir_all(download_dir(&e.paths)).unwrap();
        clean_leftovers(&e.paths);
        assert!(!e.paths.models_dir().join(".incoming-old").exists());
        assert!(download_dir(&e.paths).exists(), "a download's files are kept for resuming");
        remove(&e.paths).unwrap();
        assert!(!added_dir(&e.paths).exists());
        remove(&e.paths).unwrap(); // nothing there: fine
    }

    #[test]
    fn the_state_file_round_trips() {
        let e = env();
        assert_eq!(load_state(&e.paths), ModelState::default());
        let s = ModelState { downloading: true, terms_accepted: true, notice_dismissed: false };
        save_state(&e.paths, &s).unwrap();
        assert_eq!(load_state(&e.paths), s);
    }
}
