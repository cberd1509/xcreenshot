# Contributing to xcreenshot

Thanks for helping out! xcreenshot aims to stay small, fast and private.

## Setup

You need [Node 24+](https://nodejs.org), [pnpm](https://pnpm.io) and [Rust](https://rustup.rs).
On macOS, also install Xcode Command Line Tools (`xcode-select --install`).
On Windows, see the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm tauri dev
```

## Before opening a PR

```sh
pnpm lint && pnpm check && pnpm coverage
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings
cd .. && ./scripts/rust-coverage.sh
```

CI runs the same checks on macOS and Windows.

## Tests and coverage

- Every change comes with tests. CI fails if coverage drops below **90%** (lines, branches, functions) for the frontend or the Rust code.
- Frontend: [Vitest](https://vitest.dev) + Testing Library. Tests sit next to the code as `*.test.ts`. Generated shadcn components in `src/lib/components/ui` are excluded.
- Rust: `cargo test`, coverage with [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) (`brew install cargo-llvm-cov`). Keep `main.rs` and `lib.rs` to app wiring only; put logic in modules with unit tests.

## Commits and PRs

- Commit messages and PR titles follow [Conventional Commits](https://www.conventionalcommits.org): `feat: …`, `fix: …`, `chore: …`, `docs: …`. A Git hook checks commit messages, and CI checks PR titles (PRs are squash-merged, so the title becomes the commit).
- `pnpm install` sets up the Git hooks (Husky): staged files are linted and formatted (Oxlint, Prettier, rustfmt) before each commit.

## Versioning and releases

- xcreenshot uses [Semantic Versioning](https://semver.org). Until 1.0, `minor` is for new features and `patch` for fixes; breaking changes to settings also bump `minor`.
- Every PR adds a **changeset** (`pnpm changeset`) with a one-line, user-facing description. Internal changes use `pnpm changeset --empty`. CI checks for it.
- After merging to `main`, a bot keeps a **"chore(release): version packages"** PR open with the next version and the updated `CHANGELOG.md`. Merging it bumps the version everywhere (package.json, Cargo.toml; tauri.conf.json reads package.json) and tags `vX.Y.Z`, which builds the release.

## App icon

The icon sources are `assets/icon.svg` and `assets/tray.svg` (menu bar). After changing them, run `pnpm icons` to regenerate every size (needs `rsvg-convert`: `brew install librsvg`).

## Guidelines

- **Privacy first.** No telemetry, analytics or network calls. The only exception is the update check.
- **Keep it simple.** Capture is drag-only. New features should fit the existing flow and not add settings unless needed.
- Keep UI copy short and plain.
- Pick an issue from the [project board](https://github.com/users/cberd1509/projects/1), or open one before starting big changes.

By contributing, you agree your work is released under the [MIT License](LICENSE).
