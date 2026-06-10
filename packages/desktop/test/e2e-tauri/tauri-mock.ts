// Browser-side Tauri mock for E2E tests.
//
// tauri-driver does not support macOS (no WebDriver for WKWebView), so the
// Tauri E2E suite loads the real renderer bundle in Playwright's WebKit
// browser — the same engine family as WKWebView — and substitutes the Rust
// backend with this mock. `installTauriMock` is passed to
// `page.addInitScript`, so it runs before any renderer module: the platform
// shim sees `window.__TAURI_INTERNALS__` and boots against the mock exactly
// as it would against Tauri.
//
// IMPORTANT: the function is serialized by Playwright and executed in the
// page — it must stay fully self-contained (no imports, no outer closures).

export interface TauriMockConfig {
  bootInfo: Record<string, unknown>
  initArgs: Record<string, string>
  preferences: Record<string, unknown>
  dataCenter: Record<string, unknown>
  bootstrapConfig: Record<string, unknown>
}

export interface TauriMockHandle {
  // Dispatch an event to renderer listeners (simulates a Rust-side emit).
  emit: (event: string, payload: unknown) => void
  // Commands invoked that had no mock handler (returned null).
  unhandled: string[]
  // Every invoke call, for assertions: [command, args].
  invocations: Array<[string, unknown]>
}

export const installTauriMock = (cfg: TauriMockConfig): void => {
  type EventPayload = { event: string; id: number; payload: unknown }
  const callbacks = new Map<number, (payload: EventPayload) => void>()
  let nextCallbackId = 1
  // event name -> eventId -> callbackId
  const listeners = new Map<string, Map<number, number>>()
  let nextEventId = 1
  const unhandled: string[] = []
  const invocations: Array<[string, unknown]> = []

  const dispatchEvent = (event: string, payload: unknown): void => {
    const subs = listeners.get(event)
    if (!subs) return
    for (const [eventId, cbId] of Array.from(subs)) {
      const cb = callbacks.get(cbId)
      if (cb) cb({ event, id: eventId, payload })
    }
  }

  /* eslint-disable @typescript-eslint/no-explicit-any */
  const commands: Record<string, (args: any) => unknown> = {
    boot_info: () => cfg.bootInfo,
    window_init_args: () => cfg.initArgs,
    preferences_get_all: () => cfg.preferences,
    data_center_get_all: () => cfg.dataCenter,
    editor_bootstrap_config: () => cfg.bootstrapConfig,
    keybindings_get_user: () => ({}),
    keybindings_save_user: () => true,
    preferences_set_items: (a) => {
      Object.assign(cfg.preferences, a.settings)
      dispatchEvent('mt::user-preference', a.settings)
      return null
    },
    data_center_set_items: (a) => {
      Object.assign(cfg.dataCenter, a.settings)
      return null
    },
    // Fire-and-forget state syncs the editor sends during boot/edit.
    update_buffer_state: () => null,
    menu_update_format: () => null,
    menu_update_line_ending: () => null,
    menu_update_sidebar: () => null,
    // Event plugin — the in-page bus the platform shim subscribes through.
    'plugin:event|listen': (a) => {
      const eventId = nextEventId++
      if (!listeners.has(a.event)) listeners.set(a.event, new Map())
      listeners.get(a.event)!.set(eventId, a.handler as number)
      return eventId
    },
    'plugin:event|unlisten': (a) => {
      listeners.get(a.event)?.delete(a.eventId as number)
      return null
    },
    'plugin:event|emit': (a) => {
      dispatchEvent(a.event, a.payload)
      return null
    },
    // Single-window tests: target filtering collapses to plain dispatch.
    'plugin:event|emit_to': (a) => {
      dispatchEvent(a.event, a.payload)
      return null
    },
    'plugin:window|is_maximized': () => false,
    'plugin:window|is_fullscreen': () => false,
    'plugin:window|theme': () => 'light'
  }
  /* eslint-enable @typescript-eslint/no-explicit-any */

  const internals = {
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { label: 'main', windowLabel: 'main' }
    },
    plugins: {},
    transformCallback: (cb?: (payload: EventPayload) => void, once = false): number => {
      const id = nextCallbackId++
      if (cb) {
        callbacks.set(id, (payload) => {
          if (once) callbacks.delete(id)
          cb(payload)
        })
      }
      return id
    },
    invoke: async(cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
      invocations.push([cmd, args])
      const handler = commands[cmd]
      if (handler) return handler(args ?? {})
      unhandled.push(cmd)
      return null
    },
    convertFileSrc: (filePath: string): string => filePath
  }

  const w = window as unknown as Record<string, unknown>
  w.__TAURI_INTERNALS__ = internals
  w.__TAURI_MOCK__ = { emit: dispatchEvent, unhandled, invocations } satisfies TauriMockHandle
}
