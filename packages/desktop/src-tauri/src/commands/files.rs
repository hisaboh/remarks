//! File open flow — Phase 4 port of the Electron main "open file" path
//! (menu/dialog → `loadMarkdownFile` → `mt::open-new-tab`).
//!
//!   mt::cmd-open-file → file_open       (pick files via dialog, then open)
//!   mt::open-file     → file_open_path  (open a known path, e.g. from sidebar)
//!
//! Both build a full `MarkdownDocument` (shared/types/files.ts) — encoding
//! detection/decoding (Phase 3) plus the line-ending / trailing-newline logic
//! ported from `filesystem/markdown.ts` — and emit `mt::open-new-tab`, which
//! the editor store already listens for. The event payload is the
//! `[markdownDocument, options, selected]` tuple the renderer expects (the shim
//! spreads arrays into positional listener args).

use std::collections::VecDeque;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogResult};
use tauri_plugin_store::{Store, StoreExt};

use crate::commands::encoding;
use crate::commands::preferences::PREFERENCES_FILE;

const MARKDOWN_EXTENSIONS: &[&str] = &[
    "markdown", "mdown", "mkdn", "md", "mkd", "mdwn", "mdtxt", "mdtext", "mdx", "text", "txt",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownDocument {
    markdown: String,
    filename: String,
    pathname: Option<String>,
    encoding: String,
    line_ending: String,
    adjust_line_ending_on_save: bool,
    trim_trailing_newline: u8,
    is_mixed_line_endings: bool,
}

fn to_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn pref_bool<R: tauri::Runtime>(store: &Store<R>, key: &str, default: bool) -> bool {
    store.get(key).as_ref().and_then(Value::as_bool).unwrap_or(default)
}

fn preferred_eol<R: tauri::Runtime>(store: &Store<R>) -> String {
    let end_of_line = store
        .get("endOfLine")
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();
    match end_of_line.as_str() {
        "lf" => "lf",
        "crlf" => "crlf",
        _ if cfg!(windows) => "crlf",
        _ => "lf",
    }
    .to_string()
}

/// Port of config.ts LF_LINE_ENDING_REG `/(?:[^\r]\n)|(?:^\n$)/` and
/// CRLF_LINE_ENDING_REG `/\r\n/`: returns (has_bare_lf, has_crlf).
fn detect_line_endings(text: &str) -> (bool, bool) {
    let has_crlf = text.contains("\r\n");
    let has_lf = if text == "\n" {
        true
    } else {
        let bytes = text.as_bytes();
        // A bare LF: an '\n' at index > 0 whose previous byte isn't '\r'.
        // (\r and \n are ASCII, so byte scanning is UTF-8 safe.)
        (1..bytes.len()).any(|i| bytes[i] == b'\n' && bytes[i - 1] != b'\r')
    };
    (has_lf, has_crlf)
}

fn build_document<R: tauri::Runtime>(
    store: &Store<R>,
    pathname: &str,
) -> Result<MarkdownDocument, String> {
    let bytes = std::fs::read(pathname).map_err(to_err)?;

    let auto_guess = pref_bool(store, "autoGuessEncoding", true);
    let auto_normalize = pref_bool(store, "autoNormalizeLineEndings", false);
    let trim_pref = store
        .get("trimTrailingNewline")
        .as_ref()
        .and_then(Value::as_u64)
        .unwrap_or(2) as u8;
    let preferred = preferred_eol(store);

    let (enc, _is_bom) = encoding::detect(&bytes, auto_guess);
    let (decoded, _enc_used, _had_errors) = enc.decode(&bytes);
    let mut markdown = decoded.into_owned();

    // Line-ending detection (mirrors loadMarkdownFile).
    let (is_lf, is_crlf) = detect_line_endings(&markdown);
    let is_mixed = is_lf && is_crlf;
    let is_unknown = !is_lf && !is_crlf;
    let mut line_ending = preferred;
    if is_lf && !is_crlf {
        line_ending = "lf".into();
    } else if is_crlf && !is_lf {
        line_ending = "crlf".into();
    }

    let mut adjust = false;
    if is_mixed || is_unknown || line_ending != "lf" {
        // MarkText stores LF internally.
        markdown = markdown.replace("\r\n", "\n");
        adjust = !auto_normalize && line_ending != "lf";
    }

    // Trailing-newline detection (only when prefs leave it at the 2 sentinel).
    let mut trim = trim_pref;
    if trim == 2 {
        trim = if markdown.is_empty() {
            3
        } else if markdown.ends_with("\n\n") {
            2
        } else if markdown.ends_with('\n') {
            1
        } else {
            0
        };
    }

    let filename = Path::new(pathname)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    Ok(MarkdownDocument {
        markdown,
        filename,
        pathname: Some(pathname.to_string()),
        encoding: enc.name().to_string(),
        line_ending,
        adjust_line_ending_on_save: adjust,
        trim_trailing_newline: trim,
        is_mixed_line_endings: is_mixed,
    })
}

// Target only the requesting window — Tauri's plain `emit` broadcasts to every
// window, which would leak tabs/state across windows once multiple are open.
fn emit_open_tab(window: &WebviewWindow, doc: MarkdownDocument, options: Value, selected: bool) {
    let _ = window.emit_to(window.label(), "mt::open-new-tab", json!([doc, options, selected]));
}

/// Whether a path has a markdown extension (shared with the file watcher).
pub fn is_markdown_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| MARKDOWN_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Build the `MarkdownDocument` for a path and return it as a JSON value (used by
/// the file watcher's change payload — 4f). Keeps `MarkdownDocument` private.
pub fn build_document_json(app: &AppHandle, pathname: &str) -> Result<Value, String> {
    let store = app.store(PREFERENCES_FILE).map_err(to_err)?;
    let doc = build_document(&store, pathname)?;
    serde_json::to_value(doc).map_err(|e| e.to_string())
}

/// Build a document for a known path and open it in `window` (used by the
/// macOS Opened handler — 4e). Errors are logged, not surfaced.
pub fn open_path_in_window(app: &AppHandle, window: &WebviewWindow, pathname: &str, selected: bool) {
    let store = match app.store(PREFERENCES_FILE) {
        Ok(store) => store,
        Err(e) => {
            log::error!("open {pathname}: store unavailable: {e}");
            return;
        }
    };
    match build_document(&store, pathname) {
        Ok(doc) => {
            emit_open_tab(window, doc, json!({}), selected);
            crate::commands::watcher::watch_file(app, pathname);
            crate::menu::add_recent(app, pathname);
        }
        Err(e) => log::error!("open {pathname} failed: {e}"),
    }
}

/// Open a known path (sidebar click, search result). `options` is forwarded to
/// the renderer untouched (cursor/selection hints etc.).
#[tauri::command]
pub fn file_open_path(
    app: AppHandle,
    window: WebviewWindow,
    pathname: String,
    options: Option<Value>,
) -> Result<(), String> {
    let store = app.store(PREFERENCES_FILE).map_err(to_err)?;
    let doc = build_document(&store, &pathname)?;
    emit_open_tab(&window, doc, options.unwrap_or_else(|| json!({})), true);
    crate::commands::watcher::watch_file(&app, &pathname);
    crate::menu::add_recent(&app, &pathname);
    Ok(())
}

/// Prompt for one or more markdown files, then open each in a tab.
///
/// Uses the NON-blocking dialog API: synchronous Tauri commands run on the main
/// thread, and a `blocking_*` dialog there deadlocks with the dialog's own need
/// for the main thread (UI beachball). The callback runs when the dialog closes.
#[tauri::command]
pub fn file_open(app: AppHandle, window: WebviewWindow) {
    let app_cb = app.clone();
    app.dialog()
        .file()
        .add_filter("Markdown", MARKDOWN_EXTENSIONS)
        .add_filter("All Files", &["*"])
        .pick_files(move |picked| {
            let Some(files) = picked else {
                return; // cancelled
            };
            let store = match app_cb.store(PREFERENCES_FILE) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("open: store unavailable: {e}");
                    return;
                }
            };
            for (index, file) in files.into_iter().enumerate() {
                let Ok(path) = file.into_path() else { continue };
                let pathname = path.to_string_lossy().into_owned();
                match build_document(&store, &pathname) {
                    // Only the first opened file becomes the active tab.
                    Ok(doc) => {
                        emit_open_tab(&window, doc, json!({}), index == 0);
                        crate::commands::watcher::watch_file(&app_cb, &pathname);
                        crate::menu::add_recent(&app_cb, &pathname);
                    }
                    Err(e) => log::error!("failed to open {pathname}: {e}"),
                }
            }
        });
}

