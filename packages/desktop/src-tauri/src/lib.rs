mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            // boot info
            commands::boot_info::boot_info,
            // fs
            commands::fs::fs_is_file,
            commands::fs::fs_is_directory,
            commands::fs::fs_path_exists,
            commands::fs::fs_empty_dir,
            commands::fs::fs_ensure_dir,
            commands::fs::fs_copy,
            commands::fs::fs_move,
            commands::fs::fs_output_file,
            commands::fs::fs_write_file,
            commands::fs::fs_read_file,
            commands::fs::fs_unlink,
            commands::fs::fs_readdir,
            commands::fs::fs_stat,
            commands::fs::fs_is_executable,
            commands::fs::fs_trash_item,
            // shell
            commands::shell::shell_open_external,
            commands::shell::shell_open_path,
            commands::shell::shell_show_item,
            // clipboard
            commands::clipboard::clipboard_read_text,
            commands::clipboard::clipboard_write_text,
            commands::clipboard::clipboard_guess_file_path,
            // paths
            commands::paths::paths_is_image,
            commands::paths::paths_is_same,
        ])
        .run(tauri::generate_context!())
        .expect("error while running marktext")
}
