import { expect, test } from '@playwright/test'
import { launchEditor, typeIntoEditor } from './helpers'

// Regression coverage for the WKWebView IME composition rework
// (muya keyboard.js commitComposition). Playwright cannot drive a real input
// method, so these tests replay the exact DOM-mutation + event sequences
// logged from WKWebView with the macOS Japanese IME:
//
//  A. Enter-commit into an EMPTY paragraph: WebKit's commit destroys the
//     content span and leaves the committed text directly under the outer
//     block — the commit must repair the span and keep the text.
//  B. Commit with the caret still in the paragraph (mid-text insertion).
//  C. compositionend firing after the selection already left the paragraph
//     (click-away commit) — the model must still receive the text and the
//     caret must NOT be yanked back.

test('A: empty-paragraph commit survives WebKit destroying the content span', async({
  page
}) => {
  await launchEditor(page)

  const spanId = await page.evaluate(() => {
    const placeCaret = (node: Node, offset: number): void => {
      const range = document.createRange()
      range.setStart(node, offset)
      range.collapse(true)
      const sel = window.getSelection()!
      sel.removeAllRanges()
      sel.addRange(range)
    }
    const container = document.querySelector('.editor-component') as HTMLElement
    const span = container.querySelector('span.ag-paragraph') as HTMLElement
    const id = span.id
    const outer = span.parentElement as HTMLElement

    // Caret in the empty span, composition begins.
    placeCaret(span, 0)
    container.dispatchEvent(
      new CompositionEvent('compositionstart', { bubbles: true, data: '' })
    )

    // IME writes the preedit text into the span.
    const preedit = document.createTextNode('日本語')
    span.appendChild(preedit)
    placeCaret(preedit, 3)

    // WebKit's commit (deleteCompositionText → insertFromComposition):
    // the emptied span is destroyed; the committed text lands as a bare
    // text node directly under the outer block.
    const committed = document.createTextNode('日本語')
    outer.replaceChild(committed, span)
    placeCaret(committed, 3)

    container.dispatchEvent(
      new CompositionEvent('compositionend', { bubbles: true, data: '日本語' })
    )
    return id
  })

  // The deferred commit must repair the destroyed span (same block key) and
  // keep the committed text.
  await expect(page.locator(`#${spanId}`)).toContainText('日本語', { timeout: 5000 })

  // The repaired caret must allow an immediate Enter to split the paragraph.
  await page.keyboard.press('Enter')
  await page.keyboard.type('next', { delay: 0 })
  await expect(page.locator('.editor-component')).toContainText('日本語')
  await expect(page.locator('.editor-component')).toContainText('next')

  // A render-triggering click elsewhere must not wipe the committed text.
  await page.locator('.editor-component span.ag-paragraph').last().click()
  await expect(page.locator('.editor-component')).toContainText('日本語')
})

test('B: mid-text commit goes through the normal input pipeline', async({ page }) => {
  await launchEditor(page)

  await typeIntoEditor(page, 'helloworld')

  await page.evaluate(() => {
    const placeCaret = (node: Node, offset: number): void => {
      const range = document.createRange()
      range.setStart(node, offset)
      range.collapse(true)
      const sel = window.getSelection()!
      sel.removeAllRanges()
      sel.addRange(range)
    }
    const container = document.querySelector('.editor-component') as HTMLElement
    const span = container.querySelector('span.ag-paragraph') as HTMLElement
    // The typed text may sit in a nested element — find the text node that
    // actually holds it.
    const walker = document.createTreeWalker(span, NodeFilter.SHOW_TEXT)
    let textNode: Text | null = null
    while (walker.nextNode()) {
      const node = walker.currentNode as Text
      if (node.data.includes('helloworld')) {
        textNode = node
        break
      }
    }
    if (!textNode) throw new Error('typed text node not found')

    // Caret after "hello", compose, commit in place (span survives — the
    // composition was not the span's only content).
    placeCaret(textNode, 5)
    container.dispatchEvent(
      new CompositionEvent('compositionstart', { bubbles: true, data: '' })
    )
    textNode.insertData(5, '世界')
    placeCaret(textNode, 7)
    container.dispatchEvent(
      new CompositionEvent('compositionend', { bubbles: true, data: '世界' })
    )
  })

  await expect(page.locator('.editor-component')).toContainText('hello世界world', {
    timeout: 5000
  })

  // The commit left the caret after 世界 (offset 7): Enter must split exactly
  // there — proving the model cursor tracked the committed text — and the
  // text must survive the model-driven re-render of the click below.
  await page.keyboard.press('Enter')
  await page.keyboard.type('other', { delay: 0 })
  await page.locator('.editor-component span.ag-paragraph').first().click()
  await expect(page.locator('.editor-component')).toContainText('hello世界')
  await expect(page.locator('.editor-component')).toContainText('otherworld')
})

test('C: click-away commit syncs the model without yanking the caret back', async({
  page
}) => {
  await launchEditor(page)

  // Two paragraphs: compose into the second, click-away into the first.
  await typeIntoEditor(page, 'anchor paragraph')
  await page.keyboard.press('Enter')

  await page.evaluate(() => {
    const placeCaret = (node: Node, offset: number): void => {
      const range = document.createRange()
      range.setStart(node, offset)
      range.collapse(true)
      const sel = window.getSelection()!
      sel.removeAllRanges()
      sel.addRange(range)
    }
    const container = document.querySelector('.editor-component') as HTMLElement
    const spans = container.querySelectorAll('span.ag-paragraph')
    const target = spans[spans.length - 1] as HTMLElement
    const away = spans[0] as HTMLElement

    placeCaret(target, 0)
    container.dispatchEvent(
      new CompositionEvent('compositionstart', { bubbles: true, data: '' })
    )
    const preedit = document.createTextNode('テスト')
    target.appendChild(preedit)
    placeCaret(preedit, 3)

    // Click-away: WKWebView moves the selection first, then ends the
    // composition with EMPTY data while the text stays in the DOM
    // (observed sequence). The commit must sync the model from the DOM and
    // leave the selection where the user clicked.
    placeCaret(away.firstChild ?? away, 0)
    container.dispatchEvent(
      new CompositionEvent('compositionend', { bubbles: true, data: '' })
    )
  })

  // Model received the text: a render-triggering click must not wipe it.
  await page.locator('.editor-component span.ag-paragraph').first().click()
  await expect(page.locator('.editor-component')).toContainText('テスト', { timeout: 5000 })

  // The caret stayed at the click-away target (typing lands in paragraph 1).
  await page.keyboard.type('X', { delay: 0 })
  const firstText = await page
    .locator('.editor-component span.ag-paragraph')
    .first()
    .textContent()
  expect(firstText).toContain('X')
})
