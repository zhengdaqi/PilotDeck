#!/usr/bin/env bash
# Build each probe app for wasm32, run wasm-bindgen, and record sizes/times.
# Needs: a rustup toolchain with wasm32-unknown-unknown and wasm-bindgen-cli matching Cargo.lock.
set -euo pipefail
cd "$(dirname "$0")"
WB=${WASM_BINDGEN:-wasm-bindgen}
out=${OUT:-dist}
mkdir -p "$out"
for app in leptos dioxus yew; do
  s=$(date +%s)
  cargo build --release --target wasm32-unknown-unknown -p probe-$app 2>"$out/$app.build.log"
  e=$(date +%s)
  mkdir -p "$out/$app"
  "$WB" --target web --no-typescript --out-dir "$out/$app" --out-name app "target/wasm32-unknown-unknown/release/probe-$app.wasm"
  cat > "$out/$app/index.html" <<HTML
<!doctype html><html><head><meta charset="utf-8"></head><body><div id="main"></div><script type="module">import init from "./app.js"; init();</script></body></html>
HTML
  raw=$(stat -c %s "$out/$app/app_bg.wasm")
  gz=$(gzip -9 -c "$out/$app/app_bg.wasm" | wc -c)
  echo "$app build_s=$((e-s)) wasm_bytes=$raw wasm_gzip_bytes=$gz"
done
