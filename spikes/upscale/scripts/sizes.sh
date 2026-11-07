#!/usr/bin/env bash
# Stripped size of the probe binary per feature set (bytes), to see what each dependency adds.
set -euo pipefail
cd "$(dirname "$0")/.."
target=${CARGO_TARGET_DIR:-target}
for feat in "" probe-image-lanczos fir fir-u8x4 metalfx; do
  cargo build -q --profile probe --bin probe --no-default-features ${feat:+--features $feat}
  printf '%-22s %s\n' "${feat:-png only}" "$(stat -f %z "$target/probe/probe")"
done
