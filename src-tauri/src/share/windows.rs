//! WinRT glue for the Windows share sheet (`DataTransferManager`): calls only, on the main
//! thread apart from the file lookup. The decisions (what each notice means, when handlers go,
//! the title, the topmost band, which sheets to keep) are in `transfer.rs`, `open.rs` and
//! `session.rs`.

use super::open::{Open, TIMEOUT};
use super::session::Session;
use super::transfer::{self, Handlers, Lowered, Notice};
use super::{Rect, ShareError};
use ::windows::ApplicationModel::DataTransfer::{
    DataPackage, DataRequest, DataRequestedEventArgs, DataTransferManager, IDataPackage4,
    TargetApplicationChosenEventArgs,
};
use ::windows::Foundation::TypedEventHandler;
use ::windows::Storage::{IStorageItem, StorageFile};
use ::windows::Win32::UI::Shell::IDataTransferManagerInterop;
use ::windows::core::{AgileReference, HSTRING, IInspectable, Interface, Ref, factory};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Runtime, WebviewWindow};
use windows_collections::IIterable;

type Sheet = transfer::Sheet<Registered>;

thread_local! {
    static OPEN: RefCell<Open<Rc<Sheet>>> = RefCell::default();
    static LOWERED: RefCell<Lowered> = RefCell::default();
    /// Prunes once the newest sheet has timed out, and every older one with it.
    static TIMER: RefCell<Option<JoinHandle<()>>> = const { RefCell::new(None) };
}

static NEXT: AtomicU64 = AtomicU64::new(1);

fn failed(e: impl std::fmt::Display) -> ShareError {
    ShareError::Window(e.to_string())
}

/// The sheet opens over the window; it has no anchor.
pub fn show<R: Runtime>(
    window: &WebviewWindow<R>,
    _anchor: Rect,
    path: &Path,
    session: Session,
) -> Result<(), ShareError> {
    // Looked up here, off the main thread; the agile reference carries it there.
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path))
        .and_then(|op| op.join())
        .and_then(|file| AgileReference::new(&file))
        .map_err(failed)?;
    let title = HSTRING::from(transfer::title(path));
    crate::main_thread::wait(window, move |w| present(w, file, title, session))
        .map_err(ShareError::Window)?
}

/// Callable from any thread.
pub fn restore_level<R: Runtime>(window: &WebviewWindow<R>) {
    let target = window.clone();
    if let Err(e) = window.run_on_main_thread(move || restore(&target)) {
        log::error!("could not put the window back on top: {e}");
    }
}

fn restore<R: Runtime>(window: &WebviewWindow<R>) {
    let topmost = window.is_always_on_top().unwrap_or(false);
    if LOWERED.with_borrow_mut(|l| l.restore(window.label(), topmost))
        && let Err(e) = window.set_always_on_top(true)
    {
        log::error!("could not put the window back on top: {e}");
    }
}

fn present<R: Runtime>(
    window: &WebviewWindow<R>,
    file: AgileReference<StorageFile>,
    title: HSTRING,
    session: Session,
) -> Result<(), ShareError> {
    // Without `ShareCanceled` (before Windows 10 2004) a dismissal would never be heard of.
    DataPackage::new()
        .and_then(|p| p.cast::<IDataPackage4>())
        .map_err(|_| ShareError::Unsupported)?;
    let hwnd = window.hwnd().map_err(failed)?;
    let interop = factory::<DataTransferManager, IDataTransferManagerInterop>().map_err(failed)?;
    // SAFETY: a live window handle from Tauri, on the thread that owns it.
    let manager: DataTransferManager = unsafe { interop.GetForWindow(hwnd) }.map_err(failed)?;
    let notify = Notify {
        app: window.app_handle().clone(),
        id: NEXT.fetch_add(1, Ordering::Relaxed),
    };
    let n = notify.clone();
    let requested = manager
        .DataRequested(&TypedEventHandler::new(
            move |_, args: Ref<DataRequestedEventArgs>| {
                let request = args.ok().and_then(DataRequestedEventArgs::Request);
                if let Err(e) = request.and_then(|r| fill(&r, &file, &title, &n)) {
                    log::error!("could not fill the share sheet: {e}");
                    n.send(Notice::Canceled);
                }
                Ok(())
            },
        ))
        .map_err(failed)?;
    let n = notify.clone();
    let chosen = manager.TargetApplicationChosen(&TypedEventHandler::new(
        move |_, args: Ref<TargetApplicationChosenEventArgs>| {
            let app = args
                .ok()
                .and_then(TargetApplicationChosenEventArgs::ApplicationName)
                .map(|name| name.to_string());
            n.send(Notice::Chosen(app.unwrap_or_default()));
            Ok(())
        },
    ));
    let chosen = chosen.map_err(|e| {
        let _ = manager.RemoveDataRequested(requested);
        failed(e)
    })?;
    let handlers = Registered {
        manager,
        requested,
        chosen,
    };
    let sheet = Rc::new(Sheet::new(notify.id, session, handlers));
    // Listed before the sheet shows: its handlers may run as soon as it does.
    let dropped = OPEN.with_borrow_mut(|open| open.push(Instant::now(), sheet.clone()));
    drop(dropped);
    let topmost = window.is_always_on_top().unwrap_or(false);
    if LOWERED.with_borrow_mut(|l| l.lower(window.label(), topmost))
        && let Err(e) = window.set_always_on_top(false)
    {
        log::warn!("could not lower the window for the share sheet: {e}");
    }
    // SAFETY: as above.
    if let Err(e) = unsafe { interop.ShowShareUIForWindow(hwnd) } {
        sheet.abandon();
        prune();
        restore(window);
        return Err(failed(e));
    }
    let app = notify.app;
    let timer = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TIMEOUT).await;
        let _ = app.run_on_main_thread(prune);
    });
    if let Some(old) = TIMER.replace(Some(timer)) {
        old.abort();
    }
    Ok(())
}

