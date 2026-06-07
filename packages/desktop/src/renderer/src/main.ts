// Renderer entry point.
//
// Under Electron the sandboxed preload (src/preload/index.ts) installs the
// `window.electron` / `window.fileUtils` / … globals before any renderer
// script runs, so we can load the app immediately.
//
// Under Tauri there is no preload, so we install the equivalent globals via the
// platform shim first. `initPlatform` is async (Tauri's `invoke` has no sync
// boot-info equivalent), so the Vue app is loaded with a dynamic import only
// after the globals are in place — this guarantees modules that read
// `window.electron` at import time see a populated surface, matching Electron's
// preload ordering.

import { isTauri, initPlatform } from './platform'

const start = async (): Promise<void> => {
  if (isTauri()) {
    await initPlatform()
  }
  await import('./app')
}

void start()
