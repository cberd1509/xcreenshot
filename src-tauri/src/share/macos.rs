//! AppKit glue for `NSSharingServicePicker`: FFI calls only, on the main thread. The decisions
//! (levels, edge, events, which pickers to keep) are in `picker.rs` and `session.rs`.

use super::picker::{self, Edge, Entry, Levels, Open};
use super::session::{Event, Session};
use super::{Rect, ShareError};
use crate::macos::ns_window;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSSharingService, NSSharingServiceDelegate, NSSharingServicePicker,
    NSSharingServicePickerDelegate, NSWindow,
};
use objc2_foundation::{NSArray, NSError, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};
use std::cell::RefCell;
use std::path::Path;
use std::sync::mpsc;
use std::time::Instant;
use tauri::{Runtime, WebviewWindow};

thread_local! {
    static OPEN: RefCell<Open<Retained<Delegate>>> = RefCell::default();
    static LEVELS: RefCell<Levels> = RefCell::default();
}

/// Runs `f` on the main thread and waits for it, or runs it directly when already there.
fn on_main<R: Runtime, T: Send + 'static>(
    window: &WebviewWindow<R>,
    f: impl FnOnce(&WebviewWindow<R>, MainThreadMarker) -> T + Send + 'static,
) -> Result<T, ShareError> {
    if let Some(mtm) = MainThreadMarker::new() {
        return Ok(f(window, mtm));
    }
    let (tx, rx) = mpsc::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let mtm = MainThreadMarker::new().expect("on the main thread");
            let _ = tx.send(f(&target, mtm));
        })
        .map_err(|e| ShareError::Window(e.to_string()))?;
    rx.recv()
        .map_err(|_| ShareError::Window("the main thread did not answer".into()))
}

pub fn show<R: Runtime>(
    window: &WebviewWindow<R>,
    anchor: Rect,
    path: &Path,
    session: Session,
) -> Result<(), ShareError> {
    let path = path.to_path_buf();
    on_main(window, move |w, mtm| {
        present(w, mtm, anchor, &path, session)
    })?
}

pub fn restore_level<R: Runtime>(window: &WebviewWindow<R>) {
    let restored = on_main(window, |w, _| {
        ns_window(w)
            .map(restore)
            .map_err(|e| ShareError::Window(e.to_string()))
    });
    if let Err(e) = restored.and_then(|r| r) {
        log::error!("could not restore the window level: {e}");
    }
}

fn key(window: &NSWindow) -> usize {
    window as *const NSWindow as usize
}

fn restore(window: &NSWindow) {
    if let Some(level) = LEVELS.with_borrow_mut(|l| l.restore(key(window), window.level())) {
        window.setLevel(level);
    }
}

fn present<R: Runtime>(
    window: &WebviewWindow<R>,
    mtm: MainThreadMarker,
    anchor: Rect,
    path: &Path,
    session: Session,
) -> Result<(), ShareError> {
    let ns_window = ns_window(window).map_err(|e| ShareError::Window(e.to_string()))?;
    let view = ns_window
        .contentView()
        .ok_or_else(|| ShareError::Window("the window has no content view".into()))?;
    // The page fills the content layout rect (below the title bar), in window coordinates.
    let content = ns_window.contentLayoutRect();
    let r = anchor.in_window(Rect {
        x: content.origin.x,
        y: content.origin.y,
        width: content.size.width,
        height: content.size.height,
    });
    let rect = view.convertRect_fromView(
        NSRect::new(NSPoint::new(r.x, r.y), NSSize::new(r.width, r.height)),
        None,
    );
    let edge = match picker::edge_below(view.isFlipped()) {
        Edge::MinY => NSRectEdge::MinY,
        Edge::MaxY => NSRectEdge::MaxY,
    };
    let path = path
        .to_str()
        .ok_or_else(|| ShareError::Window("the temp path is not UTF-8".into()))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let items: Retained<NSArray<AnyObject>> =
        NSArray::from_retained_slice(&[Retained::<AnyObject>::from(url)]);
    // SAFETY: the items are file URLs, which conform to NSPasteboardWriting.
    let picker = unsafe { NSSharingServicePicker::initWithItems(mtm.alloc(), &items) };

    if let Some(level) = LEVELS.with_borrow_mut(|l| l.lower(key(ns_window), ns_window.level())) {
        ns_window.setLevel(level);
    }
    let delegate = Delegate::new(mtm, session, picker.clone(), ns_window.retain());
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    delegate.prune_after(picker::TIMEOUT.as_secs_f64());
    let dropped = OPEN.with_borrow_mut(|open| open.push(Instant::now(), delegate));
    drop(dropped);
    // Returns at once: the menu is shown without a nested tracking loop (checked on macOS 26).
    picker.showRelativeToRect_ofView_preferredEdge(rect, &view, edge);
    Ok(())
}

