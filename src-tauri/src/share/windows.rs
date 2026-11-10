//! WinRT glue for the Windows share sheet (`DataTransferManager`): calls only, on the main
//! thread apart from the file lookup. The decisions (what each notice means, the title, the
//! topmost band, which sheets to keep) are in `transfer.rs`, `open.rs` and `session.rs`.

use super::ShareError;
use super::open::{Entry, Open, TIMEOUT};
use super::session::{Event, Session};
use super::transfer::{self, Lowered, Notice};
use ::windows::ApplicationModel::DataTransfer::{
    DataPackage, DataRequest, DataRequestedEventArgs, DataTransferManager, ShareCompletedEventArgs,
    TargetApplicationChosenEventArgs,
};
use ::windows::Foundation::TypedEventHandler;
use ::windows::Storage::{IStorageItem, StorageFile};
use ::windows::Win32::UI::Shell::IDataTransferManagerInterop;
use ::windows::core::{AgileReference, HSTRING, IInspectable, Interface, Ref, factory};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Runtime, WebviewWindow};
use windows_collections::IIterable;

thread_local! {
    static OPEN: RefCell<Open<Rc<Sheet>>> = RefCell::default();
    static LOWERED: RefCell<Lowered> = RefCell::default();
}

static NEXT: AtomicU64 = AtomicU64::new(1);

fn failed(e: impl std::fmt::Display) -> ShareError {
    ShareError::Window(e.to_string())
}

/// Runs `f` on the main thread and waits for it. On the main thread already, Tauri runs it
/// right away.
fn on_main<R: Runtime, T: Send + 'static>(
    window: &WebviewWindow<R>,
    f: impl FnOnce(&WebviewWindow<R>) -> T + Send + 'static,
) -> Result<T, ShareError> {
    let (tx, rx) = std::sync::mpsc::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(f(&target));
        })
        .map_err(failed)?;
    rx.recv()
        .map_err(|_| failed("the main thread did not answer"))
}

pub fn show<R: Runtime>(
    window: &WebviewWindow<R>,
    path: &Path,
    session: Session,
) -> Result<(), ShareError> {
    // Looked up here, off the main thread; the agile reference carries it there.
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path))
        .and_then(|op| op.join())
        .and_then(|file| AgileReference::new(&file))
        .map_err(failed)?;
    let title = HSTRING::from(transfer::title(path));
    on_main(window, move |w| present(w, file, title, session))?
}

pub fn restore_level<R: Runtime>(window: &WebviewWindow<R>) {
    let topmost = window.is_always_on_top().unwrap_or(true);
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
                let request = args.ok()?.Request()?;
                if let Err(e) = fill(&request, &file, &title, &n) {
                    log::error!("could not fill the share sheet: {e}");
                    let _ = request.FailWithDisplayText(&HSTRING::from("Couldn't share"));
                    n.send(Notice::Canceled);
                }
                Ok(())
            },
        ))
        .map_err(failed)?;
    let n = notify.clone();
    let chosen = manager.TargetApplicationChosen(&TypedEventHandler::new(
        move |_, args: Ref<TargetApplicationChosenEventArgs>| {
            n.send(Notice::Chosen(args.ok()?.ApplicationName()?.to_string()));
            Ok(())
        },
    ));
    let chosen = chosen.map_err(|e| {
        let _ = manager.RemoveDataRequested(requested);
        failed(e)
    })?;
    let sheet = Rc::new(Sheet {
        id: notify.id,
        session,
        manager,
        handlers: Cell::new(Some((requested, chosen))),
        abandoned: Cell::new(false),
    });
    // In the list before the sheet shows: its handlers may run as soon as it does.
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
        sheet.abandoned.set(true);
        sheet.detach();
        prune();
        restore_level(window);
        return Err(failed(e));
    }
    let app = notify.app;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TIMEOUT).await;
        let _ = app.run_on_main_thread(prune);
    });
    Ok(())
}

/// Hands the file to the app the user picks, and listens for how the share ends.
fn fill<R: Runtime>(
    request: &DataRequest,
    file: &AgileReference<StorageFile>,
    title: &HSTRING,
    notify: &Notify<R>,
) -> ::windows::core::Result<()> {
    let data = request.Data()?;
    data.Properties()?.SetTitle(title)?;
    let item: IStorageItem = file.resolve()?.cast()?;
    data.SetStorageItemsReadOnly(&IIterable::<IStorageItem>::from(vec![Some(item)]))?;
    // Windows 10 2004 and later; on older versions this fails and the share is cancelled.
    let n = notify.clone();
    data.ShareCanceled(&TypedEventHandler::new(move |_, _: Ref<IInspectable>| {
        n.send(Notice::Canceled);
        Ok(())
    }))?;
    let n = notify.clone();
    data.ShareCompleted(&TypedEventHandler::<DataPackage, _>::new(
        move |_, args: Ref<ShareCompletedEventArgs>| {
            let name = args
                .ok()?
                .ShareTarget()
                .and_then(|t| t.ShareProvider())
                .and_then(|p| p.Title())
                .ok()
                .map(|t| t.to_string())
                .filter(|t| !t.is_empty());
            n.send(Notice::Completed(name));
            Ok(())
        },
    ))?;
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
    let Some(sheet) = sheet else {
        return;
    };
    // The sheet has closed: the window's next share must not reach this one.
    sheet.detach();
    for event in transfer::events(notice) {
        sheet.session.handle(event);
    }
    if sheet.session.is_over() {
        prune();
    }
}

/// Drops the sheets that are done or expired. Main thread.
fn prune() {
    let dropped = OPEN.with_borrow_mut(|open| open.prune(Instant::now()));
    drop(dropped);
}

/// One share: the window's manager, its handlers until the sheet closes, and the session.
struct Sheet {
    id: u64,
    session: Session,
    manager: DataTransferManager,
    handlers: Cell<Option<(i64, i64)>>,
    /// The sheet never showed: the session is dropped without a word.
    abandoned: Cell<bool>,
}

impl Sheet {
    fn detach(&self) {
        if let Some((requested, chosen)) = self.handlers.take() {
            let _ = self.manager.RemoveDataRequested(requested);
            let _ = self.manager.RemoveTargetApplicationChosen(chosen);
        }
    }
}

impl Entry for Rc<Sheet> {
    fn is_over(&self) -> bool {
        self.abandoned.get() || self.session.is_over()
    }

    fn expire(&self) {
        self.detach();
        self.session.handle(Event::Finished);
    }
}
