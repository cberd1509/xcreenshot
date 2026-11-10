# Releasing

## How a release happens

1. Merge the **"chore(release): version packages"** PR. It bumps `package.json` and writes `CHANGELOG.md`.
2. The **Release** workflow (`.github/workflows/release.yml`) sees no pending changesets and no `v<version>` release yet, so it builds a universal macOS app (Apple Silicon and Intel) and, in parallel, the 64-bit Windows installer. It creates a **draft** GitHub Release `v<version>` with:
   - `xcreenshot_<version>_universal.dmg` for people to download
   - `xcreenshot_<version>_universal.app.tar.gz` and its `.sig`, used by the in-app updater
   - `xcreenshot_<version>_x64-setup.exe`, the Windows installer, which the updater also uses, and its `.sig`
   - `latest.json`, the update manifest with the macOS and Windows entries (signed macOS builds only, not betas)
   - release notes taken from that version's `CHANGELOG.md` section, with a note about the unsigned Windows installer
3. Wait until the draft's title loses "(adding Windows)": the Windows installer and its `latest.json` entries go in last. Then download the DMG and the installer from the draft, install them and check that they work.
4. Publish the draft. Publishing creates the `v<version>` tag, and the updater starts offering the new version right away: the draft's `latest.json` already points at the tag's download URLs. The workflow then checks them.

A waiting draft is left alone by later merges that don't change the version. To rebuild it from the latest `main`, run **Release** from the Actions tab on `main`: it deletes the draft and builds a new one. If the macOS build fails or is cancelled, its unfinished draft is deleted too, so there's no draft until the next successful run. A Windows failure keeps the draft (below). A published release is never rebuilt or deleted.

The macOS job creates the draft. The Windows installer is built at the same time with a read-only token; once both builds are done, a separate job uploads it to the draft and adds its entries to the macOS job's `latest.json` (see [updates.md](updates.md#what-a-release-publishes)).

**If Windows fails** (its build, its upload, or a cancelled run), the draft and its notarized DMG stay, still titled "(adding Windows)", and the run fails. Click **Re-run failed jobs** to finish it: re-running is safe, and the built installer is kept for 30 days, so a failed upload doesn't need a new build. The job only works on the draft its own run created (by release id), so after a newer **Release** run replaced the draft, the old run refuses.

**Never publish while the title says "(adding Windows)"**: that draft has no Windows installer or `latest.json` entries. The job checks the release is still a draft right before each write, so it leaves a release published early alone; a publish in the second between a check and a write can still slip through, and the job reports it afterwards. Publishing without Windows also fails the run that publishing starts. Either way, add the installer and its `latest.json` entries by hand, or publish a patch release.

To check the release's Windows build without a release, run **Release** with **dry-run** on `main` (`gh workflow run release.yml --ref main -f dry-run=true`). It builds and signs the installer with the updater key; nothing is uploaded and no release is created or changed. It refuses other branches, because the build has the updater key; to try a branch, use the Windows test build below.

## Beta

To ship `1.0.0-beta.N` before `1.0.0`, put Changesets in pre mode. Each version PR then makes the next beta.

**Start** (on a new branch from an up-to-date `main`; open a PR and merge it):

```sh
pnpm changeset pre enter beta   # writes .changeset/pre.json
pnpm changeset                  # pick "major", summary e.g. "Version 1.0"
git add .changeset
git commit -m "chore(release): enter beta for 1.0.0"
```

The `major` changeset turns `0.x` into `1.0.0`; without it the beta would be `0.x.y-beta.0`. The version PR is then titled "chore(release): version packages (beta)" and bumps `package.json` to `1.0.0-beta.0`. Each later version PR gives `-beta.1`, `-beta.2` and so on. Don't edit the version by hand.

**What gets published:** the draft `v1.0.0-beta.N` is a GitHub **prerelease** (any version with a `-`) with the DMG, the Windows installer and the updater bundles, but no `latest.json`. Check and publish it as usual. Leave "Set as a pre-release" ticked. `releases/latest` skips prereleases, so:

- stable users' updater keeps reading the stable `latest.json` and never sees the beta;
- the landing page (which reads `releases/latest`) and the README's install links keep offering the stable release.

If the flag is changed by hand when publishing, the `updater-urls` job fixes it. A beta published as a full release is marked a prerelease straight away. A stable version published as a prerelease becomes the latest release once its `latest.json` is checked, but only if it's newer than the current latest release; otherwise the job only warns. Either fix rebuilds the landing page.

**Testing:** send testers the prerelease's page. They install the DMG or the Windows installer as usual. Betas don't update to newer betas: the updater only reads the stable `latest.json`, which is older, so testers install each beta by hand. When `1.0.0` is published they get it like everyone else, because `1.0.0` is newer than `1.0.0-beta.N`. The updater never downgrades, so leaving the beta early means reinstalling the stable release.

