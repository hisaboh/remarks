import { expect, test } from '@playwright/test'
import type { ElectronApplication, Page } from 'playwright'
import {
  launchElectron,
  enterSourceMode,
  exitSourceMode,
  sendIpcToRenderer,
  waitForEditor,
  waitForMenuReady
} from './helpers'

// issue #8: 新規タブで文字列を入力し、すぐソースコードモードへ切り替えると入力が
// 消える。muya はインライン編集を次の animation frame までバッチするため、フレーム
// 発火前にソースモードへ入ると currentFile.markdown が空のまま読まれていた。
// fix: ソースモード切替時に editor.flushPendingChanges() で同期確定する。

const readSource = (page: Page) =>
  page.evaluate(() => {
    const cm = document.querySelector('.source-code .CodeMirror') as
      | (Element & { CodeMirror?: { getValue(): string } })
      | null
    return cm && cm.CodeMirror ? cm.CodeMirror.getValue() : ''
  })

// Boot the renderer into a single fresh BLANK untitled tab — i.e. the user's
// "新しいタブを開く". The e2e harness's CLI-open path does not initialize the
// editor on this branch, so we drive the same `mt::bootstrap-editor` IPC the
// main process normally sends on did-finish-load.
const launchWithBlankTab = async(): Promise<{ app: ElectronApplication; page: Page }> => {
  const { app, page } = await launchElectron()
  await page.waitForTimeout(800)
  await sendIpcToRenderer(app, 'mt::bootstrap-editor', {
    addBlankTab: true,
    markdownList: [],
    lineEnding: 'lf',
    sideBarVisibility: false,
    tabBarVisibility: true,
    sourceCodeModeEnabled: false
  })
  await waitForEditor(page)
  await waitForMenuReady(app)
  return { app, page }
}

test.describe('issue #8 — source mode after typing in a new tab', () => {
  test('typing then entering source mode BEFORE the rAF flush keeps content', async() => {
    const { app, page } = await launchWithBlankTab()
    try {
      // Freeze the engine's batched json-change: muya schedules the state
      // commit via requestAnimationFrame. Queue (never run) those callbacks so
      // we deterministically reproduce "switched to source mode within the same
      // frame as the keystroke" — the exact race the user hits manually.
      await page.evaluate(() => {
        const w = window as unknown as {
          __rafQueue: FrameRequestCallback[]
          requestAnimationFrame: typeof window.requestAnimationFrame
        }
        w.__rafQueue = []
        w.requestAnimationFrame = ((cb: FrameRequestCallback) => {
          w.__rafQueue.push(cb)
          return 0
        }) as typeof window.requestAnimationFrame
      })

      await page.click('.editor-component', { timeout: 5000 })
      await page.keyboard.type('Hello issue 8', { delay: 0 })

      // Enter source mode while the json-change rAF is still queued.
      await enterSourceMode(page, app)

      const value = await readSource(page)
      console.log('[issue-8][race] source value =', JSON.stringify(value))
      expect(value).toContain('Hello issue 8')
    } finally {
      await app.close()
    }
  })

  test('round trip (muya -> source -> muya) preserves typed content', async() => {
    const { app, page } = await launchWithBlankTab()
    try {
      await page.click('.editor-component', { timeout: 5000 })
      await page.keyboard.type('Round trip text', { delay: 0 })

      await enterSourceMode(page, app)
      const inSource = await readSource(page)
      console.log('[issue-8][roundtrip] source value =', JSON.stringify(inSource))
      expect(inSource).toContain('Round trip text')

      await exitSourceMode(page, app)
      const backInMuya = await page.evaluate(
        () => document.querySelector('.editor-component')?.textContent ?? ''
      )
      console.log('[issue-8][roundtrip] muya text =', JSON.stringify(backInMuya))
      expect(backInMuya).toContain('Round trip text')
    } finally {
      await app.close()
    }
  })
})
