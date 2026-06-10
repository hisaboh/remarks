//! Boot info command — Tauri-native port of `src/main/ipc/bootInfo.ts`.
//!
//!   mt::boot-info / mt::boot-info-async → boot_info
//!
//! The Electron version exposes both a sync and an async channel; Tauri
//! `invoke` is always async, so the Phase 2 shim resolves the renderer's sync
//! `mt::boot-info` against a value cached at bootstrap time.

use serde::Serialize;
use std::collections::HashMap;
use tauri::{AppHandle, Manager};

const ENV_ALLOWLIST: &[&str] = &[
    "NODE_ENV",
    "PERF_TESTING",
    "APPIMAGE",
    "MARKTEXT_VERSION",
    "MARKTEXT_VERSION_STRING",
    "MARKTEXT_RIPGREP_PATH",
    "PATH",
    "HOME",
];

const MARKDOWN_EXTENSIONS: &[&str] = &[
    "markdown", "mdown", "mkdn", "md", "mkd", "mdwn", "mdtxt", "mdtext", "mdx", "text", "txt",
];

#[derive(Serialize)]
struct BootPaths {
    resources: String,
    #[serde(rename = "userData")]
    user_data: String,
    cwd: String,
    #[serde(rename = "ripgrepBinary")]
    ripgrep_binary: String,
}

#[derive(Serialize)]
pub struct BootInfo {
    platform: String,
    arch: String,
    versions: HashMap<String, String>,
    env: HashMap<String, String>,
    paths: BootPaths,
    #[serde(rename = "isUpdatable")]
    is_updatable: bool,
    #[serde(rename = "MARKDOWN_INCLUSIONS")]
    markdown_inclusions: Vec<String>,
}

/// Map Rust's OS/arch identifiers onto the Node.js values the renderer expects
/// (`process.platform` / `process.arch`).
fn node_platform() -> String {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
    .to_string()
}

fn node_arch() -> String {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    }
    .to_string()
}

fn pick_env() -> HashMap<String, String> {
    let mut out = HashMap::new();
    for key in ENV_ALLOWLIST {
        if let Ok(value) = std::env::var(key) {
            out.insert((*key).to_string(), value);
        }
    }
    out
}

#[tauri::command]
pub fn boot_info(app: AppHandle) -> BootInfo {
    let path = app.path();
    let resources = path
        .resource_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let user_data = path
        .app_data_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut versions = HashMap::new();
    versions.insert("app".to_string(), app.package_info().version.to_string());
    versions.insert("tauri".to_string(), tauri::VERSION.to_string());

    BootInfo {
        platform: node_platform(),
        arch: node_arch(),
        versions,
        env: pick_env(),
        paths: BootPaths {
            resources,
            user_data,
            cwd,
            // TODO(phase-3): bundle ripgrep and resolve its bundled path.
            ripgrep_binary: std::env::var("MARKTEXT_RIPGREP_PATH").unwrap_or_default(),
        },
        // Updater (Phase 6): release builds carry the tauri-plugin-updater
        // config (pubkey + endpoint); dev builds aren't installed bundles, so
        // an in-place update can't apply.
        is_updatable: !cfg!(debug_assertions),
        markdown_inclusions: MARKDOWN_EXTENSIONS.iter().map(|e| format!("*.{e}")).collect(),
    }
}
