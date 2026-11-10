//! Which capture backend runs, and whether what it returned can be used, plus the pure parts of
//! the ScreenCaptureKit backend (pixel copy, pixel size, when to refetch content), so they are
//! tested on every platform. The OS calls are xcap (`capture/mod.rs`) and ScreenCaptureKit
//! (`capture/sck.rs`, macOS 14+).
//!
//! xcap goes through `CGWindowListCreateImage`, which Apple has deprecated (#49). It is the
//! default while it works because it is faster. ScreenCaptureKit takes over when that call is
//! missing, fails, or returns only empty or black images, so a future macOS cannot break capture.

use super::{CaptureError, DisplayFrame};
use image::RgbaImage;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Forces a backend for testing: `xcap`, `screencapturekit` (or `sck`), or `auto` (the default).
pub const ENV: &str = "XCREENSHOT_CAPTURE_BACKEND";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Xcap,
    ScreenCaptureKit,
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Xcap => "xcap",
            Self::ScreenCaptureKit => "ScreenCaptureKit",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    /// xcap first, ScreenCaptureKit when xcap fails.
    Auto,
    Only(Backend),
}

impl Preference {
    /// Reads the value of [`ENV`]. Anything unknown is `Auto`.
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
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
            Backend::Xcap => self.xcap,
            Backend::ScreenCaptureKit => self.sck,
        }
    }
}

/// After this many xcap captures in a row that could not be used, ScreenCaptureKit goes first,
/// so a broken old API does not cost a failed attempt on every hotkey.
const XCAP_STRIKES: u32 = 3;

/// While ScreenCaptureKit goes first, xcap still goes first once in this many captures, so a
/// passing xcap problem does not keep the slower backend for the rest of the session.
const XCAP_RETRY_EVERY: u32 = 10;

/// The error when no backend can run here.
pub fn no_backend() -> CaptureError {
    CaptureError::Backend("no capture backend available".into())
}

/// How many displays each backend captured, in the order they ran.
pub type Used = Vec<(Backend, usize)>;

/// Picks the backend order for each capture and remembers how xcap has been doing.
pub struct Chooser {
    xcap_failures: AtomicU32,
    /// Captures since xcap reached [`XCAP_STRIKES`], for [`XCAP_RETRY_EVERY`].
    sck_first: AtomicU32,
}

impl Chooser {
    pub const fn new() -> Self {
        Self {
            xcap_failures: AtomicU32::new(0),
            sck_first: AtomicU32::new(0),
        }
    }

    fn struck_out(&self) -> bool {
        self.xcap_failures.load(Ordering::Relaxed) >= XCAP_STRIKES
    }

    /// The backends to try, in order. Empty if the forced one cannot run here. Only reads the
    /// state; [`Chooser::run`] updates it.
    pub fn order(&self, preference: Preference, support: Support) -> Vec<Backend> {
        const XCAP_FIRST: &[Backend] = &[Backend::Xcap, Backend::ScreenCaptureKit];
        const SCK_FIRST: &[Backend] = &[Backend::ScreenCaptureKit, Backend::Xcap];
        let retry = (self.sck_first.load(Ordering::Relaxed) + 1).is_multiple_of(XCAP_RETRY_EVERY);
        let preferred = match preference {
            Preference::Only(backend) => &[backend][..],
            Preference::Auto if self.struck_out() && !retry => SCK_FIRST,
            Preference::Auto => XCAP_FIRST,
        };
        preferred
            .iter()
            .copied()
            .filter(|&b| support.has(b))
            .collect()
    }

