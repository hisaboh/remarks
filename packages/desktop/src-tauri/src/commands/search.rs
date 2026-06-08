//! Sidebar / quick-open search — port of the Electron `ipc/ripgrep.ts` bridge.
//! Spawns `rg --json`, streams the results, and emits the `mt::rg::*` events the
//! renderer's `window.ripgrep` API (node/ripgrepSearcher.ts) consumes:
//!   mt::rg::match {searchId, payload}  · mt::rg::progress {searchId, num}
//!   mt::rg::done  {searchId}           · mt::rg::error {searchId, error}
//!   mt::rg::cancelled {searchId}
//!
//! `rg` is resolved from MARKTEXT_RIPGREP_PATH or PATH (bundling the binary for
//! packaged builds is a Phase 6 TODO).

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

struct Active {
    child: Child,
    cancelled: Arc<AtomicBool>,
}

/// In-flight searches keyed by searchId (for cancellation).
#[derive(Default)]
pub struct SearchState {
    active: Mutex<HashMap<String, Active>>,
}

fn rg_path() -> String {
    if let Ok(p) = std::env::var("MARKTEXT_RIPGREP_PATH") {
        if !p.is_empty() {
            return p;
        }
    }
    // Dev: the @vscode/ripgrep binary bundled in the workspace node_modules.
    // (Packaged builds should ship it via env/externalBin — Phase 6 TODO.)
    if let Some(p) = find_bundled_rg() {
        return p;
    }
    "rg".to_string()
}

