import { expect, test, type Page } from '@playwright/test'
import { launchEditor, PARAGRAPH_CONTENT, clickFirstParagraph } from './helpers'

const TAB = '.editor-tabs .tabs-container li'

const caretState = async(page: Page) =>
  page.evaluate((sel) => {
    const a = window.getSelection()?.anchorNode
    const el = a ? (a.nodeType === 1 ? (a as Element) : a.parentElement) : null
    const ae = document.activeElement as HTMLElement | null
    return {
      inParagraph: !!el?.closest(sel),
      activeIsEditor: !!ae?.closest('[contenteditable="true"]')
    }
  }, PARAGRAPH_CONTENT)

const fastCycleTabs = async(page: Page, count: number) => {
  await page.keyboard.down('Control')
  for (let i = 0; i < count; i++) {
    await page.keyboard.press('Tab')
  }
  await page.keyboard.up('Control')
}

test('issue#17: fast Ctrl+Tab cycling does not insert tab text or lose paragraph caret', async({
  page
}) => {
  await launchEditor(page, {
    bootstrapConfig: {
      addBlankTab: false,
      markdownList: ['first tab', 'second tab'],
      filesToOpen: [],
      restoreState: null,
      lineEnding: 'lf',
      sideBarVisibility: false,
      tabBarVisibility: true,
      sourceCodeModeEnabled: false
    }
  })
  await expect(page.locator(TAB)).toHaveCount(2)
  await page.locator(TAB).nth(1).click()
  await expect(page.locator(PARAGRAPH_CONTENT).first()).toHaveText('second tab')
  await clickFirstParagraph(page)

  await fastCycleTabs(page, 12)
  await page.waitForTimeout(250)

  const text = await page.locator(PARAGRAPH_CONTENT).first().textContent()
  // Muya represents inserted Tab as NBSP indentation; plain "\t" is checked too
  // so the assertion describes the user-visible regression.
  expect(text).toBe('second tab')
  expect(text).not.toContain('\u00a0')
  expect(text).not.toContain('\t')

  const caret = await caretState(page)
  expect(caret.inParagraph).toBe(true)
  expect(caret.activeIsEditor).toBe(true)
})
