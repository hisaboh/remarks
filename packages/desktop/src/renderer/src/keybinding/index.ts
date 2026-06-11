// Renderer-side keybinding dispatch — Phase 4 replacement for the Electron
// main keyboard subsystem (native-keymap + @hfelix/electron-localshortcut).
//
// Under Tauri there is no main-side shortcut registration. Instead a global
// keydown listener matches the pressed key combo against the platform default
// keybindings and runs the command (via mt::execute-command-by-id, the same
// path the menu and command palette use). On macOS, accelerators that are also
// application-menu items are intercepted by the menu before the webview, so
// this dispatcher naturally covers the non-menu shortcuts without double-firing.
//
// The KeyboardEvent gives us the layout-resolved key directly, so native-keymap
// is not needed. User-customized keybindings (keybindings.json + the settings
// editor) are a follow-up — this ships the platform defaults.

import { emitTo } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import keybindingsDarwin from './defaults/keybindingsDarwin'
import keybindingsLinux from './defaults/keybindingsLinux'
import keybindingsWindows from './defaults/keybindingsWindows'

const platform = (): string => window.electron?.process?.platform || 'darwin'

const defaultKeybindings = (): Map<string, string> => {
  switch (platform()) {
    case 'darwin':
      return keybindingsDarwin
    case 'win32':
      return keybindingsWindows
    default:
      return keybindingsLinux
  }
}

// User overrides loaded from keybindings.json (4d), merged over the defaults.
// An empty-string accelerator unbinds a default (acceleratorSignature returns
// null for it, so buildLookup skips it).
let userKeybindings: Map<string, string> = new Map()

const mergedKeybindings = (): Map<string, string> => {
  const merged = new Map(defaultKeybindings())
  for (const [id, accelerator] of userKeybindings) {
    merged.set(id, accelerator)
  }
  return merged
}

/** Platform default map (no user overrides) — for the settings editor. */
export const getDefaultKeybindingMap = (): Map<string, string> => defaultKeybindings()

/** Apply user overrides and rebuild the dispatch lookup (live, after a save). */
export const setUserKeybindings = (user: Map<string, string>): void => {
  userKeybindings = user
  if (signatureToCommand) signatureToCommand = buildLookup()
}

// Canonicalize an accelerator token into a stable modifier/key signature so a
// map entry like "Command+Shift+S" and a KeyboardEvent compare equal.
const canonModifier = (token: string): string | null => {
  switch (token.toLowerCase()) {
    case 'command':
    case 'cmd':
    case 'super':
    case 'meta':
      return 'Meta'
    case 'cmdorctrl':
    case 'commandorcontrol':
      return platform() === 'darwin' ? 'Meta' : 'Ctrl'
    case 'ctrl':
    case 'control':
      return 'Ctrl'
    case 'alt':
    case 'option':
      return 'Alt'
    case 'shift':
      return 'Shift'
    default:
      return null
  }
}

const canonKey = (token: string): string => {
  if (token.length === 1) return token.toUpperCase()
  // Electron accelerator key names → canonical.
  const map: Record<string, string> = {
    return: 'Enter',
    esc: 'Escape',
    space: 'Space',
    up: 'Up',
    down: 'Down',
    left: 'Left',
    right: 'Right',
    del: 'Delete',
    plus: '+'
  }
  return map[token.toLowerCase()] ?? token
}

// Build "Meta+Alt+Shift+KEY" signature with modifiers in a fixed order.
const signature = (mods: Set<string>, key: string): string => {
  const order = ['Ctrl', 'Meta', 'Alt', 'Shift']
  return [...order.filter((m) => mods.has(m)), key].join('+')
}

const acceleratorSignature = (accelerator: string): string | null => {
  if (!accelerator) return null
  const parts = accelerator.split('+')
  const mods = new Set<string>()
  let key: string | null = null
  for (const part of parts) {
    const mod = canonModifier(part)
    if (mod) mods.add(mod)
    else key = canonKey(part)
  }
  if (!key) return null
  return signature(mods, key)
}

// Derive the canonical key token from a KeyboardEvent (prefer physical `code`).
const eventKey = (e: KeyboardEvent): string | null => {
  const code = e.code
  if (code.startsWith('Key')) return code.slice(3) // KeyS → S
  if (code.startsWith('Digit')) return code.slice(5) // Digit1 → 1
  const named: Record<string, string> = {
    Comma: ',',
    Period: '.',
    Slash: '/',
    Backslash: '\\',
    Minus: '-',
    Equal: '=',
    Semicolon: ';',
    Quote: "'",
    BracketLeft: '[',
    BracketRight: ']',
    Backquote: '`',
    Tab: 'Tab',
    Enter: 'Enter',
    Escape: 'Escape',
    Backspace: 'Backspace',
    Delete: 'Delete',
    Space: 'Space',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right'
  }
  if (named[code]) return named[code]
  if (/^F\d{1,2}$/.test(code)) return code // F1..F12
  return null
}

const eventSignature = (e: KeyboardEvent): string | null => {
  const key = eventKey(e)
  if (!key) return null
  const mods = new Set<string>()
  if (e.metaKey) mods.add('Meta')
  if (e.ctrlKey) mods.add('Ctrl')
  if (e.altKey) mods.add('Alt')
  if (e.shiftKey) mods.add('Shift')
  // Bare keys (no modifier) are left to the editor — only dispatch combos.
  if (mods.size === 0) return null
  return signature(mods, key)
}

let signatureToCommand: Map<string, string> | null = null

// Native editing shortcuts must NOT be dispatched: no renderer command is
// registered for these ids (under Electron the menu items just called
// webContents.cut()/copy()/paste()/selectAll(), i.e. the browser default
// action). Matching them here would preventDefault() that default and break
// cut/copy/paste/select-all in plain text fields (e.g. the settings
// custom-CSS textarea) and in muya. WebKit and the predefined Edit menu
// items handle them natively.
const NATIVE_EDIT_COMMANDS = new Set([
  'edit.cut',
  'edit.copy',
  'edit.paste',
  'edit.select-all',
  // Dev-only menu entries handled entirely Rust-side (devtools / reload):
  // letting the keydown through lets the native menu accelerator fire.
  'view.toggle-dev-tools',
  'view.dev-reload'
])

const buildLookup = (): Map<string, string> => {
  const lookup = new Map<string, string>()
  for (const [id, accelerator] of mergedKeybindings()) {
    if (NATIVE_EDIT_COMMANDS.has(id)) continue
    const sig = acceleratorSignature(accelerator)
    // First binding for a signature wins (defaults have no intra-map dupes).
    if (sig && !lookup.has(sig)) lookup.set(sig, id)
  }
  return lookup
}

const execute = (id: string): void => {
  const label = getCurrentWindow().label
  void emitTo(label, 'mt::execute-command-by-id', id)
}

const onKeyDown = (e: KeyboardEvent): void => {
  if (e.isComposing) return // don't steal keys during IME composition
  const sig = eventSignature(e)
  if (!sig || !signatureToCommand) return
  const id = signatureToCommand.get(sig)
  if (id) {
    e.preventDefault()
    execute(id)
  }
}

/** The {commandId: accelerator} map (defaults + user), for palette display. */
export const getKeybindingMap = (): Record<string, string> =>
  Object.fromEntries(mergedKeybindings())

/** Install the global keydown dispatcher. */
export const installKeybindings = (): void => {
  signatureToCommand = buildLookup()
  window.addEventListener('keydown', onKeyDown, true)
}
