//! ScreenCaptureKit capture (macOS 14+): `SCScreenshotManager` through objc2, so no Swift
//! runtime. OS calls only; the decisions (when to fetch the content again, the pixel size, the
//! pixel copy) are in `backend.rs`, and the geometry and pixel copy are shared with `cg.rs`.
//!
//! `SCShareableContent` costs tens of milliseconds, so it is fetched once and kept. It is fetched
//! again when the displays change (a reconfiguration callback bumps [`GENERATION`]) or when a
//! display is missing from it. Each display is captured on its own thread (see `capture_each`);
//! threads that need the content at once share one fetch ([`Shared`]).

use super::{Bounds, CaptureError, backend, cg};
use block2::RcBlock;
use image::RgbaImage;
use objc2::AllocAnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGColorSpace, CGDirectDisplayID, CGDisplayChangeSummaryFlags, CGDisplayCopyColorSpace,
    CGDisplayRegisterReconfigurationCallback, CGImage,
};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCCaptureResolutionType, SCContentFilter, SCDisplay, SCRunningApplication, SCScreenshotManager,
    SCShareableContent, SCStreamConfiguration,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Once, PoisonError, mpsc};
use std::time::{Duration, Instant};

/// Bumped whenever the displays change, so the cached content is fetched again.
static GENERATION: AtomicU64 = AtomicU64::new(0);

static CONTENT: Shared<Content> = Shared::new();

/// How long to wait for a completion handler: a capture's own timeout.
const ANSWER_TIMEOUT: Duration = super::DISPLAY_TIMEOUT;

/// A refresh after a display change is running, so the callbacks of one change (one per display)
/// start one fetch.
static REFRESHING: AtomicBool = AtomicBool::new(false);

/// `kCVPixelFormatType_32BGRA`.
const BGRA: u32 = u32::from_be_bytes(*b"BGRA");

#[derive(Clone)]
struct Content {
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

/// Captures display `id` at its native resolution, without xcreenshot's windows. Returns its
/// bounds in points and its scale too, read from CoreGraphics, so this works even where xcap
/// cannot describe the display.
pub fn capture(id: u32) -> Result<(Bounds, f64, RgbaImage), CaptureError> {
    let (points, (width, height)) = cg::geometry(id);
    let scale = width as f64 / f64::from(points.width.max(1));
    let image = shoot(id, width, height, None)?;
    Ok((points, scale, image))
}

/// Fetches the content and takes a 1 px screenshot of display `id`, so the first capture does
/// not pay the one-time setup.
pub fn warm_up(id: u32) -> Result<(), CaptureError> {
    let pixel = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1.0, 1.0));
    shoot(id, 1, 1, Some(pixel)).map(drop)
}

/// Runs on a capture worker thread, which has no autorelease pool of its own, so one is made
/// here and drained after every screenshot.
fn shoot(
    id: u32,
    width: usize,
    height: usize,
    source: Option<CGRect>,
) -> Result<RgbaImage, CaptureError> {
    autoreleasepool(|_| shoot_in_pool(id, width, height, source))
}

fn shoot_in_pool(
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
    cg::to_rgba(&image.0, "ScreenCaptureKit")
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

/// The cached content, fetched again if [`backend::needs_refresh`] says so.
fn content(wanted: &[u32]) -> Result<Retained<SCShareableContent>, CaptureError> {
    watch_displays();
    let generation = GENERATION.load(Ordering::Acquire);
    let fresh =
        |at: u64, c: &Content| !backend::needs_refresh(Some((at, &c.ids)), generation, wanted);
    CONTENT
        .get(generation, ANSWER_TIMEOUT, fresh, fetch)
        .map(|c| c.shareable)
        .map_err(CaptureError::Backend)
}

/// Starts fetching the content; the completion handler hands it to [`CONTENT`].
fn fetch(pending: Arc<Fetch<Content>>) {
    let done = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            // SAFETY: the handler gets valid content or null; retained to outlive the handler.
            let content = unsafe { Retained::retain(content) }.map(|shareable| Content {
                // SAFETY: plain getters.
                ids: unsafe { shareable.displays() }
                    .iter()
                    .map(|d| unsafe { d.displayID() })
                    .collect(),
                shareable,
            });
            let now = GENERATION.load(Ordering::Acquire);
            CONTENT.store(&pending, content.ok_or_else(|| describe(error)), now);
        },
    );
    // SAFETY: the block is copied by the callee.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&done) };
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A value fetched asynchronously and kept for its display generation. Callers that need it
/// while a fetch runs share that fetch and all stop waiting at its one deadline, so a fetch that
/// never answers neither queues more fetches behind it nor holds anyone past the timeout.
struct Shared<T> {
    state: Mutex<SharedState<T>>,
}

struct SharedState<T> {
    /// The kept value and the generation it was fetched for.
    value: Option<(u64, T)>,
    fetch: Option<Arc<Fetch<T>>>,
}

/// One fetch in progress.
struct Fetch<T> {
    generation: u64,
    deadline: Instant,
    result: Mutex<Option<Result<T, String>>>,
    done: Condvar,
}

impl<T: Clone> Shared<T> {
    const fn new() -> Self {
        Self {
            state: Mutex::new(SharedState {
                value: None,
                fetch: None,
            }),
        }
    }

