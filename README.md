<div align="center">

<img src="assets/icon.svg" width="112" height="112" alt="xcreenshot icon" />

# xcreenshot

**Fast, private screenshots for macOS and Windows.**
Press a shortcut, drag, annotate, done. Nothing ever leaves your computer.

Website: https://cberd1509.github.io/xcreenshot/

[![CI](https://github.com/cberd1509/xcreenshot/actions/workflows/ci.yml/badge.svg)](https://github.com/cberd1509/xcreenshot/actions/workflows/ci.yml)
[![CodeQL](https://github.com/cberd1509/xcreenshot/actions/workflows/github-code-scanning/codeql/badge.svg)](https://github.com/cberd1509/xcreenshot/security/code-scanning)
[![Latest release](https://img.shields.io/github/v/release/cberd1509/xcreenshot?include_prereleases&sort=semver)](https://github.com/cberd1509/xcreenshot/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
<br />
[![macOS](https://img.shields.io/badge/macOS-14%2B-000000?logo=apple&logoColor=white)](#install)
[![Windows](https://img.shields.io/badge/Windows-10%2B-0078D4?logo=windows&logoColor=white)](#install)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8DB?logo=tauri&logoColor=white)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-stable-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte&logoColor=white)](https://svelte.dev)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

<img src="docs/media/overlay.png" alt="xcreenshot capture overlay with annotations" width="820" />

</div>

## Features

- **One shortcut.** <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>⌘</kbd> <kbd>S</kbd> on macOS, <kbd>PrtSc</kbd> on Windows (<kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>PrtSc</kbd> if Snipping Tool uses <kbd>PrtSc</kbd>). Change it anytime.
- **Drag to capture.** Select any part of the screen, across multiple monitors and Retina displays, pixel-exact.
- **Exact sizes.** Type a width and height, or lock the aspect ratio.
- **Remember region.** Reopen with your last selection, ready to go.
- **Annotate in place.** Arrows, lines, rectangles, circles, pen, highlighter, text and blur, in any color.
- **Copy or save.** Straight to the clipboard or a PNG file, at the size you saw it.
- **Private by design.** No accounts, no tracking, no uploads.
- **Tiny and native-feeling.** A small menu bar app, not a browser in disguise.

<p align="center">
  <img src="docs/media/demo.gif" alt="Demo: capture, annotate and copy" width="640" />
</p>

## Install

**macOS** (14 Sonoma or later, Apple Silicon and Intel)

Download the `.dmg` from the [latest release](https://github.com/cberd1509/xcreenshot/releases/latest), open it and drag xcreenshot to Applications. It's signed and notarized by Apple. It checks for updates and asks before restarting to install one.

**Windows** (10 and 11, 64-bit)

Download the `x64-setup.exe` installer from the [latest release](https://github.com/cberd1509/xcreenshot/releases/latest) and run it (if the latest release has no `.exe` yet, Windows comes with the next one). It isn't code signed yet, so the first time Windows may say it protected your PC: click **More info › Run anyway**. It checks for updates and asks before restarting to install one.

## Usage

| Action                  | macOS                                                              | Windows                                                                                                       |
| ----------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| Take a screenshot       | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>⌘</kbd> <kbd>S</kbd>                | <kbd>PrtSc</kbd>, or <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>PrtSc</kbd> if Snipping Tool uses <kbd>PrtSc</kbd> |
| Copy                    | <kbd>⌘</kbd> <kbd>C</kbd> or <kbd>Enter</kbd>                      | <kbd>Ctrl</kbd> <kbd>C</kbd> or <kbd>Enter</kbd>                                                              |
| Save as PNG             | <kbd>⌘</kbd> <kbd>S</kbd>                                          | <kbd>Ctrl</kbd> <kbd>S</kbd>                                                                                  |
| Undo / Redo             | <kbd>⌘</kbd> <kbd>Z</kbd> / <kbd>⇧</kbd> <kbd>⌘</kbd> <kbd>Z</kbd> | <kbd>Ctrl</kbd> <kbd>Z</kbd> / <kbd>Ctrl</kbd> <kbd>Y</kbd>                                                   |
| Straight lines, circles | Hold <kbd>⇧</kbd> while drawing                                    | Hold <kbd>Shift</kbd> while drawing                                                                           |
| Cancel                  | <kbd>Esc</kbd>                                                     | <kbd>Esc</kbd>                                                                                                |

On first launch, macOS asks for **Screen Recording** permission. xcreenshot walks you through it.

> **Windows 11 tip:** to use <kbd>PrtSc</kbd> alone, turn off _Settings › Accessibility › Keyboard › Use the Print screen key to open screen capture_, then pick <kbd>PrtSc</kbd> in Settings.

<p align="center">
  <img src="docs/media/settings.png" alt="xcreenshot Settings" width="560" />
</p>

## Privacy

Your screenshots stay on your computer. xcreenshot has no accounts, no analytics and no telemetry, and it never uploads anything. The only network requests are update checks and downloads from GitHub Releases. Automatic update checks can be turned off in Settings. See [PRIVACY.md](PRIVACY.md).

## Build from source

You need [Node 26+](https://nodejs.org) (LTS), [pnpm](https://pnpm.io) and [Rust](https://rustup.rs) (plus Xcode Command Line Tools on macOS, or the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) on Windows).

```sh
git clone https://github.com/cberd1509/xcreenshot.git
cd xcreenshot
pnpm install
pnpm tauri dev      # run in development
pnpm tauri build    # build the app
```

Release builds sign updates, so `pnpm tauri build` needs the signing key. To build without it, see [docs/updates.md](docs/updates.md#signing).

## Contributing

Contributions are welcome! Read [CONTRIBUTING.md](CONTRIBUTING.md) to get set up, and check the [issues](https://github.com/cberd1509/xcreenshot/issues) for something to pick up. Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).

Found a security issue? Please report it privately; see [SECURITY.md](SECURITY.md).

## Acknowledgements

Built with [Tauri](https://tauri.app), [Svelte](https://svelte.dev), [shadcn-svelte](https://shadcn-svelte.com) and [Konva](https://konvajs.org). Icons from [Lucide](https://lucide.dev).

## License

[MIT](LICENSE) © 2026 [Bervex](https://github.com/cberd1509) and contributors