**Version formats:** the app and the macOS `Info.plist` carry the full `1.0.0-beta.N` (Developer ID signing and notarization accept it). The NSIS installer's Windows file version is numeric, so Tauri derives it from the numbers only: `1.0.0.0` for every beta. An MSI (WiX) build would fail, because WiX rejects non-numeric prerelease versions; the workflow builds only NSIS, and locally use `pnpm tauri build --bundles nsis`.

**Release 1.0.0** (on a new branch; open a PR and merge it):

```sh
pnpm changeset pre exit   # marks .changeset/pre.json for exit
git add .changeset
git commit -m "chore(release): exit beta for 1.0.0"
```

The next version PR bumps `package.json` to `1.0.0` and removes `pre.json` and the used changesets. Merge it and release as above: `1.0.0` is a normal release and the updater offers it.

## Secrets

| Secret                               | What it is                                                             |
| ------------------------------------ | ---------------------------------------------------------------------- |
| `TAURI_SIGNING_PRIVATE_KEY`          | Private key that signs updates (`pnpm tauri signer generate`)          |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Its password                                                           |
| `APPLE_CERTIFICATE`                  | The Developer ID Application certificate as a base64 `.p12`            |
| `APPLE_CERTIFICATE_PASSWORD`         | The `.p12` password                                                    |
| `APPLE_API_KEY`                      | App Store Connect API key ID (10 characters, e.g. `ABC123DEFG`)        |
| `APPLE_API_ISSUER`                   | App Store Connect issuer ID (a UUID)                                   |
| `APPLE_API_KEY_P8`                   | The contents of the `AuthKey_<key id>.p8` file, including `-----BEGIN` |

The macOS and Windows builds both use `TAURI_SIGNING_PRIVATE_KEY`, and both fail without it. Windows needs no other secret. Setting `APPLE_CERTIFICATE` turns on Developer ID signing, and then the other four `APPLE_*` secrets are required: the build fails if one is missing. Without `APPLE_CERTIFICATE` you get an unsigned test build.

## Getting the Apple credentials

You need a paid [Apple Developer Program](https://developer.apple.com/programs/) membership.

**Developer ID certificate** (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`)

1. In Keychain Access, choose Certificate Assistant › Request a Certificate From a Certificate Authority, and save the request to disk.
2. At [developer.apple.com › Certificates](https://developer.apple.com/account/resources/certificates/list), add a **Developer ID Application** certificate using that request. Download it and double-click it to add it to your keychain.
3. In Keychain Access › My Certificates, right-click "Developer ID Application: …" and export it as a `.p12` with a strong password.
4. Store it: `base64 -i certificate.p12 | gh secret set APPLE_CERTIFICATE`, then `gh secret set APPLE_CERTIFICATE_PASSWORD`.

**App Store Connect API key** (`APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_P8`), used for notarization

1. At [App Store Connect › Users and Access › Integrations › App Store Connect API](https://appstoreconnect.apple.com/access/integrations/api), create a **Team key** with the **Developer** role.
2. Download the `.p8` file (you can only download it once).
3. Store them: `gh secret set APPLE_API_KEY` (the key ID), `gh secret set APPLE_API_ISSUER` (the issuer ID shown above the keys) and `gh secret set APPLE_API_KEY_P8 < AuthKey_<key id>.p8`.

## Signed and unsigned builds

- **Signed:** Tauri signs the app with the Developer ID certificate and hardened runtime, then notarizes and staples it. The workflow then notarizes and staples the DMG too. Testers open the DMG, drag xcreenshot to Applications and open it; macOS asks once to confirm opening an app downloaded from the internet.
- **Unsigned test build:** the app is only ad-hoc signed. macOS blocks the first open ("Apple could not verify…"). Testers click **Done**, go to System Settings › Privacy & Security, click **Open Anyway** next to the xcreenshot message, and confirm. After that it opens normally. The release notes say this at the top.

Unsigned builds don't upload `latest.json`, so the in-app updater never offers an unsigned build.

Either way, macOS asks for Screen Recording permission on the first capture.

## Windows

The Windows installer isn't code signed yet (Authenticode signing is #103). Its updates are still
signed with the updater key, so the app only installs updates from this project.

- **First run:** SmartScreen may say "Windows protected your PC". Click **More info › Run anyway**.
  The release notes, README and landing page say this.
- **Install:** the NSIS installer installs for the current user, with no admin prompt. If WebView2
  is missing (some Windows 10 machines), the installer downloads it from Microsoft.
- **Updates:** the updater downloads the new installer, checks its signature and runs it in passive
  mode (a progress bar, no questions), which closes xcreenshot and starts it again when done.

## Windows test builds

To try a branch on Windows before a release, use the **Windows test build**
workflow (`.github/workflows/windows-test.yml`). Run it from the Actions tab, or:

```sh
gh workflow run windows-test.yml --ref <branch>
```

It builds the NSIS installer from that branch and uploads it to a **draft** release named
`windows-test-<sha12>`. A draft sends no notifications to watchers, fires no release events (so the
updater and the landing page are untouched) and is visible only to people with access to the
repo, who can download the installer from the Releases page. The build is unsigned (SmartScreen
asks once) and has no updater files. Delete old test drafts when you're done with them; never
publish one.
