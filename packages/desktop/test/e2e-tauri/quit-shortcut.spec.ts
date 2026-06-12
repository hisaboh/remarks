import { expect, test } from '@playwright/test'
import { launchEditor, clickFirstParagraph } from './helpers'

// issue #3: Cmd+Q がアプリ終了に繋がらない。renderer の keybinding が Cmd+Q を
// file.quit にディスパッチし `mt::app-try-quit` を送るが、Tauri 側に未配線で no-op
// だった。fix で `mt::app-try-quit` → `app_try_quit` コマンドにマッピング。
//
// macOS には実 Tauri を駆動する WebDriver が無いため、ここでは実レンダラバンドルを
// WebKit + Rust モックで動かし、「Cmd+Q を押すと app_try_quit が invoke される」
// 配線（= no-op だった核心部分）を検証する。実際のプロセス終了/未保存ダイアログは
// Rust 側のユニットで担保 + 手動確認。

const countInvocations = (page: import('@playwright/test').Page, cmd: string) =>
  page.evaluate((command) => {
    const mock = (
      window as unknown as { __TAURI_MOCK__: { invocations: Array<[string, unknown]> } }
    ).__TAURI_MOCK__
    return mock.invocations.filter(([c]) => c === command).length
  }, cmd)

test('Cmd+Q dispatches file.quit and invokes app_try_quit (issue #3)', async({ page }) => {
  await launchEditor(page)
  await clickFirstParagraph(page)

  // Before: no quit attempt yet.
  expect(await countInvocations(page, 'app_try_quit')).toBe(0)

  await page.keyboard.press('Meta+KeyQ')

  // The chain is async: keydown → emit_to(mt::execute-command-by-id) →
  // file.quit command → send(mt::app-try-quit) → invoke(app_try_quit).
  await page.waitForFunction(
    () => {
      const mock = (
        window as unknown as { __TAURI_MOCK__: { invocations: Array<[string, unknown]> } }
      ).__TAURI_MOCK__
      return mock.invocations.some(([c]) => c === 'app_try_quit')
    },
    null,
    { timeout: 3000 }
  )

  const invoked = await countInvocations(page, 'app_try_quit')
  console.log('[issue-3] app_try_quit invocations =', invoked)
  expect(invoked).toBeGreaterThanOrEqual(1)
})
