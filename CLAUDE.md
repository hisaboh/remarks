# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Remarks on Markdown (MarkText fork)

## Project Overview

Remarks on Markdown ("Remarks") is a WYSIWYG markdown editor — a fork of
MarkText. It supports CommonMark, GitHub Flavored Markdown, math (KaTeX),
Mermaid diagrams, PlantUML, and multiple editing modes (focus, typewriter,
source-code).

**Desktop shell migration (this `tauri2.0` branch).** The app is being ported
from Electron to **Tauri 2.0**. Both shells currently coexist: the Vue
renderer is shared, the Tauri backend lives in `packages/desktop/src-tauri/`
(Rust), and the renderer reaches it through a platform bridge
(`src/renderer/src/platform/`) that re-exposes the same `window.electron.*` /
`window.fileUtils.*` surface the Electron preload provided. **Tauri is the
active/target shell here** (build, release, e2e); the Electron `main/` +
`preload/` path is retained until the migration completes. Where this guide
describes Electron-only mechanics, prefer the Tauri equivalent on this branch.

- **Version**: `0.20.0-dev` — see `package.json` (internal package name is
  still `marktext`; the Tauri product name is **Remarks**, identifier
  `io.github.hisaboh.remarks`)
- **License**: MIT
- **Repository (fork)**: https://github.com/hisaboh/remarks (upstream:
  https://github.com/marktext/marktext)

## Tech Stack

| Layer                              | Technology                                                                                                            |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Language                           | TypeScript 5.9 (strict mode); Rust (Tauri backend, `src-tauri/`) — `packages/muyajs/` retained as JS via ambient shim |
| Desktop shell (active)             | **Tauri 2.0** (Rust backend + system WebView; WKWebView on macOS)                                                     |
| Desktop shell (legacy, coexisting) | Electron 42                                                                                                           |
| Build system                       | Vite 7 (renderer) + Tauri CLI (`tauri build`); electron-vite 5 for the Electron path                                  |
| Packaging                          | tauri-action / `tauri build` (`.dmg`, `.app.tar.gz`, updater feed); electron-builder 26 for the Electron path         |
| Frontend framework                 | Vue 3                                                                                                                 |
| State management                   | Pinia 3                                                                                                               |
| Routing                            | Vue Router 4                                                                                                          |
| UI library                         | Element Plus                                                                                                          |
| Unit tests                         | Vitest 4                                                                                                              |
| E2E tests                          | Playwright                                                                                                            |
| Package manager                    | pnpm >=10 workspace (`packageManager: pnpm@10.33.4`)                                                                  |
| Repo layout                        | pnpm monorepo — see Directory Structure                                                                               |
| Node.js minimum                    | >=20.19.0 (PR CI: Node 22.21.1 · release CI: Node 24.14.1)                                                            |

## Directory Structure

This is a pnpm workspace. Three packages live under `packages/`, and the
root holds only shared tooling and CI-facing scripts.

