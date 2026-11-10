//! Which capture backend runs, and whether what it returned can be used, plus the pure parts of
//! the macOS backends (pixel copy, pixel size, when to refetch content), so they are tested on
//! every platform. The OS calls are xcap (`capture/mod.rs`), CoreGraphics (`capture/cg.rs`) and
//! ScreenCaptureKit (`capture/sck.rs`, macOS 14+).
//!
//! CoreGraphics and xcap both go through `CGWindowListCreateImage`, which Apple has deprecated
//! (#49). CoreGraphics makes the call itself with fewer copies of the frame, so a capture holds
//! about 33 MB less (#55); it goes first on macOS, and xcap takes over any display it could not
//! capture. They are the default while they work because they are faster than ScreenCaptureKit,
//! which takes over when that call is missing, fails, or returns only empty or black images, so a
//! future macOS cannot break capture. Elsewhere only xcap runs.

use super::{CaptureError, DisplayFrame};
use image::RgbaImage;
use std::fmt;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Forces a backend for testing: `cg` (or `coregraphics`), `xcap`, `screencapturekit` (or
/// `sck`), or `auto` (the default).
pub const ENV: &str = "XCREENSHOT_CAPTURE_BACKEND";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// `CGWindowListCreateImage` called directly (macOS only).
    CoreGraphics,
    Xcap,
    ScreenCaptureKit,
}

impl Backend {
    /// Whether it captures with `CGWindowListCreateImage` (#49), the "old API" below.
    fn old_api(self) -> bool {
        matches!(self, Self::CoreGraphics | Self::Xcap)
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CoreGraphics => "CoreGraphics",
            Self::Xcap => "xcap",
            Self::ScreenCaptureKit => "ScreenCaptureKit",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    /// CoreGraphics (macOS) first, then xcap, then ScreenCaptureKit, each for the displays the
    /// ones before could not capture.
    Auto,
    Only(Backend),
}

impl Preference {
    /// Reads the value of [`ENV`]. Anything unknown is `Auto`.
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("cg" | "coregraphics") => Self::Only(Backend::CoreGraphics),
            Some("xcap") => Self::Only(Backend::Xcap),
            Some("sck" | "screencapturekit") => Self::Only(Backend::ScreenCaptureKit),
            Some("" | "auto") | None => Self::Auto,
            Some(other) => {
                log::warn!("unknown {ENV} {other:?}, using auto");
                Self::Auto
            }
        }
    }
}

/// What this system can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Support {
    /// `CGWindowListCreateImage` exists (always true off macOS).
    pub xcap: bool,
    /// ScreenCaptureKit screenshots exist (macOS 14+).
    pub sck: bool,
}

impl Support {
    fn has(self, backend: Backend) -> bool {
        match backend {
            Backend::CoreGraphics => cfg!(target_os = "macos") && self.xcap,
            Backend::Xcap => self.xcap,
            Backend::ScreenCaptureKit => self.sck,
        }
    }
}

/// After this many captures in a row in which the old API (CoreGraphics and xcap) captured
/// nothing usable, ScreenCaptureKit goes first, so a broken old API does not cost a failed
/// attempt on every hotkey.
const OLD_API_STRIKES: u32 = 3;

/// While ScreenCaptureKit goes first, the old API still goes first once in this many captures,
/// so a passing problem does not keep the slower backend for the rest of the session.
const OLD_API_RETRY_EVERY: u32 = 10;

/// After this many captures in a row in which CoreGraphics missed a display that xcap then
/// captured, CoreGraphics is left out until the next launch, so a problem of its own (a pixel
/// format it refuses) does not cost a second capture on every hotkey.
const CG_STRIKES: u32 = 3;

/// The error when no backend can run here.
pub fn no_backend() -> CaptureError {
    CaptureError::Backend("no capture backend available".into())
}

/// How many displays each backend captured, in the order they ran.
pub type Used = Vec<(Backend, usize)>;

/// Picks the backend order for each capture and remembers how the old API has been doing.
pub struct Chooser {
    old_api_failures: AtomicU32,
    /// Captures since the old API reached [`OLD_API_STRIKES`], for [`OLD_API_RETRY_EVERY`].
    sck_first: AtomicU32,
    /// Captures in a row in which xcap had to take over from CoreGraphics ([`CG_STRIKES`]).
    cg_failures: AtomicU32,
    /// One bit per backend that has already fallen back this launch, so its later fallbacks log
    /// at info, not warn.
    fell_back: AtomicU8,
}

