import * as fs from 'node:fs'
import * as path from 'node:path'
import { expect, type Page } from '@playwright/test'
import { installTauriMock, type TauriMockConfig } from './tauri-mock'

const desktopRoot = path.resolve(__dirname, '../..')

// Build the backend snapshot the mock serves: real preference defaults (the
// same file the Rust store embeds) plus minimal editor-window bootstrap data.
export const buildMockConfig = (overrides: Partial<TauriMockConfig> = {}): TauriMockConfig => {
  const preferences = JSON.parse(
    fs.readFileSync(path.join(desktopRoot, 'static/preference.json'), 'utf-8')
  ) as Record<string, unknown>
  return {
    preferences,
    dataCenter: {
      imageFolderPath: '/tmp/marktext-e2e/images',
      screenshotFolderPath: '/tmp/marktext-e2e/screenshot'
    },
    bootInfo: {
      platform: 'darwin',
      arch: 'arm64',
      versions: { app: '0.0.0-e2e', tauri: '2.0.0' },
      env: {},
      paths: {
        resources: '/tmp/marktext-e2e/resources',
        userData: '/tmp/marktext-e2e/userData',
        cwd: '/tmp/marktext-e2e',
        ripgrepBinary: ''
      },
      isUpdatable: false,
      MARKDOWN_INCLUSIONS: ['*.md']
    },
    // Mirrors window.rs build_params for the main editor window.
    initArgs: {
      udp: '/tmp/marktext-e2e/userData',
      debug: '0',
      wid: '1',
      type: 'editor',
      cff: 'DejaVu Sans Mono',
      cfs: '14',
      hsb: '0',
      theme: 'light',
      tbs: 'custom'
    },
    // Mirrors editor.rs BootstrapConfig (camelCase serde) for a blank launch.
    bootstrapConfig: {
      addBlankTab: true,
      markdownList: [],
      filesToOpen: [],
      restoreState: null,
      lineEnding: 'lf',
      sideBarVisibility: false,
      tabBarVisibility: false,
      sourceCodeModeEnabled: false
    },
    ...overrides
  }
}

// @muyajs/core paragraph leaf (the editable content element inside a
// `.mu-paragraph` block). Keep selectors here — specs should not hard-code.
export const PARAGRAPH_CONTENT = '.editor-component .mu-paragraph-content'

export const launchEditor = async(
  page: Page,
  overrides: Partial<TauriMockConfig> = {}
): Promise<void> => {
  await page.addInitScript(installTauriMock, buildMockConfig(overrides))
  await page.goto('/')
  await page.waitForSelector('.editor-component', { state: 'attached', timeout: 15000 })
  // A blank launch renders one empty paragraph once muya has booted.
  await page.waitForSelector(PARAGRAPH_CONTENT, { state: 'attached', timeout: 15000 })
}

// Click the first paragraph content to give the editor a caret before typing.
export const clickFirstParagraph = async(page: Page): Promise<void> => {
  await page.click(PARAGRAPH_CONTENT, { timeout: 5000 })
}

export const typeIntoEditor = async(page: Page, text: string): Promise<void> => {
  await clickFirstParagraph(page)
  await page.keyboard.type(text, { delay: 0 })
}

// The first content paragraph of the document (muya renders one empty
// paragraph for a blank untitled tab).
export const firstParagraph = (page: Page) => page.locator(PARAGRAPH_CONTENT).first()

export const expectEditorContains = async(page: Page, text: string): Promise<void> => {
  await expect(page.locator('.editor-component')).toContainText(text)
}
