//! Preferences commands — Tauri-native port of `src/main/preferences/index.ts`.
//!
//!   mt::ask-for-user-preference → preferences_get_all (shim replies mt::user-preference)
//!   mt::set-user-preference     → preferences_set_items
//!   set-user-preference         → preferences_set_items
//!   mt::cmd-toggle-autosave     → preferences_toggle_autosave
//!
//! Persistence is backed by tauri-plugin-store (`preferences.json`), replacing
//! electron-store. The default settings are embedded from `static/preference.json`
//! at compile time. Changes are broadcast to every window via the
//! `mt::user-preference` event, collapsing Electron's internal
//! `broadcast-preferences-changed` → windowManager re-send hop.

use serde_json::{Map, Value};
use std::collections::HashSet;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

pub const PREFERENCES_FILE: &str = "preferences.json";

/// Default preferences, embedded from the renderer's static asset so we don't
/// depend on resource-dir bundling (deferred to Phase 6).
const DEFAULT_PREFERENCES: &str = include_str!("../../../static/preference.json");

fn to_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Map the OS locale (e.g. "ja-JP", "zh-Hans-CN") to one of the app's bundled
/// UI languages, mirroring Electron's `_initializeLanguage` (app/index.ts). Only
/// locales with a `static/locales/<lang>.json` file are returned; everything
/// else (including unmatched system locales) falls back to "en".
fn detect_system_language() -> String {
    let Some(locale) = sys_locale::get_locale() else {
        return "en".into();
    };
    let lower = locale.to_lowercase();
    // Region-specific Chinese variants first (order matters before the bare `zh`).
    let lang = if lower.starts_with("zh-tw") || lower.starts_with("zh-hant") || lower.starts_with("zh-hk") {
        "zh-TW"
    } else if lower.starts_with("zh") {
        "zh-CN"
    } else {
        match lower.split(['-', '_']).next().unwrap_or("") {
            "ja" => "ja",
            "ko" => "ko",
            "fr" => "fr",
            "de" => "de",
            "es" => "es",
            "pt" => "pt",
            _ => "en",
        }
    };
    lang.into()
}

/// Whether the OS is currently in dark mode, read from the main window's theme
/// (config-defined windows exist by the time `setup`/`init` runs). Replaces
/// Electron's `nativeTheme.shouldUseDarkColors`; defaults to light if the window
/// or theme can't be resolved.
fn system_is_dark(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.theme().ok())
        .map(|t| t == tauri::Theme::Dark)
        .unwrap_or(false)
}

/// Theme ids that are dark — ported from common/theme.ts
/// (`railscastsThemes` + `oneDarkThemes`). Used to tell whether the currently
/// selected `theme` matches the OS appearance.
const DARK_THEME_IDS: &[&str] = &[
    "dark", "material-dark", "dracula", "nord", "catppuccin-mocha", "gruvbox-dark",
    "tokyo-night", "tokyo-night-storm", "solarized-dark", "ayu-dark", "ayu-mirage",
    "everforest-dark", "rose-pine", "rose-pine-moon", "monokai-pro", "synthwave-84",
    "horizon-dark", "palenight", "oxocarbon-dark", "kanagawa", "nightfox", "cyberdream",
    "one-dark",
];

fn is_dark_theme_id(theme: &str) -> bool {
    DARK_THEME_IDS.contains(&theme)
}

/// Read a string pref from the store, falling back to `default`.
fn stored_str(app: &AppHandle, key: &str, default: &str) -> String {
    app.store(PREFERENCES_FILE)
        .ok()
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| default.to_owned())
}

fn stored_bool(app: &AppHandle, key: &str, default: bool) -> bool {
    app.store(PREFERENCES_FILE)
        .ok()
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

/// When following the system theme, the concrete theme to use right now.
fn follow_target(app: &AppHandle, light: &str, dark: &str) -> String {
    if system_is_dark(app) { dark.to_owned() } else { light.to_owned() }
}

/// Mirror Electron's `broadcast-preferences-changed` theme handling: when the
/// user enables followSystemTheme, or edits the light/dark-mode theme while
/// following the system, recompute `theme` to match the OS and fold it into the
/// same write+broadcast (`settings`).
fn apply_theme_reactions(app: &AppHandle, settings: &mut Map<String, Value>) {
    let enabling_follow =
        settings.get("followSystemTheme").and_then(Value::as_bool) == Some(true);
    let following = stored_bool(app, "followSystemTheme", false);
    let mode_changed =
        settings.contains_key("lightModeTheme") || settings.contains_key("darkModeTheme");

    if !(enabling_follow || (following && mode_changed)) {
        return;
    }
    // Prefer the incoming light/dark-mode values over the persisted ones.
    let pick = |key: &str, default: &str| -> String {
        settings
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| stored_str(app, key, default))
    };
    let (light, dark) = (pick("lightModeTheme", "light"), pick("darkModeTheme", "dark"));
    let target = follow_target(app, &light, &dark);
    settings.insert("theme".to_owned(), Value::String(target));
}

