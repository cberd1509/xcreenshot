//! Update checks through GitHub Releases (`latest.json`, signed; see `docs/updates.md`).
//!
//! Automatic checks run 10 s after launch, then every 24 h, while `checkForUpdates` is on. A
//! found update downloads quietly. Once it is ready, the menu bar gets "Restart to update" and
//! one "Update ready" window is shown while no capture is open (see `notice`). Nothing restarts
//! without a click, and the restart never cuts a capture short. "Check for updates…" always
//! answers.
//!
//! The flow works against the [`Host`] trait and injected time, so it is tested without a
//! network or a screen. [`TauriHost`] is the thin glue to the updater, the dialog plugin and
//! the notice window.

use crate::prefs;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager, Runtime};

/// First automatic check, so startup isn't slowed.
const LAUNCH_DELAY: Duration = Duration::from_secs(10);
/// Between automatic checks.
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// After a failed automatic check (offline, GitHub down, no release yet).
const RETRY: Duration = Duration::from_secs(60 * 60);
/// How often the scheduler looks: notices the preference turning on, or a check missed while
/// the computer slept.
const TICK: Duration = Duration::from_secs(10 * 60);
/// Where update files may come from. Anything else in `latest.json` is refused unfetched.
const DOWNLOADS: &str = "https://github.com/cberd1509/xcreenshot/releases/download/";

/// The updater and UI calls the flow needs.
pub trait Host: Send + Sync {
    type Update: Send + Sync;
    /// The newer version and its handle, or `None` when up to date.
    fn check(&self) -> impl Future<Output = Result<Option<(String, Self::Update)>, String>> + Send;
    /// Downloads and verifies the update.
    fn download(
        &self,
        update: &Self::Update,
    ) -> impl Future<Output = Result<Vec<u8>, String>> + Send;
    /// Installs and restarts, unless a capture is open: then `Ok(false)` and nothing happens.
    fn install_and_restart(
        &self,
        ready: Arc<Ready<Self::Update>>,
    ) -> impl Future<Output = Result<bool, String>> + Send;
    /// Shows a message without blocking the app.
    fn show(&self, text: &str) -> impl Future<Output = ()> + Send;
    /// "Update ready": whether "Restart now" was clicked.
    fn ask_restart(&self, version: &str) -> impl Future<Output = bool> + Send;
    /// Resolves once no capture is running or open.
    fn idle(&self) -> impl Future<Output = ()> + Send;
    /// Adds "Restart to update" to the menu bar.
    fn offer(&self, version: &str);
}

/// A downloaded, verified update waiting for a restart.
pub struct Ready<U> {
    version: String,
    update: U,
    bytes: Vec<u8>,
}

/// Shared state: one check or download at a time, the update that is ready, and whether its
/// prompt is open or it is being installed.
pub struct Updates<U> {
    flight: tokio::sync::Mutex<()>,
    ready: Mutex<Option<Arc<Ready<U>>>>,
    prompting: AtomicBool,
    installing: AtomicBool,
}

impl<U> Default for Updates<U> {
    fn default() -> Self {
        Self {
            flight: tokio::sync::Mutex::new(()),
            ready: Mutex::new(None),
            prompting: AtomicBool::new(false),
            installing: AtomicBool::new(false),
        }
    }
}

