// hisaboh/remarks#7: Converting a non-empty paragraph to a table / code block /
// math block / HTML block from the paragraph menu must carry the existing text
// into the new block instead of dropping it.
//
// Root cause was in @muyajs/core: `replaceBlockByLabel` only copied the source
// text for `paragraph`/`block-quote` targets, and `createTable` always built an
// empty table. Code/math/html now seed their `text`, and the table seeds its
// first header cell.
import { expect, test, type Page } from '@playwright/test'
import { launchEditor, typeIntoEditor } from './helpers'

const executeCommand = (page: Page, id: string) =>
  page.evaluate((commandId: string) => {
    const w = window as unknown as {
      __TAURI_MOCK__: { emit: (e: string, p?: unknown) => void }
    }
    w.__TAURI_MOCK__.emit('mt::execute-command-by-id', commandId)
  }, id)

const blockText = (page: Page, blockSelector: string) =>
  page.evaluate((sel: string) => {
    const el = document.querySelector(`.editor-component ${sel}`) as HTMLElement | null
    return el ? el.innerText : null
  }, blockSelector)

for (const { cmd, label, selector } of [
  { cmd: 'paragraph.code-fence', label: 'code block', selector: '.mu-code-block' },
  { cmd: 'paragraph.math-formula', label: 'math block', selector: '.mu-math-block' },
  { cmd: 'paragraph.html-block', label: 'HTML block', selector: '.mu-html-block' }
]) {
  test(`converting a paragraph to a ${label} keeps the text`, async({ page }) => {
    await launchEditor(page)
    await typeIntoEditor(page, 'abcdefg')

    await executeCommand(page, cmd)
    // focusEditorAndExecute defers the action by ~150ms.
    await page.waitForTimeout(500)

    // The target block exists and still contains the original text.
    await expect(page.locator(`.editor-component ${selector}`)).toHaveCount(1)
    const text = await blockText(page, selector)
    expect(text).toContain('abcdefg')
    // The text must not survive merely as a leftover paragraph.
    await expect(page.locator('.editor-component .mu-paragraph-content')).toHaveCount(0)
  })
}

test('converting a paragraph to a table keeps the text in the first cell', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'abcdefg')

  // The table command opens a size dialog; confirm it with the default size.
  await executeCommand(page, 'paragraph.table')
  const okButton = page.locator('.ag-insert-table-dialog .el-button--primary')
  await okButton.waitFor({ state: 'visible', timeout: 5000 })
  await okButton.click()
  await page.waitForTimeout(500)

  await expect(page.locator('.editor-component .mu-table')).toHaveCount(1)
  const firstCell = await page.evaluate(() => {
    const cell = document.querySelector(
      '.editor-component .mu-table .mu-table-cell, .editor-component .mu-table td, .editor-component .mu-table th'
    ) as HTMLElement | null
    return cell ? cell.innerText.trim() : null
  })
  expect(firstCell).toBe('abcdefg')
  await expect(page.locator('.editor-component .mu-paragraph-content')).toHaveCount(0)
})
