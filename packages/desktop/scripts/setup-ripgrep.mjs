// Copy the @vscode/ripgrep binary into src-tauri/binaries/ as a Tauri
// externalBin sidecar (rg-<target-triple>), so packaged builds ship `rg` for
// the sidebar/quick-open search. Runs before `tauri dev` / `tauri build`
// (see the pre* scripts). The binaries/ dir is git-ignored.

import { createRequire } from 'node:module'
import { execSync } from 'node:child_process'
import { copyFileSync, mkdirSync, chmodSync, existsSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const require = createRequire(import.meta.url)

const { rgPath } = require('@vscode/ripgrep')
if (!rgPath || !existsSync(rgPath)) {
  console.error('[setup-ripgrep] @vscode/ripgrep binary not found at', rgPath)
  process.exit(1)
}

// Tauri matches the sidecar named `rg-<target-triple>` (host triple here).
const host = execSync('rustc -Vv').toString().match(/host:\s*(\S+)/)?.[1]
if (!host) {
  console.error('[setup-ripgrep] could not determine the rustc host triple')
  process.exit(1)
}
const ext = process.platform === 'win32' ? '.exe' : ''
const dest = resolve(here, '..', 'src-tauri', 'binaries', `rg-${host}${ext}`)

mkdirSync(dirname(dest), { recursive: true })
copyFileSync(rgPath, dest)
chmodSync(dest, 0o755)
console.log(`[setup-ripgrep] ${rgPath} -> ${dest}`)
