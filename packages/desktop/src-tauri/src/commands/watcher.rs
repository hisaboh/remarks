//! File watcher (4f) — Phase 4 port of the Electron `filesystem/watcher.ts`
//! `'file'` path. Detects EXTERNAL changes to open markdown files and emits
//! `mt::update-file` so the renderer can prompt a reload (or auto-reload when
//! autosave is on). Uses `notify-debouncer-full`, which reports distinct event
//! kinds (Create/Modify/Remove) — resolving the prior spike's limitation that
//! debouncer-mini couldn't tell add/modify/remove apart.
//!
//! Self-saves are suppressed: `file_save`'s `write_and_react` calls
//! `ignore_change` before writing, mirroring Electron's `ignoreChangedEvent`,
//! so MarkText's own writes don't trigger a spurious "file changed on disk".
//!
//! Scope: open-file watching only. The directory/sidebar tree watch
//! (`mt::update-object-tree`) is deferred with the unported open-folder flow.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};

/// Directory names skipped while scanning/watching a project folder.
const SKIP_DIRS: &[&str] = &["node_modules", ".git"];

/// How long a self-save suppresses change events for a path. Generous to cover
/// the debounce window plus filesystem/cloud settling (cf. Electron's 1.3s).
const IGNORE_DURATION: Duration = Duration::from_millis(2000);
/// Debounce window for coalescing rapid native events.
const DEBOUNCE: Duration = Duration::from_millis(500);
/// Delay between emitting the opened folder's top level and starting the
/// recursive watch + deep scan, so the direct children render first (#12).
/// Generous enough that the heavy deep-scan event flood doesn't land in the
/// middle of the renderer's editor initialization at startup (which would push
/// back when the user can start typing); the "rest" of the tree filling in a
/// bit later is the accepted trade-off.
const DEEP_SCAN_DELAY: Duration = Duration::from_millis(400);

type FullDebouncer = Debouncer<RecommendedWatcher, RecommendedCache>;

#[derive(Default)]
pub struct WatcherState {
    debouncer: Mutex<Option<FullDebouncer>>,
    watched: Mutex<HashSet<String>>,
    ignore: Mutex<Vec<(String, Instant)>>,
    /// The sidebar project: (root path, owning window label). Events under the
    /// root become `mt::update-object-tree` (sidebar tree); others stay
    /// `mt::update-file` (open files). One project at a time (last opened).
    project: Mutex<Option<(String, String)>>,
}

/// Create the shared debouncer (call once in setup, after the app handle exists).
pub fn init(app: &AppHandle) {
    let handler_app = app.clone();
    let debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
        let events = match result {
            Ok(events) => events,
            Err(errors) => {
                for e in errors {
                    log::error!("watcher error: {e}");
                }
                return;
            }
        };
        for event in events {
            let kind = match event.kind {
                EventKind::Create(_) => "add",
                EventKind::Modify(_) => "change",
                EventKind::Remove(_) => "unlink",
                _ => continue,
            };
            let Some(path) = event.paths.last() else { continue };
            let pathname = path.to_string_lossy().into_owned();
            // Route paths under the open project folder to the sidebar tree;
            // everything else is an open-file watch.
            let project = handler_app
                .state::<WatcherState>()
                .project
                .lock()
                .unwrap()
                .clone();
            if let Some((root, label)) = project {
                if pathname.starts_with(&root) {
                    handle_tree_event(&handler_app, &label, kind, &pathname);
                    continue;
                }
            }
            if !crate::commands::files::is_markdown_path(&pathname) {
                continue;
            }
            handle_event(&handler_app, kind, &pathname);
        }
    });
    match debouncer {
        Ok(d) => *app.state::<WatcherState>().debouncer.lock().unwrap() = Some(d),
        Err(e) => log::error!("failed to create file watcher: {e}"),
    }
}

