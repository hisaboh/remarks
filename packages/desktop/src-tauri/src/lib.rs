mod commands;

use tauri::{Emitter, Manager, WindowEvent};

use commands::window::WindowRegistry;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .on_window_event(|window, event| {
            // Intercept close to run the unsaved-changes flow. Once the renderer
            // confirms (window_close / window_close_confirm marks the label),
            // we let the close through instead of re-prompting.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let registry = window.app_handle().state::<WindowRegistry>();
                if !registry.take_closing(window.label()) {
                    api.prevent_close();
                    let _ = window.emit_to(window.label(), "mt::ask-for-close", ());
                }
            }
        })
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .manage(commands::window::WindowRegistry::default())
        .setup(|app| {
            // Seed/reconcile persisted settings before the renderer asks for them.
            let handle = app.handle();
            if let Err(e) = commands::preferences::init(handle) {
                log::error!("preferences init failed: {e}");
            }
            if let Err(e) = commands::data_center::init(handle) {
                log::error!("data center init failed: {e}");
            }
            Ok(())
        })
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
            // encoding (Phase 3: ced/iconv replacement)
            commands::encoding::fs_read_text_auto,
            // editor bootstrap (Phase 4)
            commands::editor::editor_bootstrap_config,
            // file open / save (Phase 4)
            commands::files::file_open,
            commands::files::file_open_path,
            commands::files::file_save,
            commands::files::file_save_as,
            // multi-window (Phase 4)
            commands::window::window_init_args,
            commands::window::window_create,
            commands::window::window_request_close,
            commands::window::window_close,
            commands::window::window_close_confirm,
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
            // preferences
            commands::preferences::preferences_get_all,
            commands::preferences::preferences_set_items,
            commands::preferences::preferences_toggle_autosave,
            // data center
            commands::data_center::data_center_get_all,
            commands::data_center::data_center_set_items,
            commands::data_center::data_center_set_image_folder_path,
            commands::data_center::data_center_modify_image_folder_path,
            commands::data_center::data_center_ask_image_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running marktext")
}
