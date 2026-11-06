# Spike: screen capture on macOS (#5)

Measured on a MacBook (Apple Silicon), macOS 27 (Darwin 27.0), one built-in Retina display
(1800x1169 pt at 2x = 3600x2338 px). Release builds, terminal with Screen Recording allowed.

## Decision

Use the [`xcap`](https://crates.io/crates/xcap) crate (0.9.8) behind `src-tauri/src/capture.rs`.

- Native resolution on macOS: yes, 3600x2338 for a 1800x1169 pt display.
- Windows: same API (GDI by default, physical pixels, Tauri apps are per-monitor DPI aware).
  Compiles in CI; not run on Windows yet (M5).
- Fastest option tested, plain Rust build, no Swift runtime to bundle.

The OS calls are about 20 lines (`list_displays`), so swapping to ScreenCaptureKit later only
touches that function.

## Latency (capture all displays, 3600x2338 px)

| Approach                                                | First call | Warm calls |
| ------------------------------------------------------- | ---------- | ---------- |
| `xcap` (`CGWindowListCreateImage`)                      | 150–200 ms | 37–45 ms   |
| `screencapturekit` 11.1 (`SCScreenshotManager`) + RGBA  | ~290 ms    | ~135 ms    |

For ScreenCaptureKit, ~48 ms of each call is `SCShareableContent` (cacheable) and ~30 ms is
copying the `CGImage` to RGBA. The `screencapturekit` crate also needs the Swift runtime on
the rpath (`@rpath/libswift_Concurrency.dylib`), which is extra build/bundle setup.

PNG encoding of the same frame: ~30 ms with fast compression (2.3–3.4 MB), 350–500 ms with
default compression (1.8–2.8 MB). Frames are kept as raw RGBA; the overlay should get fast PNG
and saved files the smaller one. Run `cargo run --release --example capture` to measure.

## Screen Recording permission

- `has_permission()` uses `CGPreflightScreenCaptureAccess` (never prompts).
  `request_permission()` uses `CGRequestScreenCaptureAccess` (prompts once; later calls only
  return the state). After granting, the app usually has to restart.
- `capture_all()` returns `CaptureError::PermissionDenied` without touching the screen when
  preflight fails, so the system prompt only appears when we ask for it. An empty image from
  the OS (permission revoked mid-session) also maps to `PermissionDenied`.
- The permission belongs to the responsible app: in dev, the terminal running `pnpm tauri dev`;
  in release, xcreenshot.app (tied to its signing identity, so re-signing can reset it).
- Periodic re-prompt: macOS 15 added a recurring "is requesting to bypass the system private
  window picker" alert for apps that capture without the system picker. It was weekly in betas,
  then monthly at release; 15.1 shows it less for apps used regularly. Apple's 15.1 notes target
  "deprecated content capture technologies", which includes `CGWindowListCreateImage`.
  Developers report ScreenCaptureKit apps get it too. I found nothing documented for macOS 26/27
  and could not observe a re-prompt in this spike (it takes weeks).

## Retina and multiple displays

- `DisplayFrame` has logical position/size (points on macOS, converted from pixels on Windows)
  plus `scale_factor` and the native image. `crop()` maps a logical selection to pixels with
  rounded edges, clamps to the display and rejects empty selections.
- Only one display was available here, so multi-monitor (mixed 1x/2x, negative origins) is
  covered by unit tests only.

## Open risks

- `CGWindowListCreateImage` is deprecated (obsoleted in the macOS 15 SDK). It still works on
  macOS 27, but Apple could remove it or prompt more often for it. Fallback: ScreenCaptureKit
  in `list_displays`.
- Re-prompt behavior on macOS 26/27 needs checking on a signed build over a few weeks.
- Displays are captured one after another; with several 5K/6K displays this may need threads.
- Windows mixed-DPI: logical origins are approximate; the overlay may need physical positions.
