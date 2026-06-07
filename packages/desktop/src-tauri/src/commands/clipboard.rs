//! Clipboard commands — Tauri-native port of the clipboard half of
//! `src/main/ipc/shell.ts`.
//!
//!   mt::clipboard::read-text       → clipboard_read_text
//!   mt::clipboard::write-text      → clipboard_write_text
//!   mt::clipboard::guess-file-path → clipboard_guess_file_path
//!
//! Text read/write go through the clipboard-manager plugin's Rust API.

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[tauri::command]
pub fn clipboard_read_text(app: AppHandle) -> String {
    app.clipboard().read_text().unwrap_or_default()
}

#[tauri::command]
pub fn clipboard_write_text(app: AppHandle, text: String) {
    let _ = app.clipboard().write_text(text);
}

/// Returns a file path if the clipboard currently holds a file reference
/// (drag-from-Finder / copy-in-Explorer), else `None`.
///
/// TODO(phase-3): the Electron version reads `NSFilenamesPboardType` (macOS)
/// and `FileNameW` (Windows). Faithful parity needs native pasteboard access
/// (objc2 on macOS); deferred to the native-dependency phase. Returning `None`
/// keeps the "paste image from file" affordance inert rather than wrong.
#[tauri::command]
pub fn clipboard_guess_file_path() -> Option<String> {
    None
}