// ---- save -------------------------------------------------------------------

/// Encoding to write with, parsed from the renderer's `options.encoding`, which
/// is either a `{ encoding, isBom }` object or a bare string.
fn parse_encoding(options: &Value) -> (String, bool) {
    match options.get("encoding") {
        Some(Value::String(s)) => (s.clone(), false),
        Some(Value::Object(o)) => {
            let name = o
                .get("encoding")
                .and_then(Value::as_str)
                .unwrap_or("utf8")
                .to_string();
            let is_bom = o.get("isBom").and_then(Value::as_bool).unwrap_or(false);
            (name, is_bom)
        }
        _ => ("utf8".to_string(), false),
    }
}

/// Port of config.ts convertLineEndings: normalize to LF, then to CRLF if asked.
fn convert_line_endings(text: &str, line_ending: &str) -> String {
    let lf = text.replace("\r\n", "\n");
    if line_ending == "crlf" {
        lf.replace('\n', "\r\n")
    } else {
        lf
    }
}

/// Port of writeMarkdownFile's encode step (iconv → encoding_rs).
fn encode_markdown(markdown: &str, options: &Value) -> Vec<u8> {
    let adjust = options
        .get("adjustLineEndingOnSave")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let line_ending = options
        .get("lineEnding")
        .and_then(Value::as_str)
        .unwrap_or("lf");
    let content = if adjust {
        convert_line_endings(markdown, line_ending)
    } else {
        markdown.to_string()
    };

    let (enc_name, is_bom) = parse_encoding(options);
    let enc = encoding_rs::Encoding::for_label(enc_name.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    if enc == encoding_rs::UTF_8 {
        let mut bytes = if is_bom { vec![0xEF, 0xBB, 0xBF] } else { Vec::new() };
        bytes.extend_from_slice(content.as_bytes());
        bytes
    } else {
        // NOTE: encoding_rs::encode yields UTF-8 for UTF-16 targets (it has no
        // UTF-16 encoder); legacy single/multi-byte encodings round-trip fine.
        // TODO(phase-4): handle UTF-16 save if a real file needs it.
        let (cow, _enc_used, _had_errors) = enc.encode(&content);
        cow.into_owned()
    }
}

fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Append the resolved extension (default `.md`) if the path lacks it.
fn ensure_extension(path: String) -> String {
    let ext = Path::new(&path)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_else(|| ".md".to_string());
    if path.ends_with(&ext) {
        path
    } else {
        format!("{path}{ext}")
    }
}

/// Write the encoded markdown and notify the renderer (set-pathname for a
/// new/changed path, tab-saved otherwise; tab-save-failure on error).
fn write_and_react(window: &WebviewWindow, id: &str, file_path: &str, markdown: &str, options: &Value, emit_pathname: bool) {
    let label = window.label();
    let app = window.app_handle();
    let bytes = encode_markdown(markdown, options);
    // Suppress the watcher's change event for our own write (4f).
    crate::commands::watcher::ignore_change(&app, file_path);
    match std::fs::write(file_path, bytes) {
        Ok(()) => {
            if emit_pathname {
                let _ = window.emit_to(
                    label,
                    "mt::set-pathname",
                    json!({ "id": id, "pathname": file_path, "filename": basename(file_path) }),
                );
            } else {
                let _ = window.emit_to(label, "mt::tab-saved", id);
            }
            // Watch the (possibly new) path for external changes (4f).
            crate::commands::watcher::watch_file(&app, file_path);
            // Record in the recently-used documents menu (4g).
            crate::menu::add_recent(&app, file_path);
        }
        Err(e) => {
            log::error!("save failed for {file_path}: {e}");
            let _ = window.emit_to(label, "mt::tab-save-failure", json!([id, e.to_string()]));
        }
    }
}

/// Show a save dialog (NON-blocking — see file_open) seeded with the default
/// directory/filename, invoking `cb` with the chosen path (or None if cancelled).
fn save_dialog<F: FnOnce(Option<String>) + Send + 'static>(
    app: &AppHandle,
    default_full_path: &str,
    cb: F,
) {
    let path = Path::new(default_full_path);
    let mut builder = app.dialog().file();
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            builder = builder.set_directory(dir);
        }
    }
    if let Some(name) = path.file_name() {
        builder = builder.set_file_name(name.to_string_lossy());
    }
    builder.save_file(move |fp| {
        cb(fp
            .and_then(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned()))
    });
}

