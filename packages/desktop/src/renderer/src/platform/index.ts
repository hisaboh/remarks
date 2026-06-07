// Tauri platform shim — installs the same `window.*` globals the Electron
// preload (src/preload/index.ts) used to expose, but backed by Tauri.
//
// Loaded only when running under Tauri (see `isTauri`). Under Electron the
// preload still provides these globals, so `initPlatform` is skipped.
//
// `initPlatform` is async because Tauri's `invoke` has no synchronous
// equivalent of the preload's `ipcRenderer.sendSync('mt::boot-info')`. It must
// be awaited before the Vue app (and muya) load — see main.ts.

import pathe from 'pathe'
import { emitTo } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { BootInfo } from '@shared/types/ipc'
import { ipcRenderer, invoke, send, setCachedBootInfo, setBootstrapTrigger } from './ipc'

export const isTauri = (): boolean => '__TAURI_INTERNALS__' in window

// ---- pure path predicates (mirrors of the preload implementations) ---------

const MARKDOWN_EXTENSIONS = [
  'markdown', 'mdown', 'mkdn', 'md', 'mkd', 'mdwn', 'mdtxt', 'mdtext', 'mdx', 'text', 'txt'
] as const

const hasMarkdownExtension = (filename: string): boolean => {
  if (!filename || typeof filename !== 'string') return false
  return MARKDOWN_EXTENSIONS.some((ext) => filename.toLowerCase().endsWith(`.${ext}`))
}

const isChildOfDirectory = (dir: string, child: string): boolean => {
  if (!dir || !child) return false
  const relative = pathe.relative(dir, child)
  return !!relative && !relative.startsWith('..') && !pathe.isAbsolute(relative)
}

const isSamePathSync = (a: string, b: string, isNormalized = false): boolean => {
  if (!a || !b) return false
  const x = isNormalized ? a : pathe.normalize(a)
  const y = isNormalized ? b : pathe.normalize(b)
  if (x.length !== y.length) return false
  if (x === y) return true
  // Case-insensitive filesystem approximation (the preload's blocking sync-IPC
  // fallback isn't available under Tauri).
  return x.toLowerCase() === y.toLowerCase()
}

// ---- write/read payload bridging -------------------------------------------

const toWireData = (data: string | Uint8Array): string | number[] =>
  typeof data === 'string' ? data : Array.from(data)

const fromReadResult = (res: unknown): string | Uint8Array => {
  if (typeof res === 'string') return res
  if (Array.isArray(res)) return Uint8Array.from(res as number[])
  return res as Uint8Array
}

// ---- API objects ------------------------------------------------------------

const buildFileUtils = (bootInfo: BootInfo): FileUtilsAPI => ({
  isFile: (p) => invoke('mt::fs::is-file', p) as Promise<boolean>,
  isDirectory: (p) => invoke('mt::fs::is-directory', p) as Promise<boolean>,
  emptyDir: (p) => invoke('mt::fs::empty-dir', p) as Promise<void>,
  copy: (src, dest) => invoke('mt::fs::copy', src, dest) as Promise<void>,
  ensureDir: (p) => invoke('mt::fs::ensure-dir', p) as Promise<void>,
  outputFile: (p, data) => invoke('mt::fs::output-file', p, toWireData(data)) as Promise<void>,
  move: (src, dest) => invoke('mt::fs::move', src, dest) as Promise<void>,
  stat: (p) => invoke('mt::fs::stat', p) as Promise<import('@shared/types/files').SerializedStat>,
  writeFile: (p, data) => invoke('mt::fs::write-file', p, toWireData(data)) as Promise<void>,
  readFile: async (p, encoding) => fromReadResult(await invoke('mt::fs::read-file', p, encoding)),
  pathExists: (p) => invoke('mt::fs::path-exists', p) as Promise<boolean>,
  unlink: (p) => invoke('mt::fs::unlink', p) as Promise<void>,
  readdir: (p) => invoke('mt::fs::readdir', p) as Promise<string[]>,
  isExecutable: (p) => invoke('mt::fs::is-executable', p) as Promise<boolean>,
  isChildOfDirectory,
  hasMarkdownExtension,
  isSamePathSync,
  isImageFile: (p) => invoke('mt::paths::is-image', p) as Promise<boolean>,
  MARKDOWN_INCLUSIONS: bootInfo.MARKDOWN_INCLUSIONS || []
})

const pathAPI: PathAPI = {
  basename: (p, ext?) => pathe.basename(p, ext),
  dirname: (p) => pathe.dirname(p),
  extname: (p) => pathe.extname(p),
  join: (...args) => pathe.join(...args),
  resolve: (...args) => pathe.resolve(...args),
  relative: (from, to) => pathe.relative(from, to),
  isAbsolute: (p) => pathe.isAbsolute(p),
  normalize: (p) => pathe.normalize(p),
  parse: (p) => pathe.parse(p),
  format: (o) => pathe.format(o),
  sep: pathe.sep,
  delimiter: pathe.delimiter
}

const shellAPI: ElectronShellAPI = {
  openExternal: (url) => invoke('mt::shell::open-external', url) as Promise<void>,
  showItemInFolder: (fullPath) => send('mt::shell::show-item', fullPath),
  openPath: (fullPath) => invoke('mt::shell::open-path', fullPath) as Promise<string>
}

const clipboardAPI: ElectronClipboardAPI = {
  writeText: (text) => send('mt::clipboard::write-text', text),
  readText: () => invoke('mt::clipboard::read-text') as Promise<string>,
  guessFilePath: () => invoke('mt::clipboard::guess-file-path') as Promise<string | null>
}

