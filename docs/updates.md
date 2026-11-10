# Updates

xcreenshot updates itself from GitHub Releases with [`tauri-plugin-updater`](https://v2.tauri.app/plugin/updater/). The logic is in `src-tauri/src/updates.rs`.

## How it behaves

- **Automatic checks** run 10 seconds after launch, then every 24 hours. Every 10 minutes the app looks whether checks are allowed and the last check is 24 hours old (or the clock went backwards). So a computer that slept through a check catches up soon after it wakes. A check that didn't run (another one was in progress) stays due, and a failed one (offline, no release yet) is retried an hour later.
- **Download quietly.** A found update downloads in the background and its signature is verified. Nothing is shown yet; progress goes to the log file.
- **Offer the restart.** Once the update is stored, the menu bar gets **Restart to update (vX)** above Quit, and one prompt appears: an xcreenshot window titled **Update ready** ("xcreenshot X is downloaded. Restart to finish updating.") with **Later** and **Restart now** (Enter; Esc is Later). It opens once no capture is open, hides while a capture is open and comes back after, and doesn't block the app, so the capture hotkey keeps working. Only one prompt is open at a time. **Later** keeps the menu bar item, and a manual check offers the restart again. Once an update is ready, automatic checks stop until the next launch.
- **Restart.** From the prompt or the menu bar item: wait until the overlay closes, then install and restart in one step on the main thread, where captures start, so none can begin in between. If one did start before that step, wait again. The wait listens for the overlay going idle; it doesn't poll. Only one install runs, and nothing restarts without a click.
- **Check for updates…** always answers: "Already checking for updates.", the restart prompt if an update is ready or found, "You're up to date." or "Couldn't check for updates: …". These messages also wait until the overlay is closed.
- **Errors** from automatic checks (offline, or no release yet) are only logged. A failed install is shown once the overlay is closed.

The updater runs in Rust. The webview gets no updater or dialog permissions, and the CSP is unchanged.

Automatic checks follow `checkForUpdates` in `prefs.json` (on by default), read before each check. Settings › General › Check for updates turns them off. A manual check always runs.

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

The **Release** workflow (`.github/workflows/release.yml`, see [releasing.md](releasing.md)) builds one universal macOS app (Apple silicon and Intel) with [`tauri-apps/tauri-action`](https://github.com/tauri-apps/tauri-action), and the 64-bit Windows NSIS installer, and uploads to the GitHub release:

| File                                             | Used by                     |
| ------------------------------------------------ | --------------------------- |
| `xcreenshot_<version>_universal.dmg`             | people installing the app   |
| `xcreenshot_<version>_universal.app.tar.gz`      | the updater (the update)    |
| `xcreenshot_<version>_universal.app.tar.gz.sig`  | reference copy of the signature |
| `xcreenshot_<version>_x64-setup.exe`             | people installing it on Windows, and the updater |
| `xcreenshot_<version>_x64-setup.exe.sig`         | its signature, copied into `latest.json` |
| `latest.json` (signed builds, not betas)         | the updater (the manifest)  |

The updater only sees it once the release is **published** (not a draft or prerelease, and marked "latest"), so `releases/latest/download/latest.json` resolves to it. Betas are prereleases with no `latest.json`, so the updater never offers them ([releasing.md](releasing.md#beta)). After publishing, `latest.json` looks like this; all the macOS entries point at the same universal `.app.tar.gz`, and both Windows entries at the installer:

```json
{
  "version": "1.2.0",
  "notes": "<the release notes>",
  "pub_date": "2026-11-06T12:00:00.000Z",
  "platforms": {
    "darwin-aarch64": { "signature": "<contents of the .sig>", "url": "<.../download/v1.2.0/xcreenshot_1.2.0_universal.app.tar.gz>" },
    "darwin-x86_64": { "signature": "<contents of the .sig>", "url": "<.../download/v1.2.0/xcreenshot_1.2.0_universal.app.tar.gz>" },
    "darwin-aarch64-app": { "signature": "<contents of the .sig>", "url": "<.../download/v1.2.0/xcreenshot_1.2.0_universal.app.tar.gz>" },
    "darwin-x86_64-app": { "signature": "<contents of the .sig>", "url": "<.../download/v1.2.0/xcreenshot_1.2.0_universal.app.tar.gz>" },
    "windows-x86_64": { "signature": "<contents of the .exe.sig>", "url": "<.../download/v1.2.0/xcreenshot_1.2.0_x64-setup.exe>" },
    "windows-x86_64-nsis": { "signature": "<contents of the .exe.sig>", "url": "<.../download/v1.2.0/xcreenshot_1.2.0_x64-setup.exe>" }
  }
}
```

- `version` is the `package.json` version (SemVer, no `v`). The app only offers versions newer than its own.
- `signature` is the text of the `.sig` file, not a URL.
- `url` must start with `https://github.com/cberd1509/xcreenshot/releases/download/`, or the app refuses it (`src-tauri/src/updates.rs`). tauri-action writes GitHub API asset URLs instead (`https://api.github.com/repos/cberd1509/xcreenshot/releases/assets/<id>`). The draft has no tag yet, but its download URLs are predictable, so right after creating the draft the macOS job rewrites each `url` to `https://github.com/cberd1509/xcreenshot/releases/download/v<version>/<asset name>` (`node scripts/release.ts updater-urls`); signatures and the version stay as they were.
- Nothing reads a draft's `latest.json`: `releases/latest` only sees published releases. Publishing creates the tag, so the URLs work as soon as the release is (#135). When it's published, the `updater-urls` job checks that each names one of its assets, and rewrites `latest.json` only if something differs.
- **How it's assembled:** tauri-action writes `latest.json` with the macOS entries when the macOS job creates the draft. After both builds finish, the `windows-upload` job downloads it, adds `windows-x86_64` and `windows-x86_64-nsis` (`node scripts/release.ts add-windows`), with the `.exe.sig` text and the tag download URL, and uploads it back. It first checks the macOS entries name assets of the draft, and points them at the tag if the macOS job couldn't. Only one job writes the file at a time. The updater looks for `windows-x86_64-nsis` first, then `windows-x86_64`.
- Unsigned test builds (no Developer ID certificate) don't upload `latest.json`, so the updater never offers them, on either platform.
- **On Windows**, installing runs the new NSIS installer in passive mode (`/P /R /UPDATE`, the plugin's default `installMode`): xcreenshot exits, a progress bar shows, and the installer starts it again. The installer isn't code signed yet (#103), but the update's signature is checked like on macOS.
