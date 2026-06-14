// hisaboh/remarks#10: Cmd+Q / window close must prompt for unsaved changes.
// The close flow (`mt::ask-for-close`) used to skip the Save/Don't Save/Cancel
// confirmation when the `restoreAll` startup action was set (the default), so
// the app quit silently with unsaved edits. It must always confirm when there
// are unsaved files; only a fully-saved document closes without a prompt.
import { expect, test, type Page } from '@playwright/test'
import { launchEditor, typeIntoEditor } from './helpers'

const invokedCommands = (page: Page) =>
  page.evaluate(() => {
    const w = window as unknown as { __TAURI_MOCK__: { invocations: Array<[string, unknown]> } }
    return w.__TAURI_MOCK__.invocations.map((c) => c[0])
  })

const askForClose = (page: Page) =>
  page.evaluate(() => {
    const w = window as unknown as { __TAURI_MOCK__: { emit: (e: string, p?: unknown) => void } }
    w.__TAURI_MOCK__.emit('mt::ask-for-close', null)
  })

test('close with unsaved changes asks to confirm (restoreAll default)', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'unsaved edit')
  // Let the json-change → save-tracking flip the tab to unsaved.
  await page.waitForTimeout(400)

  await askForClose(page)
  await page.waitForTimeout(400)

  const cmds = await invokedCommands(page)
  // Must route through the confirm dialog, NOT close silently.
  expect(cmds).toContain('window_close_confirm')
  expect(cmds).not.toContain('window_close')
})

test('close with nothing unsaved closes without a prompt', async({ page }) => {
  await launchEditor(page)
  // No edits → the blank tab stays saved.
  await page.waitForTimeout(300)

  await askForClose(page)
  await page.waitForTimeout(400)

  const cmds = await invokedCommands(page)
  expect(cmds).toContain('window_close')
  expect(cmds).not.toContain('window_close_confirm')
})
