import { resolve, dirname } from 'path'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import svgLoader from 'vite-svg-loader'
import postcssPresetEnv from 'postcss-preset-env'
import packageJson from './package.json' with { type: 'json' }
import { fileURLToPath } from 'url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

// Tauri expects a fixed dev server port and to keep its own console output
// visible (no Vite clear-screen), and to know the host so it can be reached
// from the macOS/Windows/Linux webview.
// See https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  root: resolve(__dirname, 'src/renderer'),
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // Don't watch the Rust backend — Tauri's own watcher restarts the app
      ignored: ['**/src-tauri/**']
    }
  },
  build: {
    outDir: resolve(__dirname, 'out/renderer'),
    emptyOutDir: true,
    // Tauri runs on recent Chromium/Safari/WebKitGTK; target the same as
    // electron.vite.config.ts did so the bundled output is consistent.
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13'
  },
  define: {
    MARKTEXT_VERSION: JSON.stringify(packageJson.version),
    MARKTEXT_VERSION_STRING: JSON.stringify(`v${packageJson.version}`),
    // Some bundled deps (e.g. `custom-event` via `dragula`) reference the
    // Node-only `global` at module load — undefined in the Tauri webview.
    // Substitute it with `globalThis` at build time so the imports don't
    // throw before Vue mounts.
    global: 'globalThis'
  },
  assetsInclude: ['**/*.md'],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src/renderer/src'),
      common: resolve(__dirname, 'src/common'),
      muya: resolve(__dirname, '../muyajs'),
      '@shared': resolve(__dirname, 'src/shared'),
      path: 'pathe'
    },
    extensions: ['.mjs', '.ts', '.js', '.json', '.vue']
  },
  optimizeDeps: {
    include: ['pako', 'pathe'],
    esbuildOptions: {
      define: {
        global: 'globalThis'
      }
    }
  },
  plugins: [vue(), svgLoader()],
  css: {
    postcss: {
      plugins: [
        postcssPresetEnv({
          stage: 0,
          features: { 'nesting-rules': true }
        })
      ]
    }
  }
})
