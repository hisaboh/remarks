//! Path predicate commands — Tauri-native port of `src/main/ipc/paths.ts`
//! (backed by `common/filesystem/paths.ts`).
//!
//!   mt::paths::is-image     → paths_is_image
//!   mt::paths::is-same-sync → paths_is_same

use std::path::Path;

const IMAGE_EXTENSIONS: &[&str] = &["jpeg", "jpg", "png", "gif", "svg", "webp"];

#[tauri::command]
pub fn paths_is_image(path: String) -> bool {
    let p = Path::new(&path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext {
        Some(ext) => IMAGE_EXTENSIONS.contains(&ext.as_str()) && p.is_file(),
        None => false,
    }
}

#[tauri::command]
pub fn paths_is_same(a: String, b: String) -> bool {
    // Mirror isSamePathSync: compare canonicalized paths when both resolve,
    // otherwise fall back to a normalized string comparison (case-insensitive
    // on macOS/Windows).
    if let (Ok(ca), Ok(cb)) = (
        std::fs::canonicalize(&a),
        std::fs::canonicalize(&b),
    ) {
        return ca == cb;
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        a.eq_ignore_ascii_case(&b)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        a == b
    }
}
