#!/usr/bin/env bash
# Build the Leptos web UI to dist/ (needs wasm32-unknown-unknown + wasm-bindgen-cli matching Cargo.lock).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown -p pilotdeck-web-ui
mkdir -p dist
"${WASM_BINDGEN:-wasm-bindgen}" --target web --no-typescript --out-dir dist --out-name app target/wasm32-unknown-unknown/release/pilotdeck-web-ui.wasm
cat > dist/index.html <<HTML
<!doctype html><html><head><meta charset="utf-8"><title>PilotDeck</title></head><body><script type="module">import init from "./app.js"; init();</script></body></html>
HTML
echo "wasm_bytes=$(stat -c %s dist/app_bg.wasm)"
