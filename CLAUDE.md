# xcreenshot

A fast, privacy-first screenshot app for macOS (first) and Windows. It runs in the menu bar or tray: press the hotkey, drag a region, annotate it, then copy, save or share. MIT licensed, app ID `com.bervex.xcreenshot`.

Read `CONTRIBUTING.md` for setup and `docs/PLAN.md` for the roadmap. The backlog lives in GitHub issues and milestones (M1–M5).

## Stack

- **Shell:** Tauri 2 (Rust, edition 2024) in `src-tauri/`
- **UI:** Svelte 5 + Vite (no SvelteKit), Tailwind 4, shadcn-svelte (`src/lib/components/ui/` is generated, don't hand-edit it), Lucide icons
- **Tooling:** Node 26 LTS (`.nvmrc`) and pnpm; commit hooks need `pnpm` and `~/.cargo/bin` on `PATH`

## Commands

```sh
pnpm tauri dev            # run the app
pnpm lint                 # oxlint + prettier
pnpm check                # svelte-check + tsc
pnpm coverage             # vitest, 90% minimum
pnpm format               # autofix lint + formatting
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test
./scripts/rust-coverage.sh  # Rust coverage, 90% minimum (skips main.rs/lib.rs and share/macos.rs)
```

## Before opening a PR

1. Every change ships with tests. Coverage must stay at **90% or more** for both TypeScript and Rust; CI enforces it.
2. Run `pnpm lint && pnpm check && pnpm coverage`, plus clippy, `cargo test` and `./scripts/rust-coverage.sh` if Rust changed.
3. **Run `/code-review` and `/simplify` on the change and apply their fixes before opening (or updating) the PR.** This is required for every PR, including small ones.
4. User-facing changes (`feat`, `fix`, `perf`, or breaking `!`) need a changeset: `pnpm changeset`, with one plain sentence. Internal changes (`chore`, `ci`, `docs`, `test`, `refactor`) don't.

## Git and PRs

- Conventional Commits are enforced by commitlint (hook and PR title). Keep body lines at 100 characters or less.
- Never commit to `main`; branch, open a PR, squash-merge. The PR title becomes the commit, so it must be conventional too.
- Required checks: **CI result** and **PR title and changeset**. CodeQL blocks high-severity alerts.
- Pin GitHub Actions to a commit SHA with a version comment (`uses: owner/action@<sha> # vX.Y.Z`).
- `package.json` is the only version source (`tauri.conf.json` reads it). Don't add a version to `Cargo.toml`.

## Code conventions

- **Rust:** `lib.rs` only wires plugins, tray and shortcuts; logic lives in modules with unit tests. Keep pure logic (geometry, parsing) separate from OS calls so it's testable without a real screen. Put platform code behind `cfg(target_os = ...)` and keep Windows compiling. Coverage skips only `share/macos.rs`, because it is thin FFI glue with no logic. Don't add logic there, and don't widen the exclusion; move decisions into a tested module instead.
- **Frontend:** TypeScript strict, Svelte 5 runes, components covered by Testing Library tests.
- **Speed:** hotkey-to-overlay must stay fast (target under 150 ms). Capture happens before any window appears. Benchmark capture changes with `cargo run --release --example capture`; don't swap the backend for a slower one.
- **Privacy:** no telemetry, analytics or accounts. The only network requests are the update check (`latest.json` from GitHub Releases) and, when there is one, the update download. Checks are on by default and can be turned off. Keep the strict CSP in `tauri.conf.json`.
- **Design:** the [design canvas](https://claude.ai/artifact/RAcQ8kKKNBJCeC4Z3MriRr) is the source of truth for layout, copy, spacing and color. `design/` mirrors it in the repo: read the artboards there, and sync it when the canvas changes (see `design/README.md`). `design/` only changes when someone syncs it: before any UI work, compare the canvas version with `design/sync.json` and sync first if they differ. Every UI change names the artboard it follows and matches it. A screen with no artboard gets one on the canvas before it's built.
- **UI copy:** use the design's copy as written. Where the design has none, keep it short and plain: don't over-explain or add privacy and marketing lines. When simplifying, change the wording, not the layout.
- **Platforms:** when a platform ships, or its install steps change, update the README's Install section and the landing page (`site/`, artboard `Landing.dc.html`) in the same PR. Neither should offer a download that doesn't exist yet.

## Ask the maintainer first

Releases and store publishing; signing certificates, secrets or API keys; changing the app ID, license or the version number by hand; anything that costs money.
