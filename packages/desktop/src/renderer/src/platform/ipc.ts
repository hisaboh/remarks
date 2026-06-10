// Tauri IPC shim — reproduces the Electron `window.electron.ipcRenderer`
// surface (send / sendSync / invoke / on / once / removeAllListeners) on top of
// Tauri's `invoke` + event system.
//
// Phase 1 decided the Rust commands use snake_case names; the renderer keeps
// its `mt::*` channel names. The maps below translate each channel into the
// corresponding command plus the camelCase argument keys Tauri expects (Tauri
// converts camelCase JS keys → snake_case Rust params automatically).

import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { listen, emit, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { BootInfo } from '@shared/types/ipc'
import { getDefaultKeybindingMap, setUserKeybindings } from '../keybinding'

interface CmdSpec {
  command: string
  // Positional-argument names, in order, mapped onto a named args object.
  params?: string[]
  // Constant args merged into every call (for channels that carry no args but
  // map to a command needing a discriminator, e.g. window kind).
  fixed?: Record<string, unknown>
}

// renderer → main, Promise-returning channels.
const INVOKE_MAP: Record<string, CmdSpec> = {
  'mt::boot-info-async': { command: 'boot_info' },
  'mt::fonts::list': { command: 'fonts_list' },
  'mt::cmd::exists': { command: 'cmd_exists', params: ['name'] },
  'mt::uploader::upload': { command: 'uploader_upload', params: ['req'] },
  'mt::fs::is-file': { command: 'fs_is_file', params: ['path'] },
  'mt::fs::is-directory': { command: 'fs_is_directory', params: ['path'] },
  'mt::fs::empty-dir': { command: 'fs_empty_dir', params: ['path'] },
  'mt::fs::copy': { command: 'fs_copy', params: ['src', 'dest'] },
  'mt::fs::ensure-dir': { command: 'fs_ensure_dir', params: ['path'] },
  'mt::fs::output-file': { command: 'fs_output_file', params: ['path', 'data'] },
  'mt::fs::move': { command: 'fs_move', params: ['src', 'dest'] },
  'mt::fs::stat': { command: 'fs_stat', params: ['path'] },
  'mt::fs::write-file': { command: 'fs_write_file', params: ['path', 'data'] },
  'mt::fs::read-file': { command: 'fs_read_file', params: ['path', 'encoding'] },
  // Phase 3: read + auto-detect encoding (chardetng) + decode to UTF-8.
  // Consumed by the Phase 4 file-open flow.
  'mt::fs::read-text-auto': { command: 'fs_read_text_auto', params: ['path', 'autoGuess'] },
  'mt::fs::path-exists': { command: 'fs_path_exists', params: ['path'] },
  'mt::fs::unlink': { command: 'fs_unlink', params: ['path'] },
  'mt::fs::readdir': { command: 'fs_readdir', params: ['path'] },
  'mt::fs::is-executable': { command: 'fs_is_executable', params: ['path'] },
  'mt::fs-trash-item': { command: 'fs_trash_item', params: ['path'] },
  'mt::shell::open-external': { command: 'shell_open_external', params: ['url'] },
  'mt::shell::open-path': { command: 'shell_open_path', params: ['fullPath'] },
  'mt::clipboard::read-text': { command: 'clipboard_read_text' },
  'mt::clipboard::guess-file-path': { command: 'clipboard_guess_file_path' },
  'mt::paths::is-image': { command: 'paths_is_image', params: ['path'] },
  'mt::ask-for-image-path': { command: 'data_center_ask_image_path' },
  'mt::editor::bootstrap-config': { command: 'editor_bootstrap_config' },
  // Session restore: the renderer's debounced buffer snapshot (no mt:: prefix).
  'update-buffer-state': { command: 'update_buffer_state', params: ['state'] },
  'mt::window::init-args': { command: 'window_init_args' },
  'mt::menu::popup': { command: 'menu_popup', params: ['template', 'position'] },
  'mt::menu::popup-application': { command: 'menu_popup_application' },
  // ripgrep search start (the whole request object is one command arg).
  'mt::rg::start': { command: 'rg_start', params: ['req'] }
}

// renderer → main, fire-and-forget channels that map to a command.
const SEND_MAP: Record<string, CmdSpec> = {
  'mt::shell::open-external': { command: 'shell_open_external', params: ['url'] },
  'mt::shell::show-item': { command: 'shell_show_item', params: ['fullPath'] },
  'mt::clipboard::write-text': { command: 'clipboard_write_text', params: ['text'] },
  'mt::set-user-preference': { command: 'preferences_set_items', params: ['settings'] },
  'set-user-preference': { command: 'preferences_set_items', params: ['settings'] },
  'mt::cmd-toggle-autosave': { command: 'preferences_toggle_autosave' },
  'mt::cmd-open-file': { command: 'file_open' },
  'mt::open-file': { command: 'file_open_path', params: ['pathname', 'options'] },
  'mt::response-file-save': {
    command: 'file_save',
    params: ['id', 'filename', 'pathname', 'markdown', 'options', 'defaultPath']
  },
  'mt::response-file-save-as': {
    command: 'file_save_as',
    params: ['id', 'filename', 'pathname', 'markdown', 'options', 'defaultPath']
  },
  // Closing tab(s) with unsaved changes → Save/Don't Save/Cancel then close.
  'mt::save-and-close-tabs': { command: 'save_and_close_tabs', params: ['unsavedFiles'] },
  // Save All without closing (Save-All command / ASK_FOR_SAVE_ALL(false)).
  'mt::save-tabs': { command: 'save_all_tabs', params: ['unsavedFiles'] },
  'mt::set-user-data': { command: 'data_center_set_items', params: ['settings'] },
  'set-image-folder-path': { command: 'data_center_set_image_folder_path', params: ['path'] },
  'mt::ask-for-modify-image-folder-path': {
    command: 'data_center_modify_image_folder_path',
    params: ['imagePath']
  },
  // Multi-window creation (Phase 4).
  'mt::cmd-new-editor-window': { command: 'window_create', fixed: { kind: 'editor' } },
  'app-create-editor-window': { command: 'window_create', fixed: { kind: 'editor' } },
  'mt::open-setting-window': { command: 'window_create', fixed: { kind: 'settings' } },
  'app-create-settings-window': { command: 'window_create', fixed: { kind: 'settings' } },
  'mt::open-keybindings-config': {
    command: 'window_create',
    fixed: { kind: 'settings', category: 'keybindings' }
  },
  // Window close flow (Phase 4).
  'mt::cmd-close-window': { command: 'window_request_close' },
  'mt::close-window': { command: 'window_close' },
  'mt::close-window-confirm': { command: 'window_close_confirm', params: ['unsavedFiles'] },
  // Menu checkbox/radio state sync (Phase 4a). First arg is the windowId, which
  // the single global macOS menu ignores — '_wid' is a throwaway param name.
  'mt::update-format-menu': { command: 'menu_update_format', params: ['_wid', 'formats'] },
  'mt::update-line-ending-menu': {
    command: 'menu_update_line_ending',
    params: ['_wid', 'lineEnding']
  },
  'mt::update-sidebar-menu': { command: 'menu_update_sidebar', params: ['_wid', 'visible'] },
  // Open a folder as the sidebar project (sidebar button + command/menu).
  'mt::ask-for-open-project-in-sidebar': { command: 'project_open' },
  'mt::cmd-open-folder': { command: 'project_open' },
  'mt::open-folder-path': { command: 'project_open_path', params: ['path'] },
  'mt::rg::cancel': { command: 'rg_cancel', params: ['searchId'] }
}

// Built-in Tauri window controls — handled without a custom Rust command.
const WINDOW_CONTROL = new Set([
  'mt::win::minimize',
  'mt::win::maximize',
  'mt::win::unmaximize',
  'mt::win::toggle-maximize',
  'mt::win::close',
  'mt::win::set-fullscreen',
  'mt::win::toggle-fullscreen'
])

const buildArgs = (spec: CmdSpec, args: unknown[]): Record<string, unknown> => {
  const out: Record<string, unknown> = { ...spec.fixed }
  spec.params?.forEach((name, i) => {
    out[name] = args[i]
  })
  return out
}

const handleWindowControl = (channel: string, args: unknown[]): void => {
  const win = getCurrentWindow()
  switch (channel) {
    case 'mt::win::minimize':
      void win.minimize()
      break
    case 'mt::win::maximize':
      void win.maximize()
      break
    case 'mt::win::unmaximize':
      void win.unmaximize()
      break
    case 'mt::win::toggle-maximize':
      void win.toggleMaximize()
      break
    case 'mt::win::close':
      void win.close()
      break
    case 'mt::win::set-fullscreen':
      void win.setFullscreen(!!args[0])
      break
    case 'mt::win::toggle-fullscreen':
      void win.isFullscreen().then((f) => win.setFullscreen(!f))
      break
  }
}

export const invoke = (channel: string, ...args: unknown[]): Promise<unknown> => {
  // Keybinding settings editor (4d). The renderer's KeybindingConfigurator
  // expects real Map instances (JSON can't carry Maps), so convert here.
  if (channel === 'mt::keybinding-get-keyboard-info') {
    // No native-keymap under Tauri; en-US fallback (atom-keymap handles empty).
    return Promise.resolve({ layout: null, keymap: {} })
  }
  if (channel === 'mt::keybinding-get-pref-keybindings') {
    return tauriInvoke('keybindings_get_user').then((user) => ({
      defaultKeybindings: getDefaultKeybindingMap(),
      userKeybindings: new Map(Object.entries((user ?? {}) as Record<string, string>))
    }))
  }
  if (channel === 'mt::keybinding-save-user-keybindings') {
    const map = (args[0] ?? new Map()) as Map<string, string>
    const bindings = Object.fromEntries(map)
    return tauriInvoke('keybindings_save_user', { bindings }).then((ok) => {
      // Apply live so customizations take effect without a restart.
      if (ok) setUserKeybindings(new Map(map))
      return ok
    })
  }
  // Spell checking: on macOS WKWebView underlines misspelled words natively via
  // muya's HTML `spellcheck` container attribute (set/toggled in the renderer
  // through editor.setOptions), and the OS spell checker auto-detects language.
  // So these channels resolve to macOS no-op values — no backend round-trip.
  if (channel === 'mt::spellchecker-set-enabled') {
    return Promise.resolve(true)
  }
  if (channel === 'mt::spellchecker-switch-language') {
    return Promise.resolve(null)
  }
  if (
    channel === 'mt::spellchecker-get-available-dictionaries' ||
    channel === 'mt::spellchecker-get-custom-dictionary-words'
  ) {
    return Promise.resolve([])
  }
  if (channel === 'mt::spellchecker-remove-word') {
    return Promise.resolve(false)
  }
  const spec = INVOKE_MAP[channel]
  if (!spec) {
    console.warn(`[platform] unimplemented invoke channel: ${channel}`)
    return Promise.resolve(undefined)
  }
  return tauriInvoke(spec.command, buildArgs(spec, args))
}

export const send = (channel: string, ...args: unknown[]): void => {
  if (WINDOW_CONTROL.has(channel)) {
    handleWindowControl(channel, args)
    return
  }
  // "ask for X" channels: fetch the snapshot and re-emit the renderer event the
  // Electron windowManager used to broadcast.
  if (channel === 'mt::ask-for-user-preference') {
    void tauriInvoke('preferences_get_all').then((data) => emit('mt::user-preference', data))
    return
  }
  if (channel === 'mt::ask-for-user-data') {
    void tauriInvoke('data_center_get_all').then((data) => emit('mt::user-preference', data))
    return
  }
  if (channel === 'mt::request-keybindings') {
    keybindingsResponder?.()
    return
  }
  // Initial language load (i18n/index.ts): reply with the configured locale.
  if (channel === 'mt::get-current-language') {
    void tauriInvoke('preferences_get_all').then((data) =>
      emit('mt::current-language', (data as { language?: string })?.language || 'en')
    )
    return
  }
  const spec = SEND_MAP[channel]
  if (!spec) {
    console.debug(`[platform] unhandled send channel (no-op): ${channel}`)
    return
  }
  tauriInvoke(spec.command, buildArgs(spec, args)).catch((err) =>
    console.error(`[platform] send ${channel} failed:`, err)
  )
}

// ---- Event subscription (main → renderer) ----------------------------------

type Listener = (event: unknown, ...args: unknown[]) => void

// Track active unlisten fns per channel so removeAllListeners() can dispose them.
const registry = new Map<string, Set<UnlistenFn>>()

// One-shot hook fired the first time the editor's `mt::bootstrap-editor`
// listener attaches — lets the platform drive the bootstrap handshake the
// moment the consumer is ready, instead of guessing with a timer.
let bootstrapTrigger: (() => void) | null = null
let bootstrapFired = false
export const setBootstrapTrigger = (fn: () => void): void => {
  bootstrapTrigger = fn
}

// Responder for `mt::request-keybindings` — the keybinding module emits the
// `mt::keybindings-response` the renderer expects (handled locally; no main).
let keybindingsResponder: (() => void) | null = null
export const setKeybindingsResponder = (fn: () => void): void => {
  keybindingsResponder = fn
}

const subscribe = (channel: string, listener: Listener, once: boolean): (() => void) => {
  let unlisten: UnlistenFn | null = null
  let disposed = false

  const wrapped = (payload: unknown): void => {
    // Electron listeners are `(event, ...args)`. Rust emits a single payload;
    // spread it when it's already an arg tuple, else pass as the sole arg.
    if (Array.isArray(payload)) listener({}, ...payload)
    else listener({}, payload)
    if (once) dispose()
  }

  const dispose = (): void => {
    disposed = true
    if (unlisten) {
      unlisten()
      registry.get(channel)?.delete(unlisten)
      unlisten = null
    }
  }

  // Scope to THIS window. The global `listen` registers with EventTarget::Any,
  // which Tauri's match_any_or_filter short-circuits to true — so an Any
  // listener receives EVERY emit_to(label) regardless of target, leaking
  // window-targeted events (e.g. mt::ask-for-close to the settings window also
  // firing the editor window's handler → wrong window closes). A label target
  // is matched by the filter, while global `emit` (no filter) still reaches it.
  void listen(channel, (e) => wrapped(e.payload), { target: getCurrentWindow().label }).then(
    (fn) => {
    if (disposed) {
      fn()
      return
    }
    unlisten = fn
    if (!registry.has(channel)) registry.set(channel, new Set())
    registry.get(channel)!.add(fn)

    // The editor store has now attached its bootstrap listener — kick off the
    // backend-driven handshake exactly once.
    if (channel === 'mt::bootstrap-editor' && bootstrapTrigger && !bootstrapFired) {
      bootstrapFired = true
      bootstrapTrigger()
    }
  })

  return dispose
}

let cachedBootInfo: BootInfo | undefined

export const setCachedBootInfo = (info: BootInfo): void => {
  cachedBootInfo = info
}

export const ipcRenderer = {
  send,
  invoke,
  sendSync: (channel: string, ..._args: unknown[]): unknown => {
    if (channel === 'mt::boot-info') return cachedBootInfo
    // mt::paths::is-same-sync is the only other sync channel; the shim's
    // fileUtils.isSamePathSync resolves it locally, so this path is unused.
    console.warn(`[platform] sendSync unsupported under Tauri: ${channel}`)
    return undefined
  },
  on: (channel: string, listener: Listener): (() => void) => subscribe(channel, listener, false),
  once: (channel: string, listener: Listener): (() => void) => subscribe(channel, listener, true),
  removeAllListeners: (channel: string): void => {
    const set = registry.get(channel)
    if (!set) return
    for (const fn of set) fn()
    registry.delete(channel)
  }
}
