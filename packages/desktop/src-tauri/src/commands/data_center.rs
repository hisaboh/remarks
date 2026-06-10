//! Data center commands — Tauri-native port of `src/main/dataCenter/index.ts`.
//!
//!   mt::ask-for-user-data                 → data_center_get_all (shim replies mt::user-preference)
//!   mt::set-user-data                     → data_center_set_items
//!   set-image-folder-path                 → data_center_set_image_folder_path
//!   mt::ask-for-modify-image-folder-path  → data_center_modify_image_folder_path
//!   mt::ask-for-image-path                → data_center_ask_image_path
//!
//! Persistence is backed by tauri-plugin-store (`dataCenter.json`). The
//! Electron version routed a few keys through keytar, but `encryptKeys` is
//! empty today, so the encrypted path is a no-op and keytar is skipped here
//! (keytar → keyring is Phase 3). Changes broadcast as `mt::user-preference`.

use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_store::StoreExt;

pub const DATA_CENTER_FILE: &str = "dataCenter.json";

const IMAGE_EXTENSIONS: &[&str] = &["jpeg", "jpg", "png", "gif", "svg", "webp"];

fn to_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn user_data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path().app_data_dir().map_err(to_err)
}

/// Mirrors `DataCenter.init()` — seed defaults on first run, migrate legacy
/// uploader values otherwise. Run from the Tauri `setup` hook.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let store = app.store(DATA_CENTER_FILE).map_err(to_err)?;
    let user_data = user_data_dir(app)?;
    let image_folder = user_data.join("images");
    let screenshot_folder = user_data.join("screenshot");

    // Seed any MISSING defaults (not just on a fully-empty first run): a store
    // migrated from the Electron install (commands/migration.rs) arrives
    // non-empty but without the folder-path keys — the migration drops paths
    // that pointed into the old Electron userData dir so they get re-created
    // here under the Tauri data dir.
    if !store.has("imageFolderPath") {
        store.set(
            "imageFolderPath",
            Value::String(image_folder.to_string_lossy().into_owned()),
        );
    }
    if !store.has("screenshotFolderPath") {
        store.set(
            "screenshotFolderPath",
            Value::String(screenshot_folder.to_string_lossy().into_owned()),
        );
        std::fs::create_dir_all(&screenshot_folder).map_err(to_err)?;
    }
    if !store.has("webImages") {
        store.set("webImages", Value::Array(vec![]));
    }
    if !store.has("cloudImages") {
        store.set("cloudImages", Value::Array(vec![]));
    }
    if !store.has("currentUploader") {
        store.set("currentUploader", Value::String("picgo".into()));
    } else if let Some(uploader) = store.get("currentUploader").as_ref().and_then(Value::as_str) {
        // Migrate uploader values that no longer exist.
        if uploader == "none" || uploader == "github" {
            store.set("currentUploader", Value::String("picgo".into()));
        }
    }

    store.save().map_err(to_err)
}

fn set_items_internal(app: &AppHandle, settings: &Map<String, Value>) -> Result<(), String> {
    let store = app.store(DATA_CENTER_FILE).map_err(to_err)?;
    for (key, value) in settings {
        if key == "screenshotFolderPath" {
            if let Some(path) = value.as_str() {
                std::fs::create_dir_all(path).map_err(to_err)?;
            }
        }
        store.set(key, value.clone());
    }
    store.save().map_err(to_err)?;
    let _ = app.emit("mt::user-preference", Value::Object(settings.clone()));
    Ok(())
}

#[tauri::command]
pub fn data_center_get_all(app: AppHandle) -> Result<Value, String> {
    let store = app.store(DATA_CENTER_FILE).map_err(to_err)?;
    let mut map = Map::new();
    for (key, value) in store.entries() {
        map.insert(key, value);
    }
    Ok(Value::Object(map))
}

#[tauri::command]
pub fn data_center_set_items(app: AppHandle, settings: Map<String, Value>) -> Result<(), String> {
    set_items_internal(&app, &settings)
}

#[tauri::command]
pub fn data_center_set_image_folder_path(app: AppHandle, path: String) -> Result<(), String> {
    let mut change = Map::new();
    change.insert("imageFolderPath".into(), Value::String(path));
    set_items_internal(&app, &change)
}

/// Prompt for a folder (when no path is given) and store it as the image folder.
/// Non-blocking dialog (sync commands run on the main thread — a blocking dialog
/// there deadlocks the UI).
#[tauri::command]
pub fn data_center_modify_image_folder_path(app: AppHandle, image_path: Option<String>) {
    if let Some(path) = image_path {
        let mut change = Map::new();
        change.insert("imageFolderPath".into(), Value::String(path));
        let _ = set_items_internal(&app, &change);
        return;
    }
    let app_cb = app.clone();
    app.dialog().file().pick_folder(move |folder| {
        if let Some(path) = folder.and_then(|fp| fp.into_path().ok()) {
            let mut change = Map::new();
            change.insert(
                "imageFolderPath".into(),
                Value::String(path.to_string_lossy().into_owned()),
            );
            let _ = set_items_internal(&app_cb, &change);
        }
    });
}

/// Prompt for an image file and return its path (empty string if cancelled).
/// `async` so it runs off the main thread; bridges the non-blocking dialog
/// callback back to a return value via a channel.
#[tauri::command]
pub async fn data_center_ask_image_path(app: AppHandle) -> String {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    app.dialog()
        .file()
        .add_filter("Images", IMAGE_EXTENSIONS)
        .pick_file(move |file| {
            let path = file
                .and_then(|fp| fp.into_path().ok())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let _ = tx.send(path);
        });
    tauri::async_runtime::spawn_blocking(move || rx.recv().unwrap_or_default())
        .await
        .unwrap_or_default()
}
