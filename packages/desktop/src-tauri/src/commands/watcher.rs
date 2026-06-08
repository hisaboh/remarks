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
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};

/// How long a self-save suppresses change events for a path. Generous to cover
/// the debounce window plus filesystem/cloud settling (cf. Electron's 1.3s).
const IGNORE_DURATION: Duration = Duration::from_millis(2000);
/// Debounce window for coalescing rapid native events.
const DEBOUNCE: Duration = Duration::from_millis(500);

type FullDebouncer = Debouncer<RecommendedWatcher, RecommendedCache>;

#[derive(Default)]
pub struct WatcherState {
    debouncer: Mutex<Option<FullDebouncer>>,
    watched: Mutex<HashSet<String>>,
    ignore: Mutex<Vec<(String, Instant)>>,
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
