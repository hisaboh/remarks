//! Multi-window management — Phase 4 port of windowManager / windows/{editor,
//! setting}.ts. Replaces the "main window only" limitation.
//!
//!   mt::cmd-new-editor-window / app-create-editor-window → window_create(editor)
//!   mt::open-setting-window / app-create-settings-window  → window_create(settings)
//!   (renderer bootstrap)                                  → window_init_args
//!
//! Electron passed per-window state via URL query args (wid/type/udp/theme…).
//! Tauri child windows can't easily carry a query string across the dev/prod
//! URL base, so instead each window asks `window_init_args` for its args (keyed
//! by window label) and the renderer applies them via history.replaceState.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde_json::Value;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_store::StoreExt;

use crate::commands::preferences::PREFERENCES_FILE;

#[derive(Clone)]
struct WinArgs {
    window_id: u32,
    win_type: String,
}

/// Label → args for windows created at runtime. The main window (label "main")
/// has no entry and falls back to the default editor args.
#[derive(Default)]
pub struct WindowRegistry {
    args: Mutex<HashMap<String, WinArgs>>,
}

// Main window is window id 1; runtime windows start at 2.
static NEXT_WINDOW_ID: AtomicU32 = AtomicU32::new(2);

fn pref_str<R: tauri::Runtime>(app: &AppHandle<R>, key: &str, default: &str) -> String {
    app.store(PREFERENCES_FILE)
        .ok()
        .and_then(|s| s.get(key))
        .and_then(|v| match v {
            Value::String(s) => Some(s),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(if b { "1".into() } else { "0".into() }),
            _ => None,
        })
        .unwrap_or_else(|| default.to_string())
}

fn pref_flag<R: tauri::Runtime>(app: &AppHandle<R>, key: &str, default: bool) -> String {
    let v = app
        .store(PREFERENCES_FILE)
        .ok()
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(default);
    if v { "1".into() } else { "0".into() }
}

fn build_params(app: &AppHandle, window_id: u32, win_type: &str) -> HashMap<String, String> {
    let udp = app
        .path()
        .app_data_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    HashMap::from([
        ("udp".into(), udp),
        ("debug".into(), "0".into()),
        ("wid".into(), window_id.to_string()),
        ("type".into(), win_type.to_string()),
        ("cff".into(), pref_str(app, "codeFontFamily", "DejaVu Sans Mono")),
        ("cfs".into(), pref_str(app, "codeFontSize", "14")),
        ("hsb".into(), pref_flag(app, "hideScrollbar", false)),
        ("theme".into(), pref_str(app, "theme", "light")),
        ("tbs".into(), pref_str(app, "titleBarStyle", "custom")),
    ])
}

/// Returns the URL-arg map for the calling window (looked up by its label).
#[tauri::command]
pub fn window_init_args(app: AppHandle, window: WebviewWindow) -> HashMap<String, String> {
    let label = window.label().to_string();
    let registry = app.state::<WindowRegistry>();
    let (window_id, win_type) = {
        let map = registry.args.lock().unwrap();
        match map.get(&label) {
            Some(a) => (a.window_id, a.win_type.clone()),
            None => (1, "editor".to_string()), // main window
        }
    };
    build_params(&app, window_id, &win_type)
}

/// Create (or focus) a window. `kind` is "editor" or "settings"; `category`
/// optionally deep-links a settings tab (type becomes "settings/<category>").
#[tauri::command]
pub fn window_create(
    app: AppHandle,
    kind: String,
    category: Option<String>,
) -> Result<(), String> {
    // Settings is a singleton — focus the existing one.
    if kind == "settings" {
        if let Some(existing) = app.get_webview_window("settings") {
            let _ = existing.set_focus();
            return Ok(());
        }
    }

    let id = NEXT_WINDOW_ID.fetch_add(1, Ordering::SeqCst);
    let (label, win_type, title, width, height) = if kind == "settings" {
        let win_type = match &category {
            Some(c) => format!("settings/{c}"),
            None => "settings".to_string(),
        };
        ("settings".to_string(), win_type, "Preferences".to_string(), 980.0, 800.0)
    } else {
        (format!("editor-{id}"), "editor".to_string(), "MarkText".to_string(), 1200.0, 800.0)
    };

    app.state::<WindowRegistry>()
        .args
        .lock()
        .unwrap()
        .insert(label.clone(), WinArgs { window_id: id, win_type });

    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(width, height)
        .min_inner_size(600.0, 400.0)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}