/// Drops the pickers that are done. Runs from the run loop, never inside their callbacks.
fn prune() {
    let dropped = OPEN.with_borrow_mut(|open| open.prune(Instant::now()));
    drop(dropped);
}

/// Dev builds: dismisses every open share sheet, as if the user cancelled.
#[cfg(debug_assertions)]
pub fn close_all() {
    let open: Vec<_> = OPEN.with_borrow(|open| {
        open.iter()
            .filter(|d| !d.is_over())
            .map(|d| d.ivars().picker.clone())
            .collect()
    });
    open.iter().for_each(|p| p.close());
}

struct Ivars {
    session: Session,
    /// Kept alive while it is open; only read by `close_all`.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    picker: Retained<NSSharingServicePicker>,
    window: Retained<NSWindow>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and `Delegate` does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "XcreenshotShareDelegate"]
    #[ivars = Ivars]
    struct Delegate;

    impl Delegate {
        #[unsafe(method(xcreenshotPrune))]
        fn prune_now(&self) {
            prune();
        }
    }

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSSharingServicePickerDelegate for Delegate {
        /// The menu closed, with the chosen service or none. After a choice the window stays
        /// lowered: the caller closes it (see `share::restore_level`).
        #[unsafe(method(sharingServicePicker:didChooseSharingService:))]
        fn did_choose(&self, _picker: &NSSharingServicePicker, service: Option<&NSSharingService>) {
            if service.is_none() {
                restore(&self.ivars().window);
            }
            self.handle(picker::event_for(service.map(|s| s.title().to_string())));
        }

        /// The chosen service reports back to the same object.
        #[unsafe(method_id(sharingServicePicker:delegateForSharingService:))]
        fn delegate_for(
            &self,
            _picker: &NSSharingServicePicker,
            _service: &NSSharingService,
        ) -> Option<Retained<ProtocolObject<dyn NSSharingServiceDelegate>>> {
            Some(ProtocolObject::from_retained(self.retain()))
        }
    }

    unsafe impl NSSharingServiceDelegate for Delegate {
        #[unsafe(method(sharingService:didShareItems:))]
        fn did_share(&self, _service: &NSSharingService, _items: &NSArray) {
            self.handle(Event::Finished);
        }

        #[unsafe(method(sharingService:didFailToShareItems:error:))]
        fn did_fail(&self, _service: &NSSharingService, _items: &NSArray, error: &NSError) {
            log::warn!("sharing failed: {}", error.localizedDescription());
            self.handle(Event::Finished);
        }
    }
);

impl Delegate {
    fn new(
        mtm: MainThreadMarker,
        session: Session,
        picker: Retained<NSSharingServicePicker>,
        window: Retained<NSWindow>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            session,
            picker,
            window,
        });
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }

    fn handle(&self, event: Event) {
        self.ivars().session.handle(event);
        if self.ivars().session.is_over() {
            self.cancel_timeout();
            self.prune_after(0.0);
        }
    }

    /// Cancels the timeout prune, which would otherwise keep `self`, its picker and window
    /// alive for 10 minutes.
    fn cancel_timeout(&self) {
        // SAFETY: a documented NSObject class method; nil object matches the scheduled call.
        let _: () = unsafe {
            msg_send![
                NSObject::class(),
                cancelPreviousPerformRequestsWithTarget: self,
                selector: sel!(xcreenshotPrune),
                object: std::ptr::null::<AnyObject>()
            ]
        };
    }

    /// Schedules a prune on the main run loop, which keeps `self` alive until it has run.
    fn prune_after(&self, seconds: f64) {
        // SAFETY: `xcreenshotPrune` is defined above, takes no argument and returns nothing.
        let _: () = unsafe {
            msg_send![
                self,
                performSelector: sel!(xcreenshotPrune),
                withObject: std::ptr::null::<AnyObject>(),
                afterDelay: seconds
            ]
        };
    }
}

impl Entry for Retained<Delegate> {
    fn is_over(&self) -> bool {
        self.ivars().session.is_over()
    }

    fn expire(&self) {
        self.cancel_timeout();
        self.ivars().session.handle(Event::Finished);
    }
}
