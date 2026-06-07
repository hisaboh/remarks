//! Context-menu popups — Phase 4 port of the `mt::menu::popup` handler in
//! main/ipc/window.ts. The renderer (contextMenu/popupMenu.ts) serializes a
//! template, calls this, and listens for `mt::menu::click {id}` / `mt::menu::closed`.
//!
//! Popup item ids are registered in PopupMenuState so the shared `on_menu_event`
//! (lib.rs) can tell them apart from application-menu command ids and route
//! them back as `mt::menu::click` instead of `mt::execute-command-by-id`.

use std::sync::Mutex;

use serde_json::Value;
use tauri::menu::{CheckMenuItemBuilder, ContextMenu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, Window, Wry};

/// Ids belonging to the currently-shown popup menu (one popup at a time).
#[derive(Default)]
pub struct PopupMenuState {
    ids: Mutex<std::collections::HashSet<String>>,
}

impl PopupMenuState {
    fn replace(&self, ids: std::collections::HashSet<String>) {
        *self.ids.lock().unwrap() = ids;
    }
    pub fn is_popup_id(&self, id: &str) -> bool {
        self.ids.lock().unwrap().contains(id)
    }
    pub fn clear(&self) {
        self.ids.lock().unwrap().clear();
    }
}

/// Collect every (non-separator) item id from the template tree.
fn collect_ids(items: &[Value], out: &mut std::collections::HashSet<String>) {
    for item in items {
        if item.get("type").and_then(Value::as_str) == Some("separator") {
            continue;
        }
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            out.insert(id.to_string());
        }
        if let Some(sub) = item.get("submenu").and_then(Value::as_array) {
            collect_ids(sub, out);
        }
    }
}

fn build_items(app: &AppHandle, items: &[Value]) -> Vec<Box<dyn tauri::menu::IsMenuItem<Wry>>> {
    let mut built: Vec<Box<dyn tauri::menu::IsMenuItem<Wry>>> = Vec::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) == Some("separator") {
            if let Ok(sep) = PredefinedMenuItem::separator(app) {
                built.push(Box::new(sep));
            }
            continue;
        }
        let id = item.get("id").and_then(Value::as_str).unwrap_or("");
        let label = item.get("label").and_then(Value::as_str).unwrap_or("");
        let enabled = item.get("enabled").and_then(Value::as_bool).unwrap_or(true);

        if let Some(sub) = item.get("submenu").and_then(Value::as_array) {
            let children = build_items(app, sub);
            let mut sb = SubmenuBuilder::new(app, label).enabled(enabled);
            for child in &children {
                sb = sb.item(child.as_ref());
            }
            if let Ok(submenu) = sb.build() {
                built.push(Box::new(submenu));
            }
            continue;
        }

        // Checkbox items carry a `checked` flag (type "checkbox").
        if item.get("type").and_then(Value::as_str) == Some("checkbox") {
            let checked = item.get("checked").and_then(Value::as_bool).unwrap_or(false);
            if let Ok(it) = CheckMenuItemBuilder::with_id(id, label)
                .enabled(enabled)
                .checked(checked)
                .build(app)
            {
                built.push(Box::new(it));
            }
        } else if let Ok(it) = MenuItemBuilder::with_id(id, label).enabled(enabled).build(app) {
            built.push(Box::new(it));
        }
    }
    built
}

#[tauri::command]
pub fn menu_popup(
    app: AppHandle,
    window: Window,
    template: Vec<Value>,
    position: Option<Value>,
) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    collect_ids(&template, &mut ids);
    app.state::<PopupMenuState>().replace(ids);

    // Native menus must be built and shown on the main thread (macOS NSMenu);
    // Tauri commands run on a worker thread, so dispatch there. The template is
    // Send, so build inside the closure.
    let app_for_thread = app.clone();
    let window_for_popup = window.clone();
    window
        .run_on_main_thread(move || {
            let items = build_items(&app_for_thread, &template);
            let mut mb = MenuBuilder::new(&app_for_thread);
            for it in &items {
                mb = mb.item(it.as_ref());
            }
            let menu = match mb.build() {
                Ok(m) => m,
                Err(e) => {
                    log::error!("context menu build failed: {e}");
                    return;
                }
            };
            let result = match position.as_ref() {
                Some(p) => {
                    let x = p.get("x").and_then(Value::as_f64).unwrap_or(0.0);
                    let y = p.get("y").and_then(Value::as_f64).unwrap_or(0.0);
                    menu.popup_at(window_for_popup, LogicalPosition::new(x, y))
                }
                None => menu.popup(window_for_popup),
            };
            if let Err(e) = result {
                log::error!("context menu popup failed: {e}");
            }
        })
        .map_err(|e| e.to_string())
}

/// Application menu popup (custom title-bar "hamburger"). The native app menu is
/// global on macOS, so there's nothing window-local to pop up — no-op for now.
#[tauri::command]
pub fn menu_popup_application() {
    // TODO(phase-4): non-macOS custom title bar app-menu popup.
}

/// Called from lib.rs's on_menu_event when a clicked id belongs to a popup.
pub fn route_popup_click(app: &AppHandle, id: &str) {
    app.state::<PopupMenuState>().clear();
    let focused = app
        .webview_windows()
        .into_values()
        .find(|w| w.is_focused().unwrap_or(false));
    if let Some(window) = focused {
        let label = window.label().to_string();
        let _ = window.emit_to(&label, "mt::menu::click", serde_json::json!({ "id": id }));
        // No reliable "menu dismissed" signal from Tauri; emit closed after a
        // click so the renderer cleans up its per-popup listeners.
        // TODO(phase-4): emit closed on dismiss-without-click too.
        let _ = window.emit_to(&label, "mt::menu::closed", ());
    }
}
