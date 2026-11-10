# Share sheet

`src-tauri/src/share/` shows the native share sheet for a PNG. macOS uses `NSSharingServicePicker`. Windows (#30) is not done yet and returns `ShareError::Unsupported`.

## API

```rust
pub fn share_png(
    window: &WebviewWindow,
    png: &[u8],
    anchor: Rect,
    on_done: impl FnOnce(ShareOutcome) + Send + 'static,
) -> Result<Opened, ShareError>

pub struct Opened { pub write: Duration, pub picker: Duration }

pub enum ShareOutcome { Chosen(String), Cancelled }

pub fn restore_level(window: &WebviewWindow)
```

- `anchor` is in logical (CSS) pixels relative to the page, origin top left: pass the share button's `getBoundingClientRect()`. The menu opens below it.
- `share_png` returns as soon as the menu is on screen, without waiting for the user (the menu runs no nested tracking loop). It can be called from any thread, but it waits for the main thread to show the menu, so call it from an async command, not a sync one (those run on the main thread).
- `on_done` runs once, on the main thread, when the menu closes: `Chosen(title)` (e.g. `"AirDrop"`) before that service shows its own UI, or `Cancelled` (also for a picker dropped after 10 minutes without an answer). It is not called if `share_png` returns an error.
- Errors (`kind` when serialized): `unsupported`, `notReady` (`share::init` was not called), `io` (temp file), `window` (no native window or view).
- `Opened` says how long the temp file write and showing the menu took, for the log.
- `share::init(app)` runs at startup (already wired in `lib.rs`).
- The chosen service shows its UI in its own window (`sourceWindowForShareItems` returns nil), with no transition from the source (`transitionImageForShareItem` returns nil). Otherwise it would hang a sheet on the overlay, which has just closed, and AppKit would bring that window back unpainted: a black screen (#118).

## From the overlay

The overlay's Share button (macOS only; it is hidden where there is no share sheet) calls `overlay_share` with the same request as Copy and Save (`docs/annotations.md`), plus `anchor`, the button's rect. `overlay/export.rs` crops and annotates the selection, encodes it like a save (blur baked in, same `pHYs` density) and opens the sheet under the button. Each share logs its steps once the sheet is open: `share sheet open for WxH px (crop, encode, write, picker)`. `picker` is the main thread building and showing the menu; the other steps run on workers.

The overlay sits at the screen saver level (1000), which would hide the menu (a pop-up menu window, level 101). `share_png` lowers it to 100 while the menu is open: still over the menu bar and Dock. So the overlay:

1. Releases its global Esc shortcut before calling `share_png`, or Esc goes to the overlay instead of closing the menu. While the sheet is open, copy, save, another share, Esc from the page and closing the overlay do nothing.
2. In `on_done`:
   - `Chosen`: remembers the region (like Copy and Save), closes the overlay right away without hiding the app, then calls `restore_level(&window)`. The window stays at 100 until then, so the service's own UI (AirDrop, Mail…) is not hidden behind it. Hiding the app, as other closes do to give focus back, would hide that UI too.
   - `Cancelled`: the level is already back. It takes Esc back (or closes, if it can't); the overlay stays open with the selection.

The level is only restored if nobody changed it since, and a second share while the window is still lowered keeps the original level on record.

## Temp files

The PNG is written to `<app cache dir>/share/xcreenshot-2026-11-06-13.22.05.png` (`-2`, `-3`… within the same second). The write truncates, so no trailing bytes from an older file can survive (aCropalypse). A name is only reused after its old file is gone.

The file is deleted at once on `Cancelled`, and 30 s after the chosen service reports `didShare` or `didFail` (some services read the file late). One sweeper task on the async runtime handles those deadlines. Services that never report back leave the file until the next startup, which deletes every leftover `xcreenshot-*.png` in that folder. Open pickers are dropped once their session ends, after 10 minutes, or when more than 8 are open.

## Trying it

Dev builds only, `XCREENSHOT_DEBUG_SHARE=<mode> pnpm tauri dev` opens Settings and shares a test PNG after a second, logging the outcome:

- `1`: Settings at its normal level.
- `overlay`: Settings at the screen saver level first. The menu still shows above it.
- `cancel`: like `overlay`, then dismisses the sheet after 2 s. The temp file is deleted and Settings goes back to the screen saver level.
