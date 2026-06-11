//! One-time import of Electron-era user data (Phase 6).
//!
//! The Electron build stored its userData at `<config_dir>/marktext`
//! (e.g. `~/Library/Application Support/marktext` on macOS) with
//! electron-store's plain-JSON-map files. On a fresh Tauri install we seed the
//! Tauri stores from there so existing users keep their settings.
//! `preferences::init` / `data_center::init` run right afterwards and
//! reconcile the imported keys against the current defaults (dropping stale
//! keys, adding new ones) — the same upgrade semantics an Electron update had.
//!
//! NOT migrated: session buffers (`editor-buffer*.json` — different
//! per-window format), `spellcheck.json` (macOS uses the native OS spell
//! checker), and recently-used documents (managed by the OS under Electron on
//! macOS, not a file).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

use super::data_center::DATA_CENTER_FILE;
use super::preferences::PREFERENCES_FILE;

/// Bundle identifier the app shipped under before the rename to
/// "Remarks on Markdown" (io.github.hisaboh.remarks). Its data dir is left
/// in place as a backup; we only copy out of it.
const OLD_TAURI_IDENTIFIER: &str = "app.marktext.marktext";

fn old_user_data_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().config_dir().ok()?.join("marktext");
    dir.is_dir().then_some(dir)
}

/// Recursively copy `src` into `dst`, skipping files that already exist at
/// the destination. Returns the number of files copied.
fn copy_dir_missing(src: &Path, dst: &Path) -> std::io::Result<usize> {
    let mut copied = 0usize;
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copied += copy_dir_missing(&entry.path(), &target)?;
        } else if !target.exists() {
            std::fs::copy(entry.path(), &target)?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// One-time import of the previous Tauri install's user data after the
/// identifier change (app.marktext.marktext → io.github.hisaboh.remarks).
/// The stores, keybindings.json, editor-buffer/ and recently-used documents
/// all live in the per-identifier config/data dirs, so a plain copy of any
/// files the new dirs don't have yet brings everything across; the
/// preferences/data-center inits reconcile the stores right afterwards.
/// Must run BEFORE `import_electron_data` so a migrated (non-empty) store
/// stops the Electron import from overwriting newer Tauri-era settings.
pub fn import_old_tauri_data(app: &AppHandle) {
    // config_dir and data_dir coincide on macOS but differ on Linux; handle
    // both pairs and dedupe.
    let pairs = [
        (
            app.path()
                .config_dir()
                .ok()
                .map(|d| d.join(OLD_TAURI_IDENTIFIER)),
            app.path().app_config_dir().ok(),
        ),
        (
            app.path()
                .data_dir()
                .ok()
                .map(|d| d.join(OLD_TAURI_IDENTIFIER)),
            app.path().app_data_dir().ok(),
        ),
    ];
    let mut done: Vec<PathBuf> = Vec::new();
    for (old_dir, new_dir) in pairs {
        let (Some(old_dir), Some(new_dir)) = (old_dir, new_dir) else {
            continue;
        };
        if !old_dir.is_dir() || old_dir == new_dir || done.contains(&old_dir) {
            continue;
        }
        match copy_dir_missing(&old_dir, &new_dir) {
            Ok(n) if n > 0 => {
                log::info!(
                    "migrated {n} files from the previous install at {}",
                    old_dir.display()
                );
            }
            Ok(_) => {}
            Err(e) => log::error!(
                "failed to migrate previous install data from {}: {e}",
                old_dir.display()
            ),
        }
        done.push(old_dir);
    }
}

fn read_map(path: &Path) -> Option<Map<String, Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()?
        .as_object()
        .cloned()
}

/// True for values that point into the old Electron userData directory —
/// e.g. the default `imageFolderPath` under `<userData>/images`. Those are
/// dropped so the init seeding recreates them under the Tauri data dir.
fn points_into(value: &Value, dir: &Path) -> bool {
    value
        .as_str()
        .is_some_and(|s| Path::new(s).starts_with(dir))
}

pub fn import_electron_data(app: &AppHandle) {
    let Some(old_dir) = old_user_data_dir(app) else {
        return;
    };

    for file in [PREFERENCES_FILE, DATA_CENTER_FILE] {
        let Ok(store) = app.store(file) else { continue };
        // Only a still-empty store is a true first run; anything else has
        // already been initialized (or migrated) by a previous Tauri launch.
        if !store.keys().is_empty() {
            continue;
        }
        let Some(map) = read_map(&old_dir.join(file)) else {
            continue;
        };
        let mut imported = 0usize;
        for (key, value) in map {
            if points_into(&value, &old_dir) {
                continue;
            }
            store.set(key, value);
            imported += 1;
        }
        if imported > 0 {
            log::info!("migrated {imported} {file} entries from the Electron install");
        }
    }

    // User keybindings are a plain file (commands/keybindings.rs), not a store.
    if let Ok(config_dir) = app.path().app_config_dir() {
        let target = config_dir.join("keybindings.json");
        let source = old_dir.join("keybindings.json");
        if !target.exists() && source.is_file() {
            let _ = std::fs::create_dir_all(&config_dir);
            if std::fs::copy(&source, &target).is_ok() {
                log::info!("migrated keybindings.json from the Electron install");
            }
        }
    }
}