/// Hands the file to the app the user picks, and listens for how the share ends. The cancel
/// handler goes first: after it, a failure shows in the sheet and its dismissal reports back.
fn fill<R: Runtime>(
    request: &DataRequest,
    file: &AgileReference<StorageFile>,
    title: &HSTRING,
    notify: &Notify<R>,
) -> ::windows::core::Result<()> {
    let data = request.Data()?;
    let n = notify.clone();
    data.ShareCanceled(&TypedEventHandler::new(move |_, _: Ref<IInspectable>| {
        n.send(Notice::Canceled);
        Ok(())
    }))?;
    let n = notify.clone();
    data.ShareCompleted(&TypedEventHandler::new(move |_, _| {
        n.send(Notice::Completed);
        Ok(())
    }))?;
    let filled = data
        .Properties()
        .and_then(|p| p.SetTitle(title))
        .and_then(|()| {
            let item: IStorageItem = file.resolve()?.cast()?;
            data.SetStorageItemsReadOnly(&IIterable::<IStorageItem>::from(vec![Some(item)]))
        });
    if let Err(e) = filled {
        log::error!("could not fill the share sheet: {e}");
        let _ = request.FailWithDisplayText(&HSTRING::from("Couldn't share"));
    }
    Ok(())
}

/// Sends a sheet's notices to the main thread, where its session lives. The handlers may run
/// on any thread.
struct Notify<R: Runtime> {
    app: AppHandle<R>,
    id: u64,
}

impl<R: Runtime> Clone for Notify<R> {
    fn clone(&self) -> Self {
        Self {
            app: self.app.clone(),
            id: self.id,
        }
    }
}

impl<R: Runtime> Notify<R> {
    fn send(&self, notice: Notice) {
        let id = self.id;
        if let Err(e) = self.app.run_on_main_thread(move || dispatch(id, notice)) {
            log::error!("could not report the share sheet: {e}");
        }
    }
}

/// Main thread. No borrow of the list is held while the session runs the caller's callback.
fn dispatch(id: u64, notice: Notice) {
    let sheet = OPEN.with_borrow(|open| open.iter().find(|s| s.id == id).cloned());
    if let Some(sheet) = sheet {
        sheet.notify(notice);
        prune();
    }
}

/// Drops the sheets that are over or expired, and the timer once none is left. Main thread.
fn prune() {
    let (dropped, empty) = OPEN.with_borrow_mut(|open| {
        let dropped = open.prune(Instant::now());
        (dropped, open.iter().next().is_none())
    });
    drop(dropped);
    if empty && let Some(timer) = TIMER.take() {
        timer.abort();
    }
}

/// A sheet's handlers on its window's manager.
struct Registered {
    manager: DataTransferManager,
    requested: i64,
    chosen: i64,
}

impl Handlers for Registered {
    fn remove(self) {
        let _ = self.manager.RemoveDataRequested(self.requested);
        let _ = self.manager.RemoveTargetApplicationChosen(self.chosen);
    }
}
