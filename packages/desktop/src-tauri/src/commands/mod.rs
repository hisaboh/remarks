//! Tauri command surface — Phase 1 IPC redesign.
//!
//! Naming convention (decided 2026-06-07): commands use Tauri-native
//! `snake_case`; the renderer keeps its `mt::*` channel names and the Phase 2
//! `platform` shim maps each channel to the corresponding `invoke('…')` call.
//! The TypeScript contract in `src/shared/types/ipc.ts` stays the source of
//! truth for argument/return shapes.
//!
//! Window controls (minimize/maximize/close/fullscreen/is-maximized/
//! is-fullscreen) are intentionally NOT reimplemented here — they are covered
//! by Tauri's built-in `core:window:*` commands, which the shim calls through
//! `@tauri-apps/api/window`. Native context-menu popups
//! (`mt::menu::popup*`) need native menus and are deferred to the
//! multi-window/menu phase (Phase 4).

pub mod boot_info;
pub mod clipboard;
pub mod cmd;
pub mod context_menu;
pub mod data_center;
pub mod editor;
pub mod encoding;
pub mod files;
pub mod fonts;
pub mod fs;
pub mod keybindings;
pub mod paths;
pub mod preferences;
pub mod search;
pub mod shell;
pub mod watcher;
pub mod window;