fn documents_dir(app: &AppHandle) -> String {
    app.path()
        .document_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The top heading from the markdown (lowest #-level, first wins) — Electron's
/// getRecommendTitleFromMarkdownString. Empty if there are no headings. (Like
/// upstream this also matches headings inside code fences.)
fn recommend_title(markdown: &str) -> String {
    let mut best: Option<(usize, String)> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if !(1..=6).contains(&hashes) {
            continue;
        }
        let rest = &trimmed[hashes..];
        if !rest.starts_with(' ') {
            continue;
        }
        let content = rest.trim();
        if content.is_empty() {
            continue;
        }
        if best.as_ref().is_none_or(|(lvl, _)| hashes < *lvl) {
            best = Some((hashes, content.to_string()));
        }
    }
    best.map(|(_, c)| c).unwrap_or_default()
}

/// Suggested base filename for an untitled save: heading from content (4g),
/// else the current filename, else "Untitled". Path separators are stripped so
/// the value is safe to use as a dialog default file name.
fn recommend_filename(markdown: &str, filename: &str) -> String {
    let base = {
        let title = recommend_title(markdown);
        if !title.is_empty() {
            title
        } else if !filename.is_empty() {
            filename.to_string()
        } else {
            "Untitled".to_string()
        }
    };
    base.replace(['/', '\\'], " ")
}

/// Save (Cmd+S). Existing path writes in place; a new file prompts for a path.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn file_save(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
    filename: String,
    pathname: Option<String>,
    markdown: String,
    options: Value,
    default_path: Option<String>,
) -> Result<(), String> {
    // Existing path: write in place (no dialog), emit tab-saved.
    if let Some(p) = pathname {
        let file_path = ensure_extension(p);
        write_and_react(&window, &id, &file_path, &markdown, &options, false);
        return Ok(());
    }
    // New file: prompt for a path (non-blocking), then write + set-pathname.
    let recommend = recommend_filename(&markdown, &filename);
    let dir = default_path.unwrap_or_else(|| documents_dir(&app));
    let default = format!("{dir}/{recommend}.md");
    save_dialog(&app, &default, move |chosen| {
        if let Some(file_path) = chosen {
            let file_path = ensure_extension(file_path);
            write_and_react(&window, &id, &file_path, &markdown, &options, true);
        }
    });
    Ok(())
}

