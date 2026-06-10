//! System font enumeration — port of `main/ipc/fonts.ts` (the `font-list` npm
//! module). Returns the available font family names for the preferences font
//! autocomplete (`mt::fonts::list`). macOS uses Core Text directly; other
//! platforms return an empty list (mac-first migration scope).

/// Sorted, de-duplicated list of installed font family names. Hidden system
/// families (names starting with `.`, e.g. ".SF NS") are filtered out to match
/// what `font-list` surfaced.
#[tauri::command]
pub fn fonts_list() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let mut names: Vec<String> = core_text::font_manager::copy_available_font_family_names()
            .iter()
            .map(|name| name.to_string())
            .filter(|name| !name.starts_with('.'))
            .collect();
        names.sort();
        names.dedup();
        names
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}
