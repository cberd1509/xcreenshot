# xcreenshot

A fast, privacy-first, open-source screenshot tool for macOS (Windows coming soon).

> Status: early development. See the [project board](https://github.com/cberd1509/xcreenshot/projects) and [milestones](https://github.com/cberd1509/xcreenshot/milestones).

## Features (v0.1 goals)

- Global hotkey — **⌃⇧⌘S** on macOS, **PrintScreen** on Windows (configurable)
- Drag to select a region on a frozen, pixel-exact overlay (multi-monitor, Retina-aware)
- Remember the last region (LightShot-style)
- Inline annotations: pen, line, arrow, rectangle, ellipse, text, highlighter, blur — with colors and stroke widths
- Copy to clipboard, save as PNG (native Save dialog, remembers your last folder), or share via the native OS share sheet

## Privacy

- No telemetry, no accounts, no uploads. Ever.
- The only network request is an optional update check against GitHub Releases (can be disabled).

## Tech

[Tauri 2](https://tauri.app) (Rust) · Svelte 5 · shadcn-svelte · Konva

## License

[MIT](LICENSE)