```
<repo-root>/
  package.json              Workspace orchestrator — every CI-facing script
                            proxies to packages/desktop via `pnpm --filter
                            marktext ...`. CI invocations are unchanged.
  pnpm-workspace.yaml       `packages: ['packages/*']` plus allowBuilds.
  pnpm-lock.yaml            Single lockfile, shared across all packages.
  eslint.config.js          Root ESLint v9 flat config (covers desktop +
                            muyajs; website has its own ESLint v8 config
                            and is ignored here).
  scripts/                  Workspace-level scripts. postinstall.ts,
                            minify-locales.ts, generateThirdPartyLicense.ts,
                            validateLicenses.ts, thirdPartyChecker.ts all
                            target packages/desktop internally.
  docs/                     Long-form developer docs.
  dist/                     Packaged installers from electron-builder
                            (git-ignored; electron-builder writes here via
                            `directories.output: ../../dist` so CI artifact
                            globs `dist/*` still apply).
  packages/
    desktop/                The desktop app (package name: "marktext"; Tauri
                            product name "Remarks"). Hosts both the Tauri and
                            the legacy Electron shells over a shared renderer.
      package.json          Holds Vue / Tauri / Electron / build-time deps and
                            the dev/build/test/typecheck scripts. Consumes the
                            editor engine via @muyajs/core (packages/muya).
      src-tauri/            Tauri 2.0 backend (Rust). tauri.conf.json,
                            Cargo.toml, capabilities/, icons/, binaries/
                            (ripgrep sidecar). src/: main.rs, lib.rs (setup +
                            invoke_handler), menu.rs (native menu), and
                            commands/ (window, files, fs, clipboard,
                            preferences, keybindings, …) — the IPC commands the
                            renderer's platform bridge invokes.
      electron.vite.config.ts
      electron-builder.yml  directories.output points at ../../dist (Electron path).
      tsconfig.json / tsconfig.base.json
      vitest.config.ts
      patches/              pnpm patches consumed by patch-package.
      build/                electron-builder resources (icons, entitlements,
                            NSIS scripts).
      static/               Static assets bundled into the app
                            (icons, themes, locales).
      out/                  electron-vite output (git-ignored).
      test/
        unit/               Vitest specs → pnpm test / pnpm test:unit
        e2e/                Electron Playwright specs + playwright.config.ts
                            → pnpm test:e2e
        e2e-tauri/          Tauri renderer E2E — runs the real renderer in
                            Playwright WebKit with the Rust backend mocked
                            (tauri-mock.ts) → pnpm test:e2e:tauri. WKWebView
                            has no WebDriver on macOS, so native menus / OS
                            dialogs aren't automatable here.
      src/
        common/             Pure Node.js utilities usable from main, preload,
                            and renderer.
        main/               Electron main process (IO, native dialogs, window
                            management, auto-updater).
        preload/            Electron preload scripts. The renderer runs
                            sandboxed (contextIsolation: true,
                            nodeIntegration: false, sandbox: true since
                            #4244) — all Node access flows through the typed
                            contextBridge surface in
                            packages/desktop/src/preload/index.ts.
        renderer/           Vue 3 application (editor UI, Pinia stores).
                            Shared by both shells.
          src/
            components/     Vue single-file components.
            store/          Pinia stores (editor.ts, preferences.ts,
                            layout.ts, …).
            pages/          Top-level Vue pages / routes.
            router/         Vue Router configuration.
            platform/       Tauri platform bridge (index.ts, ipc.ts,
                            dragRegion.ts, fileDrop.ts). Maps the `mt::*`
                            IPC channels / `window.electron.*` surface onto
                            Tauri `invoke`/events so the renderer is shell-
                            agnostic. keybinding/ also pushes the effective
                            accelerator map to the native menu from here.
        shared/             Cross-process types (`shared/types/`) and the
                            IPC contract (`shared/types/ipc.ts`).
        types/              Ambient .d.ts declarations.
    muyajs/                 Legacy markdown editor engine
                            (name: "@marktext/muyajs"). Primarily JS + DOM,
                            avoids Electron APIs. Exception:
                            packages/muyajs/lib/parser/render/plantuml.js
                            imports Node's `zlib`. Being retired: the
                            desktop renderer now consumes @muyajs/core
                            (packages/muya) as its editor engine; only a
                            handful of legacy `muya/` alias call sites
                            remain (see #4244 era sandbox work for the
                            boundary tightening).
      lib/
        contentState/       Block structure and document transformations.
        parser/             Markdown parser.
        renderers/          WYSIWYG renderer.
        ui/                 Inline toolbar, emoji picker, etc.
        utils/              Internal utilities.
      themes/               Editor themes (Prism + fonts).
    muya/                   TypeScript rewrite of muya
                            (name: "@muyajs/core"; upstream:
                            https://github.com/marktext/muya). Built on
                            ot-json1 + ot-text-unicode + snabbdom + marked@16
                            + rxjs. Self-contained: own eslint config
                            (antfu), own stylelint, own madge, own vitest
                            spec suites (CommonMark + GFM). Now the editor
                            engine the desktop renderer consumes; legacy
                            packages/muyajs is being retired. See
                            packages/muya/CLAUDE.md for layout and commands.
      src/                  TS source. Public entrypoint src/index.ts.
      test/spec/            CommonMark 0.31 + GFM 0.29-gfm conformance.
      examples/             muya-examples — vite vanilla-TS dev demo
                            (listed in pnpm-workspace.yaml).
      e2e/                  muya-e2e — Playwright suite. CI runs Chromium
                            only via muya-e2e.yml; Firefox + WebKit are
                            wired in playwright.config.ts but deferred
                            until BACKLOG Phase 3 lands engine-independent
                            specs.
    website/                marktext-website (Vite + React 18). Standalone
                            toolchain; depends on @muyajs/core from npm,
                            not on the local muyajs package. Not part of
                            desktop CI today.
      src/ / public/ / build/ / vite.config.ts / tsconfig.json
```

The root has no `src/`, `test/`, `static/`, or `build/` of its own anymore — they all live in `packages/desktop/`.

## Development Workflow

All commands run from the repo root. The root `package.json` proxies every
desktop-specific script to `packages/desktop` via `pnpm --filter marktext`,
so the names and behavior are unchanged from the pre-monorepo layout.

```bash
# Install dependencies (runs scripts/postinstall.ts automatically — patches
# native-keymap, downloads Electron, rebuilds native modules, minifies locales)
pnpm install

# Run in development mode — TAURI (active shell on this branch)
# Builds the Rust backend (cargo) and serves the Vite renderer with HMR.
# Changes to src-tauri/ Rust require a restart; renderer changes hot-reload.
pnpm -C packages/desktop run dev:tauri
# Renderer-only Vite dev server (what the e2e-tauri harness drives), no backend:
pnpm -C packages/desktop run dev:renderer

# Run in development mode — ELECTRON (legacy path)
# Renderer hot-reloads automatically. Pressing Ctrl+R in the dev window reloads
# the renderer (which re-runs the preload script); changes to the main process
# require restarting `pnpm run dev`.
pnpm run dev

# Preview the last electron-vite build (no rebuild). PERF_TESTING=true is set automatically.
pnpm run start

# Build without packaging — fast path for verifying the renderer/main compile
pnpm run build:unpack

# Auto-format the repo with Prettier (separate from `lint`, which only checks)
pnpm run format

# Minify locale files (required for production builds, skip during dev)
pnpm run minify-locales

# Performance debugging — exposes a Node inspector on :5858 against the previewed build
pnpm run perf:inspect       # attach when ready
pnpm run perf:inspect-brk   # break on first line

# Website (not yet wired into CI)
pnpm --filter marktext-website dev      # Vite dev server
pnpm --filter marktext-website build    # static build → packages/website/build/
```

If you need to invoke a script directly inside a package, use
`pnpm --filter <name> <script>` or `pnpm -C packages/<name> <script>`.

### Command execution conventions

- **Prefer repo-root-relative paths; avoid absolute paths in shell commands.**
  Use `packages/desktop/...`, `pnpm -C packages/<pkg> ...`, or
  `pnpm --filter <name> ...` rather than `/Users/.../marktext/...`. Don't
  prefix commands with `cd <repo-root> &&` — that's already the working
  directory, and a `cd` in a compound command forces the call to run
  unsandboxed. (Editor tools like Read/Edit/Write still require absolute
  `file_path` arguments — this convention is about paths written inside Bash
  command strings.)
- **Run commands inside the sandbox by default.** Only disable the sandbox for
  operations that genuinely need it — network access (`gh`, `git push`,
  fetching), binding a dev-server port, or full builds that write outside the
  workspace. Read-only and in-workspace commands stay sandboxed.

## Build Commands

```bash
# Tauri (active shell). Builds the Rust backend + renderer and produces the
# platform bundle (macOS: .dmg + .app.tar.gz + updater signature).
pnpm -C packages/desktop run build:tauri

# Electron (legacy path):
pnpm run build:win    # Windows x64 — NSIS installer + zip
pnpm run build:mac    # macOS x64 + arm64 — DMG + zip
pnpm run build:linux  # Linux — AppImage, snap, deb, rpm, tar.gz
```

The Electron platform build scripts automatically run `minify-locales` and
`electron-rebuild` before packaging.

### Release (Tauri)

Pushing a `vX.Y.Z[-prerelease]` git tag whose version matches
`packages/desktop/package.json` triggers `.github/workflows/release-tauri.yml`:
it builds macOS arm64 via tauri-action and creates a **draft** GitHub Release
with the `.dmg`, `.app.tar.gz`, `.sig`, and `latest.json` (updater feed). The
draft is published manually after checking artifacts; the updater endpoint only
serves published releases. The tag version must equal the app version or CI
fails. Updater signing uses repo secrets (`TAURI_SIGNING_PRIVATE_KEY` +
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`); Apple Developer ID signing/notarization
is a separate follow-up (builds are currently unsigned). See
`docs/release_tauri.md`. The Electron release path (`release.yml`) still exists.

## Testing

```bash
pnpm run test          # All unit tests (Vitest)
pnpm run test:unit     # Unit tests only
pnpm run test:e2e      # Electron end-to-end tests (Playwright)
pnpm -C packages/desktop run test:e2e:tauri  # Tauri renderer E2E (WebKit + Rust mock, test/e2e-tauri/)
pnpm run lint          # ESLint (run before committing; CI enforces)
pnpm run typecheck     # vue-tsc --noEmit (CI enforces)

# muya (the @muyajs/core editor engine) has its own suites:
pnpm -C packages/muya test         # unit (co-located src/**/__tests__)
pnpm -C packages/muya test:spec    # CommonMark + GFM conformance
pnpm -C packages/muya lint:types   # tsc --noEmit

