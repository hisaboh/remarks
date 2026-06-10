import * as path from 'node:path'
import { defineConfig, devices } from '@playwright/test'

// Tauri renderer E2E — runs the real renderer (vite dev server) in Playwright
// WebKit with the Rust backend mocked (see tauri-mock.ts). WebKit is the
// engine family WKWebView uses, so engine-specific behavior (IME composition,
// selection) is exercised where it actually differs from Chromium/Electron.
//
// Real-app E2E via tauri-driver remains a Linux/Windows CI follow-up:
// tauri-driver has no macOS support (there is no WKWebView WebDriver).

const desktopRoot = path.resolve(__dirname, '../..')

export default defineConfig({
  workers: 1,
  testMatch: '**/*.spec.ts',
  timeout: 30000,
  use: {
    baseURL: 'http://localhost:5173',
    headless: true,
    viewport: { width: 1280, height: 720 }
  },
  projects: [
    {
      name: 'webkit',
      use: { ...devices['Desktop Safari'] }
    }
  ],
  webServer: {
    command: 'pnpm run dev:renderer',
    cwd: desktopRoot,
    url: 'http://localhost:5173',
    reuseExistingServer: true,
    timeout: 60000
  }
})