impl Chooser {
    pub const fn new() -> Self {
        Self {
            old_api_failures: AtomicU32::new(0),
            sck_first: AtomicU32::new(0),
            cg_failures: AtomicU32::new(0),
            fell_back: AtomicU8::new(0),
        }
    }

    /// Whether this is `backend`'s first fallback since launch (warned about; later ones not).
    fn first_fallback(&self, backend: Backend) -> bool {
        let bit = 1 << backend as u8;
        self.fell_back.fetch_or(bit, Ordering::Relaxed) & bit == 0
    }

    fn cg_left_out(&self) -> bool {
        self.cg_failures.load(Ordering::Relaxed) >= CG_STRIKES
    }

    fn struck_out(&self) -> bool {
        self.old_api_failures.load(Ordering::Relaxed) >= OLD_API_STRIKES
    }

    /// The backends to try, in order. Empty if the forced one cannot run here. Only reads the
    /// state; [`Chooser::run`] updates it.
    pub fn order(&self, preference: Preference, support: Support) -> Vec<Backend> {
        use Backend::{CoreGraphics, ScreenCaptureKit, Xcap};
        // CoreGraphics is filtered out off macOS, so xcap goes first there.
        const OLD_API_FIRST: &[Backend] = &[CoreGraphics, Xcap, ScreenCaptureKit];
        const SCK_FIRST: &[Backend] = &[ScreenCaptureKit, CoreGraphics, Xcap];
        let retry =
            (self.sck_first.load(Ordering::Relaxed) + 1).is_multiple_of(OLD_API_RETRY_EVERY);
        let preferred = match preference {
            Preference::Only(backend) => &[backend][..],
            Preference::Auto if self.struck_out() && !retry => SCK_FIRST,
            Preference::Auto => OLD_API_FIRST,
        };
        preferred
            .iter()
            .copied()
            .filter(|&b| support.has(b))
            .filter(|&b| b != CoreGraphics || preference != Preference::Auto || !self.cg_left_out())
            .collect()
    }

