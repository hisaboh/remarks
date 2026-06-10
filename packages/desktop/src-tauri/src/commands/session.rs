//! Session buffer persistence — port of `main/editorBufferStore/index.ts`.
//!
//! The renderer debounces a snapshot of its open tabs (with content, incl.
//! unsaved), the project root and layout, and sends it via `update-buffer-state`.
//! We persist one file per window label under `<app_config_dir>/editor-buffer/`.
//! On startup the main window's buffer is read back by `editor_bootstrap_config`
//! and replayed to the renderer as `mt::load-state` (see the bootstrap shim),
//! restoring the previous session when `startUpAction == "restoreAll"`.

use std::path::PathBuf;

use serde_json::Value;
use tauri::{AppHandle, Manager, WebviewWindow};

const SESSION_SUBDIR: &str = "editor-buffer";

fn session_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_config_dir().ok()?.join(SESSION_SUBDIR);
    if !dir.exists() {
        std::fs::create_dir_all(&dir).ok()?;
    }
    Some(dir)
}

fn buffer_path(app: &AppHandle, label: &str) -> Option<PathBuf> {
    Some(session_dir(app)?.join(format!("{label}_buffer.json")))
}

/// Persist a window's buffered editor state (the debounced `update-buffer-state`
/// from the renderer). Written atomically via a temp file + rename, mirroring
/// EditorBufferStore.writeBufferStoreFile.
#[tauri::command]
pub fn update_buffer_state(
    app: AppHandle,
    window: WebviewWindow,
    state: Value,
) -> Result<bool, String> {
    let Some(path) = buffer_path(&app, window.label()) else {
        return Ok(false);
    };
    let data = serde_json::to_vec(&state).map_err(|e| e.to_string())?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, &data).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Read a window's persisted buffered state, but only if it has at least one tab
/// (an empty/absent buffer means "nothing to restore" → fall back to a blank
/// tab in the bootstrap config).
pub fn read_buffer_state(app: &AppHandle, label: &str) -> Option<Value> {
    let path = buffer_path(app, label)?;
    let content = std::fs::read_to_string(&path).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    let has_tabs = value
        .get("tabs")
        .and_then(Value::as_array)
        .is_some_and(|tabs| !tabs.is_empty());
    has_tabs.then_some(value)
}
