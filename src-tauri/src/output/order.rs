//! Keeps copies in the order they were asked for. A larger capture takes longer to encode, so
//! two copies close together could reach the clipboard in the other order; an older one never
//! replaces a newer one.

use super::Clipboard;
use crate::capture::CaptureError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

/// Numbers copies and lets each write only if no newer one was written. Not
/// `region::Order`, which numbers saves when they finish.
pub struct CopyOrder {
    next: AtomicU64,
    /// The newest copy written. Held while writing, so writes take turns.
    written: Mutex<u64>,
}

impl CopyOrder {
    pub const fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
            written: Mutex::new(0),
        }
    }

    /// A number for a new copy, higher than every earlier one.
    pub fn ticket(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    /// Runs `write` for copy `ticket`, unless a newer copy is already on the clipboard. That
    /// copy is what the user asked for last, so skipping counts as done.
    pub fn write(
        &self,
        ticket: u64,
        write: impl FnOnce() -> Result<(), CaptureError>,
    ) -> Result<(), CaptureError> {
        let mut written = self.written.lock().unwrap_or_else(PoisonError::into_inner);
        if ticket < *written {
            log::info!("skipped copy {ticket}: copy {} is newer", *written);
            return Ok(());
        }
        write()?;
        *written = ticket;
        Ok(())
    }
}

/// `clipboard`, writing only while `wanted()` holds; otherwise [`CaptureError::Ended`]. Under
/// [`ordered`], it is asked inside the order's lock, right before the write, so nothing waits
/// between the answer and the write.
pub fn guarded(
    clipboard: Clipboard,
    wanted: impl Fn() -> bool + Send + Sync + 'static,
) -> Clipboard {
    Box::new(move |image, density| {
        if !wanted() {
            return Err(CaptureError::Ended);
        }
        clipboard(image, density)
    })
}

/// `clipboard` for one copy, numbered by `order` now, when the copy is asked for.
pub fn ordered(order: &'static CopyOrder, clipboard: Clipboard) -> Clipboard {
    let ticket = order.ticket();
    Box::new(move |image, density| order.write(ticket, || clipboard(image, density)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::Density;
    use image::RgbaImage;
    use std::cell::RefCell;
    use std::sync::Arc;

    fn busy() -> CaptureError {
        CaptureError::Clipboard("busy".into())
    }

    #[test]
    fn an_older_copy_never_replaces_a_newer_one() {
        let order = CopyOrder::new();
        let (first, second) = (order.ticket(), order.ticket());
        assert!(second > first);
        let written = RefCell::new(Vec::new());
        let write = |n| {
            written.borrow_mut().push(n);
            Ok(())
        };
        order.write(second, || write(second)).unwrap();
        order.write(first, || write(first)).unwrap();
        assert_eq!(*written.borrow(), [second], "the older copy was skipped");
        // The same copy again is not older.
        order.write(second, || write(second)).unwrap();
        assert_eq!(*written.borrow(), [second, second]);
    }

    #[test]
    fn a_failed_copy_does_not_hold_back_older_ones() {
        let order = CopyOrder::new();
        let (first, second) = (order.ticket(), order.ticket());
        assert_eq!(order.write(second, || Err(busy())), Err(busy()));
        let ran = RefCell::new(false);
        order
            .write(first, || {
                *ran.borrow_mut() = true;
                Ok(())
            })
            .unwrap();
        assert!(*ran.borrow());
    }

    #[test]
    fn a_clipboard_is_numbered_when_the_copy_is_asked_for() {
        static ORDER: CopyOrder = CopyOrder::new();
        let written = Arc::new(Mutex::new(Vec::new()));
        let clipboard = |tag: u8| {
            let written = written.clone();
            ordered(
                &ORDER,
                Box::new(move |_, _| {
                    written.lock().unwrap().push(tag);
                    Ok(())
                }),
            )
        };
        let (older, newer) = (clipboard(1), clipboard(2));
        let image = RgbaImage::new(1, 1);
        newer(&image, Density::ONE).unwrap();
        older(&image, Density::ONE).unwrap();
        assert_eq!(*written.lock().unwrap(), [2]);
    }

    #[test]
    fn a_guarded_copy_asks_inside_the_order_lock_and_writes_only_if_wanted() {
        static ORDER: CopyOrder = CopyOrder::new();
        let written = Arc::new(Mutex::new(0));
        let wanted = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let clipboard = || {
            let (written, wanted) = (written.clone(), wanted.clone());
            let write: Clipboard = Box::new(move |_, _| {
                *written.lock().unwrap() += 1;
                Ok(())
            });
            ordered(
                &ORDER,
                guarded(write, move || {
                    // Asked while the order is locked: no other copy can write meanwhile.
                    assert!(ORDER.written.try_lock().is_err());
                    wanted.load(Ordering::SeqCst)
                }),
            )
        };
        let image = RgbaImage::new(1, 1);
        clipboard()(&image, Density::ONE).unwrap();
        assert_eq!(*written.lock().unwrap(), 1);
        wanted.store(false, Ordering::SeqCst);
        let ended = clipboard()(&image, Density::ONE);
        assert_eq!(ended, Err(CaptureError::Ended));
        assert_eq!(
            *written.lock().unwrap(),
            1,
            "an ended capture leaves the clipboard"
        );
    }
}
