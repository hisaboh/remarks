//! Native application menu — Phase 4 port of the Electron main/menu/ tree.
//!
//! Each custom item's id is a renderer command id (see commands/index.ts). On
//! click we emit `mt::execute-command-by-id` to the focused window, which the
//! renderer's command center runs — so the menu reuses the entire existing
//! command chain (File→Open → mt::cmd-open-file → file_open, etc.). Standard
//! text-editing items (undo/copy/paste…) use native predefined roles.
//!
//! Checkbox/radio state sync (4a): format marks, line ending and the sidebar
//! toggle are `CheckMenuItem`s registered in [`MenuState`] so the renderer can
//! reflect editor state onto them via the `menu_update_*` commands (mapped from
//! `mt::update-format-menu` / `-line-ending-menu` / `-sidebar-menu`). macOS has a
//! single global menu, so one registry covers all windows; per-window menu
//! variation and focus-driven re-sync are still later tasks.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem,
    SubmenuBuilder,
};
use tauri::{AppHandle, Emitter, Manager};

/// Menu item id that opens the settings window (handled specially, not a
/// renderer command).
pub const PREFERENCES_ID: &str = "app.preferences";

const LINE_ENDING_CRLF_ID: &str = "file.line-ending-crlf";
const LINE_ENDING_LF_ID: &str = "file.line-ending-lf";
const SIDEBAR_ID: &str = "view.toggle-sidebar";

/// Renderer format-state key → menu item id. Mirrors MENU_ID_FORMAT_MAP in
/// main/menu/actions/format.ts (only the marks that have a menu entry).
const FORMAT_MAP: &[(&str, &str)] = &[
    ("format.strong", "strong"),
    ("format.emphasis", "em"),
    ("format.inline-code", "inline_code"),
    ("format.strike", "del"),
    ("format.hyperlink", "link"),
    ("format.image", "image"),
];

/// Holds clonable handles to the checkable menu items so the `menu_update_*`
/// commands can toggle their state after the menu is built.
#[derive(Default)]
pub struct MenuState {
    checks: Mutex<HashMap<String, CheckMenuItem<tauri::Wry>>>,
}

impl MenuState {
    fn register(&self, item: &CheckMenuItem<tauri::Wry>) {
        self.checks
            .lock()
            .unwrap()
            .insert(item.id().as_ref().to_string(), item.clone());
    }

    fn set(&self, id: &str, checked: bool) {
        if let Some(item) = self.checks.lock().unwrap().get(id) {
            let _ = item.set_checked(checked);
        }
    }
}

fn cmd(
    app: &AppHandle,
    id: &str,
    label: &str,
    accel: Option<&str>,
) -> tauri::Result<tauri::menu::MenuItem<tauri::Wry>> {
    let mut builder = MenuItemBuilder::with_id(id, label);
    if let Some(a) = accel {
        builder = builder.accelerator(a);
    }
    builder.build(app)
}

/// Build a checkable item and register it in [`MenuState`] (managed before
/// `build_menu` runs) so it can later be toggled by the renderer.
fn check(
    app: &AppHandle,
    id: &str,
    label: &str,
    accel: Option<&str>,
) -> tauri::Result<CheckMenuItem<tauri::Wry>> {
    let mut builder = CheckMenuItemBuilder::with_id(id, label).checked(false);
    if let Some(a) = accel {
        builder = builder.accelerator(a);
    }
    let item = builder.build(app)?;
    app.state::<MenuState>().register(&item);
    Ok(item)
}

pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    // macOS application menu (About / Preferences / Quit).
    let app_menu = SubmenuBuilder::new(app, "MarkText")
        .item(&PredefinedMenuItem::about(app, None, None)?)
        .separator()
        .item(&cmd(app, PREFERENCES_ID, "Preferences…", Some("CmdOrCtrl+,"))?)
        .separator()
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, None)?)
        .build()?;

    // Radio-like line-ending submenu (state synced via menu_update_line_ending).
    let line_ending_menu = SubmenuBuilder::new(app, "Line Ending")
        .item(&check(app, LINE_ENDING_CRLF_ID, "Carriage Return and Line Feed (CRLF)", None)?)
        .item(&check(app, LINE_ENDING_LF_ID, "Line Feed (LF)", None)?)
        .build()?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&cmd(app, "file.new-tab", "New Tab", Some("CmdOrCtrl+T"))?)
        .item(&cmd(app, "file.new-window", "New Window", Some("CmdOrCtrl+Shift+N"))?)
        .separator()
        .item(&cmd(app, "file.open-file", "Open File…", Some("CmdOrCtrl+O"))?)
        .separator()
        .item(&cmd(app, "file.save", "Save", Some("CmdOrCtrl+S"))?)
        .item(&cmd(app, "file.save-as", "Save As…", Some("CmdOrCtrl+Shift+S"))?)
        .item(&line_ending_menu)
        .separator()
        .item(&cmd(app, "file.close-tab", "Close Tab", Some("CmdOrCtrl+W"))?)
        .item(&cmd(app, "file.close-window", "Close Window", Some("CmdOrCtrl+Shift+W"))?)
        .build()?;

    // Native editing roles + app find/replace commands.
    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .item(&PredefinedMenuItem::undo(app, Some("Undo"))?)
        .item(&PredefinedMenuItem::redo(app, Some("Redo"))?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some("Cut"))?)
        .item(&PredefinedMenuItem::copy(app, Some("Copy"))?)
        .item(&PredefinedMenuItem::paste(app, Some("Paste"))?)
        .item(&PredefinedMenuItem::select_all(app, Some("Select All"))?)
        .separator()
        .item(&cmd(app, "edit.find", "Find", Some("CmdOrCtrl+F"))?)
        .item(&cmd(app, "edit.replace", "Replace", Some("CmdOrCtrl+Option+F"))?)
        .build()?;

    let paragraph_menu = SubmenuBuilder::new(app, "Paragraph")
        .item(&cmd(app, "paragraph.heading-1", "Heading 1", Some("CmdOrCtrl+1"))?)
        .item(&cmd(app, "paragraph.heading-2", "Heading 2", Some("CmdOrCtrl+2"))?)
        .item(&cmd(app, "paragraph.heading-3", "Heading 3", Some("CmdOrCtrl+3"))?)
        .item(&cmd(app, "paragraph.heading-4", "Heading 4", Some("CmdOrCtrl+4"))?)
        .item(&cmd(app, "paragraph.heading-5", "Heading 5", Some("CmdOrCtrl+5"))?)
        .item(&cmd(app, "paragraph.heading-6", "Heading 6", Some("CmdOrCtrl+6"))?)
        .separator()
        .item(&cmd(app, "paragraph.table", "Table", None)?)
        .item(&cmd(app, "paragraph.code-fence", "Code Fence", None)?)
        .item(&cmd(app, "paragraph.quote-block", "Quote Block", None)?)
        .item(&cmd(app, "paragraph.order-list", "Ordered List", None)?)
        .item(&cmd(app, "paragraph.bullet-list", "Bullet List", None)?)
        .item(&cmd(app, "paragraph.task-list", "Task List", None)?)
        .item(&cmd(app, "paragraph.horizontal-line", "Horizontal Line", None)?)
        .build()?;

    // Format marks are checkable (state synced via menu_update_format).
    let format_menu = SubmenuBuilder::new(app, "Format")
        .item(&check(app, "format.strong", "Strong", Some("CmdOrCtrl+B"))?)
        .item(&check(app, "format.emphasis", "Emphasis", Some("CmdOrCtrl+I"))?)
        .item(&cmd(app, "format.underline", "Underline", Some("CmdOrCtrl+U"))?)
        .item(&check(app, "format.inline-code", "Inline Code", None)?)
        .item(&check(app, "format.strike", "Strikethrough", None)?)
        .separator()
        .item(&check(app, "format.hyperlink", "Hyperlink", Some("CmdOrCtrl+L"))?)
        .item(&check(app, "format.image", "Image", None)?)
        .separator()
        .item(&cmd(app, "format.clear-format", "Clear Format", None)?)
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "View")
        .item(&check(app, SIDEBAR_ID, "Toggle Sidebar", None)?)
        .item(&cmd(app, "view.toggle-tabbar", "Toggle Tab Bar", None)?)
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .item(&PredefinedMenuItem::minimize(app, None)?)
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .build()?;

    MenuBuilder::new(app)
        .items(&[
            &app_menu,
            &file_menu,
            &edit_menu,
            &paragraph_menu,
            &format_menu,
            &view_menu,
            &window_menu,
        ])
        .build()
}

/// First focused webview window, if any.
fn focused_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.webview_windows()
        .into_values()
        .find(|w| w.is_focused().unwrap_or(false))
}

/// Route a menu click: special-case Preferences and the line-ending entries,
/// else forward the id as a renderer command to the focused window.
pub fn handle_menu_event(app: &AppHandle, id: &str) {
    if id == PREFERENCES_ID {
        crate::commands::window::open_settings(app);
        return;
    }
    // Context-menu popup items are routed back as mt::menu::click, not as
    // application commands.
    if app
        .state::<crate::commands::context_menu::PopupMenuState>()
        .is_popup_id(id)
    {
        crate::commands::context_menu::route_popup_click(app, id);
        return;
    }
    // Line-ending entries are command *subcommands*, which the renderer's
    // command center can't dispatch by id — emit the dedicated event instead
    // (editor store listens on mt::set-line-ending). It re-sends
    // mt::update-line-ending-menu, which toggles the check state.
    if id == LINE_ENDING_CRLF_ID || id == LINE_ENDING_LF_ID {
        let value = if id == LINE_ENDING_CRLF_ID { "crlf" } else { "lf" };
        if let Some(window) = focused_window(app) {
            let _ = window.emit_to(window.label(), "mt::set-line-ending", value);
        }
        return;
    }
    if let Some(window) = focused_window(app) {
        let _ = window.emit_to(window.label(), "mt::execute-command-by-id", id);
    }
}

// ---- state sync commands (mapped from mt::update-*-menu) ---------------------

/// `mt::update-format-menu` — check the marks active at the current selection.
#[tauri::command]
pub fn menu_update_format(app: AppHandle, formats: HashMap<String, bool>) {
    let state = app.state::<MenuState>();
    for (menu_id, key) in FORMAT_MAP {
        state.set(menu_id, formats.get(*key).copied().unwrap_or(false));
    }
}

/// `mt::update-line-ending-menu` — check CRLF or LF for the current tab.
#[tauri::command]
pub fn menu_update_line_ending(app: AppHandle, line_ending: String) {
    let state = app.state::<MenuState>();
    let is_crlf = line_ending == "crlf";
    state.set(LINE_ENDING_CRLF_ID, is_crlf);
    state.set(LINE_ENDING_LF_ID, !is_crlf);
}

/// `mt::update-sidebar-menu` — reflect sidebar visibility.
#[tauri::command]
pub fn menu_update_sidebar(app: AppHandle, visible: bool) {
    app.state::<MenuState>().set(SIDEBAR_ID, visible);
}