/// React to an OS appearance change (Tauri `WindowEvent::ThemeChanged`): if
/// following the system theme, switch `theme` to the matching light/dark theme.
/// Ports Electron's `nativeTheme.on('updated')` handler. The dedup (target ==
/// current) keeps the per-window event from re-broadcasting once it's applied.
pub fn on_system_theme_changed(app: &AppHandle, is_dark: bool) {
    if !stored_bool(app, "followSystemTheme", false) {
        return;
    }
    let light = stored_str(app, "lightModeTheme", "light");
    let dark = stored_str(app, "darkModeTheme", "dark");
    let target = if is_dark { dark } else { light };
    if target != stored_str(app, "theme", "light") {
        let mut change = Map::new();
        change.insert("theme".to_owned(), Value::String(target));
        let _ = set_items_internal(app, change);
    }
}

/// Populate defaults / reconcile against the embedded default set. Mirrors
/// `Preference.init()` — run from the Tauri `setup` hook before the renderer
/// asks for preferences.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let store = app.store(PREFERENCES_FILE).map_err(to_err)?;
    let defaults: Map<String, Value> =
        serde_json::from_str(DEFAULT_PREFERENCES).map_err(to_err)?;

    // A fresh store has no keys — equivalent to "preferences file does not exist".
    let first_run = store.keys().is_empty();

    if first_run {
        for (key, value) in &defaults {
            store.set(key, value.clone());
        }
        // First-run: pick the UI language from the OS locale (the default file
        // ships "en"; override only when the system locale maps to another
        // bundled catalog). Mirrors Electron's _initializeLanguage.
        let lang = detect_system_language();
        if lang != "en" {
            store.set("language", Value::String(lang));
        }
        // First-run: start in the dark theme when the OS is in dark mode (the
        // default file ships "light"). Mirrors Electron's init.
        if system_is_dark(app) {
            store.set("theme", Value::String("dark".into()));
        }
    } else {
        // Remove outdated settings no longer present in the defaults.
        let default_keys: HashSet<&String> = defaults.keys().collect();
        for key in store.keys() {
            if !default_keys.contains(&key) {
                store.delete(&key);
            }
        }
        // Add newly introduced default entries.
        for (key, value) in &defaults {
            if !store.has(key) {
                store.set(key, value.clone());
            }
        }
        // Migration 0.18.6: startUpAction "lastState" → "openLastFolder".
        if store.get("startUpAction").as_ref().and_then(Value::as_str) == Some("lastState") {
            store.set("startUpAction", Value::String("openLastFolder".into()));
        }
    }

    // Startup: when following the system theme, re-sync `theme` to the current OS
    // appearance (mirrors Electron's ready()). No broadcast — the renderer reads
    // the value on bootstrap.
    if store
        .get("followSystemTheme")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let current = store
            .get("theme")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "light".into());
        let sys_dark = system_is_dark(app);
        if is_dark_theme_id(&current) != sys_dark {
            let light = store
                .get("lightModeTheme")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| "light".into());
            let dark = store
                .get("darkModeTheme")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| "dark".into());
            let target = if sys_dark { dark } else { light };
            if target != current {
                store.set("theme", Value::String(target));
            }
        }
    }

    store.save().map_err(to_err)
}

/// Shared write path — persists each entry and broadcasts the changed subset.
pub(crate) fn set_items_internal(
    app: &AppHandle,
    mut settings: Map<String, Value>,
) -> Result<(), String> {
    // React to theme-related changes before persisting so the recomputed `theme`
    // is written and broadcast in the same pass (mirrors Electron's
    // broadcast-preferences-changed handler).
    apply_theme_reactions(app, &mut settings);

    let store = app.store(PREFERENCES_FILE).map_err(to_err)?;
    for (key, value) in &settings {
        store.set(key, value.clone());
    }
    store.save().map_err(to_err)?;

    // Re-sync pref-backed menu checks (theme radio / follow-system / autosave).
    crate::menu::on_preferences_changed(app, &settings);

    // Rebuild the native menu so its labels pick up the new UI language (4j).
    // Sync command → already on the main thread, safe for macOS menu mutation.
    let language_changed = settings.contains_key("language");

    // The title bar style can't change live, so it's never pushed to renderers.
    settings.remove("titleBarStyle");
    if !settings.is_empty() {
        let _ = app.emit("mt::user-preference", Value::Object(settings));
    }
    if language_changed {
        crate::menu::rebuild_menu(app);
    }
    Ok(())
}

#[tauri::command]
pub fn preferences_get_all(app: AppHandle) -> Result<Value, String> {
    let store = app.store(PREFERENCES_FILE).map_err(to_err)?;
    let mut map = Map::new();
    for (key, value) in store.entries() {
        map.insert(key, value);
    }
    Ok(Value::Object(map))
}

#[tauri::command]
pub fn preferences_set_items(app: AppHandle, settings: Map<String, Value>) -> Result<(), String> {
    set_items_internal(&app, settings)
}

#[tauri::command]
pub fn preferences_toggle_autosave(app: AppHandle) -> Result<(), String> {
    let store = app.store(PREFERENCES_FILE).map_err(to_err)?;
    let current = store
        .get("autoSave")
        .as_ref()
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut change = Map::new();
    change.insert("autoSave".into(), Value::Bool(!current));
    set_items_internal(&app, change)
}
