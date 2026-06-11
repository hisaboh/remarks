import { expect, test } from '@playwright/test'
import { launchEditor, PARAGRAPH_CONTENT } from './helpers'

test('boots the renderer against the Tauri mock with a blank untitled tab', async({ page }) => {
  await launchEditor(page)

  await expect(page.locator('.editor-component')).toBeVisible()
  // A blank launch renders one empty content paragraph.
  await expect(page.locator(PARAGRAPH_CONTENT).first()).toBeAttached()
})

test('bootstrap handshake reaches the backend exactly once', async({ page }) => {
  await launchEditor(page)

  const counts = await page.evaluate(() => {
    const mock = (
      window as unknown as {
        __TAURI_MOCK__: { invocations: Array<[string, unknown]> }
      }
    ).__TAURI_MOCK__
    const count = (cmd: string) => mock.invocations.filter(([c]) => c === cmd).length
    return {
      bootInfo: count('boot_info'),
      bootstrap: count('editor_bootstrap_config')
    }
  })
  expect(counts.bootInfo).toBe(1)
  expect(counts.bootstrap).toBe(1)
})
