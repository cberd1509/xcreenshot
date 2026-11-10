//! ScreenCaptureKit capture (macOS 14+): `SCScreenshotManager` through objc2, so no Swift
//! runtime. OS calls only; the decisions (when to fetch the content again, the pixel size, the
//! pixel copy) are in `backend.rs`.
//!
//! `SCShareableContent` costs tens of milliseconds, so it is fetched once and kept. It is fetched
//! again when the displays change (a reconfiguration callback bumps [`GENERATION`]) or when a
//! display is missing from it. Each display is captured on its own thread (see `capture_each`).

use super::{Bounds, CaptureError, backend};
use block2::RcBlock;
use image::RgbaImage;
use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGColorSpace, CGDataProvider, CGDirectDisplayID, CGDisplayChangeSummaryFlags,
    CGDisplayCopyColorSpace, CGDisplayCopyDisplayMode, CGDisplayMode,
    CGDisplayRegisterReconfigurationCallback, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCCaptureResolutionType, SCContentFilter, SCDisplay, SCRunningApplication, SCScreenshotManager,
    SCShareableContent, SCStreamConfiguration,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, Once, PoisonError, mpsc};
use std::time::Duration;

/// Bumped whenever the displays change, so the cached content is fetched again.
static GENERATION: AtomicU64 = AtomicU64::new(0);

static CONTENT: Mutex<Option<Content>> = Mutex::new(None);

/// How long to wait for a completion handler: a capture's own timeout. The content lock is held
/// while fetching, so a longer wait would hold up the next capture's displays too.
const ANSWER_TIMEOUT: Duration = super::DISPLAY_TIMEOUT;

/// A refresh after a display change is running, so the callbacks of one change (one per display)
/// start one fetch.
static REFRESHING: AtomicBool = AtomicBool::new(false);

/// `kCVPixelFormatType_32BGRA`.
const BGRA: u32 = u32::from_be_bytes(*b"BGRA");

struct Content {
    generation: u64,
    ids: Vec<u32>,
    shareable: Retained<SCShareableContent>,
}

// SAFETY: shareable content is immutable once fetched (`Sendable` in Swift).
unsafe impl Send for Content {}

/// Moves an OS object to the thread waiting for it.
struct Sendable<T>(T);

// SAFETY: only immutable ScreenCaptureKit and CoreGraphics objects are wrapped (shareable
// content, images), which are thread-safe (`Sendable` in Swift).
unsafe impl<T> Send for Sendable<T> {}

/// Whether `SCScreenshotManager` exists (macOS 14+).
pub fn available() -> bool {
    objc2::available!(macos = 14.0)
}

/// Captures display `id`, `points` big, at its native resolution, without xcreenshot's windows.
pub fn capture(id: u32, points: Bounds) -> Result<RgbaImage, CaptureError> {
    let mode = CGDisplayCopyDisplayMode(id);
    let (width, height) = backend::pixel_size(
        (points.width, points.height),
        CGDisplayMode::pixel_width(mode.as_deref()),
        CGDisplayMode::width(mode.as_deref()),
    );
    shoot(id, width, height, None)
}

/// Fetches the content and takes a 1 px screenshot of display `id`, so the first capture does
/// not pay the one-time setup.
pub fn warm_up(id: u32) -> Result<(), CaptureError> {
    let pixel = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1.0, 1.0));
    shoot(id, 1, 1, Some(pixel)).map(drop)
}

fn shoot(
    id: u32,
    width: usize,
    height: usize,
    source: Option<CGRect>,
) -> Result<RgbaImage, CaptureError> {
    let content = content(&[id])?;
    // SAFETY: plain getters on the content and its displays.
    let display = unsafe { content.displays() }
        .iter()
        .find(|d| unsafe { d.displayID() } == id)
        .ok_or_else(|| CaptureError::Backend(format!("display {id} disappeared")))?;
    let filter = filter(&content, &display);
    let config = unsafe { SCStreamConfiguration::new() };
    // Kept alive until the capture is done: the configuration does not retain it.
    let color_space = CGColorSpace::name(Some(&CGDisplayCopyColorSpace(id)));
    // SAFETY: setters on a configuration this thread owns; `color_space` outlives it.
    unsafe {
        config.setWidth(width);
        config.setHeight(height);
        config.setShowsCursor(false);
        config.setPixelFormat(BGRA);
        config.setCaptureResolution(SCCaptureResolutionType::Best);
        if let Some(source) = source {
            config.setSourceRect(source);
        }
        if let Some(name) = &color_space {
            config.setColorSpaceName(name);
        }
    }
    let image = answer(|tx| {
        let done = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
            // SAFETY: the handler gets a valid image or null; retained to outlive the handler.
            let image = NonNull::new(image).map(|p| Sendable(unsafe { CFRetained::retain(p) }));
            let _ = tx.try_send(image.ok_or_else(|| describe(error)));
        });
        // SAFETY: the filter and configuration are valid; the block is copied by the callee.
        unsafe {
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                &filter,
                &config,
                Some(&done),
            );
        }
    })?;
    drop(color_space);
    to_rgba(&image.0)
}

/// Captures `display` and everything on it except xcreenshot's own windows (overlays, toast,
/// notices), wherever they are. The app is in the content once it has a window, shown or not;
/// the overlay windows exist from startup, before any capture.
fn filter(content: &SCShareableContent, display: &SCDisplay) -> Retained<SCContentFilter> {
    let me = std::process::id() as i32;
    // SAFETY: plain getters, then a designated initializer with arrays from the same content.
    unsafe {
        let ours: Vec<Retained<SCRunningApplication>> = content
            .applications()
            .iter()
            .filter(|app| app.processID() == me)
            .collect();
        SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            display,
            &NSArray::from_retained_slice(&ours),
            &NSArray::new(),
        )
    }
}

