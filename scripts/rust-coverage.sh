#!/usr/bin/env bash
# Runs Rust tests with coverage and fails if line coverage is under the minimum.
# main.rs/lib.rs are app wiring only and are excluded; logic lives in other modules.
# While there is no measurable code yet (0 lines), the check passes.
set -euo pipefail
MIN="${MIN_COVERAGE:-90}"
cd "$(dirname "$0")/../src-tauri"

summary=$(cargo llvm-cov --ignore-filename-regex '(^|/)(main|lib)\.rs$' --json --summary-only)
read -r count covered <<<"$(jq -r '.data[0].totals.lines | "\(.count) \(.covered)"' <<<"$summary")"

if [ "$count" -eq 0 ]; then
  echo "Rust coverage: no measurable code yet, skipping the ${MIN}% check."
  exit 0
fi

pct=$(awk -v c="$covered" -v n="$count" 'BEGIN { printf "%.2f", 100 * c / n }')
echo "Rust line coverage: ${pct}% (${covered}/${count}), minimum ${MIN}%"
awk -v p="$pct" -v m="$MIN" 'BEGIN { exit (p + 0 >= m + 0) ? 0 : 1 }' || {
  echo "Rust coverage is below ${MIN}%." >&2
  exit 1
}
