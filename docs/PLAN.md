# xcreenshot — Project Plan

## Decisions
| Area | Choice |
|---|---|
| Framework | Tauri 2 (Rust core + system webview) |
| Frontend | Svelte 5 + Vite (no SvelteKit), shadcn-svelte + Tailwind, Konva for annotations |
| Platforms | macOS first; Windows in M5 (CI builds Windows from day one) |
| Default hotkey | macOS ⌃⇧⌘S · Windows PrintScreen (configurable) |
| Capture UX | Frozen-frame overlay per display, inline toolbar on selection |
| License | MIT (keeps App Store / Microsoft Store distribution open) |
| macOS distribution | Developer ID signed + notarized DMG on GitHub Releases, Homebrew tap, Tauri updater |
| Windows distribution | SignPath-signed MSI/EXE on GitHub Releases + winget |
| Backlog | Mac App Store, Microsoft Store, Linux, P2P device sync |

## Architecture
1. **Hotkey** (tauri-plugin-global-shortcut) → Rust captures all displays at native resolution (ScreenCaptureKit on macOS, DXGI/GDI on Windows).
2. One borderless always-on-top **overlay window per display**, frozen screenshot as background → pixel-exact selection, no flicker.
3. **Selection**: drag a region, or hover-highlight + click a window (SCShareableContent/CGWindowList on macOS, EnumWindows + DWM bounds on Windows). Optional **remember last region**.
4. **Inline toolbar** (Konva): pen, line, arrow, rect, ellipse, text, highlighter, blur/pixelate, color, stroke, undo/redo.
5. **Output**: clipboard (tauri-plugin-clipboard-manager), save (folder + filename pattern, PNG/JPG), **share** via custom plugin (NSSharingServicePicker / DataTransferManager).
6. **Tray + settings**: hotkey recorder, save folder, format, remember-region, launch at login, update check toggle, permission onboarding.

## Privacy
No telemetry, strict CSP, minimal Tauri capabilities, single optional network call (update check).

## Risks
- macOS Screen Recording permission (TCC) and possible periodic re-prompts on recent macOS — verify in M1 spike.
- Window enumeration on macOS needs objc2 / ScreenCaptureKit bindings.
- Mixed-DPI multi-monitor coordinate math.
- Windows 11 binds PrtSc to Snipping Tool by default — detect & guide the user.
- Homebrew official cask needs notability → start with own tap. SignPath OSS requires an application.

## Milestones
- **M0 Foundation** — repo, scaffold, CI, UI design.
- **M1 Capture core (macOS)** — tray, hotkey, overlay, region/window select, remember region, copy/save.
- **M2 Annotations** — tools, colors, text, blur, undo/redo, flattened export.
- **M3 Output & settings** — share plugin, settings window, hotkey recorder, autostart, onboarding.
- **M4 macOS release v0.1.0** — signing, notarization, updater, DMG, Homebrew tap.
- **M5 Windows** — capture port, PrintScreen, share, SignPath MSI/EXE, winget.
- **Backlog** — App Store, Microsoft Store, Linux, P2P sync, OCR, pin-to-screen, scrolling capture.
