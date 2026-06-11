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
pub const CHECK_UPDATES_ID: &str = "app.check-updates";

const LINE_ENDING_CRLF_ID: &str = "file.line-ending-crlf";
const LINE_ENDING_LF_ID: &str = "file.line-ending-lf";
const SIDEBAR_ID: &str = "view.toggle-sidebar";
const TABBAR_ID: &str = "view.toggle-tabbar";
const SOURCE_CODE_ID: &str = "view.source-code-mode";
const TYPEWRITER_ID: &str = "view.typewriter-mode";
const FOCUS_ID: &str = "view.focus-mode";
const AUTOSAVE_ID: &str = "file.toggle-auto-save";
const ALWAYS_ON_TOP_ID: &str = "window.toggle-always-on-top";

// Theme menu items carry a `theme:` prefix (theme names are not renderer
// command ids); handle_menu_event writes the preference directly.
const THEME_PREFIX: &str = "theme:";
const THEME_FOLLOW_SYSTEM_ID: &str = "theme:follow-system";

// Help menu items open external URLs (`help:` prefix → URL table).
const HELP_PREFIX: &str = "help:";
const HELP_LINKS: &[(&str, &str, &str)] = &[
    ("help:markdown", "menu.help.markdownReference", "https://marktext.me/docs/markdown-syntax"),
    ("help:changelog", "menu.help.changelog", "https://github.com/marktext/marktext/releases"),
    ("help:follow-us", "menu.help.followUs", "https://twitter.com/marktextapp"),
    ("help:support", "menu.help.support", "https://github.com/sponsors/marktext"),
    (
        "help:ask-question",
        "menu.help.askQuestion",
        "https://github.com/marktext/marktext/discussions",
    ),
    ("help:report-bug", "menu.help.reportBug", "https://github.com/marktext/marktext/issues"),
    ("help:view-source", "menu.help.viewSource", "https://github.com/marktext/marktext"),
    (
        "help:license",
        "menu.help.license",
        "https://github.com/marktext/marktext/blob/develop/LICENSE",
    ),
];

// Theme id → locale key, in the Electron menu's light/dark grouping.
const LIGHT_THEMES: &[(&str, &str)] = &[
    ("ayu-light", "menu.theme.ayuLight"),
    ("light", "menu.theme.cadmiumLight"),
    ("catppuccin-latte", "menu.theme.catppuccinLatte"),
    ("everforest-light", "menu.theme.everforestLight"),
    ("graphite", "menu.theme.graphiteLight"),
    ("gruvbox-light", "menu.theme.gruvboxLight"),
    ("rose-pine-dawn", "menu.theme.rosePineDawn"),
    ("solarized-light", "menu.theme.solarizedLight"),
    ("tokyo-night-light", "menu.theme.tokyoNightLight"),
    ("ulysses", "menu.theme.ulyssesLight"),
];
const DARK_THEMES: &[(&str, &str)] = &[
    ("ayu-dark", "menu.theme.ayuDark"),
    ("ayu-mirage", "menu.theme.ayuMirage"),
    ("dark", "menu.theme.cadmiumDark"),
    ("catppuccin-mocha", "menu.theme.catppuccinMocha"),
    ("cyberdream", "menu.theme.cyberdream"),
    ("dracula", "menu.theme.dracula"),
    ("everforest-dark", "menu.theme.everforestDark"),
    ("gruvbox-dark", "menu.theme.gruvboxDark"),
    ("horizon-dark", "menu.theme.horizonDark"),
    ("kanagawa", "menu.theme.kanagawa"),
    ("material-dark", "menu.theme.materialDark"),
    ("monokai-pro", "menu.theme.monokaiPro"),
    ("nightfox", "menu.theme.nightfox"),
    ("nord", "menu.theme.nord"),
    ("one-dark", "menu.theme.oneDark"),
    ("oxocarbon-dark", "menu.theme.oxocarbonDark"),
    ("palenight", "menu.theme.palenight"),
    ("rose-pine", "menu.theme.rosePine"),
    ("rose-pine-moon", "menu.theme.rosePineMoon"),
    ("solarized-dark", "menu.theme.solarizedDark"),
    ("synthwave-84", "menu.theme.synthwave84"),
    ("tokyo-night", "menu.theme.tokyoNight"),
    ("tokyo-night-storm", "menu.theme.tokyoNightStorm"),
];

