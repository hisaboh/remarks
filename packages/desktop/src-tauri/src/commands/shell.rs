//! Shell commands — Tauri-native port of the shell half of
//! `src/main/ipc/shell.ts`.
//!
//!   mt::shell::open-external → shell_open_external
//!   mt::shell::open-path     → shell_open_path
//!   mt::shell::show-item     → shell_show_item
//!
//! `open-external` / `open-path` delegate to the OS opener, and `show-item`
//! reveals the file in the platform file manager — all via the Rust API of
//! `tauri-plugin-opener` (the modern replacement for the deprecated
//! `shell.open`).

#[tauri::command]
pub fn shell_open_external(url: String) -> bool {
    tauri_plugin_opener::open_url(url, None::<String>).is_ok()
}

#[tauri::command]
pub fn shell_open_path(full_path: String) -> Result<String, String> {
    match tauri_plugin_opener::open_path(full_path, None::<String>) {
        // Electron's shell.openPath resolves to "" on success and an error
        // message string on failure.
        Ok(()) => Ok(String::new()),
        Err(e) => Ok(e.to_string()),
    }
}

#[tauri::command]
pub fn shell_show_item(full_path: String) -> Result<(), String> {
    tauri_plugin_opener::reveal_item_in_dir(full_path).map_err(|e| e.to_string())
}
