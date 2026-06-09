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

    store.save().map_err(to_err)
}

/// Shared write path — persists each entry and broadcasts the changed subset.
fn set_items_internal(app: &AppHandle, settings: &Map<String, Value>) -> Result<(), String> {
    let store = app.store(PREFERENCES_FILE).map_err(to_err)?;
    for (key, value) in settings {
        store.set(key, value.clone());
    }
    store.save().map_err(to_err)?;

    // The title bar style can't change live, so it's never pushed to renderers.
    let mut payload = settings.clone();
    payload.remove("titleBarStyle");
    if !payload.is_empty() {
        let _ = app.emit("mt::user-preference", Value::Object(payload));
    }
    // TODO(phase-4): main-side reactions to preference changes (menu rebuild,
    // native theme) — see app/index.ts and menu/index.ts in the Electron tree.
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
    set_items_internal(&app, &settings)
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
    set_items_internal(&app, &change)
}