    /// Counts a strike, or clears them, once per capture in which the old API ran.
    fn record(&self, old_api_usable: bool) {
        if old_api_usable {
            self.old_api_failures.store(0, Ordering::Relaxed);
            self.sck_first.store(0, Ordering::Relaxed);
        } else {
            self.old_api_failures.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// After a capture in which CoreGraphics ran: whether it missed a display that xcap then
    /// captured. Only that counts against it; the old API failing as a whole is `record`'s.
    fn record_cg(&self, missed: bool, xcap_captured: bool) {
        if !missed {
            self.cg_failures.store(0, Ordering::Relaxed);
        } else if xcap_captured
            && self.cg_failures.fetch_add(1, Ordering::Relaxed) + 1 == CG_STRIKES
        {
            log::warn!("CoreGraphics keeps failing where xcap works; using xcap until restart");
        }
    }

    /// Captures `ids` with each backend in `order`, giving each one only the displays that are
    /// still missing (failed, timed out or black) and only the time left of `budget`, so a
    /// fallback never stacks a second full timeout on the first. Says how many displays each
    /// backend captured. A display no backend could use keeps its first frame (a black screen is
    /// still a screen) or its error; the result fails only if no display has a frame.
    pub fn run(
        &self,
        order: &[Backend],
        ids: &[u32],
        budget: Duration,
        capture: impl FnMut(Backend, &[u32], Duration) -> Vec<Result<DisplayFrame, CaptureError>>,
    ) -> (Result<Vec<DisplayFrame>, CaptureError>, Used) {
        self.run_with_clock(order, ids, budget, capture, Instant::now)
    }

    /// [`Chooser::run`] with the time read from `now`, so tests do not depend on how fast the
    /// machine is.
    fn run_with_clock(
        &self,
        order: &[Backend],
        ids: &[u32],
        budget: Duration,
        mut capture: impl FnMut(Backend, &[u32], Duration) -> Vec<Result<DisplayFrame, CaptureError>>,
        now: impl Fn() -> Instant,
    ) -> (Result<Vec<DisplayFrame>, CaptureError>, Used) {
        if self.struck_out() {
            self.sck_first.fetch_add(1, Ordering::Relaxed);
        }
        let deadline = now() + budget;
        // `None` until a backend has tried the display.
        let mut results: Vec<Option<Result<DisplayFrame, CaptureError>>> =
            ids.iter().map(|_| None).collect();
        let mut used = vec![];
        // `None` until the old API has run; then whether it captured any display.
        let mut old_api = None;
        // `None` until CoreGraphics has run; then whether it missed a display.
        let mut cg_missed = None;
        let mut xcap_captured = false;
        for &backend in order {
            let missing: Vec<usize> = (0..ids.len())
                .filter(|&i| results[i].as_ref().is_none_or(|r| unusable(r).is_some()))
                .collect();
            if missing.is_empty() {
                break;
            }
            let left = deadline.saturating_duration_since(now());
            if left.is_zero() {
                log::warn!("no time left to try {backend}");
                break;
            }
            let wanted: Vec<u32> = missing.iter().map(|&i| ids[i]).collect();
            let mut captured = 0;
            for (&i, result) in missing.iter().zip(capture(backend, &wanted, left)) {
                let usable = match unusable(&result) {
                    None => true,
                    Some(why) => {
                        let level = if self.first_fallback(backend) {
                            log::Level::Warn
                        } else {
                            log::Level::Info
                        };
                        log::log!(level, "{backend}: display {}: {why}", ids[i]);
                        false
                    }
                };
                captured += usize::from(usable);
                // Keep the first answer, unless this one is usable, or a frame after an error.
                let better = match &results[i] {
                    None => true,
                    Some(old) => usable || (old.is_err() && result.is_ok()),
                };
                if better {
                    results[i] = Some(result);
                }
            }
            if backend.old_api() {
                old_api = Some(old_api.unwrap_or(false) || captured > 0);
            }
            match backend {
                Backend::CoreGraphics => cg_missed = Some(captured < wanted.len()),
                Backend::Xcap => xcap_captured = captured > 0,
                Backend::ScreenCaptureKit => {}
            }
            if captured > 0 {
                used.push((backend, captured));
            }
        }
        if let Some(usable) = old_api {
            self.record(usable);
        }
        if let Some(missed) = cg_missed {
            self.record_cg(missed, xcap_captured);
        }
        let results = results
            .into_iter()
            .map(|r| r.unwrap_or_else(|| Err(no_backend())))
            .collect();
        (super::keep_captured(results), used)
    }
}

/// For the log: "CoreGraphics (2)" or "CoreGraphics (1), xcap (1)".
pub fn describe(used: &[(Backend, usize)]) -> String {
    let parts: Vec<String> = used.iter().map(|(b, n)| format!("{b} ({n})")).collect();
    parts.join(", ")
}

/// Why a display's capture cannot be used, if it cannot: an error, or a black frame (what a
/// removed or stubbed-out API tends to return). A permission problem can look valid (wallpaper
/// only), so that is checked before capturing, not here.
fn unusable(result: &Result<DisplayFrame, CaptureError>) -> Option<String> {
    match result {
        Err(e) => Some(e.to_string()),
        Ok(f) if is_black(&f.image) => Some("came back black".into()),
        Ok(_) => None,
    }
}

/// Whether every pixel is exactly black (alpha aside). Every pixel is checked, so a dark frame
/// with any content is never taken for a failed one. A real screen stops at its first pixels
/// with color; only a really black frame costs a full pass.
pub fn is_black(image: &RgbaImage) -> bool {
    image
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|&[r, g, b, _]| r == 0 && g == 0 && b == 0)
}

#[cfg(any(target_os = "macos", test))]
/// Copies BGRA rows (`bytes_per_row` may include padding) into an RGBA image. `None` if the
/// buffer is too short for the size.
pub fn bgra_to_rgba(
    data: &[u8],
    width: usize,
    height: usize,
    bytes_per_row: usize,
) -> Option<RgbaImage> {
    let row = width.checked_mul(4)?;
    if width == 0 || height == 0 || bytes_per_row < row {
        return None;
    }
    let needed = bytes_per_row.checked_mul(height - 1)?.checked_add(row)?;
    if data.len() < needed {
        return None;
    }
    let mut out = Vec::with_capacity(row.checked_mul(height)?);
    // Each row without its padding, then blue and red swapped while the row is still in cache.
    // Fixed-size pixels let the compiler vectorise the swap.
    for src in data.chunks(bytes_per_row).take(height) {
        let start = out.len();
        out.extend_from_slice(&src[..row]);
        for pixel in out[start..].as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    RgbaImage::from_raw(u32::try_from(width).ok()?, u32::try_from(height).ok()?, out)
}

#[cfg(any(target_os = "macos", test))]
/// The native pixel size of a display that is `points` big, given its display mode's size in
/// pixels and in points (both unrotated). The scale is the same on both axes, so a rotated
/// display (points swapped) still gets the right size. A mode with no size counts as 1x.
pub fn pixel_size(points: (u32, u32), mode_pixels: usize, mode_points: usize) -> (usize, usize) {
    let scale = if mode_pixels == 0 || mode_points == 0 {
        1.0
    } else {
        mode_pixels as f64 / mode_points as f64
    };
    let px = |v: u32| (f64::from(v) * scale).round() as usize;
    (px(points.0), px(points.1))
}

#[cfg(any(target_os = "macos", test))]
/// Whether the cached ScreenCaptureKit content must be fetched again before capturing `wanted`:
/// there is none, the displays changed since (`generation`), or a display is missing from it.
pub fn needs_refresh(cached: Option<(u64, &[u32])>, generation: u64, wanted: &[u32]) -> bool {
    match cached {
        None => true,
        Some((at, ids)) => at != generation || wanted.iter().any(|id| !ids.contains(id)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::Bounds;
    use super::*;
    use std::cell::RefCell;

    const BOTH: Support = Support {
        xcap: true,
        sck: true,
    };
    const CG: Backend = Backend::CoreGraphics;
    const XCAP: Backend = Backend::Xcap;
    const SCK: Backend = Backend::ScreenCaptureKit;

    /// `backends` with CoreGraphics left out where it does not run (off macOS).
    fn here(backends: &[Backend]) -> Vec<Backend> {
        let cg = cfg!(target_os = "macos");
        backends
            .iter()
            .copied()
            .filter(|&b| cg || b != CG)
            .collect()
    }

    fn frame(id: u32, rgb: [u8; 3]) -> DisplayFrame {
        let b = Bounds {
            x: id as i32 * 4,
            y: 0,
            width: 4,
            height: 4,
        };
        DisplayFrame {
            id,
            logical: b,
            physical: b,
            scale_factor: 1.0,
            image: RgbaImage::from_pixel(4, 4, image::Rgba([rgb[0], rgb[1], rgb[2], 255])),
        }
    }

    type Results = Vec<Result<DisplayFrame, CaptureError>>;

    /// A fake backend: `pick` gives each display's result.
    fn each(ids: &[u32], pick: impl Fn(u32) -> Result<DisplayFrame, CaptureError>) -> Results {
        ids.iter().map(|&id| pick(id)).collect()
    }

    const BUDGET: Duration = Duration::from_secs(1);

    fn ids(frames: &[DisplayFrame]) -> Vec<u32> {
        frames.iter().map(|f| f.id).collect()
    }

    #[test]
    fn the_env_var_forces_a_backend() {
        let p = Preference::parse;
        assert_eq!(p(None), Preference::Auto);
        assert_eq!(p(Some("")), Preference::Auto);
        assert_eq!(p(Some("auto")), Preference::Auto);
        assert_eq!(p(Some("nonsense")), Preference::Auto);
        assert_eq!(p(Some("cg")), Preference::Only(CG));
        assert_eq!(p(Some("CoreGraphics")), Preference::Only(CG));
        assert_eq!(p(Some("xcap")), Preference::Only(XCAP));
        assert_eq!(p(Some(" XCAP ")), Preference::Only(XCAP));
        assert_eq!(p(Some("sck")), Preference::Only(SCK));
        assert_eq!(p(Some("ScreenCaptureKit")), Preference::Only(SCK));
        assert_eq!(
            [CG, XCAP, SCK].map(|b| b.to_string()),
            ["CoreGraphics", "xcap", "ScreenCaptureKit"]
        );
    }

    #[test]
    fn coregraphics_then_xcap_go_first_while_they_work() {
        let c = Chooser::new();
        assert_eq!(c.order(Preference::Auto, BOTH), here(&[CG, XCAP, SCK]));
        let old_macos = Support { sck: false, ..BOTH };
        assert_eq!(c.order(Preference::Auto, old_macos), here(&[CG, XCAP]));
        let removed = Support {
            xcap: false,
            ..BOTH
        };
        assert_eq!(c.order(Preference::Auto, removed), [SCK]);
    }

    #[test]
    fn a_forced_backend_runs_alone_and_only_where_it_exists() {
        let c = Chooser::new();
        assert_eq!(c.order(Preference::Only(SCK), BOTH), [SCK]);
        assert_eq!(c.order(Preference::Only(XCAP), BOTH), [XCAP]);
        // CoreGraphics is macOS only, and needs the call xcap makes.
        let cg = c.order(Preference::Only(CG), BOTH);
        assert_eq!(cg.len(), usize::from(cfg!(target_os = "macos")));
        let removed = Support {
            xcap: false,
            ..BOTH
        };
        assert!(c.order(Preference::Only(CG), removed).is_empty());
        let old_macos = Support { sck: false, ..BOTH };
        assert!(c.order(Preference::Only(SCK), old_macos).is_empty());
        let (result, used) = c.run(&[], &[1], BUDGET, |_, _, _| unreachable!());
        assert_eq!(result, Err(no_backend()));
        assert!(used.is_empty());
    }

    #[test]
    fn xcap_captures_what_coregraphics_could_not() {
        let c = Chooser::new();
        let asked = RefCell::new(vec![]);
        let (result, used) = c.run(&[CG, XCAP, SCK], &[1, 2], BUDGET, |b, ids, _| {
            asked.borrow_mut().push((b, ids.to_vec()));
            each(ids, |id| match (b, id) {
                (CG, 1) => Ok(frame(id, [5, 5, 5])),
                (CG, _) => Err(CaptureError::Backend("not BGRA".into())),
                _ => Ok(frame(id, [6, 6, 6])),
            })
        });
        assert_eq!(*asked.borrow(), [(CG, vec![1, 2]), (XCAP, vec![2])]);
        assert_eq!(used, [(CG, 1), (XCAP, 1)]);
        assert_eq!(describe(&used), "CoreGraphics (1), xcap (1)");
        assert_eq!(ids(&result.unwrap()), [1, 2]);
    }

    #[test]
    fn only_the_first_fallback_of_each_backend_is_a_warning() {
        let c = Chooser::new();
        let works = |_, ids: &[u32], _| each(ids, |id| Ok(frame(id, [1, 1, 1])));
        let _ = c.run(&[CG, XCAP], &[1], BUDGET, works);
        assert_eq!(c.fell_back.load(Ordering::Relaxed), 0, "nothing fell back");
        let _ = c.run(&[CG, XCAP], &[1], BUDGET, |b, ids, _| {
            each(ids, |id| match b {
                CG => Err(CaptureError::EmptyImage),
                _ => Ok(frame(id, [1, 1, 1])),
            })
        });
        // CoreGraphics used up its warning; xcap still has its own.
        assert!(!c.first_fallback(CG));
        assert!(c.first_fallback(XCAP));
        assert!(!c.first_fallback(XCAP));
        assert!(c.first_fallback(SCK));
    }

    #[test]
    fn coregraphics_is_left_out_when_only_it_keeps_failing() {
        let c = Chooser::new();
        let cg_broken = |b, ids: &[u32], _| {
            each(ids, |id| match b {
                CG => Err(CaptureError::Backend("not BGRA".into())),
                _ => Ok(frame(id, [1, 1, 1])),
            })
        };
        for _ in 1..CG_STRIKES {
            let _ = c.run(&[CG, XCAP, SCK], &[1], BUDGET, cg_broken);
        }
        // One capture where CoreGraphics works clears its strikes.
        let works = |_, ids: &[u32], _| each(ids, |id| Ok(frame(id, [1, 1, 1])));
        let _ = c.run(&[CG, XCAP, SCK], &[1], BUDGET, works);
        for _ in 1..CG_STRIKES {
            let _ = c.run(&[CG, XCAP, SCK], &[1], BUDGET, cg_broken);
        }
        assert_eq!(c.order(Preference::Auto, BOTH), here(&[CG, XCAP, SCK]));
        let _ = c.run(&[CG, XCAP, SCK], &[1], BUDGET, cg_broken);
        assert_eq!(c.order(Preference::Auto, BOTH), [XCAP, SCK]);
        // Forcing still runs it.
        assert_eq!(c.order(Preference::Only(CG), BOTH), here(&[CG]), "forced");
        // Both failing is the old API's problem, not a strike against CoreGraphics.
        let c = Chooser::new();
        for _ in 0..CG_STRIKES {
            let _ = c.run(&[CG, XCAP], &[1], BUDGET, |_, ids, _| {
                each(ids, |_| Err(CaptureError::EmptyImage))
            });
        }
        assert_eq!(c.cg_failures.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn coregraphics_and_xcap_failing_together_is_one_strike() {
        let c = Chooser::new();
        let broken = |b, ids: &[u32], _| {
            each(ids, |id| match b {
                SCK => Ok(frame(id, [1, 1, 1])),
                _ => Err(CaptureError::EmptyImage),
            })
        };
        for _ in 1..OLD_API_STRIKES {
            let _ = c.run(&[CG, XCAP, SCK], &[1], BUDGET, broken);
        }
        assert_eq!(c.order(Preference::Auto, BOTH), here(&[CG, XCAP, SCK]));
        let _ = c.run(&[CG, XCAP, SCK], &[1], BUDGET, broken);
        assert_eq!(c.order(Preference::Auto, BOTH), here(&[SCK, CG, XCAP]));
        // Either one working clears the strikes.
        let _ = c.run(&[CG, XCAP], &[1], BUDGET, |b, ids, _| {
            each(ids, |id| match b {
                CG => Err(CaptureError::EmptyImage),
                _ => Ok(frame(id, [1, 1, 1])),
            })
        });
        assert_eq!(c.order(Preference::Auto, BOTH), here(&[CG, XCAP, SCK]));
    }

    #[test]
    fn falls_back_when_xcap_fails_or_returns_black() {
        let c = Chooser::new();
        let tried = RefCell::new(vec![]);
        let (result, used) = c.run(&[XCAP, SCK], &[1], BUDGET, |b, ids, _| {
            tried.borrow_mut().push(b);
            each(ids, |id| match b {
                SCK => Ok(frame(id, [9, 9, 9])),
                _ => Err(CaptureError::Backend("gone".into())),
            })
        });
        assert_eq!(used, [(SCK, 1)]);
        assert_eq!(result.unwrap()[0].image.get_pixel(0, 0).0, [9, 9, 9, 255]);
        assert_eq!(*tried.borrow(), [XCAP, SCK]);

        let (_, used) = c.run(&[XCAP, SCK], &[1, 2], BUDGET, |b, ids, _| {
            each(ids, |id| match b {
                SCK => Ok(frame(id, [1, 2, 3])),
                _ => Ok(frame(id, [0, 0, 0])),
            })
        });
        assert_eq!(used, [(SCK, 2)]);
        assert_eq!(describe(&used), "ScreenCaptureKit (2)");
    }

    #[test]
    fn only_the_missing_displays_are_captured_again() {
        let c = Chooser::new();
        let asked = RefCell::new(vec![]);
        let (result, used) = c.run(&[XCAP, SCK], &[1, 2, 3], BUDGET, |b, ids, _| {
            asked.borrow_mut().push((b, ids.to_vec()));
            each(ids, |id| match (b, id) {
                (XCAP, 1) => Ok(frame(id, [5, 5, 5])),
                (XCAP, 2) => Err(CaptureError::Backend("display 2 timed out".into())),
                (XCAP, _) => Ok(frame(id, [0, 0, 0])),
                _ => Ok(frame(id, [7, 7, 7])),
            })
        });
        assert_eq!(*asked.borrow(), [(XCAP, vec![1, 2, 3]), (SCK, vec![2, 3])]);
        assert_eq!(used, [(XCAP, 1), (SCK, 2)]);
        assert_eq!(describe(&used), "xcap (1), ScreenCaptureKit (2)");
        let frames = result.unwrap();
        assert_eq!(ids(&frames), [1, 2, 3], "in display order");
        assert_eq!(frames[2].image.get_pixel(0, 0).0, [7, 7, 7, 255]);
    }

    #[test]
    fn a_black_screen_is_kept_when_nothing_does_better() {
        let c = Chooser::new();
        let (result, used) = c.run(&[XCAP, SCK], &[1, 2], BUDGET, |b, ids, _| {
            each(ids, |id| match (b, id) {
                (XCAP, 1) => Ok(frame(id, [0, 0, 0])),
                (XCAP, _) => Ok(frame(id, [4, 4, 4])),
                _ => Err(CaptureError::Backend("alert".into())),
            })
        });
        assert_eq!(ids(&result.unwrap()), [1, 2]);
        assert_eq!(used, [(XCAP, 1)]);
    }

    #[test]
    fn a_working_xcap_is_not_second_guessed() {
        let c = Chooser::new();
        let (result, used) = c.run(&[XCAP, SCK], &[1, 2], BUDGET, |b, ids, _| {
            assert_eq!(b, XCAP);
            each(ids, |id| Ok(frame(id, [5, 5, 5])))
        });
        assert_eq!(used, [(XCAP, 2)]);
        assert_eq!(ids(&result.unwrap()), [1, 2]);
    }

    #[test]
    fn with_nothing_usable_the_first_error_is_returned() {
        let c = Chooser::new();
        let (result, used) = c.run(&[XCAP, SCK], &[1], BUDGET, |b, ids, _| {
            each(ids, |_| Err(CaptureError::Backend(b.to_string())))
        });
        assert_eq!(result, Err(CaptureError::Backend("xcap".into())));
        assert!(used.is_empty());
    }

    #[test]
    fn the_fallback_only_gets_the_time_that_is_left() {
        let c = Chooser::new();
        let ms = Duration::from_millis;
        // A fake clock: each backend moves it on by the time it "took".
        let clock = std::cell::Cell::new(Instant::now());
        let now = || clock.get();
        let given = RefCell::new(vec![]);
        let _ = c.run_with_clock(
            &[XCAP, SCK],
            &[1],
            ms(200),
            |b, ids, left| {
                given.borrow_mut().push((b, left));
                clock.set(clock.get() + ms(80));
                each(ids, |_| Err(CaptureError::EmptyImage))
            },
            now,
        );
        assert_eq!(*given.borrow(), [(XCAP, ms(200)), (SCK, ms(120))]);

        // A backend that uses up the whole budget leaves nothing to try.
        let tried = RefCell::new(vec![]);
        let _ = c.run_with_clock(
            &[XCAP, SCK],
            &[1],
            ms(30),
            |b, ids, left| {
                tried.borrow_mut().push(b);
                clock.set(clock.get() + left + ms(5));
                each(ids, |_| Err(CaptureError::EmptyImage))
            },
            now,
        );
        assert_eq!(*tried.borrow(), [XCAP]);
    }

    #[test]
    fn screencapturekit_goes_first_after_xcap_keeps_failing() {
        let c = Chooser::new();
        let broken = |b, ids: &[u32], _| {
            each(ids, |id| match b {
                SCK => Ok(frame(id, [1, 1, 1])),
                _ => Err(CaptureError::EmptyImage),
            })
        };
        for _ in 0..OLD_API_STRIKES {
            assert_eq!(c.order(Preference::Auto, BOTH), here(&[CG, XCAP, SCK]));
            let _ = c.run(&[XCAP, SCK], &[1], BUDGET, broken);
        }
        assert_eq!(c.order(Preference::Auto, BOTH), here(&[SCK, CG, XCAP]));
        // Asking does not use up a turn.
        assert_eq!(c.order(Preference::Auto, BOTH), here(&[SCK, CG, XCAP]));
        // Now and then the old API gets to go first again.
        let old = here(&[CG, XCAP])[0];
        let firsts: Vec<_> = (0..2 * OLD_API_RETRY_EVERY)
            .map(|_| {
                let first = c.order(Preference::Auto, BOTH)[0];
                let _ = c.run(&[SCK], &[1], BUDGET, broken);
                first
            })
            .collect();
        assert_eq!(firsts.iter().filter(|&&b| b == old).count(), 2);
        assert_eq!(firsts[OLD_API_RETRY_EVERY as usize - 1], old);
        // Forcing still wins.
        assert_eq!(c.order(Preference::Only(XCAP), BOTH), [XCAP]);
        // xcap working again resets it.
        let _ = c.run(&[XCAP], &[1], BUDGET, |_, ids, _| {
            each(ids, |id| Ok(frame(id, [1, 1, 1])))
        });
        assert_eq!(c.order(Preference::Auto, BOTH), here(&[CG, XCAP, SCK]));
    }

    #[test]
    fn black_means_every_pixel_is_black() {
        let mut img = RgbaImage::new(1000, 700);
        assert!(is_black(&img), "alpha does not count");
        assert!(is_black(&RgbaImage::new(0, 0)));
        // One dim pixel far from any sampling grid still counts as content.
        img.put_pixel(517, 333, image::Rgba([0, 0, 1, 0]));
        assert!(!is_black(&img));
        let mut img = RgbaImage::new(3600, 2338);
        img.put_pixel(3599, 2337, image::Rgba([1, 0, 0, 255]));
        assert!(!is_black(&img), "the last pixel counts too");
        assert!(!is_black(&RgbaImage::from_pixel(
            3,
            3,
            image::Rgba([0, 9, 0, 255])
        )));
    }

    #[test]
    fn padded_rows_lose_their_padding() {
        // 3x3 pixels in rows of 16 bytes (4 of padding filled with 0xEE), as CoreGraphics pads a
        // row to its alignment. Each pixel is BGRA with blue = x, green = y, red = 9.
        let (w, h, stride) = (3, 3, 16);
        let data: Vec<u8> = (0..h)
            .flat_map(|y| {
                let pixels = (0..w).flat_map(move |x| [x as u8, y as u8, 9, 255]);
                pixels.chain([0xEE; 4])
            })
            .collect();
        assert_eq!(data.len(), stride * h);
        let img = bgra_to_rgba(&data, w, h, stride).unwrap();
        assert_eq!(img.as_raw().len(), w * h * 4, "no padding kept");
        for (x, y, p) in img.enumerate_pixels() {
            assert_eq!(p.0, [9, y as u8, x as u8, 255], "pixel {x},{y}");
        }
        // No padding: the stride is the row.
        let tight: Vec<u8> = data
            .chunks(stride)
            .flat_map(|r| r[..w * 4].to_vec())
            .collect();
        assert_eq!(bgra_to_rgba(&tight, w, h, w * 4), Some(img.clone()));
        // Extra bytes after the last row are ignored.
        let mut longer = data.clone();
        longer.extend([1; 64]);
        assert_eq!(bgra_to_rgba(&longer, w, h, stride), Some(img));
    }

    #[test]
    fn bgra_rows_become_rgba_without_padding() {
        // 2x2, 12 bytes per row (4 of padding).
        let data = [
            1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, //
            9, 10, 11, 12, 13, 14, 15, 16, 0, 0, 0, 0,
        ];
        let img = bgra_to_rgba(&data, 2, 2, 12).unwrap();
        assert_eq!(img.dimensions(), (2, 2));
        assert_eq!(
            img.into_raw(),
            [3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]
        );
        // The last row needs no padding after it.
        assert!(bgra_to_rgba(&data[..20], 2, 2, 12).is_some());
        assert!(bgra_to_rgba(&data[..19], 2, 2, 12).is_none(), "too short");
        assert!(
            bgra_to_rgba(&data, 4, 2, 12).is_none(),
            "row wider than stride"
        );
        assert!(bgra_to_rgba(&data, 0, 2, 12).is_none());
        assert!(bgra_to_rgba(&data, 2, 0, 12).is_none());
        assert!(bgra_to_rgba(&data, usize::MAX, 1, 12).is_none());
        assert!(bgra_to_rgba(&data, 1, usize::MAX, 12).is_none());
    }

    #[test]
    fn pixel_size_uses_the_mode_scale() {
        assert_eq!(pixel_size((1800, 1169), 3600, 1800), (3600, 2338));
        assert_eq!(pixel_size((1920, 1080), 1920, 1920), (1920, 1080));
        // Rotated: the points are swapped, the mode is not.
        assert_eq!(pixel_size((1169, 1800), 3600, 1800), (2338, 3600));
        assert_eq!(pixel_size((1512, 982), 3024, 1512), (3024, 1964));
        assert_eq!(pixel_size((100, 50), 0, 100), (100, 50), "no mode");
        assert_eq!(pixel_size((100, 50), 200, 0), (100, 50));
    }

    #[test]
    fn content_is_fetched_again_after_a_display_change() {
        assert!(needs_refresh(None, 0, &[]));
        assert!(!needs_refresh(Some((3, &[1, 2])), 3, &[2]));
        assert!(!needs_refresh(Some((3, &[1, 2])), 3, &[]));
        assert!(
            needs_refresh(Some((3, &[1, 2])), 4, &[1]),
            "displays changed"
        );
        assert!(
            needs_refresh(Some((3, &[1, 2])), 3, &[5]),
            "display not in it"
        );
    }
}