/// Save As — always prompts; updates the tab path when it changes.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn file_save_as(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
    filename: String,
    pathname: Option<String>,
    markdown: String,
    options: Value,
    default_path: Option<String>,
) -> Result<(), String> {
    let recommend = recommend_filename(&markdown, &filename);
    let default = pathname.clone().unwrap_or_else(|| {
        let dir = default_path.unwrap_or_else(|| documents_dir(&app));
        format!("{dir}/{recommend}.md")
    });
    let old_path = pathname;
    save_dialog(&app, &default, move |chosen| {
        if let Some(file_path) = chosen {
            let file_path = ensure_extension(file_path);
            let changed = old_path.as_deref() != Some(file_path.as_str());
            write_and_react(&window, &id, &file_path, &markdown, &options, changed);
        }
    });
    Ok(())
}

// ---- save-on-close (4c) -----------------------------------------------------

/// One entry of the `mt::close-window-confirm` unsaved-files payload.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct UnsavedFile {
    id: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    pathname: Option<String>,
    #[serde(default)]
    markdown: String,
    #[serde(default)]
    options: Value,
    #[serde(default)]
    default_path: Option<String>,
}

/// Save every unsaved file then close the window — the "Save" branch of the
/// close confirm dialog (window.rs). Path-backed files are written in place;
/// untitled files prompt a save dialog, processed one at a time. Cancelling an
/// individual untitled dialog does NOT abort the close (matches upstream
/// Electron behavior — `Promise.all(...).then(close)`).
pub fn save_unsaved_and_close(app: AppHandle, window: WebviewWindow, unsaved: Value) {
    let files: Vec<UnsavedFile> = serde_json::from_value(unsaved).unwrap_or_default();
    process_save_queue(app, window, files.into());
}

