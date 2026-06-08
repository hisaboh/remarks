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

use std::path::Path;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
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
                    Ok(doc) => emit_open_tab(&window, doc, json!({}), index == 0),
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
    let bytes = encode_markdown(markdown, options);
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
            // TODO(phase-4): window-add-file-path / recently-used / watcher hooks.
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
    let recommend = if filename.is_empty() { "Untitled".to_string() } else { filename };
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
    let recommend = if filename.is_empty() { "Untitled".to_string() } else { filename };
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
