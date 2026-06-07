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
use tauri::{AppHandle, Emitter};
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

fn emit_open_tab(app: &AppHandle, doc: MarkdownDocument, options: Value, selected: bool) {
    let _ = app.emit("mt::open-new-tab", json!([doc, options, selected]));
}

/// Open a known path (sidebar click, search result). `options` is forwarded to
/// the renderer untouched (cursor/selection hints etc.).
#[tauri::command]
pub fn file_open_path(
    app: AppHandle,
    pathname: String,
    options: Option<Value>,
) -> Result<(), String> {
    let store = app.store(PREFERENCES_FILE).map_err(to_err)?;
    let doc = build_document(&store, &pathname)?;
    emit_open_tab(&app, doc, options.unwrap_or_else(|| json!({})), true);
    Ok(())
}

/// Prompt for one or more markdown files, then open each in a tab.
#[tauri::command]
pub fn file_open(app: AppHandle) -> Result<(), String> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Markdown", MARKDOWN_EXTENSIONS)
        .add_filter("All Files", &["*"])
        .blocking_pick_files();

    let Some(files) = picked else {
        return Ok(()); // cancelled
    };

    let store = app.store(PREFERENCES_FILE).map_err(to_err)?;
    for (index, file) in files.into_iter().enumerate() {
        let Ok(path) = file.into_path() else { continue };
        let pathname = path.to_string_lossy().into_owned();
        match build_document(&store, &pathname) {
            // Only the first opened file becomes the active tab.
            Ok(doc) => emit_open_tab(&app, doc, json!({}), index == 0),
            Err(e) => log::error!("failed to open {pathname}: {e}"),
        }
    }
    Ok(())
}