fn process_save_queue(app: AppHandle, window: WebviewWindow, mut queue: VecDeque<UnsavedFile>) {
    // Write all path-backed files synchronously up to the next untitled one.
    while matches!(queue.front(), Some(f) if f.pathname.is_some()) {
        let f = queue.pop_front().unwrap();
        let file_path = ensure_extension(f.pathname.unwrap());
        write_and_react(&window, &f.id, &file_path, &f.markdown, &f.options, false);
    }
    let Some(f) = queue.pop_front() else {
        // Nothing left to save → close for real.
        crate::commands::window::mark_and_close(&app, &window);
        return;
    };
    // Untitled file → prompt for a path, write if chosen, then continue.
    let recommend = recommend_filename(&f.markdown, &f.filename);
    let dir = f.default_path.clone().unwrap_or_else(|| documents_dir(&app));
    let default = format!("{dir}/{recommend}.md");
    let app_next = app.clone();
    let window_next = window.clone();
    save_dialog(&app, &default, move |chosen| {
        if let Some(file_path) = chosen {
            let file_path = ensure_extension(file_path);
            write_and_react(&window_next, &f.id, &file_path, &f.markdown, &f.options, true);
        }
        process_save_queue(app_next, window_next, queue);
    });
}

// ---- save-and-close tabs (closing an unsaved/edited tab) --------------------

const SAVE_LABEL: &str = "Save";
const DONT_SAVE_LABEL: &str = "Don't Save";

/// `mt::save-and-close-tabs` — closing tab(s) with unsaved changes. Shows the
/// Save / Don't Save / Cancel prompt, then closes the tab(s) via
/// `mt::force-close-tabs-by-id` (Save writes first; an untitled file whose save
/// dialog is cancelled stays open).
#[tauri::command]
pub fn save_and_close_tabs(app: AppHandle, window: WebviewWindow, unsaved_files: Value) {
    let count = unsaved_files.as_array().map(|a| a.len()).unwrap_or(0);
    let files: Vec<UnsavedFile> = serde_json::from_value(unsaved_files).unwrap_or_default();
    if files.is_empty() {
        return;
    }
    app.dialog()
        .message(format!(
            "You have {count} file(s) with unsaved changes. Do you want to save them before closing?"
        ))
        .title("Unsaved Changes")
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            SAVE_LABEL.into(),
            DONT_SAVE_LABEL.into(),
            "Cancel".into(),
        ))
        .show_with_result(move |res| {
            let save = matches!(&res, MessageDialogResult::Yes)
                || matches!(&res, MessageDialogResult::Custom(s) if s == SAVE_LABEL);
            let dont_save = matches!(&res, MessageDialogResult::No)
                || matches!(&res, MessageDialogResult::Custom(s) if s == DONT_SAVE_LABEL);
            if save {
                process_close_queue(app, window, files.into(), Vec::new());
            } else if dont_save {
                let ids: Vec<&str> = files.iter().map(|f| f.id.as_str()).collect();
                force_close_tabs(&window, &ids);
            }
            // Cancel / dismissed → keep the tab(s) open.
        });
}

