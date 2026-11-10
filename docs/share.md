# Share sheet

`src-tauri/src/share/` shows the native share sheet for a PNG. macOS uses `NSSharingServicePicker`, Windows the `DataTransferManager` share sheet. Elsewhere it returns `ShareError::Unsupported`.

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

- `anchor` is in logical (CSS) pixels relative to the page, origin top left: pass the share button's `getBoundingClientRect()`. The menu opens below it on macOS. Windows ignores it: its sheet opens over the window.
- `share_png` returns as soon as the menu is on screen, without waiting for the user (the menu runs no nested tracking loop). It can be called from any thread, but it waits for the main thread to show the menu, so call it from an async command, not a sync one (those run on the main thread).
- `on_done` runs once, on the main thread, when the menu closes: `Chosen(title)` (e.g. `"AirDrop"`) before that service shows its own UI, or `Cancelled` (also for a picker dropped after 10 minutes without an answer). It is not called if `share_png` returns an error.
- Errors (`kind` when serialized): `unsupported`, `notReady` (`share::init` was not called), `io` (temp file), `window` (no native window or view).
- `Opened` says how long the temp file write and showing the menu took, for the log.
- `share::init(app)` runs at startup (already wired in `lib.rs`).
- The chosen service shows its UI in its own window (`sourceWindowForShareItems` returns nil), with no transition from the source (`transitionImageForShareItem` returns nil). Otherwise it would hang a sheet on the overlay, which has just closed, and AppKit would bring that window back unpainted: a black screen (#118).

## From the overlay

The overlay's Share button (macOS and Windows; it is hidden where there is no share sheet) calls `overlay_share` with the same request as Copy and Save (`docs/annotations.md`), plus `anchor`, the button's rect. `overlay/export.rs` crops and annotates the selection, encodes it like a save (blur baked in, same `pHYs` density) and opens the sheet under the button. Each share logs its steps once the sheet is open: `share sheet open for WxH px (crop, encode, write, picker)`. `picker` is the main thread building and showing the menu; the other steps run on workers.

The overlay sits at the screen saver level (1000), which would hide the menu (a pop-up menu window, level 101). `share_png` lowers it to 100 while the menu is open: still over the menu bar and Dock. So the overlay:

1. Releases its global Esc shortcut before calling `share_png`, or Esc goes to the overlay instead of closing the menu. While the sheet is open, copy, save, another share, Esc from the page and closing the overlay do nothing.
2. In `on_done`:
   - `Chosen`: remembers the region (like Copy and Save), closes the overlay right away without hiding the app, then calls `restore_level(&window)`. The window stays at 100 until then, so the service's own UI (AirDrop, Mail…) is not hidden behind it. Hiding the app, as other closes do to give focus back, would hide that UI too.
     - Nothing hands focus back: the service's UI takes it. But xcreenshot stays active, so on macOS AppKit would make our next window key as the overlay goes, and bring Settings (or a notice) forward over the service. So just before the overlay hides, `focus::park_for_share` hides Settings and keeps the notices hidden for the capture. They come back once another app is in front: Settings behind it without focus, the notices on top. Not when the capture started in Settings: the user was there.
     - Every close makes the overlay windows transparent and click-through before they hide (as staging does), and the next capture's paint makes them opaque again. If anything puts one back on screen after a share, it shows nothing instead of its unpainted, black page (#118).
   - `Cancelled`: the level is already back. It takes Esc back (or closes, if it can't); the overlay stays open with the selection.

The level is only restored if nobody changed it since, and a second share while the window is still lowered keeps the original level on record.

## Windows

`share/windows.rs` is the WinRT glue; what it does with each answer is decided in `transfer.rs`, `session.rs` and `open.rs`, which are tested on every platform. Coverage runs on macOS, so the glue isn't counted; keep it thin.

- The file is looked up as a `StorageFile` off the main thread (this counts as `picker` in the log line). Then, on the main thread, `IDataTransferManagerInterop::GetForWindow` gets the overlay window's `DataTransferManager`, and `ShowShareUIForWindow` opens the sheet over the window.
- `DataRequested` registers the `ShareCanceled` and `ShareCompleted` handlers first, then fills in the data: the file as a read-only storage item, with its name as the title. If filling fails, the sheet shows "Couldn't share", and dismissing it counts as cancelled.
- Outcomes: `TargetApplicationChosen` is `Chosen(app name)`, `ShareCanceled` is `Cancelled`, and `ShareCompleted` is a choice that finished (the file is deleted 30 s later). Any of them means the sheet has closed.
- `ShareCanceled` needs Windows 10 2004 or later. Without it a dismissal would go unheard, so `share_png` returns `unsupported` there.
- The overlay is topmost and could cover the sheet, so it leaves the topmost band while the sheet is open. `restore_level` puts it back, after either outcome.
- The handlers are removed from the window's `DataTransferManager` once the sheet closes, so the next share on the same overlay window starts clean. A sheet that never reports back is dropped after 10 minutes, like on macOS.
- No black flash: the picked app shows its own window, and the overlay is hidden before its page frees the frame, as for Copy.

## Temp files

The PNG is written to `<app cache dir>/share/xcreenshot-2026-11-06-13.22.05.png` (`-2`, `-3`… within the same second). The write truncates, so no trailing bytes from an older file can survive (aCropalypse). A name is only reused after its old file is gone.

The file is deleted at once on `Cancelled`, and 30 s after the chosen service reports `didShare` or `didFail` (`ShareCompleted` on Windows; some services read the file late). One sweeper task on the async runtime handles those deadlines. Services that never report back leave the file until the next startup, which deletes every leftover `xcreenshot-*.png` in that folder. Open pickers are dropped once their session ends, after 10 minutes, or when more than 8 are open.

## Trying it

Dev builds only, `XCREENSHOT_DEBUG_SHARE=<mode> pnpm tauri dev` opens Settings and shares a test PNG after a second, logging the outcome:

- `1`: Settings at its normal level.
- `overlay`: Settings at the screen saver level first. The menu still shows above it.
- `cancel`: like `overlay`, then dismisses the sheet after 2 s. The temp file is deleted and Settings goes back to the screen saver level.