# Run a single spec — paths are relative to packages/desktop. Use `-C` so
# pnpm resolves the spec path inside the desktop package's vitest config.
pnpm -C packages/desktop exec vitest run test/unit/specs/markdown-basic.spec.ts
pnpm -C packages/desktop exec vitest run -t 'partial test name'

# Single Playwright spec (playwright.config.ts lives in test/e2e/)
pnpm -C packages/desktop exec playwright test test/e2e/launch.spec.ts
pnpm -C packages/desktop exec playwright test -g 'partial test name'
```

## Code Style

Enforced by ESLint + Prettier. Run `pnpm run lint` and `pnpm run typecheck` before committing.

- 2-space indentation
- No semicolons
- Single quotes
- TypeScript with `strict: true`; see `packages/website/content/docs/dev/TYPESCRIPT.md`
- Cross-process types live in `packages/desktop/src/shared/types/`; ambient declarations in `packages/desktop/src/types/`
- IPC channels are typed via the contract in `packages/desktop/src/shared/types/ipc.ts`
- The renderer is fully sandboxed — every IPC and Node access goes through `window.electron.*` / `window.fileUtils.*` etc. (typed in `packages/desktop/src/types/global.d.ts`)

## Architecture

The editor engine is the separate `@muyajs/core` workspace package
(`packages/muya`), which the shared Vue renderer (and tests) consume directly.
The legacy `@marktext/muyajs` (`packages/muyajs`) is being retired.

### Tauri model (active shell)

```
Rust backend  (packages/desktop/src-tauri/)
  ├── Full native access (IO, dialogs, menu, updater) via #[tauri::command]s
  ├── lib.rs wires setup + the invoke_handler; menu.rs builds the native menu
  └── One process; manages WebView windows

