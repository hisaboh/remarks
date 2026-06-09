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
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem,
    Submenu, SubmenuBuilder,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

/// Menu item id that opens the settings window (handled specially, not a
/// renderer command).
pub const PREFERENCES_ID: &str = "app.preferences";

const LINE_ENDING_CRLF_ID: &str = "file.line-ending-crlf";
const LINE_ENDING_LF_ID: &str = "file.line-ending-lf";
const SIDEBAR_ID: &str = "view.toggle-sidebar";

// Recently-used documents (4g): an "Open Recent" File submenu, persisted to
// <config_dir>/recently-used-documents.json. Item ids are RECENT_PREFIX+path;
// RECENT_CLEAR_ID clears the list. (Tauri core has no native OS recent-docs
// API, so we keep our own list + menu — like Electron's Linux path.)
const RECENT_PREFIX: &str = "recent:";
const RECENT_CLEAR_ID: &str = "recent:clear";
const MAX_RECENT: usize = 12;
const RECENT_FILE: &str = "recently-used-documents.json";

/// The in-memory recently-used document list (newest first).
#[derive(Default)]
pub struct RecentDocs(Mutex<Vec<String>>);

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
/// commands can toggle their state after the menu is built, plus the dynamic
/// "Open Recent" submenu so it can be refreshed without rebuilding the menu
/// (which would reset the checkbox state).
#[derive(Default)]
pub struct MenuState {
    checks: Mutex<HashMap<String, CheckMenuItem<tauri::Wry>>>,
    open_recent: Mutex<Option<Submenu<tauri::Wry>>>,
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

// ---- recently-used documents (4g) -------------------------------------------

fn recent_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(RECENT_FILE))
}

/// Load the persisted recent list into the managed [`RecentDocs`] (call in setup
/// before `build_menu`, which reads it to populate the submenu).
pub fn load_recent(app: &AppHandle) {
    let Some(path) = recent_path(app) else { return };
    let Ok(content) = std::fs::read_to_string(&path) else { return };
    let list: Vec<String> = serde_json::from_str(&content).unwrap_or_default();
    // Drop entries that no longer exist on disk.
    let list: Vec<String> = list
        .into_iter()
        .filter(|p| std::path::Path::new(p).is_file())
        .take(MAX_RECENT)
        .collect();
    *app.state::<RecentDocs>().0.lock().unwrap() = list;
}

fn save_recent(app: &AppHandle, list: &[String]) {
    let Some(path) = recent_path(app) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(&path, json);
    }
}

/// Record a freshly opened/saved file at the top of the recent list, persist it,
/// and refresh the "Open Recent" submenu.
pub fn add_recent(app: &AppHandle, file_path: &str) {
    {
        let state = app.state::<RecentDocs>();
        let mut list = state.0.lock().unwrap();
        list.retain(|p| p != file_path);
        list.insert(0, file_path.to_string());
        list.truncate(MAX_RECENT);
        save_recent(app, &list);
    }
    refresh_recent_menu(app);
}

fn clear_recent(app: &AppHandle) {
    {
        let state = app.state::<RecentDocs>();
        state.0.lock().unwrap().clear();
        save_recent(app, &[]);
    }
    refresh_recent_menu(app);
}

/// Rebuild the "Open Recent" submenu's items from the current list.
fn refresh_recent_menu(app: &AppHandle) {
    let menu_state = app.state::<MenuState>();
    let guard = menu_state.open_recent.lock().unwrap();
    let Some(submenu) = guard.as_ref() else { return };
    // Clear existing items.
    while let Ok(Some(_)) = submenu.remove_at(0) {}
    let tr = Translator::for_app(app);
    let list = app.state::<RecentDocs>().0.lock().unwrap().clone();
    if list.is_empty() {
        // No locale key for the empty-state placeholder; keep the English text.
        if let Ok(item) = MenuItemBuilder::with_id("recent:none", "No Recent Files")
            .enabled(false)
            .build(app)
        {
            let _ = submenu.append(&item);
        }
        return;
    }
    for path in &list {
        let label = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        if let Ok(item) = MenuItemBuilder::with_id(format!("{RECENT_PREFIX}{path}"), label).build(app)
        {
            let _ = submenu.append(&item);
        }
    }
    if let Ok(sep) = PredefinedMenuItem::separator(app) {
        let _ = submenu.append(&sep);
    }
    if let Ok(clear) =
        MenuItemBuilder::with_id(RECENT_CLEAR_ID, tr.t("menu.file.clearRecentlyUsed")).build(app)
    {
        let _ = submenu.append(&clear);
    }
}

/// Rebuild and re-apply the whole menu — used when the `language` preference
/// changes so every label is re-translated. Resets the 4a checkbox state to
/// unchecked (the renderer re-syncs it on the next selection/toggle); the recent
/// list survives via [`RecentDocs`]. Must run on the main thread (macOS NSMenu).
pub fn rebuild_menu(app: &AppHandle) {
    match build_menu(app) {
        Ok(menu) => {
            if let Err(e) = app.set_menu(menu) {
                log::error!("failed to apply rebuilt menu: {e}");
            }
        }
        Err(e) => log::error!("failed to rebuild menu: {e}"),
    }
}