/// Start watching a file for external changes (deduped; no-op if already watched).
pub fn watch_file(app: &AppHandle, pathname: &str) {
    let state = app.state::<WatcherState>();
    if !state.watched.lock().unwrap().insert(pathname.to_string()) {
        return; // already watching
    }
    let mut guard = state.debouncer.lock().unwrap();
    if let Some(debouncer) = guard.as_mut() {
        if let Err(e) = debouncer.watch(Path::new(pathname), RecursiveMode::NonRecursive) {
            log::error!("watch {pathname} failed: {e}");
            state.watched.lock().unwrap().remove(pathname);
        }
    }
}

/// Suppress the next change event for a path (MarkText's own save).
pub fn ignore_change(app: &AppHandle, pathname: &str) {
    let state = app.state::<WatcherState>();
    state
        .ignore
        .lock()
        .unwrap()
        .push((pathname.to_string(), Instant::now()));
}

/// Whether a pending self-save should swallow this add/change event.
fn take_ignored(app: &AppHandle, pathname: &str) -> bool {
    let state = app.state::<WatcherState>();
    let mut ignore = state.ignore.lock().unwrap();
    let now = Instant::now();
    ignore.retain(|(_, at)| now.duration_since(*at) < IGNORE_DURATION);
    if let Some(pos) = ignore.iter().position(|(p, _)| p == pathname) {
        ignore.remove(pos);
        true
    } else {
        false
    }
}

fn mtime_ms(pathname: &str) -> Option<f64> {
    let modified = std::fs::metadata(pathname).ok()?.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_secs_f64() * 1000.0)
}

fn handle_event(app: &AppHandle, kind: &str, pathname: &str) {
    if kind == "unlink" {
        let _ = app.emit(
            "mt::update-file",
            json!({ "type": "unlink", "change": { "pathname": pathname } }),
        );
        return;
    }
    // add / change: ignore our own saves, then ship the reloaded document.
    if take_ignored(app, pathname) {
        return;
    }
    let data = match crate::commands::files::build_document_json(app, pathname) {
        Ok(data) => data,
        Err(e) => {
            log::error!("watcher reload {pathname} failed: {e}");
            return;
        }
    };
    let change = json!({
        "pathname": pathname,
        "data": data,
        "mtimeMs": mtime_ms(pathname),
    });
    let _ = app.emit("mt::update-file", json!({ "type": kind, "change": change }));
}

// ---- sidebar project folder (open-folder + directory tree watch) ------------

/// `mt::ask-for-open-project-in-sidebar` / `mt::cmd-open-folder` — pick a folder
/// (non-blocking) then load it as the sidebar project: emit `mt::open-directory`
/// (sets the root) and start watching it (the initial scan populates the tree).
#[tauri::command]
pub fn project_open(app: AppHandle, window: WebviewWindow) {
    let app_cb = app.clone();
    app.dialog().file().pick_folder(move |folder| {
        let Some(path) = folder.and_then(|fp| fp.into_path().ok()) else {
            return; // cancelled
        };
        load_project(&app_cb, &window, &path.to_string_lossy());
    });
}

/// Open a known folder as the sidebar project (e.g. dropped onto the window).
#[tauri::command]
pub fn project_open_path(app: AppHandle, window: WebviewWindow, path: String) {
    load_project(&app, &window, &path);
}

/// Set `root` as the sidebar project for `window`: emit `mt::open-directory`
/// (sets the tree root), start the recursive watch, and scan to populate.
fn load_project(app: &AppHandle, window: &WebviewWindow, root: &str) {
    let label = window.label().to_string();
    *app.state::<WatcherState>().project.lock().unwrap() = Some((root.to_string(), label.clone()));
    let _ = window.emit_to(&label, "mt::open-directory", root);
    // Populate the sidebar off the main thread (synchronous Tauri commands run
    // on the WKWebView UI thread; a large project's recursive stat/readdir walk
    // would freeze it — the beachball in #12). Two phases so the opened folder's
    // direct children appear immediately and the rest fills in afterwards:
    //   1. scan the root's top level and emit it right away;
    //   2. after a short delay, start the recursive watch (notify's file-id
    //      cache does a full recursive stat() walk on watch()) and scan the
    //      deeper subtree.
    // Doing the heavy watch + deep scan first (as before) left the sidebar empty
    // for seconds until the whole tree had been walked. The renderer still
    // applies the emitted tree events in time-sliced batches so absorbing the
    // deep-scan flood stays non-blocking.
    let app_bg = app.clone();
    let root_bg = root.to_string();
    std::thread::spawn(move || {
        // Phase 1 — opened folder's direct children, emitted immediately.
        let subdirs = scan_dir_level(&app_bg, &label, std::path::Path::new(&root_bg));
        // Phase 2 — recursive watch + deep scan, deferred so the top-level
        // listing renders first and doesn't compete with the event flood.
        std::thread::sleep(DEEP_SCAN_DELAY);
        watch_dir(&app_bg, &root_bg);
        let mut stack = subdirs;
        while let Some(dir) = stack.pop() {
            let mut children = scan_dir_level(&app_bg, &label, &dir);
            stack.append(&mut children);
        }
    });
}