/// `mt::save-tabs` — Save-All without closing (the "Save All" menu/command,
/// from `ASK_FOR_SAVE_ALL(false)`). Unlike save-and-close there is NO prompt:
/// path-backed files are written in place and untitled files prompt a save
/// dialog one at a time. The tabs stay open; `write_and_react` already emits
/// `mt::tab-saved` / `mt::set-pathname` so the renderer clears the dirty state.
#[tauri::command]
pub fn save_all_tabs(app: AppHandle, window: WebviewWindow, unsaved_files: Value) {
    let files: Vec<UnsavedFile> = serde_json::from_value(unsaved_files).unwrap_or_default();
    if files.is_empty() {
        return;
    }
    process_save_all_queue(app, window, files.into());
}

/// Save the queued files (path-backed in place, untitled via dialog) WITHOUT
/// closing anything. Mirrors process_save_queue but the terminal action is a
/// no-op instead of closing the window.
fn process_save_all_queue(app: AppHandle, window: WebviewWindow, mut queue: VecDeque<UnsavedFile>) {
    while matches!(queue.front(), Some(f) if f.pathname.is_some()) {
        let f = queue.pop_front().unwrap();
        let file_path = ensure_extension(f.pathname.unwrap());
        write_and_react(&window, &f.id, &file_path, &f.markdown, &f.options, false);
    }
    let Some(f) = queue.pop_front() else {
        return;
    };
    let recommend = recommend_filename(&f.markdown, &f.filename);
    let dir = f.default_path.clone().unwrap_or_else(|| documents_dir(&app));
    let default = format!("{dir}/{recommend}.md");
    let app_next = app.clone();
    let window_next = window.clone();
    save_dialog(&app, &default, move |chosen| {
        if let Some(file_path) = chosen {
            let file_path = ensure_extension(file_path);
            write_and_react(&window_next, &f.id, &file_path, &f.markdown, &f.options, true);
        }
        process_save_all_queue(app_next, window_next, queue);
    });
}

/// Save the queued files (path-backed in place, untitled via dialog) and close
/// the successfully-saved tabs. Mirrors process_save_queue but emits
/// force-close-tabs-by-id instead of closing the window.
fn process_close_queue(
    app: AppHandle,
    window: WebviewWindow,
    mut queue: VecDeque<UnsavedFile>,
    mut saved: Vec<String>,
) {
    while matches!(queue.front(), Some(f) if f.pathname.is_some()) {
        let f = queue.pop_front().unwrap();
        let file_path = ensure_extension(f.pathname.unwrap());
        write_and_react(&window, &f.id, &file_path, &f.markdown, &f.options, false);
        saved.push(f.id);
    }
    let Some(f) = queue.pop_front() else {
        let ids: Vec<&str> = saved.iter().map(String::as_str).collect();
        force_close_tabs(&window, &ids);
        return;
    };
    let recommend = recommend_filename(&f.markdown, &f.filename);
    let dir = f.default_path.clone().unwrap_or_else(|| documents_dir(&app));
    let default = format!("{dir}/{recommend}.md");
    let app_next = app.clone();
    let window_next = window.clone();
    save_dialog(&app, &default, move |chosen| {
        if let Some(file_path) = chosen {
            let file_path = ensure_extension(file_path);
            write_and_react(&window_next, &f.id, &file_path, &f.markdown, &f.options, true);
            saved.push(f.id);
        }
        process_close_queue(app_next, window_next, queue, saved);
    });
}

/// Emit `mt::force-close-tabs-by-id`. The id list is wrapped in an extra array
/// so the shim (which spreads a top-level array) delivers it as the single
/// `tabIdList` argument the renderer expects.
fn force_close_tabs(window: &WebviewWindow, ids: &[&str]) {
    let _ = window.emit_to(window.label(), "mt::force-close-tabs-by-id", json!([ids]));
}