/// Paragraph affiliation tag → menu item id. Mirrors MENU_ID_MAP in
/// main/menu/actions/paragraph.ts.
const PARAGRAPH_MAP: &[(&str, &str)] = &[
    ("paragraph.heading-1", "h1"),
    ("paragraph.heading-2", "h2"),
    ("paragraph.heading-3", "h3"),
    ("paragraph.heading-4", "h4"),
    ("paragraph.heading-5", "h5"),
    ("paragraph.heading-6", "h6"),
    ("paragraph.table", "figure"),
    ("paragraph.code-fence", "pre"),
    ("paragraph.html-block", "html"),
    ("paragraph.math-formula", "multiplemath"),
    ("paragraph.quote-block", "blockquote"),
    ("paragraph.order-list", "ol"),
    ("paragraph.bullet-list", "ul"),
    ("paragraph.paragraph", "p"),
    ("paragraph.horizontal-line", "hr"),
    ("paragraph.front-matter", "frontmatter"),
];

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

    fn set_enabled(&self, id: &str, enabled: bool) {
        if let Some(item) = self.checks.lock().unwrap().get(id) {
            let _ = item.set_enabled(enabled);
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
        .item(&cmd(app, CHECK_UPDATES_ID, &tr.t("menu.marktext.checkUpdates"), None)?)
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

    // Export as HTML / PDF (renderer commands → in-app export dialog).
    let export_menu = SubmenuBuilder::new(app, &tr.t("menu.file.export"))
        .item(&cmd(app, "file.export-file-html", &tr.t("menu.file.exportHtml"), None)?)
        .item(&cmd(app, "file.export-file-pdf", &tr.t("menu.file.exportPdf"), None)?)
        .build()?;

    let file_menu = SubmenuBuilder::new(app, &tr.t("menu.file.file"))
        .item(&cmd(app, "file.new-tab", &tr.t("menu.file.newTab"), Some("CmdOrCtrl+T"))?)
        .item(&cmd(app, "file.new-window", &tr.t("menu.file.newWindow"), Some("CmdOrCtrl+Shift+N"))?)
        .separator()
        .item(&cmd(app, "file.open-file", &tr.t("menu.file.openFile"), Some("CmdOrCtrl+O"))?)
        .item(&cmd(app, "file.open-folder", &tr.t("menu.file.openFolder"), Some("CmdOrCtrl+Shift+O"))?)
        .item(&open_recent)
        .separator()
        .item(&cmd(app, "file.save", &tr.t("menu.file.save"), Some("CmdOrCtrl+S"))?)
        .item(&cmd(app, "file.save-as", &tr.t("menu.file.saveAs"), Some("CmdOrCtrl+Shift+S"))?)
        .item(&check(app, AUTOSAVE_ID, &tr.t("menu.file.autoSave"), None)?)
        .separator()
        .item(&cmd(app, "file.move-file", &tr.t("menu.file.moveTo"), None)?)
        .item(&cmd(app, "file.rename-file", &tr.t("menu.file.rename"), None)?)
        .separator()
        .item(&export_menu)
        .item(&cmd(app, "file.print", &tr.t("menu.file.print"), None)?)
        .separator()
        .item(&cmd(app, "file.close-tab", &tr.t("menu.file.closeTab"), Some("CmdOrCtrl+W"))?)
        .item(&cmd(app, "file.close-window", &tr.t("menu.file.closeWindow"), Some("CmdOrCtrl+Shift+W"))?)
        .build()?;

    // Native editing roles + app commands. The line-ending submenu lives at
    // the end like Electron's Edit menu.
    let edit_menu = SubmenuBuilder::new(app, &tr.t("menu.edit.edit"))
        .item(&PredefinedMenuItem::undo(app, Some(&tr.t("menu.edit.undo")))?)
        .item(&PredefinedMenuItem::redo(app, Some(&tr.t("menu.edit.redo")))?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some(&tr.t("menu.edit.cut")))?)
        .item(&PredefinedMenuItem::copy(app, Some(&tr.t("menu.edit.copy")))?)
        .item(&PredefinedMenuItem::paste(app, Some(&tr.t("menu.edit.paste")))?)
        .separator()
        .item(&cmd(app, "edit.copy-as-rich", &tr.t("menu.edit.copyAsRich"), Some("CmdOrCtrl+Shift+C"))?)
        .item(&cmd(app, "edit.copy-as-html", &tr.t("menu.edit.copyAsHtml"), None)?)
        .item(&cmd(
            app,
            "edit.paste-as-plaintext",
            &tr.t("menu.edit.pasteAsPlainText"),
            Some("CmdOrCtrl+Shift+V"),
        )?)
        .separator()
        .item(&PredefinedMenuItem::select_all(app, Some(&tr.t("menu.edit.selectAll")))?)
        .separator()
        .item(&cmd(app, "edit.duplicate", &tr.t("menu.edit.duplicate"), Some("CmdOrCtrl+Alt+D"))?)
        .item(&cmd(
            app,
            "edit.create-paragraph",
            &tr.t("menu.edit.createParagraph"),
            Some("Shift+CmdOrCtrl+N"),
        )?)
        .item(&cmd(
            app,
            "edit.delete-paragraph",
            &tr.t("menu.edit.deleteParagraph"),
            Some("Shift+CmdOrCtrl+D"),
        )?)
        .separator()
        .item(&cmd(app, "edit.find", &tr.t("menu.edit.find"), Some("CmdOrCtrl+F"))?)
        .item(&cmd(app, "edit.replace", &tr.t("menu.edit.replace"), Some("CmdOrCtrl+Option+F"))?)
        .separator()
        .item(&cmd(
            app,
            "edit.find-in-folder",
            &tr.t("menu.edit.findInFolder"),
            Some("Shift+CmdOrCtrl+F"),
        )?)
        .separator()
        .item(&line_ending_menu)
        .build()?;

    // Paragraph-type items are checkable; the current selection's block type
    // arrives via menu_update_paragraph (mt::editor-selection-changed).
    let paragraph_menu = SubmenuBuilder::new(app, &tr.t("menu.paragraph.paragraph"))
        .item(&check(app, "paragraph.heading-1", &tr.t("menu.paragraph.heading1"), Some("CmdOrCtrl+1"))?)
        .item(&check(app, "paragraph.heading-2", &tr.t("menu.paragraph.heading2"), Some("CmdOrCtrl+2"))?)
        .item(&check(app, "paragraph.heading-3", &tr.t("menu.paragraph.heading3"), Some("CmdOrCtrl+3"))?)
        .item(&check(app, "paragraph.heading-4", &tr.t("menu.paragraph.heading4"), Some("CmdOrCtrl+4"))?)
        .item(&check(app, "paragraph.heading-5", &tr.t("menu.paragraph.heading5"), Some("CmdOrCtrl+5"))?)
        .item(&check(app, "paragraph.heading-6", &tr.t("menu.paragraph.heading6"), Some("CmdOrCtrl+6"))?)
        .separator()
        .item(&cmd(
            app,
            "paragraph.upgrade-heading",
            &tr.t("menu.paragraph.promoteHeading"),
            Some("CmdOrCtrl+Plus"),
        )?)
        .item(&cmd(
            app,
            "paragraph.degrade-heading",
            &tr.t("menu.paragraph.demoteHeading"),
            Some("CmdOrCtrl+-"),
        )?)
        .separator()
        .item(&check(app, "paragraph.table", &tr.t("menu.paragraph.table"), Some("CmdOrCtrl+Shift+T"))?)
        .item(&check(app, "paragraph.code-fence", &tr.t("menu.paragraph.codeFences"), Some("CmdOrCtrl+Alt+C"))?)
        .item(&check(app, "paragraph.quote-block", &tr.t("menu.paragraph.quoteBlock"), Some("CmdOrCtrl+Alt+Q"))?)
        .item(&check(app, "paragraph.math-formula", &tr.t("menu.paragraph.mathBlock"), Some("CmdOrCtrl+Alt+M"))?)
        .item(&check(app, "paragraph.html-block", &tr.t("menu.paragraph.htmlBlock"), Some("CmdOrCtrl+Alt+J"))?)
        .separator()
        .item(&check(app, "paragraph.order-list", &tr.t("menu.paragraph.orderedList"), Some("CmdOrCtrl+Alt+O"))?)
        .item(&check(app, "paragraph.bullet-list", &tr.t("menu.paragraph.bulletList"), Some("CmdOrCtrl+Alt+U"))?)
        .item(&check(app, "paragraph.task-list", &tr.t("menu.paragraph.taskList"), Some("CmdOrCtrl+Alt+X"))?)
        .separator()
        .item(&check(
            app,
            "paragraph.loose-list-item",
            &tr.t("menu.paragraph.looseListItem"),
            Some("CmdOrCtrl+Alt+L"),
        )?)
        .separator()
        .item(&check(app, "paragraph.paragraph", &tr.t("menu.paragraph.paragraph"), Some("CmdOrCtrl+0"))?)
        .item(&check(
            app,
            "paragraph.horizontal-line",
            &tr.t("menu.paragraph.horizontalRule"),
            Some("CmdOrCtrl+Alt+-"),
        )?)
        .item(&check(app, "paragraph.front-matter", &tr.t("menu.paragraph.frontMatter"), Some("CmdOrCtrl+Alt+Y"))?)
        .build()?;

    // Format marks are checkable (state synced via menu_update_format).
    let format_menu = SubmenuBuilder::new(app, &tr.t("menu.format.format"))
        .item(&check(app, "format.strong", &tr.t("menu.format.bold"), Some("CmdOrCtrl+B"))?)
        .item(&check(app, "format.emphasis", &tr.t("menu.format.italic"), Some("CmdOrCtrl+I"))?)
        .item(&cmd(app, "format.underline", &tr.t("menu.format.underline"), Some("CmdOrCtrl+U"))?)
        .separator()
        .item(&check(app, "format.superscript", &tr.t("menu.format.superscript"), None)?)
        .item(&check(app, "format.subscript", &tr.t("menu.format.subscript"), None)?)
        .item(&check(app, "format.highlight", &tr.t("menu.format.highlight"), None)?)
        .separator()
        .item(&check(app, "format.inline-code", &tr.t("menu.format.inlineCode"), None)?)
        .item(&check(app, "format.inline-math", &tr.t("menu.format.inlineMath"), None)?)
        .separator()
        .item(&check(app, "format.strike", &tr.t("menu.format.strikethrough"), None)?)
        .item(&check(app, "format.hyperlink", &tr.t("menu.format.hyperlink"), Some("CmdOrCtrl+L"))?)
        .item(&check(app, "format.image", &tr.t("menu.format.image"), None)?)
        .separator()
        .item(&cmd(app, "format.clear-format", &tr.t("menu.format.clearFormat"), None)?)
        .build()?;

    // Mirrors Electron's menu/templates/view.ts (minus the dev-only items).
    // Item ids are renderer command ids; the mode/layout toggles are
    // CheckMenuItems whose state arrives via menu_update_view
    // (mt::view-layout-changed) — in source-code mode the typewriter/focus
    // entries are disabled, like Electron's viewLayoutChanged.
    let view_menu = SubmenuBuilder::new(app, &tr.t("menu.view.view"))
        .item(&cmd(
            app,
            "view.command-palette",
            &tr.t("menu.view.commandPalette"),
            Some("CmdOrCtrl+Shift+P"),
        )?)
        .separator()
        .item(&check(
            app,
            SOURCE_CODE_ID,
            &tr.t("menu.view.sourceCodeMode"),
            Some("CmdOrCtrl+Alt+S"),
        )?)
        .item(&check(
            app,
            TYPEWRITER_ID,
            &tr.t("menu.view.typewriterMode"),
            Some("CmdOrCtrl+Alt+T"),
        )?)
        .item(&check(app, FOCUS_ID, &tr.t("menu.view.focusMode"), Some("CmdOrCtrl+Shift+J"))?)
        .separator()
        .item(&check(app, SIDEBAR_ID, &tr.t("menu.view.toggleSidebar"), Some("CmdOrCtrl+J"))?)
        .item(&check(app, TABBAR_ID, &tr.t("menu.view.toggleTabbar"), Some("CmdOrCtrl+Alt+B"))?)
        .item(&cmd(
            app,
            "view.toggle-toc",
            &tr.t("menu.view.toggleTableOfContents"),
            Some("CmdOrCtrl+K"),
        )?)
        .separator()
        .item(&cmd(app, "view.reload-images", &tr.t("menu.view.reloadImages"), Some("CmdOrCtrl+R"))?)
        .build()?;

    let window_menu = SubmenuBuilder::new(app, &tr.t("menu.window.window"))
        .item(&PredefinedMenuItem::minimize(app, Some(&tr.t("menu.window.minimize")))?)
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .item(&check(app, ALWAYS_ON_TOP_ID, &tr.t("menu.window.alwaysOnTop"), None)?)
        .separator()
        .item(&cmd(
            app,
            "window.toggle-full-screen",
            &tr.t("menu.window.fullScreen"),
            Some("Ctrl+CmdOrCtrl+F"),
        )?)
        .build()?;

    // Theme picker (radio-like check list, light/dark sections like Electron).
    // Items use the `theme:` prefix — handled in handle_menu_event by writing
    // the preference; checks re-sync via sync_pref_checks.
    let mut theme_builder = SubmenuBuilder::new(app, &tr.t("menu.theme.theme")).item(&check(
        app,
        THEME_FOLLOW_SYSTEM_ID,
        &tr.t("preferences.theme.followSystemTheme"),
        None,
    )?);
    theme_builder = theme_builder.separator().item(
        &MenuItemBuilder::new(tr.t("menu.theme.lightThemes"))
            .enabled(false)
            .build(app)?,
    );
    for (id, key) in LIGHT_THEMES {
        theme_builder =
            theme_builder.item(&check(app, &format!("{THEME_PREFIX}{id}"), &tr.t(key), None)?);
    }
    theme_builder = theme_builder.separator().item(
        &MenuItemBuilder::new(tr.t("menu.theme.darkThemes"))
            .enabled(false)
            .build(app)?,
    );
    for (id, key) in DARK_THEMES {
        theme_builder =
            theme_builder.item(&check(app, &format!("{THEME_PREFIX}{id}"), &tr.t(key), None)?);
    }
    let theme_menu = theme_builder.build()?;

    // Help: external links (opened Rust-side via the opener plugin).
    let mut help_builder = SubmenuBuilder::new(app, &tr.t("menu.help.help"));
    for (i, (id, key, _url)) in HELP_LINKS.iter().enumerate() {
        // Group like Electron: reference/changelog | follow/support | community/license
        if i == 2 || i == 4 {
            help_builder = help_builder.separator();
        }
        help_builder = help_builder.item(&MenuItemBuilder::with_id(*id, tr.t(key)).build(app)?);
    }
    let help_menu = help_builder.build()?;

    let menu = MenuBuilder::new(app)
        .items(&[
            &app_menu,
            &file_menu,
            &edit_menu,
            &paragraph_menu,
            &format_menu,
            &window_menu,
            &theme_menu,
            &view_menu,
            &help_menu,
        ])
        .build()?;

    // Initial pref-backed check state (theme radio + auto save).
    sync_pref_checks_from_store(app);

    Ok(menu)
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
    if id == CHECK_UPDATES_ID {
        if let Some(window) = focused_window(app) {
            crate::commands::updater::check_for_updates(app, &window);
        }
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
    // Help links open externally — no renderer involvement.
    if id.starts_with(HELP_PREFIX) {
        if let Some((_, _, url)) = HELP_LINKS.iter().find(|(item_id, _, _)| *item_id == id) {
            let _ = tauri_plugin_opener::open_url(*url, None::<String>);
        }
        return;
    }
    // Theme entries write the preference directly (theme names are not
    // renderer command ids); the pref broadcast applies it in every window
    // and sync_pref_checks refreshes the radio state.
    if id == THEME_FOLLOW_SYSTEM_ID {
        let current = read_pref_bool(app, "followSystemTheme");
        let mut change = serde_json::Map::new();
        change.insert("followSystemTheme".into(), serde_json::Value::Bool(!current));
        let _ = crate::commands::preferences::set_items_internal(app, change);
        return;
    }
    if let Some(theme_id) = id.strip_prefix(THEME_PREFIX) {
        let mut change = serde_json::Map::new();
        change.insert("theme".into(), serde_json::Value::String(theme_id.to_string()));
        let _ = crate::commands::preferences::set_items_internal(app, change);
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

/// `mt::editor-selection-changed` — check the Paragraph-menu entry matching
/// the selection's block type (Electron's updateSelectionMenus checked-state;
/// the enable/disable matrix for code/table/multiline selections is not
/// ported yet).
#[tauri::command]
pub fn menu_update_paragraph(app: AppHandle, state: serde_json::Value) {
    let menu_state = app.state::<MenuState>();
    let affiliation = state.get("affiliation").and_then(|v| v.as_object());
    for (menu_id, tag) in PARAGRAPH_MAP {
        let checked = affiliation
            .and_then(|aff| aff.get(*tag))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        menu_state.set(menu_id, checked);
    }
    let flag = |key: &str| state.get(key).and_then(serde_json::Value::as_bool).unwrap_or(false);
    menu_state.set("paragraph.task-list", flag("isTaskList"));
    menu_state.set("paragraph.loose-list-item", flag("isLooseListItem"));
}

/// Set a registered check item from other modules (e.g. always-on-top).
pub fn set_check(app: &AppHandle, id: &str, checked: bool) {
    app.state::<MenuState>().set(id, checked);
}

fn read_pref_bool(app: &AppHandle, key: &str) -> bool {
    use tauri_plugin_store::StoreExt;
    app.store("preferences.json")
        .ok()
        .and_then(|s| s.get(key))
        .as_ref()
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn read_pref_string(app: &AppHandle, key: &str) -> Option<String> {
    use tauri_plugin_store::StoreExt;
    app.store("preferences.json")
        .ok()
        .and_then(|s| s.get(key))
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// Refresh the pref-backed checks (theme radio list, follow-system, auto
/// save) from the store. Called after build_menu and from the preferences
/// write path whenever one of those keys changes.
pub fn sync_pref_checks_from_store(app: &AppHandle) {
    let state = app.state::<MenuState>();
    let theme = read_pref_string(app, "theme").unwrap_or_default();
    for (id, _) in LIGHT_THEMES.iter().chain(DARK_THEMES.iter()) {
        state.set(&format!("{THEME_PREFIX}{id}"), *id == theme);
    }
    state.set(THEME_FOLLOW_SYSTEM_ID, read_pref_bool(app, "followSystemTheme"));
    state.set(AUTOSAVE_ID, read_pref_bool(app, "autoSave"));
}

/// Hook for the preferences write path: re-sync menu checks when a relevant
/// key was changed.
pub fn on_preferences_changed(app: &AppHandle, changed: &serde_json::Map<String, serde_json::Value>) {
    if changed.contains_key("theme")
        || changed.contains_key("followSystemTheme")
        || changed.contains_key("autoSave")
    {
        sync_pref_checks_from_store(app);
    }
}

/// `mt::view-layout-changed` — reflect View-menu toggles (sidebar/tabbar and
/// the editing modes). Mirrors Electron's viewLayoutChanged: while
/// source-code mode is on, the typewriter/focus toggles are disabled.
#[tauri::command]
pub fn menu_update_view(app: AppHandle, changes: HashMap<String, serde_json::Value>) {
    let state = app.state::<MenuState>();
    for (key, value) in &changes {
        let v = value.as_bool().unwrap_or(false);
        match key.as_str() {
            "showSideBar" => state.set(SIDEBAR_ID, v),
            "showTabBar" => state.set(TABBAR_ID, v),
            "sourceCode" => {
                state.set(SOURCE_CODE_ID, v);
                state.set_enabled(TYPEWRITER_ID, !v);
                state.set_enabled(FOCUS_ID, !v);
            }
            "typewriter" => state.set(TYPEWRITER_ID, v),
            "focus" => state.set(FOCUS_ID, v),
            _ => {}
        }
    }
}
