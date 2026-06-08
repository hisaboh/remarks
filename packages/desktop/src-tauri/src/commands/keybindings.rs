//! User keybindings persistence (4d) — the renderer-side dispatcher (4-8) owns
//! the platform default maps and the actual key matching; this only loads and
//! saves the user overrides in `keybindings.json` (commandId → accelerator),
//! mirroring Electron's `keyboard/shortcutHandler.ts` configPath.
//!
//! The settings keybinding editor talks to these via the shim:
//!   mt::keybinding-get-pref-keybindings  → keybindings_get_user (+ renderer defaults)
//!   mt::keybinding-save-user-keybindings → keybindings_save_user
//!   mt::keybinding-get-keyboard-info     → stub in the shim (en-US fallback)

use std::collections::BTreeMap;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// `<config-dir>/keybindings.json` (same basename as the Electron build).
fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("keybindings.json"))
}

/// Read the user keybinding overrides. Missing/invalid file → empty map.
#[tauri::command]
pub fn keybindings_get_user(app: AppHandle) -> BTreeMap<String, String> {
    let Some(path) = config_path(&app) else {
        return BTreeMap::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Persist the user keybinding overrides; returns whether the write succeeded.
#[tauri::command]
pub fn keybindings_save_user(app: AppHandle, bindings: BTreeMap<String, String>) -> bool {
    let Some(path) = config_path(&app) else {
        return false;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(&bindings) {
        Ok(json) => std::fs::write(&path, json).is_ok(),
        Err(e) => {
            log::error!("keybindings save failed: {e}");
            false
        }
    }
}