impl<U> Updates<U> {
    fn ready(&self) -> Option<Arc<Ready<U>>> {
        self.ready
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Holds a flag set until dropped.
struct Claim<'a>(&'a AtomicBool);

impl<'a> Claim<'a> {
    fn new(flag: &'a AtomicBool) -> Option<Self> {
        (!flag.swap(true, Ordering::SeqCst)).then_some(Self(flag))
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// What an automatic check did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ran {
    /// Nothing was requested (another check was running, or an update is already ready).
    Skipped,
    Checked,
    Failed,
}

/// Whether an automatic check is due. A clock that went backwards counts as due.
fn due(last: Option<SystemTime>, now: SystemTime) -> bool {
    last.is_none_or(|last| now < last || now >= last + INTERVAL)
}

/// The automatic checks. A failed check is retried after [`RETRY`]; `sleep` returns `false`
/// to stop (tests only).
pub async fn schedule<C, S>(
    now: impl Fn() -> SystemTime,
    allowed: impl Fn() -> bool,
    mut check: impl FnMut() -> C,
    mut sleep: impl FnMut(Duration) -> S,
) where
    C: Future<Output = Ran>,
    S: Future<Output = bool>,
{
    let mut last = None;
    let mut wait = LAUNCH_DELAY;
    while sleep(wait).await {
        wait = TICK;
        if !(allowed() && due(last, now())) {
            continue;
        }
        match check().await {
            Ran::Skipped => {}
            Ran::Checked => last = Some(now()),
            // Due again in RETRY instead of a day.
            Ran::Failed => last = now().checked_sub(INTERVAL - RETRY),
        }
    }
}

/// Whether an announced download URL is one of this repo's release files.
fn trusted(url: &str) -> bool {
    url.starts_with(DOWNLOADS)
}

/// Checks and downloads. Call with the flight lock held. A found update is stored, then
/// offered in the menu bar.
async fn fetch<H: Host>(
    host: &H,
    state: &Updates<H::Update>,
) -> Result<Option<Arc<Ready<H::Update>>>, String> {
    let Some((version, update)) = host.check().await? else {
        log::info!("no update available");
        return Ok(None);
    };
    log::info!("update {version} found, downloading");
    let bytes = host.download(&update).await?;
    log::info!("update {version} ready");
    let ready = Arc::new(Ready {
        version,
        update,
        bytes,
    });
    *state.ready.lock().unwrap_or_else(PoisonError::into_inner) = Some(ready.clone());
    host.offer(&ready.version);
    Ok(Some(ready))
}

/// An automatic check: quiet unless an update is ready.
pub async fn auto_check<H: Host>(host: &H, state: &Updates<H::Update>) -> Ran {
    let Ok(flight) = state.flight.try_lock() else {
        return Ran::Skipped;
    };
    // The one place automatic checks stop: an update is already waiting for a restart.
    if state.ready().is_some() {
        return Ran::Skipped;
    }
    let found = fetch(host, state).await;
    drop(flight);
    match found {
        Ok(Some(ready)) => offer_restart(host, state, &ready.version).await,
        Ok(None) => {}
        // Offline, or no release published yet (404): a log line is enough.
        Err(e) => {
            log::info!("update check failed: {e}");
            return Ran::Failed;
        }
    }
    Ran::Checked
}

/// "Check for updates…": always answers, never while a capture is open. A ready update is
/// offered again, even after "Later".
pub async fn manual_check<H: Host>(host: &H, state: &Updates<H::Update>) {
    let Ok(flight) = state.flight.try_lock() else {
        tell(host, "Already checking for updates.").await;
        return;
    };
    let found = match state.ready() {
        Some(ready) => Ok(Some(ready)),
        None => fetch(host, state).await,
    };
    drop(flight);
    match found {
        Ok(Some(ready)) => offer_restart(host, state, &ready.version).await,
        Ok(None) => tell(host, "You're up to date.").await,
        Err(e) => {
            log::warn!("update check failed: {e}");
            tell(host, &format!("Couldn't check for updates: {e}")).await;
        }
    }
}

async fn tell<H: Host>(host: &H, text: &str) {
    host.idle().await;
    host.show(text).await;
}

/// "Update ready" with "Restart now" / "Later". Later keeps the menu bar item. Only one prompt
/// at a time: while it is open, it already answers another check.
async fn offer_restart<H: Host>(host: &H, state: &Updates<H::Update>, version: &str) {
    let Some(prompt) = Claim::new(&state.prompting) else {
        return;
    };
    host.idle().await;
    let accepted = host.ask_restart(version).await;
    drop(prompt);
    if accepted {
        restart(host, state).await;
    }
}

/// Installs the ready update and restarts, once, and never while a capture is open.
pub async fn restart<H: Host>(host: &H, state: &Updates<H::Update>) {
    let Some(ready) = state.ready() else { return };
    let Some(_installing) = Claim::new(&state.installing) else {
        return;
    };
    loop {
        host.idle().await;
        match host.install_and_restart(ready.clone()).await {
            Ok(true) => return,
            // A capture started after `idle`: wait for it again.
            Ok(false) => {}
            Err(e) => {
                log::error!("update install failed: {e}");
                tell(host, &format!("Couldn't install the update: {e}")).await;
                return;
            }
        }
    }
}

/// Download progress in whole percent, at most once per 10%.
fn progress_step(done: u64, total: Option<u64>, last: &mut u64) -> Option<u64> {
    let total = total.filter(|&t| t > 0)?;
    let percent = (done.saturating_mul(100) / total).min(100);
    let step = percent / 10 * 10;
    (step > *last).then(|| {
        *last = step;
        percent
    })
}

type AppUpdates = Updates<tauri_plugin_updater::Update>;

/// Starts the automatic checks. `offer` adds "Restart to update (vX)" to the menu bar.
pub fn start<R: Runtime>(app: &AppHandle<R>, offer: fn(&AppHandle<R>, &str)) {
    app.manage(AppUpdates::default());
    app.manage(Offer(offer));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let host = TauriHost(app.clone());
        let state = app.state::<AppUpdates>();
        // Read before each check, so turning it off takes effect within a tick.
        let allowed = || prefs::load(&app).check_for_updates;
        schedule(
            SystemTime::now,
            allowed,
            || auto_check(&host, &state),
            |d| async move {
                tokio::time::sleep(d).await;
                true
            },
        )
        .await;
    });
}

