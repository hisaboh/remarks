//! Filesystem commands — Tauri-native port of `src/main/ipc/fs.ts`.
//!
//! Renderer channel → command mapping (resolved in the Phase 2 platform shim):
//!   mt::fs::is-file       → fs_is_file
//!   mt::fs::is-directory  → fs_is_directory
//!   mt::fs::empty-dir     → fs_empty_dir
//!   mt::fs::copy          → fs_copy
//!   mt::fs::ensure-dir    → fs_ensure_dir
//!   mt::fs::output-file   → fs_output_file
//!   mt::fs::move          → fs_move
//!   mt::fs::stat          → fs_stat
//!   mt::fs::write-file    → fs_write_file
//!   mt::fs::read-file     → fs_read_file
//!   mt::fs::path-exists   → fs_path_exists
//!   mt::fs::unlink        → fs_unlink
//!   mt::fs::readdir       → fs_readdir
//!   mt::fs::is-executable → fs_is_executable
//!   mt::fs-trash-item     → fs_trash_item

use serde::Serialize;
use std::path::Path;

/// Mirrors the `SerializedStat` shape in `src/shared/types/files.ts`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedStat {
    size: u64,
    mtime_ms: f64,
    ctime_ms: f64,
    is_file: bool,
    is_directory: bool,
    is_symbolic_link: bool,
}

/// Read payload — either decoded text (when an encoding was requested) or raw
/// bytes (Buffer-equivalent). The Phase 2 shim adapts this back to the
/// `string | Uint8Array` contract in ipc.ts.
#[derive(Serialize)]
#[serde(untagged)]
pub enum ReadResult {
    Text(String),
    Bytes(Vec<u8>),
}

/// Write payload — the renderer sends either a string or a byte array.
#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum WriteData {
    Text(String),
    Bytes(Vec<u8>),
}

impl WriteData {
    fn as_bytes(&self) -> Vec<u8> {
        match self {
            WriteData::Text(s) => s.clone().into_bytes(),
            WriteData::Bytes(b) => b.clone(),
        }
    }
}

fn to_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
pub fn fs_is_file(path: String) -> bool {
    Path::new(&path).is_file()
}

#[tauri::command]
pub fn fs_is_directory(path: String) -> bool {
    Path::new(&path).is_dir()
}

#[tauri::command]
pub fn fs_path_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
pub fn fs_empty_dir(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if p.exists() {
        for entry in std::fs::read_dir(p).map_err(to_err)? {
            let entry = entry.map_err(to_err)?;
            let child = entry.path();
            if child.is_dir() {
                std::fs::remove_dir_all(&child).map_err(to_err)?;
            } else {
                std::fs::remove_file(&child).map_err(to_err)?;
            }
        }
        Ok(())
    } else {
        std::fs::create_dir_all(p).map_err(to_err)
    }
}

#[tauri::command]
pub fn fs_ensure_dir(path: String) -> Result<(), String> {
    std::fs::create_dir_all(&path).map_err(to_err)
}

#[tauri::command]
pub fn fs_copy(src: String, dest: String) -> Result<(), String> {
    let from = Path::new(&src);
    if from.is_dir() {
        let mut opts = fs_extra::dir::CopyOptions::new();
        opts.overwrite = true;
        opts.copy_inside = true;
        // copy_inside copies the *contents* of src into dest, matching
        // fs-extra's `copy(src, dest)` directory semantics.
        fs_extra::dir::copy(from, &dest, &opts).map_err(to_err)?;
        Ok(())
    } else {
        if let Some(parent) = Path::new(&dest).parent() {
            std::fs::create_dir_all(parent).map_err(to_err)?;
        }
        std::fs::copy(&src, &dest).map_err(to_err)?;
        Ok(())
    }
}

#[tauri::command]
pub fn fs_move(src: String, dest: String) -> Result<(), String> {
    // fs-extra move with `overwrite: false` — fail if the destination exists.
    if Path::new(&dest).exists() {
        return Err(format!("dest already exists: {dest}"));
    }
    if let Some(parent) = Path::new(&dest).parent() {
        std::fs::create_dir_all(parent).map_err(to_err)?;
    }
    // Try a cheap rename first; fall back to copy+remove across devices.
    match std::fs::rename(&src, &dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            let from = Path::new(&src);
            if from.is_dir() {
                let opts = fs_extra::dir::CopyOptions::new();
                fs_extra::dir::move_dir(from, &dest, &opts)
                    .map(|_| ())
                    .map_err(to_err)
            } else {
                std::fs::copy(&src, &dest).map_err(to_err)?;
                std::fs::remove_file(&src).map_err(to_err)
            }
        }
    }
}

#[tauri::command]
pub fn fs_output_file(path: String, data: WriteData) -> Result<(), String> {
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent).map_err(to_err)?;
    }
    std::fs::write(&path, data.as_bytes()).map_err(to_err)
}

#[tauri::command]
pub fn fs_write_file(path: String, data: WriteData) -> Result<(), String> {
    std::fs::write(&path, data.as_bytes()).map_err(to_err)
}

#[tauri::command]
pub fn fs_read_file(path: String, encoding: Option<String>) -> Result<ReadResult, String> {
    let bytes = std::fs::read(&path).map_err(to_err)?;
    match encoding.as_deref() {
        // Electron returns a string only when an encoding is supplied; the
        // renderer overwhelmingly uses utf8. Any other encoding falls back to
        // raw bytes for the shim to decode.
        Some("utf8") | Some("utf-8") => {
            String::from_utf8(bytes).map(ReadResult::Text).map_err(to_err)
        }
        _ => Ok(ReadResult::Bytes(bytes)),
    }
}

#[tauri::command]
pub fn fs_unlink(path: String) -> Result<(), String> {
    std::fs::remove_file(&path).map_err(to_err)
}

#[tauri::command]
pub fn fs_readdir(path: String) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&path).map_err(to_err)? {
        let entry = entry.map_err(to_err)?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

#[tauri::command]
pub fn fs_stat(path: String) -> Result<SerializedStat, String> {
    let meta = std::fs::metadata(&path).map_err(to_err)?;
    let sym = std::fs::symlink_metadata(&path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    let to_ms = |t: std::io::Result<std::time::SystemTime>| -> f64 {
        t.ok()
            .and_then(|st| st.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    };
    Ok(SerializedStat {
        size: meta.len(),
        mtime_ms: to_ms(meta.modified()),
        ctime_ms: to_ms(meta.created()),
        is_file: meta.is_file(),
        is_directory: meta.is_dir(),
        is_symbolic_link: sym,
    })
}

#[tauri::command]
pub fn fs_is_executable(path: String) -> bool {
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    #[cfg(windows)]
    {
        meta.is_file()
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && (meta.permissions().mode() & 0o111) != 0
    }
}

#[tauri::command]
pub fn fs_trash_item(path: String) -> Result<(), String> {
    trash::delete(&path).map_err(to_err)
}
