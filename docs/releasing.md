# Releasing

## How a release happens

1. Merge the **"chore(release): version packages"** PR. It bumps `package.json` and writes `CHANGELOG.md`.
2. The **Release** workflow (`.github/workflows/release.yml`) sees no pending changesets and no `v<version>` release yet, so it builds a universal macOS app (Apple Silicon and Intel). It creates a **draft** GitHub Release `v<version>` with:
   - `xcreenshot_<version>_universal.dmg` for people to download
   - `xcreenshot_<version>_universal.app.tar.gz` and its `.sig`, used by the in-app updater
   - `latest.json`, the update manifest (signed builds only)
   - release notes taken from that version's `CHANGELOG.md` section
3. Download the DMG from the draft, install it and check that it works.
4. Publish the draft. Publishing creates the `v<version>` tag, and the updater starts offering the new version.

A waiting draft is left alone by later merges that don't change the version. To rebuild it from the latest `main`, run **Release** from the Actions tab on `main`: it deletes the draft and builds a new one. If that build fails or is cancelled, its unfinished draft is deleted too, so there's no draft until the next successful run. A published release is never rebuilt or deleted.

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

The build fails without `TAURI_SIGNING_PRIVATE_KEY`. Setting `APPLE_CERTIFICATE` turns on Developer ID signing, and then the other four `APPLE_*` secrets are required: the build fails if one is missing. Without `APPLE_CERTIFICATE` you get an unsigned test build.

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
