import { expect, test } from '@playwright/test'
import { expectEditorContains, launchEditor, typeIntoEditor } from './helpers'

test('types plain text into the blank document', async({ page }) => {
  await launchEditor(page)

  await typeIntoEditor(page, 'hello webkit world')
  await expectEditorContains(page, 'hello webkit world')
})

test('Enter splits the paragraph', async({ page }) => {
  await launchEditor(page)

  await typeIntoEditor(page, 'first line')
  await page.keyboard.press('Enter')
  await page.keyboard.type('second line', { delay: 0 })

  const texts = await page
    .locator('.editor-component span.ag-paragraph')
    .allTextContents()
  const nonEmpty = texts.map((t) => t.trim()).filter(Boolean)
  expect(nonEmpty).toContain('first line')
  expect(nonEmpty).toContain('second line')
})

test('“# ” converts the paragraph into a heading', async({ page }) => {
  await launchEditor(page)

  await typeIntoEditor(page, '# Title')
  await expect(page.locator('.editor-component h1')).toContainText('Title')
})

test('typed content survives clicking another paragraph (model render round-trip)', async({
  page
}) => {
  await launchEditor(page)

  await typeIntoEditor(page, 'persistent text')
  await page.keyboard.press('Enter')
  await page.keyboard.type('other paragraph', { delay: 0 })

  // Click back into the first paragraph — muya re-renders changed blocks from
  // its model, so stale model state would wipe the text from the DOM.
  await page.locator('.editor-component span.ag-paragraph').first().click()
  await expectEditorContains(page, 'persistent text')
  await expectEditorContains(page, 'other paragraph')
})
