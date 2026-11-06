# Share sheet

`src-tauri/src/share/` shows the native share sheet for a PNG. macOS uses `NSSharingServicePicker`. Windows (#30) is not done yet and returns `ShareError::Unsupported`.

## API

```rust
pub fn share_png(window: &WebviewWindow, png: &[u8], anchor: Rect) -> Result<(), ShareError>
```

- `anchor` is in logical (CSS) pixels relative to the page, origin top left: pass the share button's `getBoundingClientRect()`. The sheet opens below it.
- Callable from any thread. It runs the picker on the main thread and returns once the sheet is open, not when sharing ends.
- Errors (`kind` when serialized): `unsupported`, `notReady` (`share::init` was not called), `io` (temp file), `window` (no native window or view).
- `share::init(app)` runs at startup (already wired in `lib.rs`).

## Temp files

The PNG is written to `<app cache dir>/share/xcreenshot-2026-11-06-13.22.05.png` (`-2`, `-3`… within the same second). The write truncates, so no trailing bytes from an older file can survive (aCropalypse).

The file is deleted at once when the user dismisses the sheet, and 30 s after the chosen service reports `didShare` or `didFail` (some services read the file late). Services that never report back leave the file until the next startup, which deletes every leftover `xcreenshot-*.png` in that folder.

## Overlay windows

The picker's menu is a pop-up menu window (level 101). The overlay sits at the screen saver level (1000), which would hide it. `share_png` handles this: a window above level 100 is lowered to 100 (still over the menu bar and Dock) while the menu is open, and put back when it closes. Callers do nothing.

Not handled yet: once a service is picked, its own UI (AirDrop, Mail…) may open under the overlay. The overlay should close after the user picks a service; `share_png` has no callback for that moment yet.

## Trying it

Dev builds only, `XCREENSHOT_DEBUG_SHARE=<mode> pnpm tauri dev` opens Settings and shares a test PNG after a second:

- `1`: Settings at its normal level.
- `overlay`: Settings at the screen saver level first. The menu still shows above it.
- `cancel`: like `overlay`, then dismisses the sheet after 2 s. The temp file is deleted and Settings goes back to the screen saver level.
