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
  shows to `overlay_confirm`, which clamps it to the image, rejects an empty one and crops
  exactly that (`capture::crop_pixels`). It only logs the size for now.
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

## Open risks

- `CGWindowListCreateImage` is deprecated (obsoleted in the macOS 15 SDK). It still works on
  macOS 27, but Apple could remove it or prompt more often for it (#49).
- Re-prompt behavior on macOS 26/27 needs checking on a signed build over a few weeks.
- Windows mixed-DPI: logical origins are approximate; use the physical bounds.
