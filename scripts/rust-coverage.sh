#!/usr/bin/env bash
# Rust tests with coverage; fails under MIN_COVERAGE (default 90%) line coverage.
# main.rs and lib.rs are app wiring only and are excluded, so logic lives in other modules.
# Until the first such module exists there is nothing to measure, and the instrumented build is skipped.
set -euo pipefail
cd "$(dirname "$0")/../src-tauri"

if [ -z "$(find src -name '*.rs' ! -name main.rs ! -name lib.rs)" ]; then
  echo "Rust coverage: no logic modules yet, skipping."
  exit 0
fi

rustup component add llvm-tools-preview >/dev/null 2>&1 || true
cargo llvm-cov --ignore-filename-regex '(^|/)(main|lib)\.rs$' --fail-under-lines "${MIN_COVERAGE:-90}"
