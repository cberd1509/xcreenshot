//! AppKit glue for `NSSharingServicePicker`. Main thread only; kept thin, the logic it relies
//! on (anchor rect, temp files) is tested in the parent module.

use super::{Done, Ended, Rect, ShareError};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSPopUpMenuWindowLevel, NSSharingService, NSSharingServiceDelegate, NSSharingServicePicker,
    NSSharingServicePickerDelegate, NSWindow,
};
use objc2_foundation::{NSArray, NSError, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::sync::mpsc;
use tauri::{Runtime, WebviewWindow};

/// The picker's menu is a pop-up menu window (101). A higher window (the overlay, at the screen
/// saver level) waits here while the menu is open: under the menu, over the menu bar and Dock.
const BELOW_MENUS: isize = NSPopUpMenuWindowLevel - 1;

/// Shows the picker on the main thread, waiting for it if called from another thread.
pub fn show<R: Runtime>(
    window: &WebviewWindow<R>,
    anchor: Rect,
    path: &Path,
    done: Done,
) -> Result<(), ShareError> {
    if MainThreadMarker::new().is_some() {
        return present(window, anchor, path, done);
    }
    let path = path.to_path_buf();
    let (tx, rx) = mpsc::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(present(&target, anchor, &path, done));
        })
        .map_err(|e| ShareError::Window(e.to_string()))?;
    rx.recv()
        .map_err(|_| ShareError::Window("the main thread did not run the share sheet".into()))?
}

fn ns_window<R: Runtime>(window: &WebviewWindow<R>) -> Result<&NSWindow, ShareError> {
    let ptr = window
        .ns_window()
        .map_err(|e| ShareError::Window(format!("no NSWindow for {}: {e}", window.label())))?;
    // SAFETY: Tauri returns the live NSWindow of this window; callers are on the main thread.
    Ok(unsafe { &*ptr.cast::<NSWindow>() })
}

/// Dev builds: puts `window` at the overlay's level (see `overlay::window::macos::raise`), to
/// try the share sheet over it.
#[cfg(debug_assertions)]
pub fn raise_like_overlay<R: Runtime>(window: &WebviewWindow<R>) {
    match ns_window(window) {
        Ok(w) => w.setLevel(objc2_app_kit::NSScreenSaverWindowLevel),
        Err(e) => log::error!("{e}"),
    }
}

fn present<R: Runtime>(
    window: &WebviewWindow<R>,
    anchor: Rect,
    path: &Path,
    done: Done,
) -> Result<(), ShareError> {
    let mtm = MainThreadMarker::new().expect("present runs on the main thread");
    let ns_window = ns_window(window)?;
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
    // Below the anchor: the bottom edge is MaxY in a flipped view.
    let edge = if view.isFlipped() {
        NSRectEdge::MaxY
    } else {
        NSRectEdge::MinY
    };

    let path = path
        .to_str()
        .ok_or_else(|| ShareError::Window("the temp path is not UTF-8".into()))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let items: Retained<NSArray<AnyObject>> =
        NSArray::from_retained_slice(&[Retained::<AnyObject>::from(url)]);
    // SAFETY: the items are file URLs, which conform to NSPasteboardWriting.
    let picker = unsafe { NSSharingServicePicker::initWithItems(mtm.alloc(), &items) };

    let level = ns_window.level();
    let raised = (level > BELOW_MENUS).then(|| {
        ns_window.setLevel(BELOW_MENUS);
        (ns_window.retain(), level)
    });
    let delegate = Delegate::new(mtm, done, picker.clone(), raised);
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    ACTIVE.with_borrow_mut(|active| {
        // Finished delegates are dropped here rather than in their own callbacks, so neither
        // they nor their picker are freed while AppKit is still calling them.
        active.retain(|d| !d.finished());
        active.push(delegate);
    });
    picker.showRelativeToRect_ofView_preferredEdge(rect, &view, edge);
    Ok(())
}

/// Dev builds: dismisses every open share sheet, as if the user cancelled.
#[cfg(debug_assertions)]
pub fn close_all() {
    let open: Vec<_> = ACTIVE.with_borrow(|active| {
        active
            .iter()
            .filter(|d| !d.finished())
            .map(|d| d.ivars().picker.clone())
            .collect()
    });
    open.iter().for_each(|p| p.close());
}

thread_local! {
    /// The picker's delegate is a weak reference, so open shares are kept alive here.
    static ACTIVE: RefCell<Vec<Retained<Delegate>>> = const { RefCell::new(Vec::new()) };
}

struct Ivars {
    done: RefCell<Option<Done>>,
    picker: Retained<NSSharingServicePicker>,
    /// The window and its own level, while it is lowered for the menu.
    raised: Cell<Option<(Retained<NSWindow>, isize)>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and `Delegate` does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "XcreenshotShareDelegate"]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSSharingServicePickerDelegate for Delegate {
        /// The menu closed. `None` means it was dismissed without a choice.
        #[unsafe(method(sharingServicePicker:didChooseSharingService:))]
        fn did_choose(&self, _picker: &NSSharingServicePicker, service: Option<&NSSharingService>) {
            if let Some((window, level)) = self.ivars().raised.take() {
                window.setLevel(level);
            }
            match service {
                Some(s) => log::info!("sharing with {}", s.title()),
                None => self.finish(Ended::Cancelled),
            }
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
            self.finish(Ended::Finished);
        }

        #[unsafe(method(sharingService:didFailToShareItems:error:))]
        fn did_fail(&self, _service: &NSSharingService, _items: &NSArray, error: &NSError) {
            log::warn!("sharing failed: {}", error.localizedDescription());
            self.finish(Ended::Finished);
        }
    }
);

impl Delegate {
    fn new(
        mtm: MainThreadMarker,
        done: Done,
        picker: Retained<NSSharingServicePicker>,
        raised: Option<(Retained<NSWindow>, isize)>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            done: RefCell::new(Some(done)),
            picker,
            raised: Cell::new(raised),
        });
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }

    fn finish(&self, ended: Ended) {
        if let Some(done) = self.ivars().done.take() {
            done(ended);
        }
    }

    fn finished(&self) -> bool {
        self.ivars().done.borrow().is_none()
    }
}
