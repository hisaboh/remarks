import { expect, test, type Page } from '@playwright/test'
import { launchEditor, PARAGRAPH_CONTENT, typeIntoEditor } from './helpers'

const TAB = '.editor-tabs .tabs-container li'

// Open a new blank untitled tab via the real menu/keyboard path: the native
// menu (and Cmd+T) emit `mt::open-new-tab`, which the editor store listens for
// and routes to NEW_UNTITLED_TAB. Driving the IPC event exercises the exact
// flow without depending on the (initially-hidden) tab bar.
const openNewTab = (page: Page) =>
  page.evaluate(() => {
    const w = window as unknown as { __TAURI_MOCK__: { emit: (e: string, p: unknown) => void } }
    w.__TAURI_MOCK__.emit('mt::open-new-tab', [])
  })

// Inspect the live DOM caret + focus. With the bug, `setContent` leaves the
// caret on the editor container (`mu-container`, one line above the first
// paragraph); the fix's `focus()` places it inside a `.mu-paragraph-content`
// and makes the contenteditable the active element so typing lands in-block.
const caretState = async(page: Page) =>
  page.evaluate((sel) => {
    const a = window.getSelection()?.anchorNode
    const el = a ? (a.nodeType === 1 ? (a as Element) : a.parentElement) : null
    const ae = document.activeElement as HTMLElement | null
    return {
      inParagraph: !!el?.closest(sel),
      where: el?.className || el?.nodeName || 'no-selection',
      activeIsEditor: !!ae?.closest('[contenteditable="true"]')
    }
  }, PARAGRAPH_CONTENT)

test('issue#15: caret lands inside the 2nd new tab paragraph, not the container', async({
  page
}) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'first tab text')

  // Two consecutive new untitled tabs (the Cmd+T path).
  await openNewTab(page)
  await openNewTab(page)
  await expect(page.locator(TAB)).toHaveCount(3)

  const caret = await caretState(page)
  console.log('[verify] #15 caret after 2 new tabs:', JSON.stringify(caret))
  // The bug parks the caret on `mu-container`; the fix puts it in the paragraph.
  expect(caret.inParagraph).toBe(true)
  expect(caret.activeIsEditor).toBe(true)

  // The "one line above the first line" symptom is a stray text container: there
  // must be no bare text node directly under the editor element.
  const strayText = await page.evaluate(() => {
    const ec = document.querySelector('.editor-component')
    let stray = ''
    ec?.childNodes.forEach((n) => {
      if (n.nodeType === Node.TEXT_NODE) stray += n.textContent
    })
    return stray.trim()
  })
  console.log('[verify] #15 stray text under .editor-component:', JSON.stringify(strayText))
  expect(strayText).toBe('')
})

test('issue#16: caret restored when switching back to an untitled tab', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'home tab')

  await openNewTab(page)
  await expect(page.locator(TAB)).toHaveCount(2)

  // Switch to tab 1, then back to the (empty, never-focused) untitled tab 2 via
  // the tab bar — the Ctrl+Tab style return path from the issue.
  await page.locator(TAB).first().click()
  await page.locator(TAB).nth(1).click()

  const caret = await caretState(page)
  console.log('[verify] #16 caret after switching back to untitled tab:', JSON.stringify(caret))
  expect(caret.inParagraph).toBe(true)
  expect(caret.activeIsEditor).toBe(true)
})
