<p align="center"><img src="docs/assets/remarks-logo.png" alt="Remarks on Markdown" width="100" height="100"></p>

<h1 align="center">Remarks on Markdown</h1>

<div align="center">
  <strong>:high_brightness: Next generation markdown editor :crescent_moon:</strong><br>
  A simple and elegant open-source markdown editor focused on speed and usability.<br>
  <sub>A fork of <a href="https://github.com/marktext/marktext">MarkText</a>, rebuilt on Tauri 2.</sub>
</div>

<br>

<div align="center">
  <!-- License -->
  <a href="LICENSE">
    <img src="https://img.shields.io/github/license/hisaboh/remarks.svg" alt="LICENSE">
  </a>
  <!-- Latest release -->
  <a href="https://github.com/hisaboh/remarks/releases/latest">
    <img src="https://img.shields.io/github/v/release/hisaboh/remarks?include_prereleases" alt="latest release">
  </a>
</div>

## About

**Remarks on Markdown** (display name: **Remarks**) is a WYSIWYG markdown editor forked from [MarkText](https://github.com/marktext/marktext). The Electron shell has been replaced with [Tauri 2](https://tauri.app), which makes the app dramatically smaller and lighter while keeping the editing experience.

Settings and documents from an existing MarkText (Electron) installation are imported automatically on first launch.

## Screenshot

![](docs/assets/marktext.png?raw=true)

## Features

- Realtime preview (WYSIWYG) and a clean and simple interface to get a distraction-free writing experience.
- Support [CommonMark Spec](https://spec.commonmark.org), [GitHub Flavored Markdown Spec](https://github.github.com/gfm/) and selective support [Pandoc markdown](https://pandoc.org/MANUAL.html#pandocs-markdown).
- Markdown extensions such as math expressions (KaTeX), front matter and emojis.
- Support paragraphs and inline style shortcuts to improve your writing efficiency.
- Output **HTML** and **PDF** files.
- Various themes: **Cadmium Light**, **Material Dark** etc.
- Various editing modes: **Source Code mode**, **Typewriter mode**, **Focus mode**.
- Paste images directly from clipboard.

## Download and Installation

Download the latest installer from the [release page](https://github.com/hisaboh/remarks/releases/latest).

#### macOS

Requires macOS 10.15 (Catalina) or later. Download the `.dmg`, drag **Remarks.app** into Applications, and launch it. The app checks GitHub Releases for updates automatically.

#### Windows / Linux

Not built yet — macOS is the primary target for now. All sources build with the standard Tauri toolchain, so other platforms are expected to follow.

## Development

Prerequisites: Node.js >= 20, [pnpm](https://pnpm.io) >= 10, and the [Rust toolchain](https://www.rust-lang.org/tools/install) (for Tauri).

```bash
pnpm install          # install dependencies
pnpm run dev:tauri    # run the Tauri app in development mode
pnpm run build:tauri  # build and bundle a release (updater signing key required)
```

Developer documentation lives in `docs/` — see `docs/move_to_tauri.md` for the Tauri migration notes. The upstream [MarkText developer docs](https://marktext.me/docs/dev/overview) still apply to the renderer and editor engine.

## Credits

Remarks on Markdown is built on the work of [Jocs](https://github.com/Jocs) and the [MarkText contributors](https://github.com/marktext/marktext/graphs/contributors). Please consider [supporting the upstream project](https://github.com/sponsors/marktext).

## License

[**MIT**](LICENSE).