/// "Check for updates…" from the menu bar.
pub fn check_now<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        manual_check(&TauriHost(app.clone()), &app.state::<AppUpdates>()).await;
    });
}

/// The version of the downloaded update waiting for a restart, if any.
pub fn ready_version<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let ready = app.try_state::<AppUpdates>()?.ready()?;
    Some(ready.version.clone())
}

/// "Restart to update" from the menu bar.
pub fn restart_now<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        restart(&TauriHost(app.clone()), &app.state::<AppUpdates>()).await;
    });
}

struct Offer<R: Runtime>(fn(&AppHandle<R>, &str));

/// The updater and dialog plugins, used from Rust only (no page permissions), and the notice
/// window.
struct TauriHost<R: Runtime>(AppHandle<R>);

impl<R: Runtime> Host for TauriHost<R> {
    type Update = tauri_plugin_updater::Update;

    async fn check(&self) -> Result<Option<(String, Self::Update)>, String> {
        use tauri_plugin_updater::UpdaterExt;
        let updater = self.0.updater().map_err(|e| e.to_string())?;
        let found = updater.check().await.map_err(|e| e.to_string())?;
        Ok(found.map(|u| (u.version.clone(), u)))
    }

    async fn download(&self, update: &Self::Update) -> Result<Vec<u8>, String> {
        if !trusted(update.download_url.as_str()) {
            return Err(format!("refused download from {}", update.download_url));
        }
        let (mut done, mut last) = (0u64, 0u64);
        let on_chunk = |chunk: usize, total: Option<u64>| {
            done += chunk as u64;
            if let Some(p) = progress_step(done, total, &mut last) {
                log::info!("update download {p}%");
            }
        };
        let bytes = update
            .download(on_chunk, || {})
            .await
            .map_err(|e| e.to_string())?;
        update.verify(&bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    }

    /// On the main thread, where captures start, so none can start between the overlay check
    /// and the restart. Windows exits into the installer from `install`.
    async fn install_and_restart(&self, ready: Arc<Ready<Self::Update>>) -> Result<bool, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let app = self.0.clone();
        self.0
            .run_on_main_thread(move || {
                if crate::overlay::is_busy(&app) {
                    let _ = tx.send(Ok(false));
                    return;
                }
                match ready.update.install(&ready.bytes) {
                    Ok(()) => app.restart(),
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        rx.await.unwrap_or(Ok(true))
    }

    /// The dialog plugin's callback API. On macOS a dialog without a parent window is a system
    /// alert shown from a background thread, so the main thread (and the hotkey) keep running.
    async fn show(&self, text: &str) {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.0
            .dialog()
            .message(text)
            .title("xcreenshot")
            .buttons(MessageDialogButtons::Ok)
            .show(move |_| {
                let _ = tx.send(());
            });
        let _ = rx.await;
    }

    /// Later, Esc or closing the window keep the update for the menu bar item.
    async fn ask_restart(&self, version: &str) -> bool {
        let version = version.to_owned();
        let notice = crate::notice::Notice::UpdateReady { version };
        crate::notice::ask(&self.0, notice).await == Some(true)
    }

    async fn idle(&self) {
        let overlay = self.0.state::<crate::overlay::Overlay>();
        loop {
            let mut closed = std::pin::pin!(overlay.idle.notified());
            closed.as_mut().enable();
            if !crate::overlay::is_busy(&self.0) {
                return;
            }
            closed.await;
        }
    }

    fn offer(&self, version: &str) {
        (self.0.state::<Offer<R>>().0)(&self.0, version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::async_runtime::block_on;

    /// Records what the flow did. `check` and `download` are canned results; `busy` is how
    /// many restarts find a capture open.
    struct Fake {
        check: Result<Option<&'static str>, &'static str>,
        download: Result<(), &'static str>,
        install: Result<(), &'static str>,
        answer: bool,
        busy: Mutex<u32>,
        log: Mutex<Vec<String>>,
    }

    impl Fake {
        fn new(check: Result<Option<&'static str>, &'static str>) -> Self {
            Self {
                check,
                download: Ok(()),
                install: Ok(()),
                answer: true,
                busy: Mutex::new(0),
                log: Mutex::default(),
            }
        }

        fn note(&self, s: impl Into<String>) {
            self.log.lock().unwrap().push(s.into());
        }

        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl Host for Fake {
        type Update = &'static str;

        async fn check(&self) -> Result<Option<(String, &'static str)>, String> {
            self.note("check");
            self.check
                .map(|v| v.map(|v| (v.to_owned(), v)))
                .map_err(str::to_owned)
        }

        async fn download(&self, update: &&'static str) -> Result<Vec<u8>, String> {
            self.note(format!("download {update}"));
            self.download.map(|()| vec![1]).map_err(str::to_owned)
        }

        async fn install_and_restart(
            &self,
            ready: Arc<Ready<&'static str>>,
        ) -> Result<bool, String> {
            let mut busy = self.busy.lock().unwrap();
            if *busy > 0 {
                *busy -= 1;
                self.note("busy");
                return Ok(false);
            }
            assert_eq!(ready.bytes, [1]);
            self.note(format!("install {}", ready.update));
            self.install.map(|()| true).map_err(str::to_owned)
        }

        async fn show(&self, text: &str) {
            self.note(format!("tell {text}"));
        }

        async fn ask_restart(&self, version: &str) -> bool {
            self.note(format!("ask restart {version}"));
            self.answer
        }

        async fn idle(&self) {
            self.note("idle");
        }

        fn offer(&self, version: &str) {
            self.note(format!("offer {version}"));
        }
    }

    const ASK: &str = "ask restart 1.2.0";

    #[test]
    fn automatic_check_downloads_then_offers_a_restart() {
        let host = Fake::new(Ok(Some("1.2.0")));
        let state = Updates::default();
        assert_eq!(block_on(auto_check(&host, &state)), Ran::Checked);
        assert_eq!(
            host.log(),
            [
                "check",
                "download 1.2.0",
                "offer 1.2.0",
                "idle",
                ASK,
                "idle",
                "install 1.2.0"
            ]
        );
    }

    #[test]
    fn a_capture_started_after_idle_delays_the_restart() {
        let host = Fake::new(Ok(Some("1.2.0")));
        *host.busy.lock().unwrap() = 1;
        let state = Updates::default();
        block_on(auto_check(&host, &state));
        assert_eq!(host.log()[5..], ["idle", "busy", "idle", "install 1.2.0"]);
    }

    #[test]
    fn later_keeps_the_update_and_stops_automatic_checks() {
        let mut host = Fake::new(Ok(Some("1.2.0")));
        host.answer = false;
        let state = Updates::default();
        assert_eq!(block_on(auto_check(&host, &state)), Ran::Checked);
        assert_eq!(host.log().last().unwrap(), ASK);
        assert!(state.ready().is_some());

        assert_eq!(block_on(auto_check(&host, &state)), Ran::Skipped);
        assert_eq!(host.log().len(), 5, "already ready: no request");

        block_on(manual_check(&host, &state));
        assert_eq!(
            host.log()[5..],
            ["idle", ASK],
            "manual offers the ready update"
        );

        block_on(restart(&host, &state));
        assert_eq!(
            host.log().last().unwrap(),
            "install 1.2.0",
            "the menu bar item"
        );
    }

    #[test]
    fn one_prompt_and_one_install_at_a_time() {
        let host = Fake::new(Ok(Some("1.2.0")));
        let state = Updates::default();
        block_on(fetch(&host, &state)).unwrap();
        let before = host.log().len();

        let prompt = Claim::new(&state.prompting).unwrap();
        block_on(manual_check(&host, &state));
        assert_eq!(host.log().len(), before, "the open prompt is the answer");
        drop(prompt);

        let installing = Claim::new(&state.installing).unwrap();
        block_on(restart(&host, &state));
        assert_eq!(host.log().len(), before, "already installing");
        drop(installing);
    }

    #[test]
    fn automatic_check_is_quiet_unless_an_update_is_ready() {
        let host = Fake::new(Ok(None));
        assert_eq!(
            block_on(auto_check(&host, &Updates::default())),
            Ran::Checked
        );
        assert_eq!(host.log(), ["check"]);

        let host = Fake::new(Err("404"));
        assert_eq!(
            block_on(auto_check(&host, &Updates::default())),
            Ran::Failed
        );
        assert_eq!(host.log(), ["check"]);

        let mut host = Fake::new(Ok(Some("1.2.0")));
        host.download = Err("offline");
        let state = Updates::default();
        assert_eq!(block_on(auto_check(&host, &state)), Ran::Failed);
        assert_eq!(host.log(), ["check", "download 1.2.0"]);
        assert!(state.ready().is_none());
    }

    #[test]
    fn manual_check_always_answers() {
        let host = Fake::new(Ok(None));
        block_on(manual_check(&host, &Updates::default()));
        assert_eq!(host.log(), ["check", "idle", "tell You're up to date."]);

        let host = Fake::new(Err("offline"));
        block_on(manual_check(&host, &Updates::default()));
        assert_eq!(
            host.log(),
            ["check", "idle", "tell Couldn't check for updates: offline"]
        );

        let host = Fake::new(Ok(None));
        let state = Updates::default();
        let busy = state.flight.try_lock().unwrap();
        block_on(manual_check(&host, &state));
        assert_eq!(block_on(auto_check(&host, &state)), Ran::Skipped);
        drop(busy);
        assert_eq!(host.log(), ["idle", "tell Already checking for updates."]);
    }

    #[test]
    fn failed_install_is_reported_without_restarting() {
        let mut host = Fake::new(Ok(Some("1.2.0")));
        host.install = Err("bad signature");
        let state = Updates::default();
        block_on(auto_check(&host, &state));
        assert_eq!(
            host.log()[5..],
            [
                "idle",
                "install 1.2.0",
                "idle",
                "tell Couldn't install the update: bad signature"
            ]
        );
        assert!(Claim::new(&state.installing).is_some(), "can be retried");
        let host = Fake::new(Ok(None));
        block_on(restart(&host, &Updates::default()));
        assert!(host.log().is_empty(), "nothing ready: nothing to do");
    }

    #[test]
    fn only_this_repos_release_files_are_downloaded() {
        assert!(trusted(
            "https://github.com/cberd1509/xcreenshot/releases/download/v1.2.0/x.app.tar.gz"
        ));
        assert!(!trusted("https://example.com/x.app.tar.gz"));
        assert!(!trusted(
            "https://github.com/someone/else/releases/download/v1/x"
        ));
        assert!(!trusted(
            "http://github.com/cberd1509/xcreenshot/releases/download/v1/x"
        ));
    }

    #[test]
    fn due_after_a_day_or_when_the_clock_went_back() {
        let t = SystemTime::UNIX_EPOCH + INTERVAL;
        assert!(due(None, t));
        assert!(!due(Some(t), t));
        assert!(!due(Some(t), t + INTERVAL - Duration::from_secs(1)));
        assert!(due(Some(t), t + INTERVAL));
        assert!(due(Some(t), t - Duration::from_secs(1)));
    }

    /// Runs the scheduler on a fake clock for `run`, returning when checks ran.
    fn scheduled(run: Duration, allowed: impl Fn(Duration) -> bool, ran: Ran) -> Vec<Duration> {
        let start = SystemTime::UNIX_EPOCH + INTERVAL;
        let elapsed = Mutex::new(Duration::ZERO);
        let now = || *elapsed.lock().unwrap();
        let checks = Mutex::new(Vec::new());
        block_on(schedule(
            || start + now(),
            || allowed(now()),
            || {
                checks.lock().unwrap().push(now());
                async move { ran }
            },
            |d| {
                *elapsed.lock().unwrap() += d;
                let more = now() <= run;
                async move { more }
            },
        ));
        checks.into_inner().unwrap()
    }

    #[test]
    fn scheduler_checks_on_launch_then_daily() {
        let two_days = LAUNCH_DELAY + 2 * INTERVAL;
        assert_eq!(
            scheduled(two_days, |_| true, Ran::Checked),
            [
                LAUNCH_DELAY,
                LAUNCH_DELAY + INTERVAL,
                LAUNCH_DELAY + 2 * INTERVAL
            ]
        );
        assert!(
            scheduled(two_days, |_| false, Ran::Checked).is_empty(),
            "not allowed"
        );

        let on_later = scheduled(INTERVAL, |t| t >= Duration::from_secs(3600), Ran::Checked);
        assert_eq!(
            on_later,
            [LAUNCH_DELAY + 6 * TICK],
            "allowed again: within a tick"
        );

        let skipped = scheduled(3 * TICK, |_| true, Ran::Skipped);
        assert_eq!(skipped.len(), 3, "a check that didn't run stays due");

        let failed = scheduled(3 * RETRY, |_| true, Ran::Failed);
        assert_eq!(
            failed,
            [LAUNCH_DELAY, LAUNCH_DELAY + RETRY, LAUNCH_DELAY + 2 * RETRY],
            "a failed check is retried in an hour"
        );
    }

    #[test]
    fn progress_is_logged_every_ten_percent() {
        let mut last = 0;
        assert_eq!(progress_step(5, Some(100), &mut last), None);
        assert_eq!(progress_step(12, Some(100), &mut last), Some(12));
        assert_eq!(progress_step(15, Some(100), &mut last), None);
        assert_eq!(progress_step(35, Some(100), &mut last), Some(35));
        assert_eq!(progress_step(100, Some(100), &mut last), Some(100));
        assert_eq!(progress_step(100, Some(100), &mut last), None);
        assert_eq!(progress_step(10, None, &mut last), None);
        assert_eq!(progress_step(10, Some(0), &mut last), None);
    }

    /// Serves one canned HTTP response on a local port; returns its URL.
    fn serve_once(body: String) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        url
    }

    /// The real host against a `latest.json` served on a local port: no request leaves the
    /// machine.
    #[test]
    fn tauri_host_reads_latest_json_and_refuses_foreign_downloads() {
        use std::sync::atomic::AtomicBool;
        use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
        static OFFERED: AtomicBool = AtomicBool::new(false);

        let target = tauri_plugin_updater::target().unwrap();
        let mut platforms = serde_json::Map::new();
        platforms.insert(
            target,
            serde_json::json!({ "signature": "sig", "url": "http://127.0.0.1:9/evil.tar.gz" }),
        );
        let latest = serde_json::json!({ "version": "99.0.0", "platforms": platforms });
        let mut context = mock_context(noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "pubkey": "dW50cnVzdGVk",
                "endpoints": [serve_once(latest.to_string())],
                "dangerousInsecureTransportProtocol": true,
            }),
        );
        let app = mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .manage(crate::overlay::Overlay::default())
            .manage(crate::notice::Notices::default())
            .build(context)
            .unwrap();
        let offer: fn(&AppHandle<MockRuntime>, &str) = |_, v| {
            assert_eq!(v, "99.0.0");
            OFFERED.store(true, Ordering::SeqCst);
        };
        start(app.handle(), offer);
        let host = TauriHost(app.handle().clone());

        let (version, update) = block_on(host.check()).unwrap().expect("an update");
        assert_eq!(version, "99.0.0");
        let refused = block_on(host.download(&update)).unwrap_err();
        assert!(
            refused.starts_with("refused download from http://127.0.0.1:9/"),
            "{refused}"
        );

        block_on(host.idle());
        host.offer("99.0.0");
        assert!(OFFERED.load(Ordering::SeqCst));

        let asking = TauriHost(app.handle().clone());
        let asked = tauri::async_runtime::spawn(async move { asking.ask_restart("99.0.0").await });
        let shown = crate::notice::Notice::UpdateReady {
            version: "99.0.0".into(),
        };
        let label = crate::notice::tests::opened(app.handle(), &shown);
        crate::notice::notice_answer(app.get_webview_window(&label).unwrap(), true).unwrap();
        assert!(block_on(asked).unwrap(), "Restart now");
        assert!(app.state::<AppUpdates>().ready().is_none());
        assert_eq!(ready_version(app.handle()), None);
    }

    #[test]
    fn no_ready_version_before_the_updater_starts() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        assert_eq!(ready_version(app.handle()), None);
    }
}
