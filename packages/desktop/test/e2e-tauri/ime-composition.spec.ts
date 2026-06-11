// IME composition against @muyajs/core under WebKit.
//
// WKWebView delivers the IME-commit Enter as a keydown with keyCode 229
// AFTER compositionend has already cleared the engine's isComposed flag
// (the legacy muyajs engine needed the same guard). These specs replay the
// engine-boundary event shapes; full IME behavior is verified interactively
// on the real app (Phase 4 QA).
import { expect, test, type Page } from '@playwright/test'
import { launchEditor, PARAGRAPH_CONTENT, typeIntoEditor } from './helpers'

const paragraphTexts = async(page: Page): Promise<string[]> =>
  (await page.locator(PARAGRAPH_CONTENT).allTextContents()).map((t) => t.trim())

// Dispatch a keydown shaped like the WKWebView IME-commit Enter:
// key 'Enter', keyCode 229, isComposing false (compositionend already fired).
const dispatchImeCommitEnter = (selector: string): void => {
  const target = document.querySelector(selector)!
  const ev = new KeyboardEvent('keydown', {
    key: 'Enter',
    bubbles: true,
    cancelable: true,
    composed: true
  })
  Object.defineProperty(ev, 'keyCode', { get: () => 229 })
  target.dispatchEvent(ev)
}

// Dispatch a keydown Enter still flagged as composing (Chromium shape).
const dispatchComposingEnter = (selector: string): void => {
  const target = document.querySelector(selector)!
  const ev = new KeyboardEvent('keydown', {
    key: 'Enter',
    bubbles: true,
    cancelable: true,
    composed: true,
    isComposing: true
  })
  target.dispatchEvent(ev)
}

test('the WKWebView IME-commit Enter (keyCode 229) does not split the paragraph', async({
  page
}) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'にほんご')

  await page.evaluate(dispatchImeCommitEnter, PARAGRAPH_CONTENT)

  const texts = await paragraphTexts(page)
  expect(texts).toEqual(['にほんご'])
})

test('an Enter with isComposing=true does not split the paragraph', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'へんかんちゅう')

  await page.evaluate(dispatchComposingEnter, PARAGRAPH_CONTENT)

  const texts = await paragraphTexts(page)
  expect(texts).toEqual(['へんかんちゅう'])
})

test('a real Enter after the IME commit still splits the paragraph', async({ page }) => {
  await launchEditor(page)
  await typeIntoEditor(page, 'にほんご')

  await page.evaluate(dispatchImeCommitEnter, PARAGRAPH_CONTENT)
  await page.keyboard.press('Enter')
  await page.keyboard.type('つぎのぎょう', { delay: 0 })

  const texts = await paragraphTexts(page)
  expect(texts).toContain('にほんご')
  expect(texts).toContain('つぎのぎょう')
})

test('composition commit via compositionend persists the text', async({ page }) => {
  await launchEditor(page)
  await page.click(PARAGRAPH_CONTENT)

  // Replay the engine-boundary composition sequence: compositionstart →
  // DOM text mutation + input(insertCompositionText) → compositionend.
  // The engine must ignore the mid-composition input and commit the DOM
  // text on compositionend.
  await page.evaluate((selector: string) => {
    const target = document.querySelector(selector)!
    target.dispatchEvent(
      new CompositionEvent('compositionstart', { bubbles: true, cancelable: true })
    )
    target.textContent = '日本語入力'
    // A real IME keeps the caret inside the composed text; replacing
    // textContent dropped the DOM selection, so restore it before the
    // commit (the engine reads the cursor during inputHandler).
    const textNode = target.firstChild!
    const len = textNode.textContent?.length ?? 0
    window.getSelection()?.setBaseAndExtent(textNode, len, textNode, len)
    const input = new InputEvent('input', {
      bubbles: true,
      cancelable: false,
      inputType: 'insertCompositionText',
      data: '日本語入力'
    })
    target.dispatchEvent(input)
    target.dispatchEvent(
      new CompositionEvent('compositionend', {
        bubbles: true,
        cancelable: true,
        data: '日本語入力'
      })
    )
  }, PARAGRAPH_CONTENT)

  const texts = await paragraphTexts(page)
  expect(texts).toEqual(['日本語入力'])

  // The committed text survives a model round-trip (Enter + more typing).
  await page.keyboard.press('Enter')
  await page.keyboard.type('えいご', { delay: 0 })
  const after = await paragraphTexts(page)
  expect(after).toContain('日本語入力')
  expect(after).toContain('えいご')
})