const webFrameAPI: ElectronWebFrameAPI = {
  setZoomFactor: (factor) => {
    if (typeof factor === 'number' && factor > 0) void getCurrentWebview().setZoom(factor)
  },
  // Tauri only exposes a zoom *factor*; approximate Chrome's level→factor curve.
  setZoomLevel: (level) => {
    if (typeof level === 'number') void getCurrentWebview().setZoom(1.2 ** level)
  }
}

const webUtilsAPI: ElectronWebUtilsAPI = {
  // TODO(phase-5): file drag-and-drop paths arrive via Tauri window drag events,
  // not synchronously from a File object.
  getPathForFile: () => ''
}

const windowControlAPI: ElectronWindowControlAPI = {
  minimize: () => send('mt::win::minimize'),
  maximize: () => send('mt::win::maximize'),
  unmaximize: () => send('mt::win::unmaximize'),
  toggleMaximize: () => send('mt::win::toggle-maximize'),
  close: () => send('mt::win::close'),
  setFullScreen: (flag) => send('mt::win::set-fullscreen', flag),
  toggleFullScreen: () => send('mt::win::toggle-fullscreen'),
  isMaximized: () => getCurrentWindow().isMaximized(),
  isFullScreen: () => getCurrentWindow().isFullscreen(),
  popupMenu: (template, position) => void invoke('mt::menu::popup', template, position),
  popupApplicationMenu: (position) => void invoke('mt::menu::popup-application', position)
}

const noopDisposer = (): void => {}

// Stubs for handlers not yet ported (fonts, i18n, cmd, ripgrep, uploader) —
// they resolve to benign defaults so the renderer can still boot. Each is a
// later-phase TODO.
const commandExistsAPI: CommandExistsAPI = {
  exists: () => Promise.resolve(false)
}
const i18nUtilsAPI: I18nUtilsAPI = {
  loadTranslations: () => Promise.resolve({})
}
const ripgrepAPI: RipgrepAPI = {
  start: () => Promise.resolve({ searchId: '' }),
  cancel: noopDisposer,
  onMatch: () => noopDisposer,
  onProgress: () => noopDisposer,
  onDone: () => noopDisposer,
  onError: () => noopDisposer,
  onCancelled: () => noopDisposer
}
const uploaderAPI: UploaderAPI = {
  uploadImage: () => Promise.reject(new Error('uploader not implemented under Tauri yet'))
}
const fontsAPI: FontsAPI = {
  list: () => Promise.resolve([])
}

// ---- default URL args (single editor window) -------------------------------

// The Electron windows were spawned with query args (windowId, userDataPath,
// theme…). Tauri child windows can't carry a query string across the dev/prod
// URL base, so each window asks the backend for its args (keyed by window
// label) and applies them — the main window gets default editor args.
const applyWindowArgs = async (): Promise<void> => {
  const current = new URLSearchParams(window.location.search)
  if (current.has('wid')) return
  const args = (await invoke('mt::window::init-args')) as Record<string, string>
  const params = new URLSearchParams(args)
  history.replaceState(null, '', `?${params.toString()}${window.location.hash}`)
}

// Backend-driven `mt::bootstrap-editor` handshake. Registered as the one-shot
// trigger fired when the editor store attaches its listener (see ipc.ts): we
// fetch the config the Electron main process used to build (from preferences)
// and emit the event the editor store waits on.
// TODO(phase-4): pass opened files (CLI args / restored session) into
// markdownList, and split per-window config once multi-window lands.
const registerBootstrapHandshake = (): void => {
  setBootstrapTrigger(() => {
    const label = getCurrentWindow().label
    void invoke('mt::editor::bootstrap-config')
      // Target THIS window only — a broadcast would re-bootstrap other editors.
      .then((config) => emitTo(label, 'mt::bootstrap-editor', config))
      .catch((err) => console.error('[platform] bootstrap config failed:', err))
  })
}

// ---- install ----------------------------------------------------------------

export const initPlatform = async (): Promise<void> => {
  const bootInfo = (await invoke('mt::boot-info-async')) as BootInfo
  setCachedBootInfo(bootInfo)
  await applyWindowArgs()

  const processInfo = {
    platform: bootInfo.platform,
    arch: bootInfo.arch,
    versions: bootInfo.versions || {},
    env: bootInfo.env || {},
    resourcesPath: bootInfo.paths?.resources,
    cwd: bootInfo.paths?.cwd
  }

  const electronAPI: ElectronAPI = {
    // The shim is intentionally loosely typed (string channels) — it's a
    // runtime bridge, not a per-channel typed surface like the preload.
    ipcRenderer: ipcRenderer as unknown as ElectronIpcRenderer,
    shell: shellAPI,
    clipboard: clipboardAPI,
    webFrame: webFrameAPI,
    webUtils: webUtilsAPI,
    process: processInfo,
    paths: bootInfo.paths || {},
    isUpdatable: !!bootInfo.isUpdatable,
    windowControl: windowControlAPI
  }

  const w = window as unknown as Record<string, unknown>
  w.electron = electronAPI
  w.fileUtils = buildFileUtils(bootInfo)
  w.path = pathAPI
  w.commandExists = commandExistsAPI
  w.i18nUtils = i18nUtilsAPI
  w.ripgrep = ripgrepAPI
  w.uploader = uploaderAPI
  w.fonts = fontsAPI
  w.rgPath = bootInfo.paths?.ripgrepBinary || ''
  w.process = {
    ...processInfo,
    cwd: () => bootInfo.paths?.cwd,
    nextTick: (fn: (...args: unknown[]) => void, ...args: unknown[]) =>
      Promise.resolve().then(() => fn(...args))
  }

  // Arm the bootstrap handshake before the editor store (loaded with the Vue
  // app) attaches its listener.
  registerBootstrapHandshake()
}
