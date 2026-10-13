#!/usr/bin/env bash
# Build the wasm npm wrapper into npm/pkg (needs wasm32-unknown-unknown + wasm-bindgen-cli 0.2.129).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
"${WASM_BINDGEN:-wasm-bindgen}" --target web --out-dir npm/pkg target/wasm32-unknown-unknown/release/pilotdeck_sdk_wasm.wasm
echo "wasm_bytes=$(stat -c %s npm/pkg/pilotdeck_sdk_wasm_bg.wasm)"