    fn record(&self, backend: Backend, usable: bool) {
        if backend == Backend::Xcap {
            if usable {
                self.xcap_failures.store(0, Ordering::Relaxed);
                self.sck_first.store(0, Ordering::Relaxed);
            } else {
                self.xcap_failures.fetch_add(1, Ordering::Relaxed);
            }
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
        mut capture: impl FnMut(Backend, &[u32], Duration) -> Vec<Result<DisplayFrame, CaptureError>>,
    ) -> (Result<Vec<DisplayFrame>, CaptureError>, Used) {
        if self.struck_out() {
            self.sck_first.fetch_add(1, Ordering::Relaxed);
        }
        let deadline = Instant::now() + budget;
        // `None` until a backend has tried the display.
        let mut results: Vec<Option<Result<DisplayFrame, CaptureError>>> =
            ids.iter().map(|_| None).collect();
        let mut used = vec![];
        for &backend in order {
            let missing: Vec<usize> = (0..ids.len())
                .filter(|&i| results[i].as_ref().is_none_or(|r| unusable(r).is_some()))
                .collect();
            if missing.is_empty() {
                break;
            }
            let left = deadline.saturating_duration_since(Instant::now());
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
                        log::warn!("{backend}: display {}: {why}", ids[i]);
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
            self.record(backend, captured > 0);
            if captured > 0 {
                used.push((backend, captured));
            }
        }
        let results = results
            .into_iter()
            .map(|r| r.unwrap_or_else(|| Err(no_backend())))
            .collect();
        (super::keep_captured(results), used)
    }
}

/// For the log: "xcap (2)" or "xcap (1), ScreenCaptureKit (1)".
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
    let mut out = Vec::with_capacity(row * height);
    // One pass: copy each row without its padding, swapping blue and red on the way.
    for y in 0..height {
        let start = y * bytes_per_row;
        for &[b, g, r, a] in data[start..start + row].as_chunks::<4>().0 {
            out.extend_from_slice(&[r, g, b, a]);
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
    const XCAP: Backend = Backend::Xcap;
    const SCK: Backend = Backend::ScreenCaptureKit;

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
        assert_eq!(p(Some("xcap")), Preference::Only(XCAP));
        assert_eq!(p(Some(" XCAP ")), Preference::Only(XCAP));
        assert_eq!(p(Some("sck")), Preference::Only(SCK));
        assert_eq!(p(Some("ScreenCaptureKit")), Preference::Only(SCK));
        assert_eq!(
            (XCAP.to_string(), SCK.to_string()),
            ("xcap".into(), "ScreenCaptureKit".into())
        );
    }

    #[test]
    fn xcap_goes_first_while_it_works() {
        let c = Chooser::new();
        assert_eq!(c.order(Preference::Auto, BOTH), [XCAP, SCK]);
        let old_macos = Support { sck: false, ..BOTH };
        assert_eq!(c.order(Preference::Auto, old_macos), [XCAP]);
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
        let old_macos = Support { sck: false, ..BOTH };
        assert!(c.order(Preference::Only(SCK), old_macos).is_empty());
        let (result, used) = c.run(&[], &[1], BUDGET, |_, _, _| unreachable!());
        assert_eq!(result, Err(no_backend()));
        assert!(used.is_empty());
    }

    #[test]
    fn falls_back_when_xcap_fails_or_returns_black() {
        let c = Chooser::new();
        let tried = RefCell::new(vec![]);
        let (result, used) = c.run(&[XCAP, SCK], &[1], BUDGET, |b, ids, _| {
            tried.borrow_mut().push(b);
            each(ids, |id| match b {
                XCAP => Err(CaptureError::Backend("gone".into())),
                SCK => Ok(frame(id, [9, 9, 9])),
            })
        });
        assert_eq!(used, [(SCK, 1)]);
        assert_eq!(result.unwrap()[0].image.get_pixel(0, 0).0, [9, 9, 9, 255]);
        assert_eq!(*tried.borrow(), [XCAP, SCK]);

        let (_, used) = c.run(&[XCAP, SCK], &[1, 2], BUDGET, |b, ids, _| {
            each(ids, |id| match b {
                XCAP => Ok(frame(id, [0, 0, 0])),
                SCK => Ok(frame(id, [1, 2, 3])),
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
                (SCK, _) => Ok(frame(id, [7, 7, 7])),
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
                (SCK, _) => Err(CaptureError::Backend("alert".into())),
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
        let budget = Duration::from_millis(200);
        let given = RefCell::new(vec![]);
        let started = Instant::now();
        let _ = c.run(&[XCAP, SCK], &[1], budget, |b, ids, left| {
            given.borrow_mut().push((b, left));
            if b == XCAP {
                std::thread::sleep(Duration::from_millis(80));
            }
            each(ids, |_| Err(CaptureError::EmptyImage))
        });
        let given = given.into_inner();
        assert_eq!(given[0].0, XCAP);
        assert!(given[0].1 <= budget && given[0].1 > budget - Duration::from_millis(50));
        assert_eq!(given[1].0, SCK);
        assert!(given[1].1 <= budget - Duration::from_millis(80));

        // A backend that uses up the whole budget leaves nothing to try.
        let tried = RefCell::new(vec![]);
        let _ = c.run(
            &[XCAP, SCK],
            &[1],
            Duration::from_millis(30),
            |b, ids, left| {
                tried.borrow_mut().push(b);
                std::thread::sleep(left + Duration::from_millis(5));
                each(ids, |_| Err(CaptureError::EmptyImage))
            },
        );
        assert_eq!(*tried.borrow(), [XCAP]);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn screencapturekit_goes_first_after_xcap_keeps_failing() {
        let c = Chooser::new();
        let broken = |b, ids: &[u32], _| {
            each(ids, |id| match b {
                XCAP => Err(CaptureError::EmptyImage),
                SCK => Ok(frame(id, [1, 1, 1])),
            })
        };
        for _ in 0..XCAP_STRIKES {
            assert_eq!(c.order(Preference::Auto, BOTH), [XCAP, SCK]);
            let _ = c.run(&[XCAP, SCK], &[1], BUDGET, broken);
        }
        assert_eq!(c.order(Preference::Auto, BOTH), [SCK, XCAP]);
        // Asking does not use up a turn.
        assert_eq!(c.order(Preference::Auto, BOTH), [SCK, XCAP]);
        // Now and then xcap gets to go first again.
        let firsts: Vec<_> = (0..2 * XCAP_RETRY_EVERY)
            .map(|_| {
                let first = c.order(Preference::Auto, BOTH)[0];
                let _ = c.run(&[SCK], &[1], BUDGET, broken);
                first
            })
            .collect();
        assert_eq!(firsts.iter().filter(|&&b| b == XCAP).count(), 2);
        assert_eq!(firsts[XCAP_RETRY_EVERY as usize - 1], XCAP);
        // Forcing still wins.
        assert_eq!(c.order(Preference::Only(XCAP), BOTH), [XCAP]);
        // xcap working again resets it.
        let _ = c.run(&[XCAP], &[1], BUDGET, |_, ids, _| {
            each(ids, |id| Ok(frame(id, [1, 1, 1])))
        });
        assert_eq!(c.order(Preference::Auto, BOTH), [XCAP, SCK]);
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