// ---- menu i18n -------------------------------------------------------------
// The menu is built in Rust, so its labels are translated here from the embedded
// locale catalogs — the same `static/locales/*.json` the renderer uses — keyed
// by the current `language` preference, with English as the fallback. The menu
// is rebuilt by `rebuild_menu` whenever the language pref changes.

macro_rules! locale_str {
    ($file:literal) => {
        include_str!(concat!("../../static/locales/", $file))
    };
}

const LOCALES: &[(&str, &str)] = &[
    ("en", locale_str!("en.json")),
    ("zh-CN", locale_str!("zh-CN.json")),
    ("zh-TW", locale_str!("zh-TW.json")),
    ("ja", locale_str!("ja.json")),
    ("ko", locale_str!("ko.json")),
    ("fr", locale_str!("fr.json")),
    ("de", locale_str!("de.json")),
    ("es", locale_str!("es.json")),
    ("pt", locale_str!("pt.json")),
];

/// Looks up dot-separated keys (e.g. `menu.file.newTab`) in a locale catalog,
/// falling back to English, then to the key itself.
struct Translator {
    primary: serde_json::Value,
    fallback: serde_json::Value,
}

impl Translator {
    fn for_app(app: &AppHandle) -> Self {
        let lang = app
            .store(crate::commands::preferences::PREFERENCES_FILE)
            .ok()
            .and_then(|s| s.get("language"))
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "en".into());
        Self::new(&lang)
    }

    fn new(lang: &str) -> Self {
        let load = |l: &str| {
            LOCALES
                .iter()
                .find(|(k, _)| *k == l)
                .and_then(|(_, s)| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null)
        };
        Self { primary: load(lang), fallback: load("en") }
    }

    fn t(&self, key: &str) -> String {
        nav(&self.primary, key)
            .or_else(|| nav(&self.fallback, key))
            .unwrap_or_else(|| key.to_owned())
    }
}

