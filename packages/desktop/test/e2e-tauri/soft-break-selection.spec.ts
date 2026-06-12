// hisaboh/remarks#1: Shift+Down from the head of a soft-wrapped (Shift+Enter)
// line must extend the selection across the soft line break. WKWebView's
// native selection cannot move the focus vertically at all, so the editor
// probes the adjacent visual line itself.
import { expect, test, type Page } from '@playwright/test'
import { launchEditor, PARAGRAPH_CONTENT, typeIntoEditor } from './helpers'

const selectionString = (page: Page) =>
  page.evaluate(() => window.getSelection()?.toString() ?? '')

test('Shift+Down selects the soft-wrapped first line incl. its break', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, '11111111111111111111')
  await page.keyboard.press('Shift+Enter')
  await page.keyboard.type('2222222222222222', { delay: 0 })

  // Caret to the head of visual line 1.
  await page.keyboard.press('ArrowUp')
  await page.keyboard.press('Meta+ArrowLeft')

  await page.keyboard.press('Shift+ArrowDown')

  const selected = await selectionString(page)
  expect(selected).toContain('11111111111111111111')
  expect(selected).not.toContain('2222')

  // A second Shift+Down extends into the soft-wrapped second line.
  await page.keyboard.press('Shift+ArrowDown')
  const extended = await selectionString(page)
  expect(extended).toContain('2222')
})

test('Shift+Up selects the soft-wrapped line upwards', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, '11111111111111111111')
  await page.keyboard.press('Shift+Enter')
  await page.keyboard.type('2222222222222222', { delay: 0 })

  // Caret to the head of visual line 2.
  await page.keyboard.press('Meta+ArrowLeft')

  await page.keyboard.press('Shift+ArrowUp')
  const selected = await selectionString(page)
  expect(selected).toContain('11111111111111111111')
})
