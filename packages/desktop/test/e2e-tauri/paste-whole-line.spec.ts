// Pasting whole line(s) — a copy that includes the line break must paste as
// standalone line(s) at a block start instead of merging into the existing
// block (the @muyajs/core port of the Remarks muyajs behavior, and the
// copy side must preserve that trailing newline).
import { expect, test, type Page } from '@playwright/test'
import { launchEditor, PARAGRAPH_CONTENT, typeIntoEditor } from './helpers'

const setupTwoLines = async(page: Page): Promise<void> => {
  await launchEditor(page)
  await typeIntoEditor(page, 'alpha')
  await page.keyboard.press('Enter')
  await page.keyboard.type('beta', { delay: 0 })
}

// Selects from the start of the first paragraph to the start of the second
// (i.e. "alpha\n") and fires muya's copy handler with a synthetic clipboard.
const copyLineWithNewline = (selector: string): { text: string; html: string } => {
  const findText = (el: Element): Node => {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT)
    return walker.nextNode() ?? el
  }
  const paragraphs = document.querySelectorAll(selector)
  const a = paragraphs[0]
  const b = paragraphs[1]
  window.getSelection()?.setBaseAndExtent(findText(a), 0, b.firstChild ?? b, 0)
  const dt = new DataTransfer()
  const ev = new ClipboardEvent('copy', { clipboardData: dt, bubbles: true, cancelable: true })
  a.dispatchEvent(ev)
  return { text: dt.getData('text/plain'), html: dt.getData('text/html') }
}

const caretToStartOfBeta = async(page: Page): Promise<void> => {
  await page.locator(PARAGRAPH_CONTENT, { hasText: 'beta' }).click()
  await page.keyboard.press('Meta+ArrowLeft')
}

const pasteText = (args: { selector: string; text: string }): Promise<(string | null)[]> => {
  const dt = new DataTransfer()
  dt.setData('text/plain', args.text)
  const ev = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })
  const paragraphs = document.querySelectorAll(args.selector)
  paragraphs[paragraphs.length - 1].dispatchEvent(ev)
  return new Promise((resolve) => {
    setTimeout(() => {
      resolve(
        Array.from(document.querySelectorAll(args.selector)).map((s) => s.textContent)
      )
    }, 300)
  })
}

test('copying a line incl. its newline keeps the trailing \\n on the clipboard', async({
  page
}) => {
  await setupTwoLines(page)

  const copied = await page.evaluate(copyLineWithNewline, PARAGRAPH_CONTENT)
  expect(copied.text).toBe('alpha\n')
})

test('Shift+Down from the line start selects through the line break', async({ page }) => {
  await setupTwoLines(page)

  // Caret to the start of "alpha", then extend down. WKWebView's native
  // selection clamps this at the line end (user-reported; Playwright's
  // newer WebKit happens to cross on its own) — the editor extends the
  // focus into the next block itself so the behavior is deterministic and
  // the selection includes the line terminator on both engines.
  await page.locator(PARAGRAPH_CONTENT, { hasText: 'alpha' }).click()
  await page.keyboard.press('Meta+ArrowLeft')
  await page.keyboard.press('Shift+ArrowDown')

  const copied = await page.evaluate((selector: string) => {
    const dt = new DataTransfer()
    const ev = new ClipboardEvent('copy', { clipboardData: dt, bubbles: true, cancelable: true })
    document.querySelector(selector)!.dispatchEvent(ev)
    return { text: dt.getData('text/plain') }
  }, PARAGRAPH_CONTENT)
  expect(copied.text).toBe('alpha\n')
})

test('pasting "alpha\\n" at the start of a line inserts a line instead of merging', async({
  page
}) => {
  await setupTwoLines(page)
  await caretToStartOfBeta(page)

  const paragraphs = await page.evaluate(pasteText, {
    selector: PARAGRAPH_CONTENT,
    text: 'alpha\n'
  })
  expect(paragraphs).toEqual(['alpha', 'alpha', 'beta'])

  // The caret stays at the start of the pushed-down line.
  await page.keyboard.type('X', { delay: 0 })
  await expect(page.locator('.editor-component')).toContainText('Xbeta')
})

test('pasting multiple whole lines at a line start inserts them all', async({ page }) => {
  await setupTwoLines(page)
  await caretToStartOfBeta(page)

  const paragraphs = await page.evaluate(pasteText, {
    selector: PARAGRAPH_CONTENT,
    text: 'one\n\ntwo\n'
  })
  expect(paragraphs).toEqual(['alpha', 'one', 'two', 'beta'])
})

test('pasting "alpha" (no trailing newline) at a line start still merges', async({ page }) => {
  await setupTwoLines(page)
  await caretToStartOfBeta(page)

  const paragraphs = await page.evaluate(pasteText, {
    selector: PARAGRAPH_CONTENT,
    text: 'alpha'
  })
  expect(paragraphs).toEqual(['alpha', 'alphabeta'])
})

test('pasting a whole line onto the EMPTY last line keeps the empty line', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'alpha')
  await page.keyboard.press('Enter')
  await page.keyboard.type('beta', { delay: 0 })
  await page.keyboard.press('Enter')
  // caret now sits on the empty last line

  const paragraphs = await page.evaluate(pasteText, {
    selector: PARAGRAPH_CONTENT,
    text: 'alpha\n'
  })
  expect(paragraphs).toEqual(['alpha', 'beta', 'alpha', ''])
})

test('muya→muya round trip: copy line incl. newline, paste at another line start', async({
  page
}) => {
  await setupTwoLines(page)

  const copied = await page.evaluate(copyLineWithNewline, PARAGRAPH_CONTENT)
  await caretToStartOfBeta(page)
  const paragraphs = await page.evaluate(pasteText, {
    selector: PARAGRAPH_CONTENT,
    text: copied.text
  })
  expect(paragraphs).toEqual(['alpha', 'alpha', 'beta'])
})