/// Walk up from the executable to the workspace `node_modules/.pnpm` and find the
/// platform `@vscode/ripgrep` binary. Dev-only convenience.
fn find_bundled_rg() -> Option<String> {
    let arch = if cfg!(target_arch = "aarch64") {
        "darwin-arm64"
    } else {
        "darwin-x64"
    };
    let mut dir = std::env::current_exe().ok()?;
    while dir.pop() {
        let pnpm = dir.join("node_modules/.pnpm");
        let Ok(entries) = std::fs::read_dir(&pnpm) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("@vscode+ripgrep-{arch}@")) {
                let rg = entry
                    .path()
                    .join(format!("node_modules/@vscode/ripgrep-{arch}/bin/rg"));
                if rg.is_file() {
                    return Some(rg.to_string_lossy().into_owned());
                }
            }
        }
    }
    None
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SearchOptions {
    #[serde(default)]
    is_regexp: bool,
    #[serde(default)]
    is_case_sensitive: bool,
    #[serde(default)]
    is_whole_word: bool,
    #[serde(default)]
    follow_symlinks: bool,
    #[serde(default)]
    max_file_size: Option<Value>,
    #[serde(default)]
    include_hidden: bool,
    #[serde(default)]
    no_ignore: bool,
    #[serde(default)]
    leading_context_line_count: Option<u32>,
    #[serde(default)]
    trailing_context_line_count: Option<u32>,
    #[serde(default)]
    inclusions: Vec<String>,
    #[serde(default)]
    exclusions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RgRequest {
    search_id: String,
    mode: String,
    directories: Vec<String>,
    pattern: String,
    #[serde(default)]
    options: SearchOptions,
}

/// Build the common rg flags shared by text and files mode.
fn common_flags(args: &mut Vec<String>, o: &SearchOptions) {
    if o.follow_symlinks {
        args.push("--follow".into());
    }
    if o.include_hidden {
        args.push("--hidden".into());
    }
    if o.no_ignore {
        args.push("--no-ignore".into());
    }
    for inc in &o.inclusions {
        args.push("--iglob".into());
        args.push(inc.clone());
    }
    for exc in &o.exclusions {
        args.push("--iglob".into());
        args.push(format!("!{exc}"));
    }
}

#[tauri::command]
pub fn rg_start(app: AppHandle, window: WebviewWindow, req: RgRequest) {
    let label = window.label().to_string();
    let mut args: Vec<String> = Vec::new();
    let files_mode = req.mode == "files";

    if files_mode {
        args.push("--files".into());
        common_flags(&mut args, &req.options);
    } else {
        args.push("--json".into());
        let o = &req.options;
        let mut regexp: Option<String> = None;
        if o.is_regexp {
            // prepareRegexp: unescape \/ ; map "--" to literal.
            let r = if req.pattern == "--" {
                "\\-\\-".to_string()
            } else {
                req.pattern.replace("\\/", "/")
            };
            if r.contains("\\n") {
                args.push("--multiline".into());
            }
            args.push("--regexp".into());
            args.push(r.clone());
            regexp = Some(r);
        } else {
            args.push("--fixed-strings".into());
        }
        if o.is_case_sensitive {
            args.push("--case-sensitive".into());
        } else {
            args.push("--ignore-case".into());
        }
        if o.is_whole_word {
            args.push("--word-regexp".into());
        }
        if let Some(mfs) = &o.max_file_size {
            let v = mfs.as_str().map(String::from).or_else(|| mfs.as_u64().map(|n| n.to_string()));
            if let Some(v) = v {
                args.push("--max-filesize".into());
                args.push(v);
            }
        }
        if let Some(n) = o.leading_context_line_count {
            args.push("--before-context".into());
            args.push(n.to_string());
        }
        if let Some(n) = o.trailing_context_line_count {
            args.push("--after-context".into());
            args.push(n.to_string());
        }
        common_flags(&mut args, o);
        let _ = regexp; // already pushed
    }
    args.push("--".into());
    if !files_mode && !req.options.is_regexp {
        args.push(req.pattern.clone());
    }
    for dir in &req.directories {
        args.push(dir.clone());
    }

    let child = Command::new(rg_path())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            let _ = app.emit_to(
                &label,
                "mt::rg::error",
                json!({ "searchId": req.search_id, "error": e.to_string() }),
            );
            return;
        }
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = app.emit_to(
            &label,
            "mt::rg::error",
            json!({ "searchId": req.search_id, "error": "no stdout" }),
        );
        return;
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    app.state::<SearchState>().active.lock().unwrap().insert(
        req.search_id.clone(),
        Active {
            child,
            cancelled: cancelled.clone(),
        },
    );

    let search_id = req.search_id;
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut pending_paths: u64 = 0;
        // text mode: accumulate matches per file (begin → match* → end).
        let mut current: Option<(String, Vec<Value>)> = None;
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if line.is_empty() {
                continue;
            }
            if files_mode {
                // Each stdout line is a file path.
                pending_paths += 1;
                let _ = app.emit_to(&label, "mt::rg::progress", json!({ "searchId": search_id, "num": pending_paths }));
                let _ = app.emit_to(&label, "mt::rg::match", json!({ "searchId": search_id, "payload": line }));
                continue;
            }
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            match msg.get("type").and_then(Value::as_str) {
                Some("begin") => {
                    current = Some((text_field(&msg["data"]["path"]), Vec::new()));
                }
                Some("match") => {
                    if let Some((_, matches)) = current.as_mut() {
                        let data = &msg["data"];
                        let line_text = text_field(&data["lines"]);
                        let ln = data["line_number"].as_u64().unwrap_or(1) as usize;
                        let base = ln.saturating_sub(1);
                        if let Some(subs) = data["submatches"].as_array() {
                            for sub in subs {
                                let start = sub["start"].as_u64().unwrap_or(0) as usize;
                                let end = sub["end"].as_u64().unwrap_or(0) as usize;
                                let (r0, c0) = byte_to_pos(&line_text, start);
                                let (r1, c1) = byte_to_pos(&line_text, end);
                                matches.push(json!({
                                    "matchText": text_field(&sub["match"]),
                                    "lineText": trim_newline(&line_text),
                                    "range": [[base + r0, c0], [base + r1, c1]],
                                    "leadingContextLines": [],
                                    "trailingContextLines": [],
                                }));
                            }
                        }
                    }
                }
                Some("end") => {
                    if let Some((file_path, matches)) = current.take() {
                        pending_paths += 1;
                        let _ = app.emit_to(&label, "mt::rg::progress", json!({ "searchId": search_id, "num": pending_paths }));
                        let _ = app.emit_to(&label, "mt::rg::match", json!({ "searchId": search_id, "payload": { "filePath": file_path, "matches": matches } }));
                    }
                }
                _ => {}
            }
        }
        // EOF: natural finish unless cancelled (cancel emits its own event).
        let still_active = app
            .state::<SearchState>()
            .active
            .lock()
            .unwrap()
            .remove(&search_id)
            .is_some();
        if still_active && !cancelled.load(Ordering::SeqCst) {
            let _ = app.emit_to(&label, "mt::rg::done", json!({ "searchId": search_id }));
        }
    });
}

#[tauri::command]
pub fn rg_cancel(app: AppHandle, window: WebviewWindow, search_id: String) {
    let removed = app.state::<SearchState>().active.lock().unwrap().remove(&search_id);
    if let Some(mut active) = removed {
        active.cancelled.store(true, Ordering::SeqCst);
        let _ = active.child.kill();
        let _ = app.emit_to(window.label(), "mt::rg::cancelled", json!({ "searchId": search_id }));
    }
}

/// ripgrep --json text field: `{text}` or base64 `{bytes}` (best effort: text).
fn text_field(v: &Value) -> String {
    v.get("text").and_then(Value::as_str).unwrap_or_default().to_string()
}

fn trim_newline(s: &str) -> &str {
    s.strip_suffix('\n').unwrap_or(s)
}

/// Byte offset within `text` → (row offset, char column). Handles multi-line
/// match blocks (submatch.end past the first line) and Unicode.
fn byte_to_pos(text: &str, byte: usize) -> (usize, usize) {
    let prefix = &text[..byte.min(text.len())];
    let row = prefix.matches('\n').count();
    let col = match prefix.rfind('\n') {
        Some(i) => prefix[i + 1..].chars().count(),
        None => prefix.chars().count(),
    };
    (row, col)
}
