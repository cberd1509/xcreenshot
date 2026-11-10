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

    /// Captures with each backend in `order` until one gives a usable result, and says which
    /// one it was. With none usable, the last result is returned as it came.
    pub fn run(
        &self,
        order: &[Backend],
        mut capture: impl FnMut(Backend) -> Result<Vec<DisplayFrame>, CaptureError>,
    ) -> (Result<Vec<DisplayFrame>, CaptureError>, Option<Backend>) {
        if self.struck_out() {
            self.sck_first.fetch_add(1, Ordering::Relaxed);
        }
        let mut last = (Err(no_backend()), None);
        for &backend in order {
            let result = capture(backend);
            let problem = unusable(&result);
            self.record(backend, problem.is_none());
            match problem {
                None => return (result, Some(backend)),
                Some(why) => log::warn!("{backend} capture unusable: {why}"),
            }
            last = (result, Some(backend));
        }
        last
    }
}

/// Why a capture cannot be used, if it cannot: an error, or every display black (what a
/// removed or stubbed-out API tends to return). A screen that is really all black (no menu bar)
/// is rare; it only costs a second capture. A permission problem can look valid (wallpaper
/// only), so that is checked before capturing, not here.
fn unusable(result: &Result<Vec<DisplayFrame>, CaptureError>) -> Option<String> {
    match result {
        Err(e) => Some(e.to_string()),
        Ok(frames) if frames.iter().all(|f| is_black(&f.image)) => {
            Some("every display came back black".into())
        }
        Ok(_) => None,
    }
}

/// Whether every sampled pixel is black, on a grid of about 64x64 points. A real screen
/// (menu bar, cursorless desktop) is never all black at that density.
pub fn is_black(image: &RgbaImage) -> bool {
    let (w, h) = image.dimensions();
    let step = |n: u32| (n / 64).max(1) as usize;
    (0..h).step_by(step(h)).all(|y| {
        (0..w)
            .step_by(step(w))
            .all(|x| image.get_pixel(x, y).0[..3] == [0, 0, 0])
    })
}

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

    fn frame(rgb: [u8; 3]) -> DisplayFrame {
        let b = Bounds {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        };
        DisplayFrame {
            id: 1,
            logical: b,
            physical: b,
            scale_factor: 1.0,
            image: RgbaImage::from_pixel(4, 4, image::Rgba([rgb[0], rgb[1], rgb[2], 255])),
        }
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
        let (result, used) = c.run(&[], |_| unreachable!());
        assert!(matches!(result, Err(CaptureError::Backend(_))));
        assert_eq!(used, None);
    }

    #[test]
    fn falls_back_when_xcap_fails_or_returns_black() {
        let c = Chooser::new();
        let tried = RefCell::new(vec![]);
        let (result, used) = c.run(&[XCAP, SCK], |b| {
            tried.borrow_mut().push(b);
            match b {
                XCAP => Err(CaptureError::Backend("gone".into())),
                SCK => Ok(vec![frame([9, 9, 9])]),
            }
        });
        assert_eq!(used, Some(SCK));
        assert_eq!(result.unwrap()[0].image.get_pixel(0, 0).0, [9, 9, 9, 255]);
        assert_eq!(*tried.borrow(), [XCAP, SCK]);

        let (_, used) = c.run(&[XCAP, SCK], |b| match b {
            XCAP => Ok(vec![frame([0, 0, 0]), frame([0, 0, 0])]),
            SCK => Ok(vec![frame([1, 2, 3])]),
        });
        assert_eq!(used, Some(SCK));
        let (_, used) = c.run(&[XCAP, SCK], |b| match b {
            XCAP => Ok(vec![]),
            SCK => Ok(vec![frame([1, 2, 3])]),
        });
        assert_eq!(used, Some(SCK), "nothing captured counts as black");
    }

    #[test]
    fn a_working_xcap_is_not_second_guessed() {
        let c = Chooser::new();
        let (_, used) = c.run(&[XCAP, SCK], |b| {
            assert_eq!(b, XCAP);
            Ok(vec![frame([0, 0, 0]), frame([5, 5, 5])])
        });
        assert_eq!(used, Some(XCAP), "one black display among others is fine");
    }

    #[test]
    fn with_nothing_usable_the_last_result_is_returned() {
        let c = Chooser::new();
        let (result, used) = c.run(&[XCAP, SCK], |b| Err(CaptureError::Backend(b.to_string())));
        assert_eq!(
            result,
            Err(CaptureError::Backend("ScreenCaptureKit".into()))
        );
        assert_eq!(used, Some(SCK));
    }

    #[test]
    fn screencapturekit_goes_first_after_xcap_keeps_failing() {
        let c = Chooser::new();
        let broken = |b| match b {
            XCAP => Err(CaptureError::EmptyImage),
            SCK => Ok(vec![frame([1, 1, 1])]),
        };
        for _ in 0..XCAP_STRIKES {
            assert_eq!(c.order(Preference::Auto, BOTH), [XCAP, SCK]);
            let _ = c.run(&[XCAP, SCK], broken);
        }
        assert_eq!(c.order(Preference::Auto, BOTH), [SCK, XCAP]);
        // Asking does not use up a turn.
        assert_eq!(c.order(Preference::Auto, BOTH), [SCK, XCAP]);
        // Now and then xcap gets to go first again.
        let firsts: Vec<_> = (0..2 * XCAP_RETRY_EVERY)
            .map(|_| {
                let first = c.order(Preference::Auto, BOTH)[0];
                let _ = c.run(&[SCK], broken);
                first
            })
            .collect();
        assert_eq!(firsts.iter().filter(|&&b| b == XCAP).count(), 2);
        assert_eq!(firsts[XCAP_RETRY_EVERY as usize - 1], XCAP);
        // Forcing still wins.
        assert_eq!(c.order(Preference::Only(XCAP), BOTH), [XCAP]);
        // xcap working again resets it.
        let _ = c.run(&[XCAP], |_| Ok(vec![frame([1, 1, 1])]));
        assert_eq!(c.order(Preference::Auto, BOTH), [XCAP, SCK]);
    }

    #[test]
    fn black_means_every_sampled_pixel_is_black() {
        let mut img = RgbaImage::new(1000, 700);
        assert!(is_black(&img), "alpha does not count");
        img.put_pixel(0, 0, image::Rgba([0, 0, 1, 0]));
        assert!(!is_black(&img));
        assert!(is_black(&RgbaImage::new(0, 0)));
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
