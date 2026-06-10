//! `mt::cmd::exists` — port of `main/ipc/cmd.ts` (the `command-exists` npm
//! module). Reports whether an executable is resolvable on PATH; used by the
//! image-uploader settings to detect `node` / `npm` / `picgo`.

/// Whether `name` resolves to an executable on PATH. As in the Electron handler,
/// `picgo` on macOS additionally probes the common npm/Homebrew install paths,
/// since a GUI app's PATH often omits `/usr/local/bin` and `/opt/homebrew/bin`.
#[tauri::command]
pub fn cmd_exists(name: String) -> bool {
    if which::which(&name).is_ok() {
        return true;
    }

    #[cfg(target_os = "macos")]
    if name == "picgo" {
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            "/usr/local/bin/picgo".to_string(),
            "/opt/homebrew/bin/picgo".to_string(),
            format!("{home}/.npm-global/bin/picgo"),
            format!("{home}/.npm/bin/picgo"),
            "/usr/local/lib/node_modules/.bin/picgo".to_string(),
        ];
        return candidates.iter().any(|p| std::path::Path::new(p).exists());
    }

    false
}
