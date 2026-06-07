//! Native application menu — Phase 4 port of the Electron main/menu/ tree.
//!
//! Each custom item's id is a renderer command id (see commands/index.ts). On
//! click we emit `mt::execute-command-by-id` to the focused window, which the
//! renderer's command center runs — so the menu reuses the entire existing
//! command chain (File→Open → mt::cmd-open-file → file_open, etc.). Standard
//! text-editing items (undo/copy/paste…) use native predefined roles.
//!
//! Not yet ported: the full Paragraph/Format/View trees, checkbox/radio state
//! sync (mt::update-format-menu etc.), and per-window menu variation.

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager};

/// Menu item id that opens the settings window (handled specially, not a
/// renderer command).
pub const PREFERENCES_ID: &str = "app.preferences";

fn cmd(app: &AppHandle, id: &str, label: &str, accel: Option<&str>) -> tauri::Result<tauri::menu::MenuItem<tauri::Wry>> {
    let mut builder = MenuItemBuilder::with_id(id, label);
    if let Some(a) = accel {
        builder = builder.accelerator(a);
    }
    builder.build(app)
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

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&cmd(app, "file.new-tab", "New Tab", Some("CmdOrCtrl+T"))?)
        .item(&cmd(app, "file.new-window", "New Window", Some("CmdOrCtrl+Shift+N"))?)
        .separator()
        .item(&cmd(app, "file.open-file", "Open File…", Some("CmdOrCtrl+O"))?)
        .separator()
        .item(&cmd(app, "file.save", "Save", Some("CmdOrCtrl+S"))?)
        .item(&cmd(app, "file.save-as", "Save As…", Some("CmdOrCtrl+Shift+S"))?)
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

    let format_menu = SubmenuBuilder::new(app, "Format")
        .item(&cmd(app, "format.strong", "Strong", Some("CmdOrCtrl+B"))?)
        .item(&cmd(app, "format.emphasis", "Emphasis", Some("CmdOrCtrl+I"))?)
        .item(&cmd(app, "format.underline", "Underline", Some("CmdOrCtrl+U"))?)
        .item(&cmd(app, "format.inline-code", "Inline Code", None)?)
        .item(&cmd(app, "format.strike", "Strikethrough", None)?)
        .separator()
        .item(&cmd(app, "format.hyperlink", "Hyperlink", Some("CmdOrCtrl+L"))?)
        .item(&cmd(app, "format.image", "Image", None)?)
        .separator()
        .item(&cmd(app, "format.clear-format", "Clear Format", None)?)
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
            &window_menu,
        ])
        .build()
}

/// Route a menu click: special-case Preferences, else forward the id as a
/// renderer command to the focused window.
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
    let focused = app
        .webview_windows()
        .into_values()
        .find(|w| w.is_focused().unwrap_or(false));
    if let Some(window) = focused {
        let _ = window.emit_to(window.label(), "mt::execute-command-by-id", id);
    }
}
