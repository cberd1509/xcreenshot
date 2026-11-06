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
cargo llvm-cov --ignore-filename-regex '(^|/)(main|lib)\.rs$' --fail-under-lines 90
```

CI runs the same checks on macOS and Windows.

## Tests and coverage

- Every change comes with tests. CI fails if coverage drops below **90%** (lines, branches, functions) for the frontend or the Rust code.
- Frontend: [Vitest](https://vitest.dev) + Testing Library. Tests sit next to the code as `*.test.ts`. Generated shadcn components in `src/lib/components/ui` are excluded.
- Rust: `cargo test`, coverage with [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) (`brew install cargo-llvm-cov`). Keep `main.rs` and `lib.rs` to app wiring only; put logic in modules with unit tests.

## Guidelines

- **Privacy first.** No telemetry, analytics or network calls. The only exception is the update check.
- **Keep it simple.** Capture is drag-only. New features should fit the existing flow and not add settings unless needed.
- Keep UI copy short and plain.
- Pick an issue from the [project board](https://github.com/users/cberd1509/projects/1), or open one before starting big changes.

By contributing, you agree your work is released under the [MIT License](LICENSE).