/// The cached content, fetched again if [`backend::needs_refresh`] says so. The lock is held
/// while fetching, so displays captured at once share one fetch.
fn content(wanted: &[u32]) -> Result<Retained<SCShareableContent>, CaptureError> {
    watch_displays();
    let mut cache = CONTENT.lock().unwrap_or_else(PoisonError::into_inner);
    let generation = GENERATION.load(Ordering::Acquire);
    let cached = cache.as_ref().map(|c| (c.generation, c.ids.as_slice()));
    if backend::needs_refresh(cached, generation, wanted) {
        *cache = Some(fetch(generation)?);
    }
    Ok(cache
        .as_ref()
        .map(|c| c.shareable.clone())
        .expect("filled above"))
}

fn fetch(generation: u64) -> Result<Content, CaptureError> {
    let shareable = answer(|tx| {
        let done = RcBlock::new(
            move |content: *mut SCShareableContent, error: *mut NSError| {
                // SAFETY: the handler gets valid content or null; retained to outlive the handler.
                let content = unsafe { Retained::retain(content) }.map(Sendable);
                let _ = tx.try_send(content.ok_or_else(|| describe(error)));
            },
        );
        // SAFETY: the block is copied by the callee.
        unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&done) };
    })?
    .0;
    // SAFETY: plain getters.
    let ids = unsafe { shareable.displays() }
        .iter()
        .map(|d| unsafe { d.displayID() })
        .collect();
    Ok(Content {
        generation,
        ids,
        shareable,
    })
}

/// Starts an async call with a sender for its result, then waits for it.
fn answer<T>(
    start: impl FnOnce(mpsc::SyncSender<Result<Sendable<T>, String>>),
) -> Result<Sendable<T>, CaptureError> {
    let (tx, rx) = mpsc::sync_channel(1);
    start(tx);
    rx.recv_timeout(ANSWER_TIMEOUT)
        .map_err(|_| CaptureError::Backend("ScreenCaptureKit did not answer".into()))?
        .map_err(CaptureError::Backend)
}

fn describe(error: *mut NSError) -> String {
    // SAFETY: the handler's error is valid or null.
    unsafe { error.as_ref() }.map_or_else(
        || "ScreenCaptureKit returned nothing".into(),
        |e| e.localizedDescription().to_string(),
    )
}

/// The screenshot is BGRA (8 bits per channel, little-endian with alpha first) in SDR, rows
/// maybe padded. Anything else is refused rather than shown with the wrong colours.
fn to_rgba(image: &CGImage) -> Result<RgbaImage, CaptureError> {
    let bad = |what: &str| CaptureError::Backend(format!("ScreenCaptureKit image: {what}"));
    let alpha_first = matches!(
        CGImage::alpha_info(Some(image)),
        CGImageAlphaInfo::PremultipliedFirst
            | CGImageAlphaInfo::NoneSkipFirst
            | CGImageAlphaInfo::First
    );
    if CGImage::bits_per_pixel(Some(image)) != 32
        || CGImage::byte_order_info(Some(image)) != CGImageByteOrderInfo::Order32Little
        || !alpha_first
    {
        return Err(bad("not BGRA"));
    }
    let data = CGDataProvider::data(CGImage::data_provider(Some(image)).as_deref())
        .ok_or_else(|| bad("no pixels"))?;
    // SAFETY: `data` is immutable and alive for the copy.
    let bytes = unsafe { data.as_bytes_unchecked() };
    backend::bgra_to_rgba(
        bytes,
        CGImage::width(Some(image)),
        CGImage::height(Some(image)),
        CGImage::bytes_per_row(Some(image)),
    )
    .ok_or_else(|| bad("wrong size"))
}

/// Registers [`on_display_change`] once.
fn watch_displays() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: a plain function with no user data, never unregistered.
        let err = unsafe {
            CGDisplayRegisterReconfigurationCallback(Some(on_display_change), std::ptr::null_mut())
        };
        if err.0 != 0 {
            log::warn!("cannot watch display changes ({err:?}); content is fetched per display");
        }
    });
}

/// Called after displays are added, removed or changed: marks the content stale and fetches it
/// again off this (main) thread, so the next capture does not wait for it.
unsafe extern "C-unwind" fn on_display_change(
    _: CGDirectDisplayID,
    flags: CGDisplayChangeSummaryFlags,
    _: *mut c_void,
) {
    if flags.contains(CGDisplayChangeSummaryFlags::BeginConfigurationFlag) {
        return;
    }
    GENERATION.fetch_add(1, Ordering::AcqRel);
    if REFRESHING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(|| {
        if let Err(e) = content(&[]) {
            log::warn!("could not refresh ScreenCaptureKit content: {e}");
        }
        REFRESHING.store(false, Ordering::Release);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_on_supported_macos() {
        // CI and dev Macs run macOS 14 or later.
        assert!(available());
    }

    #[test]
    fn errors_are_described() {
        assert_eq!(
            describe(std::ptr::null_mut()),
            "ScreenCaptureKit returned nothing"
        );
        let e = NSError::new(-1, objc2_foundation::ns_string!("SCStreamErrorDomain"));
        let text = describe(Retained::as_ptr(&e).cast_mut());
        assert!(!text.is_empty());
    }

    #[test]
    fn an_unanswered_call_times_out() {
        let r = answer::<()>(drop);
        assert!(matches!(r, Err(CaptureError::Backend(_))));
        let r = answer::<()>(|tx| tx.send(Err("denied".into())).unwrap());
        assert!(matches!(r, Err(CaptureError::Backend(e)) if e == "denied"));
    }
}
