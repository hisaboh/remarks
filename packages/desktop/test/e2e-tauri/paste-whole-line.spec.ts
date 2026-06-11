// Pasting whole line(s) — a copy that includes the line break must paste as
// standalone line(s) at a block start instead of merging into the existing
// block (and the muya copy handler must preserve that trailing newline).
import { expect, test, type Page } from '@playwright/test'
import { launchEditor, typeIntoEditor } from './helpers'

const setupTwoLines = async(page: Page): Promise<void> => {
  await launchEditor(page)
  await typeIntoEditor(page, 'alpha')
  await page.keyboard.press('Enter')
  await page.keyboard.type('beta', { delay: 0 })
}

// Selects from the start of the first paragraph to the start of the second
// (i.e. "alpha\n") and fires muya's copy handler with a synthetic clipboard.
const copyLineWithNewline = (): { text: string; html: string } => {
  const spans = document.querySelectorAll('.editor-component span.ag-paragraph')
  const a = spans[0]
  const b = spans[1]
  window.getSelection()?.setBaseAndExtent(a.firstChild ?? a, 0, b.firstChild ?? b, 0)
  const dt = new DataTransfer()
  const ev = new ClipboardEvent('copy', { clipboardData: dt, bubbles: true, cancelable: true })
  a.dispatchEvent(ev)
  return { text: dt.getData('text/plain'), html: dt.getData('text/html') }
}

const caretToStartOfBeta = async(page: Page): Promise<void> => {
  await page.locator('.editor-component span.ag-paragraph', { hasText: 'beta' }).click()
  await page.keyboard.press('Meta+ArrowLeft')
}

const pasteText = (text: string): Promise<(string | null)[]> => {
  const dt = new DataTransfer()
  dt.setData('text/plain', text)
  const ev = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })
  const target = document.querySelectorAll('.editor-component span.ag-paragraph')[1]
  target.dispatchEvent(ev)
  return new Promise((resolve) => {
    setTimeout(() => {
      resolve(
        Array.from(document.querySelectorAll('.editor-component span.ag-paragraph')).map(
          (s) => s.textContent
        )
      )
    }, 300)
  })
}

test('copying a line incl. its newline keeps the trailing \\n on the clipboard', async({
  page
}) => {
  await setupTwoLines(page)

  const copied = await page.evaluate(copyLineWithNewline)
  expect(copied.text).toBe('alpha\n')
})

test('pasting "alpha\\n" at the start of a line inserts a line instead of merging', async({
  page
}) => {
  await setupTwoLines(page)
  await caretToStartOfBeta(page)

  const paragraphs = await page.evaluate(pasteText, 'alpha\n')
  expect(paragraphs).toEqual(['alpha', 'alpha', 'beta'])

  // The caret stays at the start of the pushed-down line.
  await page.keyboard.type('X', { delay: 0 })
  await expect(page.locator('.editor-component')).toContainText('Xbeta')
})

test('pasting multiple whole lines at a line start inserts them all', async({ page }) => {
  await setupTwoLines(page)
  await caretToStartOfBeta(page)

  const paragraphs = await page.evaluate(pasteText, 'one\n\ntwo\n')
  expect(paragraphs).toEqual(['alpha', 'one', 'two', 'beta'])
})

test('pasting "alpha" (no trailing newline) at a line start still merges', async({ page }) => {
  await setupTwoLines(page)
  await caretToStartOfBeta(page)

  const paragraphs = await page.evaluate(pasteText, 'alpha')
  expect(paragraphs).toEqual(['alpha', 'alphabeta'])
})

test('pasting a whole line onto the EMPTY last line keeps the empty line', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'alpha')
  await page.keyboard.press('Enter')
  await page.keyboard.type('beta', { delay: 0 })
  await page.keyboard.press('Enter')
  // caret now sits on the empty last line

  const paragraphs = await page.evaluate((text: string) => {
    const dt = new DataTransfer()
    dt.setData('text/plain', text)
    const ev = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })
    const spans = document.querySelectorAll('.editor-component span.ag-paragraph')
    spans[spans.length - 1].dispatchEvent(ev)
    return new Promise((resolve) => {
      setTimeout(() => {
        resolve(
          Array.from(document.querySelectorAll('.editor-component span.ag-paragraph')).map(
            (s) => s.textContent
          )
        )
      }, 300)
    })
  }, 'alpha\n')
  expect(paragraphs).toEqual(['alpha', 'beta', 'alpha', ''])
})

test('muya→muya round trip: copy line incl. newline, paste at another line start', async({
  page
}) => {
  await setupTwoLines(page)

  const copied = await page.evaluate(copyLineWithNewline)
  await caretToStartOfBeta(page)
  const paragraphs = await page.evaluate(pasteText, copied.text)
  expect(paragraphs).toEqual(['alpha', 'alpha', 'beta'])
})
