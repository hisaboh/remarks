import { afterEach, describe, expect, it, vi } from 'vitest'
import bus from '@/bus'
import commands from '@/commands'

// Regression for hisaboh/remarks#6 — "新しいタブを開いた時、新しいタブに移動しない".
//
// On the Tauri build both the native menu item and the Cmd/Ctrl+T shortcut
// dispatch the renderer `file.new-tab` command, which emits
// `mt::new-untitled-tab` on the bus. The store handler selects the new tab
// only when the payload's `selected` is truthy:
//
//   bus.on('mt::new-untitled-tab', ({ selected = true } = {}) => {
//     this.NEW_UNTITLED_TAB({ markdown, selected })
//   })
//
// The bug was emitting `selected: ''` — a falsy value that is *not* undefined,
// so the `= true` default never kicks in. The tab was pushed but never made
// the current file, so the editor kept showing the previous tab.
describe('file.new-tab command (#6)', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('emits a new-untitled-tab payload that the store treats as selected', async() => {
    const cmd = commands.find((c) => c.id === 'file.new-tab')
    const execute = cmd?.execute
    expect(execute).toBeTypeOf('function')

    const emit = vi.spyOn(bus, 'emit')
    await execute?.()

    // mitt types `emit` as single-arg, so the recorded call tuple needs an
    // explicit cast to reach the payload.
    const calls = emit.mock.calls as unknown as Array<[string, { selected?: unknown }?]>
    const call = calls.find((c) => c[0] === 'mt::new-untitled-tab')
    expect(call).toBeTruthy()

    // Mirror the store handler's default-destructuring: a falsy-but-defined
    // `selected` (the old `''` bug) would survive the `= true` default and
    // leave the new tab unselected.
    const { selected = true } = call?.[1] ?? {}
    expect(selected).toBeTruthy()
  })
})
