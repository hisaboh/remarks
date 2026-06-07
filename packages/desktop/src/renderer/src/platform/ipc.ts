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

interface CmdSpec {
  command: string
  // Positional-argument names, in order, mapped onto a named args object.
  params?: string[]
}

// renderer → main, Promise-returning channels.
const INVOKE_MAP: Record<string, CmdSpec> = {
  'mt::boot-info-async': { command: 'boot_info' },
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
  'mt::ask-for-image-path': { command: 'data_center_ask_image_path' }
}

// renderer → main, fire-and-forget channels that map to a command.
const SEND_MAP: Record<string, CmdSpec> = {
  'mt::shell::open-external': { command: 'shell_open_external', params: ['url'] },
  'mt::shell::show-item': { command: 'shell_show_item', params: ['fullPath'] },
  'mt::clipboard::write-text': { command: 'clipboard_write_text', params: ['text'] },
  'mt::set-user-preference': { command: 'preferences_set_items', params: ['settings'] },
  'set-user-preference': { command: 'preferences_set_items', params: ['settings'] },
  'mt::cmd-toggle-autosave': { command: 'preferences_toggle_autosave' },
  'mt::set-user-data': { command: 'data_center_set_items', params: ['settings'] },
  'set-image-folder-path': { command: 'data_center_set_image_folder_path', params: ['path'] },
  'mt::ask-for-modify-image-folder-path': {
    command: 'data_center_modify_image_folder_path',
    params: ['imagePath']
  }
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
  const out: Record<string, unknown> = {}
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

  void listen(channel, (e) => wrapped(e.payload)).then((fn) => {
    if (disposed) {
      fn()
      return
    }
    unlisten = fn
    if (!registry.has(channel)) registry.set(channel, new Set())
    registry.get(channel)!.add(fn)
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
