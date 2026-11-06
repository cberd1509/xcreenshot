# Updates

xcreenshot updates itself from GitHub Releases with [`tauri-plugin-updater`](https://v2.tauri.app/plugin/updater/). The logic is in `src-tauri/src/updates.rs`.

## How it behaves

- **Automatic checks** run 10 seconds after launch, then every 24 hours. Every 10 minutes the app looks whether checks are allowed and the last check is 24 hours old (or the clock went backwards). So a computer that slept through a check catches up soon after it wakes. A check that didn't run (another one was in progress) stays due, and a failed one (offline, no release yet) is retried an hour later.
- **Download quietly.** A found update downloads in the background and its signature is verified. Nothing is shown yet; progress goes to the log file.
- **Offer the restart.** Once the update is stored, the menu bar gets **Restart to update (vX)** above Quit, and one prompt appears, "Update ready: vX", with **Restart now** and **Later**. The prompt waits until no capture is open and doesn't block the app: on macOS it is a system alert shown from a background thread, so the capture hotkey keeps working. Only one prompt is open at a time. **Later** keeps the menu bar item, and a manual check offers the restart again. Once an update is ready, automatic checks stop until the next launch.
- **Restart.** From the prompt or the menu bar item: wait until the overlay closes, then install and restart in one step on the main thread, where captures start, so none can begin in between. If one did start before that step, wait again. The wait listens for the overlay going idle; it doesn't poll. Only one install runs, and nothing restarts without a click.
- **Check for updates…** always answers: "Already checking for updates.", the restart prompt if an update is ready or found, "You're up to date." or "Couldn't check for updates: …". These messages also wait until the overlay is closed.
- **Errors** from automatic checks (offline, or no release yet) are only logged. A failed install is shown once the overlay is closed.

The updater runs in Rust. The webview gets no updater or dialog permissions, and the CSP is unchanged.

Automatic checks follow `checkForUpdates` in `prefs.json` (on by default), read before each check. The Settings switch for it comes with #20. A manual check always runs.

## What gets requested

A `GET` of:

```
https://github.com/cberd1509/xcreenshot/releases/latest/download/latest.json
```

and, when it lists a newer version, the update file it points to. Only files under `https://github.com/cberd1509/xcreenshot/releases/download/` are fetched; any other URL is refused without a request. Nothing else is sent; GitHub sees the IP address and that a check happened.

## Signing

`bundle.createUpdaterArtifacts` is on, so release builds sign each update with the minisign key whose public half is `plugins.updater.pubkey` in `tauri.conf.json`. The app refuses updates that don't match it.

The build needs two secrets (set in the repository, not available locally):

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Without them, a full `pnpm tauri build` fails at the signing step. To build installers locally, turn the updater artifacts off for that build:

```sh
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

`pnpm tauri dev` and `pnpm tauri build --no-bundle` (what CI runs) don't sign anything and need no key.

## What a release publishes

The **Release** workflow (`.github/workflows/release.yml`, see [releasing.md](releasing.md)) builds one universal macOS app (Apple silicon and Intel) with [`tauri-apps/tauri-action`](https://github.com/tauri-apps/tauri-action) and uploads to the GitHub release:

| File                                             | Used by                     |
| ------------------------------------------------ | --------------------------- |
| `xcreenshot_<version>_universal.dmg`             | people installing the app   |
| `xcreenshot_<version>_universal.app.tar.gz`      | the updater (the update)    |
| `xcreenshot_<version>_universal.app.tar.gz.sig`  | reference copy of the signature |
| `latest.json` (signed builds only)               | the updater (the manifest)  |

The updater only sees it once the release is **published** (not a draft or prerelease, and marked "latest"), so `releases/latest/download/latest.json` resolves to it. tauri-action writes `latest.json` like this; all four macOS entries point at the same universal `.app.tar.gz`:

```json
{
  "version": "1.2.0",
  "notes": "<the release notes>",
  "pub_date": "2026-11-06T12:00:00.000Z",
  "platforms": {
    "darwin-aarch64": { "signature": "<contents of the .sig>", "url": "<universal .app.tar.gz>" },
    "darwin-x86_64": { "signature": "<contents of the .sig>", "url": "<universal .app.tar.gz>" },
    "darwin-aarch64-app": { "signature": "<contents of the .sig>", "url": "<universal .app.tar.gz>" },
    "darwin-x86_64-app": { "signature": "<contents of the .sig>", "url": "<universal .app.tar.gz>" }
  }
}
```

- `version` is the `package.json` version (SemVer, no `v`). The app only offers versions newer than its own.
- `signature` is the text of the `.sig` file, not a URL.
- `url` is the GitHub API URL of the release asset (`https://api.github.com/repos/cberd1509/xcreenshot/releases/assets/<id>`), which the updater downloads directly. It stays valid when the draft is published.
- Unsigned test builds (no Developer ID certificate) don't upload `latest.json`, so the updater never offers them.
- Windows isn't built yet. When it is, its NSIS installer adds a `windows-x86_64` entry.
