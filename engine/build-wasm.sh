#!/usr/bin/env bash
# Build the WASM package for use in the frontend.
# Output: engine/pkg/  (gitignored; copy or reference from the frontend build)
#
# Prerequisites:
#   cargo install wasm-pack
#   rustup target add wasm32-unknown-unknown

set -euo pipefail

cd "$(dirname "$0")"

wasm-pack build \
  --target web \
  --out-dir pkg \
  -- \
  --features wasm

echo "Built: pkg/wasim_engine.js + pkg/wasim_engine_bg.wasm"
