//! Editor bootstrap — Phase 4 replacement for the Electron main process
//! pushing `mt::bootstrap-editor` from `windows/editor.ts`.
//!
//! The renderer's editor store (store/editor.ts) initializes only when it
//! receives a bootstrap config. Electron built that config on the main side
//! from preferences and the files to open; this command rebuilds it from the
//! ported preferences store. The renderer shim invokes this when the editor's
//! `mt::bootstrap-editor` listener attaches, then emits the event with the
//! result — an event-driven handshake (no fixed timing).

use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_store::{Store, StoreExt};

use crate::commands::preferences::PREFERENCES_FILE;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapConfig {
    add_blank_tab: bool,
    markdown_list: Vec<String>,
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

    Ok(BootstrapConfig {
        // Fresh launch with no files to open → start with one blank tab.
        // TODO(phase-4): markdown_list from CLI file args / restored session.
        add_blank_tab: true,
        markdown_list: vec![],
        line_ending,
        side_bar_visibility: pref_bool(&store, "sideBarVisibility", false),
        tab_bar_visibility: pref_bool(&store, "tabBarVisibility", false),
        source_code_mode_enabled: pref_bool(&store, "sourceCodeModeEnabled", false),
    })
}
