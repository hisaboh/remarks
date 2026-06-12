// hisaboh/remarks#2: Paragraph-menu formats (and their shortcuts) must apply
// to the block holding the caret. The command flow re-focuses the editor
// first (focusEditorAndExecute), and Editor.focus() used to reset the caret
// to the FIRST block — so "Heading 1" always converted line 1.
import { expect, test, type Page } from '@playwright/test'
import { launchEditor, PARAGRAPH_CONTENT, typeIntoEditor } from './helpers'

const executeCommand = (page: Page, id: string) =>
  page.evaluate((commandId: string) => {
    const w = window as unknown as {
      __TAURI_MOCK__: { emit: (e: string, p?: unknown) => void }
    }
    w.__TAURI_MOCK__.emit('mt::execute-command-by-id', commandId)
  }, id)

test('heading-1 command converts the caret line, not the first line', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'first line')
  await page.keyboard.press('Enter')
  await page.keyboard.type('second line', { delay: 0 })
  // Caret sits on "second line".

  await executeCommand(page, 'paragraph.heading-1')
  // focusEditorAndExecute defers the action by 150ms.
  await page.waitForTimeout(500)

  const result = await page.evaluate(() => ({
    headings: Array.from(document.querySelectorAll('.editor-component .mu-atx-heading')).map(
      (h) => (h as HTMLElement).innerText.trim()
    ),
    paragraphs: Array.from(
      document.querySelectorAll('.editor-component .mu-paragraph-content')
    ).map((p) => p.textContent)
  }))
  expect(result.headings).toEqual(['second line'])
  expect(result.paragraphs).toEqual(['first line'])
})

test('quote-block command wraps the caret line', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'first line')
  await page.keyboard.press('Enter')
  await page.keyboard.type('second line', { delay: 0 })

  await executeCommand(page, 'paragraph.quote-block')
  await page.waitForTimeout(500)

  const quoted = await page.evaluate(() =>
    Array.from(document.querySelectorAll('.editor-component .mu-block-quote')).map((b) =>
      (b as HTMLElement).innerText.trim()
    )
  )
  expect(quoted).toEqual(['second line'])
})

test('typing still works after a command refocuses the editor', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'first line')
  await page.keyboard.press('Enter')
  await page.keyboard.type('second', { delay: 0 })

  await executeCommand(page, 'paragraph.heading-2')
  await page.waitForTimeout(500)
  await page.keyboard.type(' more', { delay: 0 })

  const headings = await page.evaluate(() =>
    Array.from(document.querySelectorAll('.editor-component .mu-atx-heading')).map((h) =>
      (h as HTMLElement).innerText.trim()
    )
  )
  expect(headings).toEqual(['second more'])
})