fn nav(value: &serde_json::Value, key: &str) -> Option<String> {
    let mut cur = value;
    for seg in key.split('.') {
        cur = cur.get(seg)?;
    }
    cur.as_str().map(str::to_owned)
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
    let tr = Translator::for_app(app);

    // macOS application menu (About / Preferences / Quit). The About/Hide/Quit
    // locale strings already include the app name (e.g. "Quit MarkText").
    let app_menu = SubmenuBuilder::new(app, &tr.t("menu.marktext.title"))
        .item(&PredefinedMenuItem::about(app, Some(&tr.t("menu.marktext.about")), None)?)
        .separator()
        .item(&cmd(app, PREFERENCES_ID, &tr.t("menu.marktext.preferences"), Some("CmdOrCtrl+,"))?)
        .separator()
        .item(&PredefinedMenuItem::hide(app, Some(&tr.t("menu.marktext.hide")))?)
        .item(&PredefinedMenuItem::hide_others(app, Some(&tr.t("menu.marktext.hideOthers")))?)
        .item(&PredefinedMenuItem::show_all(app, Some(&tr.t("menu.marktext.showAll")))?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some(&tr.t("menu.marktext.quit")))?)
        .build()?;

    // Radio-like line-ending submenu (state synced via menu_update_line_ending).
    let line_ending_menu = SubmenuBuilder::new(app, &tr.t("menu.edit.lineEnding"))
        .item(&check(app, LINE_ENDING_CRLF_ID, &tr.t("menu.edit.lineEndingCrlf"), None)?)
        .item(&check(app, LINE_ENDING_LF_ID, &tr.t("menu.edit.lineEndingLf"), None)?)
        .build()?;

    // Dynamic "Open Recent" submenu (4g) — populated from the loaded list and
    // refreshed in place on open/save (stored in MenuState).
    let open_recent = SubmenuBuilder::new(app, &tr.t("menu.file.openRecent")).build()?;
    *app.state::<MenuState>().open_recent.lock().unwrap() = Some(open_recent.clone());
    refresh_recent_menu(app);

    let file_menu = SubmenuBuilder::new(app, &tr.t("menu.file.file"))
        .item(&cmd(app, "file.new-tab", &tr.t("menu.file.newTab"), Some("CmdOrCtrl+T"))?)
        .item(&cmd(app, "file.new-window", &tr.t("menu.file.newWindow"), Some("CmdOrCtrl+Shift+N"))?)
        .separator()
        .item(&cmd(app, "file.open-file", &tr.t("menu.file.openFile"), Some("CmdOrCtrl+O"))?)
        .item(&open_recent)
        .separator()
        .item(&cmd(app, "file.save", &tr.t("menu.file.save"), Some("CmdOrCtrl+S"))?)
        .item(&cmd(app, "file.save-as", &tr.t("menu.file.saveAs"), Some("CmdOrCtrl+Shift+S"))?)
        .item(&line_ending_menu)
        .separator()
        .item(&cmd(app, "file.close-tab", &tr.t("menu.file.closeTab"), Some("CmdOrCtrl+W"))?)
        .item(&cmd(app, "file.close-window", &tr.t("menu.file.closeWindow"), Some("CmdOrCtrl+Shift+W"))?)
        .build()?;

    // Native editing roles + app find/replace commands.
    let edit_menu = SubmenuBuilder::new(app, &tr.t("menu.edit.edit"))
        .item(&PredefinedMenuItem::undo(app, Some(&tr.t("menu.edit.undo")))?)
        .item(&PredefinedMenuItem::redo(app, Some(&tr.t("menu.edit.redo")))?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some(&tr.t("menu.edit.cut")))?)
        .item(&PredefinedMenuItem::copy(app, Some(&tr.t("menu.edit.copy")))?)
        .item(&PredefinedMenuItem::paste(app, Some(&tr.t("menu.edit.paste")))?)
        .item(&PredefinedMenuItem::select_all(app, Some(&tr.t("menu.edit.selectAll")))?)
        .separator()
        .item(&cmd(app, "edit.find", &tr.t("menu.edit.find"), Some("CmdOrCtrl+F"))?)
        .item(&cmd(app, "edit.replace", &tr.t("menu.edit.replace"), Some("CmdOrCtrl+Option+F"))?)
        .build()?;

    let paragraph_menu = SubmenuBuilder::new(app, &tr.t("menu.paragraph.paragraph"))
        .item(&cmd(app, "paragraph.heading-1", &tr.t("menu.paragraph.heading1"), Some("CmdOrCtrl+1"))?)
        .item(&cmd(app, "paragraph.heading-2", &tr.t("menu.paragraph.heading2"), Some("CmdOrCtrl+2"))?)
        .item(&cmd(app, "paragraph.heading-3", &tr.t("menu.paragraph.heading3"), Some("CmdOrCtrl+3"))?)
        .item(&cmd(app, "paragraph.heading-4", &tr.t("menu.paragraph.heading4"), Some("CmdOrCtrl+4"))?)
        .item(&cmd(app, "paragraph.heading-5", &tr.t("menu.paragraph.heading5"), Some("CmdOrCtrl+5"))?)
        .item(&cmd(app, "paragraph.heading-6", &tr.t("menu.paragraph.heading6"), Some("CmdOrCtrl+6"))?)
        .separator()
        .item(&cmd(app, "paragraph.table", &tr.t("menu.paragraph.table"), None)?)
        .item(&cmd(app, "paragraph.code-fence", &tr.t("menu.paragraph.codeFences"), None)?)
        .item(&cmd(app, "paragraph.quote-block", &tr.t("menu.paragraph.quoteBlock"), None)?)
        .item(&cmd(app, "paragraph.order-list", &tr.t("menu.paragraph.orderedList"), None)?)
        .item(&cmd(app, "paragraph.bullet-list", &tr.t("menu.paragraph.bulletList"), None)?)
        .item(&cmd(app, "paragraph.task-list", &tr.t("menu.paragraph.taskList"), None)?)
        .item(&cmd(app, "paragraph.horizontal-line", &tr.t("menu.paragraph.horizontalRule"), None)?)
        .build()?;

    // Format marks are checkable (state synced via menu_update_format).
    let format_menu = SubmenuBuilder::new(app, &tr.t("menu.format.format"))
        .item(&check(app, "format.strong", &tr.t("menu.format.bold"), Some("CmdOrCtrl+B"))?)
        .item(&check(app, "format.emphasis", &tr.t("menu.format.italic"), Some("CmdOrCtrl+I"))?)
        .item(&cmd(app, "format.underline", &tr.t("menu.format.underline"), Some("CmdOrCtrl+U"))?)
        .item(&check(app, "format.inline-code", &tr.t("menu.format.inlineCode"), None)?)
        .item(&check(app, "format.strike", &tr.t("menu.format.strikethrough"), None)?)
        .separator()
        .item(&check(app, "format.hyperlink", &tr.t("menu.format.hyperlink"), Some("CmdOrCtrl+L"))?)
        .item(&check(app, "format.image", &tr.t("menu.format.image"), None)?)
        .separator()
        .item(&cmd(app, "format.clear-format", &tr.t("menu.format.clearFormat"), None)?)
        .build()?;

    let view_menu = SubmenuBuilder::new(app, &tr.t("menu.view.view"))
        .item(&check(app, SIDEBAR_ID, &tr.t("menu.view.toggleSidebar"), None)?)
        .item(&cmd(app, "view.toggle-tabbar", &tr.t("menu.view.toggleTabbar"), None)?)
        .build()?;

    let window_menu = SubmenuBuilder::new(app, &tr.t("menu.window.window"))
        .item(&PredefinedMenuItem::minimize(app, Some(&tr.t("menu.window.minimize")))?)
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
    // Recently-used documents (4g): clear, or open the path encoded in the id.
    if id == RECENT_CLEAR_ID {
        clear_recent(app);
        return;
    }
    if let Some(path) = id.strip_prefix(RECENT_PREFIX) {
        if let Some(window) = focused_window(app) {
            crate::commands::files::open_path_in_window(app, &window, path, true);
        }
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