WebView renderer  (packages/desktop/src/renderer/, same Vue app)
  ├── Runs in the system WebView (WKWebView on macOS — a WebKit family)
  ├── No Electron preload; the platform bridge (src/renderer/src/platform/)
  │   re-exposes window.electron.* / mt::* over Tauri invoke + events
  └── Hosts @muyajs/core (WYSIWYG) and CodeMirror (source-code mode)
```

### Electron model (legacy, coexisting)

All Electron processes live in `packages/desktop/`. The renderer is the same
Vue app; under Electron it talks to `main/` through the `preload/` contextBridge
instead of the Tauri platform bridge.

```
main process  (packages/desktop/src/main/)
  ├── Full Node.js + Electron API access
  ├── IO, file system, native dialogs, auto-updater, spell checker
  ├── One instance per application launch
  └── Controls editor windows via IPC

preload  (packages/desktop/src/preload/)
  ├── Bridge between main and renderer
  ├── Note: editor and preferences windows use contextIsolation: false +
  │   nodeIntegration: true (see packages/desktop/src/main/config.js)
  └── Compiled to CommonJS

renderer  (packages/desktop/src/renderer/)
  ├── One process per editor window (spawned by main)
  ├── Vue 3 + Pinia — all UI state and editor interaction
  ├── Hosts both Muya (WYSIWYG) and CodeMirror (source-code mode)
  └── Compiled to ES Modules only

