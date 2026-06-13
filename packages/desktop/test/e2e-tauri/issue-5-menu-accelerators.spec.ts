// hisaboh/remarks#5: the native menu showed an accelerator that could differ
// from the shortcut that actually dispatches the command (e.g. File → New
// Window displayed Shift+Cmd+N while the keybinding system dispatches it on
// Cmd+N). The renderer now pushes its effective keybinding map (platform
// defaults merged with user overrides) to the Rust menu via
// `menu_set_accelerators`, so the displayed accelerators always match.
//
// This drives the real renderer in Playwright WebKit (Tauri E2E mock) and
// asserts the pushed map carries the effective bindings. The Rust side
// (Electron→Tauri accelerator conversion + per-item resolution) is covered by
// unit tests in src-tauri/src/menu.rs; the native NSMenu render itself cannot
// be automated on macOS (no WKWebView WebDriver).
import { expect, test } from '@playwright/test'
import { launchEditor } from './helpers'

test('renderer pushes the effective keybinding map to the native menu', async({ page }) => {
  await launchEditor(page)
  // The push happens during platform init, right after keybindings load.
  await page.waitForTimeout(600)

  const accelerators = await page.evaluate(() => {
    const w = window as unknown as {
      __TAURI_MOCK__: { invocations: Array<[string, unknown]> }
    }
    const call = w.__TAURI_MOCK__.invocations.find((c) => c[0] === 'menu_set_accelerators')
    return (call?.[1] as { accelerators?: Record<string, string> } | undefined)?.accelerators ?? null
  })

  expect(accelerators).not.toBeNull()
  // The effective binding from the keybinding system — not the menu's own
  // previously-hardcoded Shift+Cmd+N. (darwin boot → keybindingsDarwin.)
  expect(accelerators?.['file.new-window']).toBe('Command+N')
  expect(accelerators?.['file.new-tab']).toBe('Command+T')
})
