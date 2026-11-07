# Spike: screen capture on macOS (#5)

Measured on a MacBook (Apple Silicon), macOS 27 (Darwin 27.0), one built-in Retina display
(1800x1169 pt at 2x = 3600x2338 px). Release builds, terminal with Screen Recording allowed.

## Decision

Use the [`xcap`](https://crates.io/crates/xcap) crate (0.9.8) on all platforms, behind
`src-tauri/src/capture/`.

- Native resolution on macOS: yes, 3600x2338 for a 1800x1169 pt display.
- Windows: same API (GDI by default, physical pixels, Tauri apps are per-monitor DPI aware).
  Compiles in CI; not run on Windows yet (M5).
- Fastest option tested, plain Rust build, no Swift runtime to bundle.

We stay on xcap for now. Moving macOS to ScreenCaptureKit is tracked in #49. The OS calls are
two small functions (`capture_all`, `capture_display`), so that move does not touch the rest.

## Latency (capture all displays, 3600x2338 px)

| Approach                                               | First call | Warm calls |
| ------------------------------------------------------ | ---------- | ---------- |
| `xcap` (`CGWindowListCreateImage`), one thread/display | 125–190 ms | 39–44 ms   |
| `screencapturekit` 11.1 (`SCScreenshotManager`) + RGBA | ~290 ms    | ~135 ms    |

Displays are captured in parallel, so with several displays the time should be the slowest one
(only one display was available to measure). For ScreenCaptureKit, ~48 ms of each call was
`SCShareableContent` (cacheable) and ~30 ms copying the `CGImage` to RGBA; the
`screencapturekit` crate also needed the Swift runtime on the rpath.

The overlay gets the raw RGBA bytes (about 34 MB for this display), so there is no encoding on
the way to the screen. Only the final crop is encoded: a half-screen crop (1800x1169 px) takes
about 1 ms to cut and 65–90 ms to encode as PNG. Run `cargo run --release --example capture`.

## First capture and idle memory (#55)

The first capture in a process is slow: listing displays takes 30–40 ms the first time (under
0.1 ms after), and the first capture 100–190 ms (40–70 ms after). So at startup, once the overlay
windows exist, a worker thread captures one pixel of the first display and drops it
(`capture::warm_up`), only if Screen Recording is already allowed. It has the same timeout as a
capture, and its own stuck-display list, so a hotkey pressed during it still captures.

Release build, one 3600x2338 display, first capture 2 s after launch, interleaved launches:

| | hotkey to painted | capture |
| --- | --- | --- |
| No warm-up | 166–219 ms (median ~183) | 135–187 ms |
| 1 px warm-up | 87–158 ms (median ~125) | 62–116 ms |
| Full-frame warm-up | 94–159 ms (median ~109) | 70–123 ms |

A full frame was a little faster only right after launch: it also warms the allocator, which
gives the 34 MB buffers back within a minute, and it holds that memory meanwhile. With the first
capture 60 s after launch both were the same (capture 66–101 ms full, 80–97 ms 1 px), so the
1 px region is used. At 60 s, the time after the capture (fetch, draw, show) also grew, from
about 25 ms to 35–150 ms: the hidden page cools down. That part is not fixed yet.

Warm captures are unchanged (median 110–117 ms on this machine under load from other builds).

Memory after captures, closed, measured with `footprint` (each closed capture sampled 1 s after
it closed). It reaches a plateau after one or two captures and does not grow with more:

- **Main process:** about 23 MB idle, then 55–90 MB. The difference is not live data (`heap`
  shows no large allocation): it is "Malloc Large (empty)", the frame and its copy freed but
  kept by the allocator, returned within about a minute. `malloc_zone_pressure_relief` returns
  nothing for these.
- **WebContent:** about 25 MB idle, then 150–220 MB. Most of it is WebKit's full-screen layer
  backing stores (about 34 MB each), kept while the window is hidden, plus the last frame's
  `ArrayBuffer` until it is garbage collected (about 33 MB). With no frame the page now renders
  nothing (`display: none`), which drops the canvas layer's backing store: graphics
  memory after close went from 131 MB to 98 MB. Shrinking the hidden window to 1x1 freed more,
  but made painting 100+ ms slower, so it was dropped; a software canvas (`willReadFrequently`)
  and a transparent page background saved nothing.
- **GPU:** about 14 MB idle, 23–36 MB right after a capture, back to about 13 MB within 10 s.

## Crop source (#9)

The crop for copy and save (#12) needs the exact captured pixels. Two options:

- **(a) Rust keeps the frame** and sends the overlay page a copy.
- **(b) The page keeps the only frame** and crops with canvas `getImageData`. This depends on
  WebKit color management and premultiplied alpha leaving every pixel unchanged.

**Decision: (a).** The copy is cheap, and the crop stays exact by construction, with nothing to
prove on WebKit.

- Copying one 3600x2338 frame (34 MB) takes 0.7–0.9 ms warm and 1.4 ms on the first call
  (`cargo run --release --example capture` prints it). The budget for (a) was 10 ms.
- Frames are kept as `Arc<RgbaImage>`. `overlay_frame` clones the `Arc` under the lock and
  copies after releasing it, once per window.
- One scale for both sides: `overlay:show` sends the frame's pixel size and its display's
  logical size, and the page maps its selection with that ratio (the same edge rounding as
  `to_pixels`; the TS and Rust tests share cases). Enter sends the pixel rect the size badge
  shows to `overlay_copy` (or `overlay_save`), which clamps it to the image, rejects an empty
  one and crops exactly that (`capture::crop_pixels`).
- Cost: the original stays in memory while the overlay is open (about 34 MB per display at this
  size). `close` frees it.

Hotkey to painted, release build, one 3600x2338 display, 15 captures with
`XCREENSHOT_DEBUG_CAPTURE` enabled in release for the run (warm runs, first run excluded):

| | hotkey to painted (median, range) | capture (median) | after capture (median) |
| --- | --- | --- | --- |
| Before (frame moved, no copy) | 122 ms, 79–169 ms, 2 outliers at 308–322 | 96 ms | 31 ms |
| After (copy), run 1 | 136 ms, 80–162 ms, 3 outliers at 282–324 | 97 ms | 39 ms |
| After (copy), run 2 | 134 ms, 106–162 ms, 2 outliers at 230–259 | 99 ms | 33 ms |
| After review fixes (`Arc`, copy off the lock) | 122 ms, 69–136 ms, 1 outlier at 312 | 91 ms | 32 ms |

"After capture" is fetch, draw and show. It grew by 2–8 ms, near the noise between runs. The
copy itself is about 1 ms. Capture time changes the most between runs.

## Copy and save (#12)

Measured on the same Mac with a full-display selection (3600x2338 px, a busy desktop), release
build. `cargo run --release --example capture` prints the crop and the PNG levels.

| PNG compression | Time | Size |
| --- | --- | --- |
| `Fast` | 62–64 ms | 11.1–11.5 MB |
| `Level(1)` | ~230 ms | 13.0–13.4 MB |
| `Level(2)` | ~250 ms | 11.2–11.7 MB |
| `Level(3)` | 480–540 ms | 10.4–10.9 MB |
| `Level(4)` | 560–730 ms | 10.4–10.8 MB |
| `Default` | 2.2–2.5 s | 9.7–10.2 MB |

Levels 1 and 2 are slower than `Fast` and no smaller. Level 3 saves about 6% for 8x the time,
over the 300 ms budget. So one encoder, `Fast`, serves both copy and save (`encode_png`).

A save encodes the PNG on a worker while the Save dialog is open (#106), so after the dialog
the overlay waits only for the file write. A cancelled save still finishes that encode, on the
worker, and drops it. Each save logs its steps (`saved WxH px to …: crop, dialog, encode, write`).
`cargo run --release --example save` times each step on a 3840x2160 selection (a 4K display at
150%), with no screen needed. On the Mac above, release build:

| Save step | Time |
| --- | --- |
| Crop and annotations (before the dialog) | ~8 ms |
| PNG (`Fast`, `Adaptive`), now during the dialog | ~29 ms |
| File write (temp file, fsync, rename) | ~11 ms |
| Dialog to close, before (encode, write) | ~40 ms |
| Dialog to close, now (write) | ~11 ms |

Other filters with `Fast` are faster but bigger: `Paeth` ~18 ms and 17% bigger, `Sub` ~17 ms
and 29% bigger. With the encode off the save's critical path, `Adaptive` stays.

| Copy step | Time |
| --- | --- |
| Crop (`crop_pixels`) | ~1 ms |
| PNG (`Fast`) | 62–64 ms |
| Pasteboard write (PNG, TIFF promised) | ~4 ms |
| TIFF, only when an app asks for it | ~136 ms, 32 MB |

The whole copy is about 70 ms for a full display, less for a normal selection.

- **Clipboard.** One `output::Clipboard`, called off the main thread. macOS: the PNG is encoded
  on a worker, then the pasteboard is written on the main thread: the `NSPasteboardItem` is
  built first (PNG data via `NSData::from_vec`, no copy), and only then the pasteboard is
  cleared and written, with the result checked. TIFF is offered through a data provider, so
  apps that only read TIFF get it, made on request: adding it up front would cost ~136 ms on
  every copy. The provider and its PNG are released when the pasteboard is done with them
  (`pasteboardFinishedWithDataProvider:`). AppKit needs `clearContents` before `writeObjects`;
  since the item is complete before the clear, only the OS refusing the write itself loses the
  old clipboard. `tauri-plugin-clipboard-manager` is not used on macOS: it takes RGBA and puts only
  TIFF on the pasteboard. Windows: `output::win_clipboard`, which writes the registered "PNG"
  format with `pHYs` (96 DPI × scale), then `CF_DIBV5` with the same density in
  `bV5XPelsPerMeter`/`bV5YPelsPerMeter`. The plugin (arboard) wrote both without a density
  (#78). Both blocks are made before the clipboard opens, so only the OS refusing the writes
  loses the old clipboard, and either format alone counts as a copy. The clipboard is opened
  with a hidden message-only owner window. On every platform copies reach the clipboard in the
  order they were asked for (`output::order`): an older copy that finishes encoding later is
  skipped. The overlay page has no clipboard permission.
- **Save dialog.** `tauri-plugin-dialog` from Rust (`blocking_save_file` on a worker), PNG
  filter, default name `xcreenshot-YYYY-MM-DD-HH.MM.SS.png` in local time. The filter makes the
  dialog add `.png` where the platform does that. If Rust still has to add it and that file
  exists, the dialog opens again with that name, so the user confirms the overwrite. We never
  write to a name the user did not see (a file created in that folder between the check and
  the write is an accepted, tiny window). A dialog error is reported as a failed save, not
  taken for a cancel. The page has no dialog or fs permission.
- **Default folder.** The last folder saved to, if it still exists, else the Desktop on macOS
  (where its own screenshots go) and Pictures on Windows. Kept in `prefs.json` by the `prefs`
  module (`tauri-plugin-store`, Rust only): one typed struct under one key, each field falling
  back to its default on its own, paths as strings (a non-UTF-8 folder is not remembered). A
  file that is not a JSON object is renamed to `prefs.corrupt-<time>.json` (a unique name) and
  prefs start fresh. A file that cannot be read is left alone and the defaults are used.
- **Overlay level.** The overlay windows sit at screen-saver level, above the dialog. Saving
  hides them and releases Esc, so Esc cancels the dialog, but keeps the session and the frames.
  The pages keep their frame and selection while hidden; they free them only on
  `overlay:closed`, sent when the overlay closes. Rust holds Esc exactly when the phase wants
  it (`sync_escape`, after every phase change).
- **During the dialog** nothing closes the overlay (close, Esc forwarded by a page, copy), so a
  save is never lost. Once a path is picked the file is always written; a window that finished
  painting meanwhile stays hidden.
- **Cancel.** The windows come back with the selection intact, so the user can save again, copy,
  adjust it or press Esc. Closing on cancel would throw away the selection for a misclick.
- **Writing.** A temp file (`tempfile`) in the target's folder, permissions copied from the file
  it replaces, synced, renamed over the target, then the folder synced. A symlink is followed and
  the real file replaced. The file never keeps old bytes after the new PNG (aCropalypse,
  CVE-2023-28303), and a failed write leaves no half file. The PNG has only `IHDR`, `pHYs`,
  `IDAT` and `IEND`; tests check the file and the pasteboard bytes. `pHYs` is the capture's
  density (frame pixels over logical size, per axis) times 72 DPI for a macOS point or 96 for a
  Windows DIP, so apps show the image at its on-screen size: 144 DPI at 2× on macOS. Known limitation of the atomic replace: only the mode
  bits are carried over, not ACLs or extended attributes. A failed folder sync after the rename
  is only logged, since the file is complete. Writing through a symlink is deliberate: the user
  picked that name in the dialog, which confirmed the overwrite, as TextEdit and Preview do. The write happens before the overlay
  closes (its windows are already hidden), so a failure can bring it back to say so.
- **Errors.** If copy or the write fails, it is logged and the overlay stays (or comes back) with
  a short message ("Couldn't copy", "Couldn't save").

Hotkey to painted after this change, release build with `XCREENSHOT_DEBUG_CAPTURE`, one
3600x2338 display, warm runs: 70–105 ms (capture 53–79 ms), the same range as before. Nothing on
that path changed.

## Dim layer while dragging (#9)

Outside the selection is dimmed by the selection's `box-shadow` (spread `100vmax`), so moving
the selection repaints the whole display. Measured in the app (release, 3600x2338 frame) by
moving a 700x500 pt selection every frame for about 90 frames, 3 captures per variant. "Update"
is the time from the frame callback to a task posted after it, which covers style, layout and
paint of that frame (`performance.now()` has 1 ms resolution here).

| Dim layer | frame interval (p50, p95) | update (mean, p95, max) |
| --- | --- | --- |
| `box-shadow` | 17 ms, 18 ms | 1.7–2.0 ms, 3 ms, 4–6 ms |
| Four strips | 17 ms, 18 ms | 2.0–2.2 ms, 3 ms, 3–5 ms |

Both keep 60 fps with no dropped frames, and the update costs about 2 ms either way, so the
`box-shadow` stays.

## Screen Recording permission

- `has_permission()` uses `CGPreflightScreenCaptureAccess` (never prompts).
  `request_permission()` uses `CGRequestScreenCaptureAccess` (prompts once; later calls only
  return the state). After granting, the app usually has to restart.
- `capture_all()` only captures. The caller checks `has_permission()` once before it and shows
  `permissionDenied` itself. Without permission macOS can return a wallpaper-only frame (desktop
  but no windows) that looks valid, so the captured image cannot tell us permission is missing.
  An empty image is reported as `emptyImage`, not as a permission problem.
- The permission belongs to the responsible app: in dev, the terminal running `pnpm tauri dev`;
  in release, xcreenshot.app (tied to its signing identity, so re-signing can reset it).
- Periodic re-prompt: macOS 15 added a recurring "is requesting to bypass the system private
  window picker" alert for apps that capture without the system picker. It was weekly in betas,
  then monthly at release; 15.1 shows it less for apps used regularly. Apple's 15.1 notes target
  "deprecated content capture technologies", which includes `CGWindowListCreateImage`, so our
  current backend may be tied to this alert. Developers report ScreenCaptureKit apps get it too.
  I found nothing documented for macOS 26/27 and could not observe a re-prompt in this spike.

## Retina and multiple displays

- `DisplayFrame` has `logical` bounds (points/DIPs), `physical` bounds (pixels), the OS
  `scale_factor` (informational) and the native image. On macOS logical is authoritative; on
  Windows physical is, and the overlay should place windows with `PhysicalPosition`.
- `crop()` measures the scale from the image (pixels per point on each axis), so rotated
  displays and a wrong OS scale factor still crop correctly. Edges are rounded, the selection
  is clamped to the display and empty selections are rejected.
- A display that fails (asleep, unplugged) is skipped and logged; capture fails only if no
  display worked. Mirrored displays (`CGDisplayMirrorsDisplay`, or same bounds as another) are
  captured once.
- Only one display was available here, so multi-monitor (mixed 1x/2x, negative origins,
  mirroring) is covered by unit tests only.

## Windows (#28)

Not run on a real Windows PC yet. The `windows-latest` CI job builds the app and runs clippy and
`cargo test`, including a check that the process is Per-Monitor v2 DPI aware.

### DPI awareness

- **What Tauri does:** tao (Tauri's windowing crate) calls
  `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` when the event loop is created, falling
  back to per-monitor v1 on older Windows. Tauri's own manifest only adds Common Controls v6,
  with no DPI entry.
- **What we add:** `src-tauri/windows-app-manifest.xml` also declares `dpiAwareness`
  `PerMonitorV2` (with `dpiAware` `true/pm` for older Windows). `build.rs` embeds it into every
  binary, so the process is PMv2 from its first instruction, test binaries included. tao's later
  call then fails harmlessly: awareness can only be set once.
- With PMv2, every value we use is in physical pixels of one virtual desktop: xcap's monitor
  bounds (`EnumDisplaySettings` positions and modes), the captured image (GDI), the cursor
  (`GetCursorPos`) and `PhysicalPosition`. xcap's scale factor is the monitor's effective DPI
  (`GetDpiForMonitor`) divided by 96.

### Overlay placement

- Windows are placed with `PhysicalPosition` and `PhysicalSize` from the pixel bounds, moved
  first and then sized, because moving to a monitor with another DPI rescales the window
  (`WM_DPICHANGED`). The cursor is matched against the same pixel bounds. Reviewed for #28: no
  logical and physical values are mixed on Windows.
- Each page gets its display's logical size (pixels divided by that monitor's scale) and maps CSS
  px to frame pixels with that ratio.

### Mixed DPI, negative origins, hot-plug

- **Mixed DPI:** each monitor's logical bounds divide by its own scale, so logical origins of
  different monitors do not form one consistent space. Nothing places windows with them.
- **Fractional scales:** at 125% or 175% the logical size is rounded (2560 px at 175% is 1462.86,
  sent as 1463), so the page's mapping can drift by under one pixel towards the far edge.
- **Negative origins:** monitors left of or above the primary have negative pixel origins.
  `Bounds` uses `i32`, and the overlay tests cover a display at a negative origin.
- **Hot-plug:** displays are listed again on every hotkey press, and overlay windows are created
  or destroyed to match, so a monitor plugged in, removed, rescaled or moved since the last
  capture is picked up on the next one. A display that disappears during a capture is skipped.
  Changes while the overlay is open are not tracked: it keeps the old layout until it closes.

### Limits

- GDI capture: protected (DRM) video comes out black, the secure desktop (UAC prompts) cannot be
  captured, and HDR displays may look washed out.
- Only a real PC can confirm placement on mixed-DPI setups, especially that the size set after
  the move survives `WM_DPICHANGED`.

### PrintScreen and Snipping Tool (#29)

Windows 11 can give PrintScreen to Snipping Tool (Settings › Accessibility › Keyboard, "Use the
Print screen key to open screen capture"). The setting is the DWORD
`HKCU\Control Panel\Keyboard\PrintScreenKeyForSnippingEnabled` (1 on, 0 off). Windows 11 treats
a missing value as on (from 22H2 build 22621.1928, KB5027303). Microsoft does not document the
value; this comes from community guides and the KB5027303 notes.

The Windows default shortcut is bare PrintScreen, or Ctrl+Shift+PrintScreen when PrintScreen
won't reach the app: Snipping Tool has it (the setting is on, or missing on Windows 11, build
22000 and later), or it fails to register. At startup a saved PrintScreen that won't work is
replaced and saved, so Settings shows the shortcut that works, and a dialog says why once
("Open Settings" or OK). "Reset to default" picks the same, and the recorder refuses PrintScreen
while Snipping Tool has it. The decisions are in `src-tauri/src/print_screen.rs`, unit-tested on
every platform; only the registry reads are Windows-only.

## Open risks

- `CGWindowListCreateImage` is deprecated (obsoleted in the macOS 15 SDK). It still works on
  macOS 27, but Apple could remove it or prompt more often for it (#49).
- Re-prompt behavior on macOS 26/27 needs checking on a signed build over a few weeks.
- Windows mixed-DPI: logical origins are approximate; use the physical bounds (see Windows).