Muya  (packages/muyajs/)            ← workspace package @marktext/muyajs
  ├── Self-contained editor backend
  ├── Primarily avoids Electron APIs; uses Node's zlib for PlantUML encoding
  ├── Handles markdown parsing, block data structure, document export, rendering
  └── packages/muya/ (@muyajs/core, the TS rewrite from
      https://github.com/marktext/muya) has landed and is now the engine
      the desktop renderer consumes; muyajs is being retired.
```

## IPC Conventions

Most IPC channels between main and renderer use the `mt::` prefix (e.g. `mt::open-new-tab`, `mt::file-saved`). Some internal channels do not follow this convention (e.g. `language-changed`).

See `packages/website/content/docs/dev/IPC.md` for conventions and examples.

## Further Reading

`packages/website/content/docs/dev/` contains the deeper developer documentation referenced by this guide. Same files are published as the developer docs section on https://marktext.me/docs/dev/overview:

- `ARCHITECTURE.md` — process/module layering beyond the summary above
- `BUILD.md` — full platform build prerequisites (including the Arch Linux deps added recently)
- `DEBUGGING.md` — attaching debuggers to main/renderer processes
- `INTERFACE.md` — Muya and renderer public interfaces
- `IPC.md` — full IPC channel catalog and `mt::` conventions
- `LINUX_DEV.md` — Linux-specific dev environment setup
- `PERFORMANCE.md` — perf measurement workflow (pairs with `pnpm run perf:inspect`)
- `RELEASE.md` / `RELEASE_HOTFIX.md` — Electron release process
- `docs/release_tauri.md` (repo root) — Tauri release + updater signing

## Important Build Notes

- **Shell-agnostic renderer**: renderer code must reach native features through `window.electron.*` / `window.fileUtils.*` (typed in `src/types/global.d.ts`), never assume Electron — the Tauri platform bridge (`src/renderer/src/platform/`) backs the same surface. New IPC needs a `mt::*` channel mapping in the bridge AND a matching `#[tauri::command]` in `src-tauri/`.
- **Tauri build**: `pnpm -C packages/desktop run build:tauri` (prebuild stages the ripgrep sidecar). No `electron-rebuild` for the Tauri path. Rust changes in `src-tauri/` need a `dev:tauri` restart; there is no Rust fmt/clippy gate in CI.
- **CommonJS vs ESM** (Electron path): `main` and `preload` compile to CommonJS; `renderer` is ESM-only. Do not use `require()` in renderer code.
- **Minify locales**: `pnpm run minify-locales` must run before production builds. It is included in `build:win/mac/linux` but not in `dev`.
- **Native modules**: After changing Electron version, run `pnpm run rebuild-native` (`electron-rebuild -f`).
- **Hot reload**: The renderer hot-reloads via Vite HMR. `Ctrl+R` in the dev window reloads the renderer and re-runs the preload script. Changes to `main/` source are NOT picked up by a window reload — restart `pnpm run dev` to pick them up.
- **electron-builder output**: `directories.output` in `packages/desktop/electron-builder.yml` is set to `../../dist` so installers land in the repo-root `dist/` (where CI artifact globs look for them). `out/` from electron-vite stays inside `packages/desktop/`.
- **Path aliases** (defined in `packages/desktop/electron.vite.config.ts`, mirrored in `vitest.config.ts` and `tsconfig.base.json`):
  - `@` → `packages/desktop/src/renderer/src`
  - `common` → `packages/desktop/src/common`
  - `@shared` → `packages/desktop/src/shared`
  - Editor engine: the renderer imports `@muyajs/core` (the `packages/muya` TS rewrite) — e.g. `import { Muya } from '@muyajs/core'` in `editor.vue`.
  - `muya` → `../muyajs` (i.e. `packages/muyajs`). The legacy alias (`muya/lib/...`) remains for the handful of call sites still on `@marktext/muyajs`, which is being retired.
- **Workspace deps**: muya's own npm runtime deps (`github-markdown-css`, `katex`, `dompurify`, `snabbdom`, …) are declared in `packages/muyajs/package.json` so Node module resolution from `packages/muyajs/lib/*.js` finds them inside the workspace rather than walking out to a parent directory.
- **Patches**: `patch-package` patches live at `packages/desktop/patches/`. The root `postinstall` calls patch-package with `cwd=packages/desktop` so the path resolves correctly.

## Contribution

- Submit PRs to the **`develop`** branch (not `main`).
- Reference the related issue in the PR description.
- Run `pnpm run lint` before submitting.
- All PRs must pass CI before merge.
- See `.github/CONTRIBUTING.md` for the full contributing guide.
