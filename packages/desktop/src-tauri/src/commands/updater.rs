//! Auto-updater (Phase 6) — ports the Electron flow in
//! main/menu/actions/marktext.ts onto tauri-plugin-updater.
//!
//! Contract with the renderer (store/autoUpdates.ts):
//! - renderer sends `mt::check-for-update` → we check the configured endpoint
//! - found       → emit `mt::UPDATE_AVAILABLE` (notification with confirm)
//! - none        → emit `mt::UPDATE_NOT_AVAILABLE`
//! - failure     → emit `mt::UPDATE_ERROR`
//! - renderer answers `mt::NEED_UPDATE` { needUpdate } → download + install,
//!   emit `mt::UPDATE_DOWNLOADED`, then restart into the new version.

use std::sync::Mutex;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct UpdaterState {
    /// One check/download at a time (mirrors Electron's `runningUpdate`).
    running: Mutex<bool>,
    /// The update found by the last check, awaiting the user's confirmation.
    pending: Mutex<Option<Update>>,
    /// Label of the window that asked — status events go back to it.
    target: Mutex<Option<String>>,
}

fn notify(app: &AppHandle, event: &str, message: String) {
    let label = app
        .state::<UpdaterState>()
        .target
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| "main".to_string());
    let _ = app.emit_to(&label, event, message);
}

fn set_running(app: &AppHandle, value: bool) -> bool {
    let state = app.state::<UpdaterState>();
    let mut running = state.running.lock().unwrap();
    let was = *running;
    *running = value;
    was
}

/// Check the update endpoint and report back to `window`. Shared by the
/// `mt::check-for-update` IPC channel and the native menu item.
pub fn check_for_updates(app: &AppHandle, window: &WebviewWindow) {
    if set_running(app, true) {
        return; // a check or download is already in flight
    }
    {
        let state = app.state::<UpdaterState>();
        *state.target.lock().unwrap() = Some(window.label().to_string());
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let updater = match app.updater() {
            Ok(updater) => updater,
            Err(e) => {
                notify(&app, "mt::UPDATE_ERROR", format!("Error: {e}"));
                set_running(&app, false);
                return;
            }
        };
        match updater.check().await {
            Ok(Some(update)) => {
                *app.state::<UpdaterState>().pending.lock().unwrap() = Some(update);
                notify(
                    &app,
                    "mt::UPDATE_AVAILABLE",
                    "Found an update, do you want download and install now?".to_string(),
                );
                // Like Electron: the gate reopens while the user decides; the
                // pending update is consumed by updater_need_update.
                set_running(&app, false);
            }
            Ok(None) => {
                notify(
                    &app,
                    "mt::UPDATE_NOT_AVAILABLE",
                    "Current version is up-to-date.".to_string(),
                );
                set_running(&app, false);
            }
            Err(e) => {
                notify(&app, "mt::UPDATE_ERROR", format!("Error: {e}"));
                set_running(&app, false);
            }
        }
    });
}

#[tauri::command]
pub fn updater_check(app: AppHandle, window: WebviewWindow) {
    check_for_updates(&app, &window);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedUpdatePayload {
    need_update: bool,
}

/// `mt::NEED_UPDATE` — the user's answer to the UPDATE_AVAILABLE notification.
#[tauri::command]
pub fn updater_need_update(app: AppHandle, payload: NeedUpdatePayload) {
    let pending = app.state::<UpdaterState>().pending.lock().unwrap().take();
    if !payload.need_update {
        return;
    }
    let Some(update) = pending else {
        return;
    };
    if set_running(&app, true) {
        return;
    }

    tauri::async_runtime::spawn(async move {
        match update.download_and_install(|_, _| {}, || {}).await {
            Ok(()) => {
                notify(
                    &app,
                    "mt::UPDATE_DOWNLOADED",
                    "Update downloaded, application will be quit for update...".to_string(),
                );
                // Give the renderer a moment to show the notification, then
                // swap to the new version (Electron: quitAndInstall).
                std::thread::sleep(std::time::Duration::from_millis(1500));
                app.restart();
            }
            Err(e) => {
                notify(&app, "mt::UPDATE_ERROR", format!("Error: {e}"));
                set_running(&app, false);
            }
        }
    });
}
