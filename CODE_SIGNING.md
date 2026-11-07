# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

This policy covers the Windows builds of xcreenshot. The macOS builds are signed with the maintainer's Apple Developer ID and notarized by Apple.

## What is signed

Only the installers and executables of xcreenshot, built by GitHub Actions from this repository (`.github/workflows/release.yml`). Nothing is signed by hand or built on a personal machine. Third-party components are part of the build and are not signed separately.

## Team

xcreenshot has one maintainer, who holds every role:

| Role                     | Who                                       |
| ------------------------ | ----------------------------------------- |
| Committers and reviewers | [cberd1509](https://github.com/cberd1509) |
| Approvers                | [cberd1509](https://github.com/cberd1509) |

Changes from anyone else arrive as pull requests and are reviewed by the maintainer before they are merged. Every signing request is approved by hand. The maintainer uses multi-factor authentication for GitHub and SignPath.

## Privacy

xcreenshot keeps your screenshots on your device. Its only network requests are update checks and update downloads from GitHub Releases, which can be turned off in Settings. See [PRIVACY.md](PRIVACY.md).

## Reporting

Report a signed binary that doesn't come from this project, or any other concern, by opening an issue at <https://github.com/cberd1509/xcreenshot/issues>, or privately through [GitHub security advisories](https://github.com/cberd1509/xcreenshot/security/advisories/new).
