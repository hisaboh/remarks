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

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, TitleBarStyle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogResult};
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
    /// Labels confirmed to close — the CloseRequested handler lets these through
    /// instead of re-running the unsaved-changes prompt.
    closing: Mutex<HashSet<String>>,
}

impl WindowRegistry {
    pub fn mark_closing(&self, label: &str) {
        self.closing.lock().unwrap().insert(label.to_string());
    }

    /// Returns true (and clears the flag) if `label` was marked for closing.
    pub fn take_closing(&self, label: &str) -> bool {
        self.closing.lock().unwrap().remove(label)
    }
}

// Main window is window id 1; runtime windows start at 2.
static NEXT_WINDOW_ID: AtomicU32 = AtomicU32::new(2);

/// Diagonal cascade step (logical px) for stacked new editor windows (4h).
const CASCADE_OFFSET: f64 = 30.0;

/// Cascade a new editor window off the currently focused window so they don't
/// stack exactly on top of each other. Returns a logical (x, y), wrapping back
/// to the monitor's top-left margin when the step would run off-screen. None
/// when there's no reference window (let the OS place the first one).
fn cascade_position(app: &AppHandle) -> Option<(f64, f64)> {
    let reference = app
        .webview_windows()
        .into_values()
        .find(|w| w.is_focused().unwrap_or(false))
        .or_else(|| app.webview_windows().into_values().next())?;

    let scale = reference.scale_factor().ok()?;
    let pos = reference.outer_position().ok()?.to_logical::<f64>(scale);
    let mut x = pos.x + CASCADE_OFFSET;
    let mut y = pos.y + CASCADE_OFFSET;

    // Wrap to the monitor's top-left margin if the next step would push the
    // title bar off the work area.
    if let Ok(Some(monitor)) = reference.current_monitor() {
        let m_pos = monitor.position().to_logical::<f64>(scale);
        let m_size = monitor.size().to_logical::<f64>(scale);
        if x + CASCADE_OFFSET > m_pos.x + m_size.width
            || y + CASCADE_OFFSET > m_pos.y + m_size.height
        {
            x = m_pos.x + CASCADE_OFFSET;
            y = m_pos.y + CASCADE_OFFSET;
        }
    }
    Some((x, y))
}

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
    create_or_focus(&app, &kind, category)
}

/// Open (or focus) the settings window — used by the native menu.
pub fn open_settings(app: &AppHandle) {
    if let Err(e) = create_or_focus(app, "settings", None) {
        log::error!("failed to open settings window: {e}");
    }
}

fn create_or_focus(app: &AppHandle, kind: &str, category: Option<String>) -> Result<(), String> {
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

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(width, height)
        .min_inner_size(600.0, 400.0)
        // Frameless-overlay like the Electron build: traffic lights float over a
        // content area that fills the window; the renderer draws its own title bar.
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true);
    // Cascade editor windows; settings is a singleton, leave it centered (4h).
    if kind != "settings" {
        if let Some((x, y)) = cascade_position(app) {
            builder = builder.position(x, y);
        }
    }
    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}

// ---- close flow -------------------------------------------------------------
//
// Window close (X button or the Close-Window command) is intercepted in
// lib.rs's CloseRequested handler, which emits `mt::ask-for-close`. The
// renderer replies with `mt::close-window` (nothing unsaved) or
// `mt::close-window-confirm` (unsaved files → confirm dialog).

/// Start the close flow for the current window (Close-Window command).
#[tauri::command]
pub fn window_request_close(window: WebviewWindow) -> Result<(), String> {
    window
        .emit_to(window.label(), "mt::ask-for-close", ())
        .map_err(|e| e.to_string())
}

/// `mt::window-toggle-always-on-top` — flip the window's always-on-top state
/// and reflect it on the Window-menu check item.
#[tauri::command]
pub fn window_toggle_always_on_top(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    let current = window.is_always_on_top().map_err(|e| e.to_string())?;
    window
        .set_always_on_top(!current)
        .map_err(|e| e.to_string())?;
    crate::menu::set_check(&app, "window.toggle-always-on-top", !current);
    Ok(())
}

/// Mark the window as closing (so the CloseRequested handler lets it through)
/// and close it for real.
pub fn mark_and_close(app: &AppHandle, window: &WebviewWindow) {
    app.state::<WindowRegistry>().mark_closing(window.label());
    if let Err(e) = window.close() {
        log::error!("close failed for {}: {e}", window.label());
    }
}

/// Renderer confirmed there's nothing to save — close for real.
#[tauri::command]
pub fn window_close(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    mark_and_close(&app, &window);
    Ok(())
}

const SAVE_LABEL: &str = "Save";
const DONT_SAVE_LABEL: &str = "Don't Save";

/// Renderer reported unsaved files — show the Save / Don't Save / Cancel prompt
/// (4c). Save writes each unsaved file (prompting a path for untitled ones) then
/// closes; Don't Save closes immediately; Cancel keeps the window open.
#[tauri::command]
pub fn window_close_confirm(app: AppHandle, window: WebviewWindow, unsaved_files: Value) {
    let count = unsaved_files.as_array().map(|a| a.len()).unwrap_or(0);
    // NON-blocking dialog (see file_open): sync commands run on the main thread,
    // and a blocking dialog there deadlocks the UI. Decide in the callback.
    let app_cb = app.clone();
    app.dialog()
        .message(format!(
            "You have {count} file(s) with unsaved changes. Do you want to save them before closing?"
        ))
        .title("Unsaved Changes")
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            SAVE_LABEL.into(),
            DONT_SAVE_LABEL.into(),
            "Cancel".into(),
        ))
        .show_with_result(move |res| {
            // rfd returns Custom(label) for custom buttons; map Yes/No defensively.
            let save = matches!(&res, MessageDialogResult::Yes)
                || matches!(&res, MessageDialogResult::Custom(s) if s == SAVE_LABEL);
            let dont_save = matches!(&res, MessageDialogResult::No)
                || matches!(&res, MessageDialogResult::Custom(s) if s == DONT_SAVE_LABEL);
            if save {
                crate::commands::files::save_unsaved_and_close(app_cb, window, unsaved_files);
            } else if dont_save {
                mark_and_close(&app_cb, &window);
            }
            // Cancel / dismissed → keep the window open.
        });
}
