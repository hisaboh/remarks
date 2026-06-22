// Compile the macOS "Liquid Glass" app icon (Icon Composer .icon bundle) into
// the asset-catalog form the system needs, so packaged macOS builds ship the
// dynamic Tahoe icon instead of a flat .icns.
//
// Source : docs/icon/remarks.icon  (layered PNGs + icon.json, edited in Xcode's
//          Icon Composer)
// Output : src-tauri/icons/Assets.car   — compiled catalog containing an icon
//                                          named `remarks` (CFBundleIconName)
//          src-tauri/icons/remarks.icns  — flat fallback render for < macOS 26
//
// Both outputs are git-ignored and regenerated here; tauri.conf.json copies
// them into Contents/Resources/ via `bundle.resources`, and src-tauri/Info.plist
// points CFBundleIconName/CFBundleIconFile at `remarks`.
//
// Runs before `tauri dev` / `tauri build` (see the pre* scripts). Requires
// macOS + Xcode (actool). On any other platform, or when actool / the source
// bundle is missing, it skips quietly — the macOS Tahoe icon only applies to
// the macOS bundle, which is the only Tauri target this fork ships.

import { execFileSync } from 'node:child_process'
import { existsSync, mkdtempSync, copyFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '..', '..', '..')
const iconSource = resolve(repoRoot, 'docs', 'icon', 'remarks.icon')
const iconsDir = resolve(here, '..', 'src-tauri', 'icons')

// The dynamic icon is a macOS-only concept and actool ships with Xcode.
if (process.platform !== 'darwin') {
  console.log('[setup-app-icon] not macOS — skipping Liquid Glass icon build')
  process.exit(0)
}

if (!existsSync(iconSource)) {
  console.log(`[setup-app-icon] no icon source at ${iconSource} — skipping`)
  process.exit(0)
}

let hasActool = false
try {
  execFileSync('xcrun', ['--find', 'actool'], { stdio: 'ignore' })
  hasActool = true
} catch {
  hasActool = false
}
if (!hasActool) {
  console.warn('[setup-app-icon] actool not found (install Xcode) — skipping')
  process.exit(0)
}

// actool writes Assets.car + <name>.icns + a partial Info.plist. We only keep
// the catalog and the fallback .icns; the icon name (`remarks`) is what the
// merged src-tauri/Info.plist references via CFBundleIconName.
const stage = mkdtempSync(join(tmpdir(), 'remarks-icon-'))
try {
  execFileSync(
    'xcrun',
    [
      'actool',
      iconSource,
      '--compile',
      stage,
      '--app-icon',
      'remarks',
      '--output-partial-info-plist',
      join(stage, 'partial.plist'),
      '--platform',
      'macosx',
      '--minimum-deployment-target',
      '26.0',
      '--target-device',
      'mac',
      '--output-format',
      'human-readable-text'
    ],
    { stdio: ['ignore', 'ignore', 'inherit'] }
  )

  for (const name of ['Assets.car', 'remarks.icns']) {
    const from = join(stage, name)
    if (!existsSync(from)) {
      console.error(`[setup-app-icon] actool did not produce ${name}`)
      process.exit(1)
    }
    copyFileSync(from, join(iconsDir, name))
  }
  console.log(`[setup-app-icon] ${iconSource} -> ${iconsDir}/{Assets.car,remarks.icns}`)
} finally {
  rmSync(stage, { recursive: true, force: true })
}