/// Recursively watch a directory for changes (ongoing tree updates).
fn watch_dir(app: &AppHandle, root: &str) {
    let state = app.state::<WatcherState>();
    let mut guard = state.debouncer.lock().unwrap();
    if let Some(debouncer) = guard.as_mut() {
        if let Err(e) = debouncer.watch(std::path::Path::new(root), RecursiveMode::Recursive) {
            log::error!("watch dir {root} failed: {e}");
        }
    }
}

/// Scan a single directory level: emit addDir/add tree events for its entries
/// and return its sub-directories so the caller can descend. notify, unlike
/// chokidar, doesn't replay existing entries, hence the explicit scan.
fn scan_dir_level(app: &AppHandle, label: &str, dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut subdirs = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return subdirs };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if ft.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            emit_tree(app, label, "addDir", json!({ "pathname": path.to_string_lossy() }));
            subdirs.push(path);
        } else if ft.is_file() {
            let p = path.to_string_lossy().into_owned();
            if crate::commands::files::is_markdown_path(&p) {
                emit_tree(app, label, "add", file_change(&p, &name));
            }
        }
    }
    subdirs
}

/// Map an ongoing watch event under the project root to a tree update.
fn handle_tree_event(app: &AppHandle, label: &str, kind: &str, pathname: &str) {
    let path = std::path::Path::new(pathname);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if SKIP_DIRS.contains(&name.as_str()) {
        return;
    }
    match kind {
        "add" => match std::fs::metadata(pathname) {
            // Create: distinguish dir vs markdown file by stat.
            Ok(meta) if meta.is_dir() => {
                emit_tree(app, label, "addDir", json!({ "pathname": pathname }))
            }
            Ok(_) if crate::commands::files::is_markdown_path(pathname) => {
                emit_tree(app, label, "add", file_change(pathname, &name))
            }
            _ => {}
        },
        "change" => {
            if crate::commands::files::is_markdown_path(pathname) {
                emit_tree(
                    app,
                    label,
                    "change",
                    json!({ "pathname": pathname, "mtimeMs": mtime_ms(pathname) }),
                );
            }
        }
        "unlink" => {
            // The path is gone; can't stat. Markdown extension ⇒ file, else dir.
            // The renderer no-ops if the kind doesn't match a tree node.
            let tree_kind = if crate::commands::files::is_markdown_path(pathname) {
                "unlink"
            } else {
                "unlinkDir"
            };
            emit_tree(app, label, tree_kind, json!({ "pathname": pathname }));
        }
        _ => {}
    }
}

fn emit_tree(app: &AppHandle, label: &str, kind: &str, change: serde_json::Value) {
    let _ = app.emit_to(
        label,
        "mt::update-object-tree",
        json!({ "type": kind, "change": change }),
    );
}

/// Tree-node metadata for an `add` event (no file content — addFile ignores it).
fn file_change(pathname: &str, name: &str) -> serde_json::Value {
    let meta = std::fs::metadata(pathname).ok();
    let birth = meta
        .as_ref()
        .and_then(|m| m.created().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0);
    json!({
        "pathname": pathname,
        "name": name,
        "isFile": true,
        "isDirectory": false,
        "isMarkdown": true,
        "birthTime": birth,
        "mtimeMs": mtime_ms(pathname),
    })
}