    /// The kept value if `fresh` accepts it. Otherwise the result of the fetch already running
    /// for `generation`, or of a new one that `start` begins (outside the lock, so a handler
    /// that answers at once can store).
    fn get(
        &self,
        generation: u64,
        timeout: Duration,
        fresh: impl Fn(u64, &T) -> bool,
        start: impl FnOnce(Arc<Fetch<T>>),
    ) -> Result<T, String> {
        let (pending, new) = {
            let mut state = lock(&self.state);
            if let Some((at, value)) = &state.value
                && fresh(*at, value)
            {
                return Ok(value.clone());
            }
            match &state.fetch {
                Some(f) if f.generation == generation && Instant::now() < f.deadline => {
                    (Arc::clone(f), false)
                }
                _ => {
                    let f = Arc::new(Fetch {
                        generation,
                        deadline: Instant::now() + timeout,
                        result: Mutex::new(None),
                        done: Condvar::new(),
                    });
                    state.fetch = Some(Arc::clone(&f));
                    (f, true)
                }
            }
        };
        if new {
            start(Arc::clone(&pending));
        }
        pending.wait()
    }

    /// Ends `pending` with `result` and wakes its callers. The value is kept only if the
    /// generation is still the one it was fetched for (`now`): content fetched while the
    /// displays changed may lack the new ones.
    fn store(&self, pending: &Arc<Fetch<T>>, result: Result<T, String>, now: u64) {
        {
            let mut state = lock(&self.state);
            if state
                .fetch
                .as_ref()
                .is_some_and(|f| Arc::ptr_eq(f, pending))
            {
                state.fetch = None;
            }
            if let Ok(value) = &result
                && now == pending.generation
            {
                state.value = Some((now, value.clone()));
            }
        }
        *lock(&pending.result) = Some(result);
        pending.done.notify_all();
    }
}

impl<T: Clone> Fetch<T> {
    fn wait(&self) -> Result<T, String> {
        let mut result = lock(&self.result);
        loop {
            if let Some(r) = &*result {
                return r.clone();
            }
            let left = self.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("ScreenCaptureKit did not answer".into());
            }
            result = self
                .done
                .wait_timeout(result, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
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
        let e = NSError::new(-3801, objc2_foundation::ns_string!("SCStreamErrorDomain"));
        let text = describe(Retained::as_ptr(&e).cast_mut());
        assert_eq!(text, e.localizedDescription().to_string());
        assert!(text.contains("-3801"), "{text}");
    }

    fn shared() -> &'static Shared<u32> {
        Box::leak(Box::new(Shared::new()))
    }

    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn callers_at_once_share_one_fetch() {
        let s = shared();
        let starts = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let callers: Vec<_> = (0..3)
            .map(|_| {
                let starts = Arc::clone(&starts);
                std::thread::spawn(move || {
                    s.get(
                        1,
                        WAIT,
                        |_, _| false,
                        |pending| {
                            starts.fetch_add(1, Ordering::SeqCst);
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_millis(50));
                                s.store(&pending, Ok(7), 1);
                            });
                        },
                    )
                })
            })
            .collect();
        for c in callers {
            assert_eq!(c.join().unwrap(), Ok(7));
        }
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        // Kept: no new fetch while it is fresh.
        let kept = s.get(1, WAIT, |at, v| at == 1 && *v == 7, |_| unreachable!());
        assert_eq!(kept, Ok(7));
    }

    #[test]
    fn a_fetch_that_never_answers_times_out_once_for_everyone() {
        let s = shared();
        let timeout = Duration::from_millis(50);
        let started = Instant::now();
        let pendings = Arc::new(Mutex::new(vec![]));
        let callers: Vec<_> = (0..3)
            .map(|_| {
                let pendings = Arc::clone(&pendings);
                std::thread::spawn(move || {
                    s.get(1, timeout, |_, _| false, |p| lock(&pendings).push(p))
                })
            })
            .collect();
        for c in callers {
            assert!(c.join().unwrap().is_err());
        }
        assert!(started.elapsed() < Duration::from_secs(2), "no queueing");
        assert_eq!(lock(&pendings).len(), 1);
        // After its deadline, the next caller starts a new fetch.
        let r = s.get(1, WAIT, |_, _| false, |p| s.store(&p, Ok(3), 1));
        assert_eq!(r, Ok(3));
    }

    #[test]
    fn content_from_before_a_display_change_is_not_kept() {
        let s = shared();
        // The displays changed (generation 2) while generation 1 was fetched.
        let r = s.get(1, WAIT, |_, _| true, |p| s.store(&p, Ok(5), 2));
        assert_eq!(r, Ok(5), "the caller still gets it");
        let r = s.get(2, WAIT, |_, _| true, |p| s.store(&p, Ok(6), 2));
        assert_eq!(r, Ok(6), "fetched again, not the stale one");
        // A failed fetch is not kept either.
        let s = shared();
        let r = s.get(
            1,
            WAIT,
            |_, _| true,
            |p| s.store(&p, Err("denied".into()), 1),
        );
        assert_eq!(r, Err("denied".into()));
        let r = s.get(1, WAIT, |_, _| true, |p| s.store(&p, Ok(1), 1));
        assert_eq!(r, Ok(1));
    }

    #[test]
    fn an_unanswered_call_times_out() {
        let r = answer::<()>(drop);
        assert!(matches!(r, Err(CaptureError::Backend(_))));
        let r = answer::<()>(|tx| tx.send(Err("denied".into())).unwrap());
        assert!(matches!(r, Err(CaptureError::Backend(e)) if e == "denied"));
    }
}
