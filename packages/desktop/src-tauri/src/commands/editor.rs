//! Editor bootstrap — Phase 4 replacement for the Electron main process
//! pushing `mt::bootstrap-editor` from `windows/editor.ts`.
//!
//! The renderer's editor store (store/editor.ts) initializes only when it
//! receives a bootstrap config. Electron built that config on the main side
//! from preferences and the files to open; this command rebuilds it from the
//! ported preferences store. The renderer shim invokes this when the editor's
//! `mt::bootstrap-editor` listener attaches, then emits the event with the
//! result — an event-driven handshake (no fixed timing).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::{Store, StoreExt};

use crate::commands::preferences::PREFERENCES_FILE;

/// Files to open on launch (4e): CLI argv (`marktext file.md`) plus macOS
/// file-association "Open With" (RunEvent::Opened). Drained by the first
/// editor bootstrap; once bootstrapped, later Opened events open directly in
/// the focused window. Full session-buffer restore is a separate (deferred) task.
#[derive(Default)]
pub struct PendingOpen {
    files: Mutex<Vec<String>>,
    bootstrapped: AtomicBool,
}

impl PendingOpen {
    /// Seed from the process argv, keeping only paths that exist as files.
    pub fn from_args() -> Self {
        let files = std::env::args()
            .skip(1)
            .filter(|arg| Path::new(arg).is_file())
            .collect();
        Self {
            files: Mutex::new(files),
            bootstrapped: AtomicBool::new(false),
        }
    }

    pub fn push(&self, paths: impl IntoIterator<Item = String>) {
        self.files.lock().unwrap().extend(paths);
    }

    pub fn is_bootstrapped(&self) -> bool {
        self.bootstrapped.load(Ordering::SeqCst)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapConfig {
    add_blank_tab: bool,
    markdown_list: Vec<String>,
    /// File paths to open after init (the shim sends `mt::open-file` for each).
    files_to_open: Vec<String>,
    line_ending: String,
    side_bar_visibility: bool,
    tab_bar_visibility: bool,
    source_code_mode_enabled: bool,
}

fn pref_bool<R: tauri::Runtime>(store: &Store<R>, key: &str, default: bool) -> bool {
    store
        .get(key)
        .as_ref()
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

#[tauri::command]
pub fn editor_bootstrap_config(app: AppHandle) -> Result<BootstrapConfig, String> {
    let store = app.store(PREFERENCES_FILE).map_err(|e| e.to_string())?;

    // Mirror Preference.getPreferredEol(): explicit lf/crlf wins, otherwise the
    // platform default (CRLF on Windows, LF elsewhere).
    let end_of_line = store
        .get("endOfLine")
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();
    let line_ending = match end_of_line.as_str() {
        "lf" => "lf",
        "crlf" => "crlf",
        _ if cfg!(windows) => "crlf",
        _ => "lf",
    }
    .to_string();

    // Launch files (4e) go to the FIRST window to bootstrap; later windows
    // (and re-bootstraps) start blank. Drained once.
    let pending = app.state::<PendingOpen>();
    let files_to_open = if pending.bootstrapped.swap(true, Ordering::SeqCst) {
        vec![]
    } else {
        std::mem::take(&mut *pending.files.lock().unwrap())
    };

    Ok(BootstrapConfig {
        // No launch files → start with one blank tab; otherwise open them.
        add_blank_tab: files_to_open.is_empty(),
        markdown_list: vec![],
        files_to_open,
        line_ending,
        side_bar_visibility: pref_bool(&store, "sideBarVisibility", false),
        tab_bar_visibility: pref_bool(&store, "tabBarVisibility", false),
        source_code_mode_enabled: pref_bool(&store, "sourceCodeModeEnabled", false),
    })
}

/// Handle macOS file-association / "Open With" (RunEvent::Opened). If the editor
/// has already bootstrapped, open the files in the focused window now; otherwise
/// queue them for the first bootstrap to pick up.
#[cfg(target_os = "macos")]
pub fn handle_opened(app: &AppHandle, urls: Vec<tauri::Url>) {
    let paths: Vec<String> = urls
        .iter()
        .filter_map(|u| u.to_file_path().ok())
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if paths.is_empty() {
        return;
    }
    let pending = app.state::<PendingOpen>();
    if !pending.is_bootstrapped() {
        pending.push(paths);
        return;
    }
    // App already running → open in the focused (else any) window now.
    let window = app
        .webview_windows()
        .into_values()
        .find(|w| w.is_focused().unwrap_or(false))
        .or_else(|| app.webview_windows().into_values().next());
    if let Some(window) = window {
        let mut selected = true;
        for path in paths {
            crate::commands::files::open_path_in_window(app, &window, &path, selected);
            selected = false;
        }
    }
}
