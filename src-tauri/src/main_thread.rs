//! Running a closure on the main thread and waiting for its answer.

use tauri::{Runtime, WebviewWindow};

/// Runs `f` on the main thread and waits for what it returns. On the main thread already,
/// Tauri runs it at once. Fails if the event loop is gone.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub(crate) fn wait<R: Runtime, T: Send + 'static>(
    window: &WebviewWindow<R>,
    f: impl FnOnce(&WebviewWindow<R>) -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(f(&target));
        })
        .map_err(|e| e.to_string())?;
    rx.recv()
        .map_err(|_| "the main thread did not answer".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    #[test]
    fn runs_on_the_main_thread_and_returns_the_answer() {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        let window = WebviewWindowBuilder::new(&app, "w", WebviewUrl::default())
            .build()
            .unwrap();
        assert_eq!(wait(&window, |w| w.label().to_owned()), Ok("w".to_owned()));
        // From another thread too (the mock runs it at once, as no event loop runs).
        let other = window.clone();
        let answer = std::thread::spawn(move || wait(&other, |_| 42))
            .join()
            .unwrap();
        assert_eq!(answer, Ok(42));
    }
}
