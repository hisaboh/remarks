// Release build wrapper for `tauri build` (Phase 6).
//
// bundle.createUpdaterArtifacts needs the updater's minisign private key in
// TAURI_SIGNING_PRIVATE_KEY(_PATH). For local builds, fall back to the
// developer's key at ~/.tauri/marktext.key when the env is not already set
// (CI sets TAURI_SIGNING_PRIVATE_KEY from a secret instead).
//
// macOS code signing / notarization stay fully env-driven (picked up by the
// Tauri bundler when present, skipped otherwise):
//   APPLE_SIGNING_IDENTITY  e.g. "Developer ID Application: Name (TEAMID)"
//   APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID   (notarization)

import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

const env = { ...process.env }

if (!env.TAURI_SIGNING_PRIVATE_KEY) {
  const keyPath = join(homedir(), '.tauri', 'marktext.key')
  if (existsSync(keyPath)) {
    // The bundler's updater signing only reads TAURI_SIGNING_PRIVATE_KEY
    // (key content; the *_PATH variant is not honored at build time).
    env.TAURI_SIGNING_PRIVATE_KEY = readFileSync(keyPath, 'utf-8')
    // The key was generated without a password (`tauri signer generate --ci`);
    // an explicit empty password skips the interactive prompt, which would
    // otherwise fail in non-TTY builds ("Device not configured").
    env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ??= ''
    console.log(`[build-tauri] using updater signing key at ${keyPath}`)
  } else {
    console.error(
      '[build-tauri] no updater signing key (TAURI_SIGNING_PRIVATE_KEY[_PATH] unset and ' +
        '~/.tauri/marktext.key missing) — `createUpdaterArtifacts` will fail.\n' +
        '[build-tauri] generate one with: pnpm exec tauri signer generate -w ~/.tauri/marktext.key'
    )
    process.exit(1)
  }
}

if (!env.APPLE_SIGNING_IDENTITY && process.platform === 'darwin') {
  console.warn(
    '[build-tauri] APPLE_SIGNING_IDENTITY not set — the bundle will not be code-signed/notarized.'
  )
}

const result = spawnSync('pnpm', ['exec', 'tauri', 'build', ...process.argv.slice(2)], {
  stdio: 'inherit',
  env
})
process.exit(result.status ?? 1)
