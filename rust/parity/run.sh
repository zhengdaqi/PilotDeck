#!/usr/bin/env bash
# Build the Rust CLI and run the chat-search parity suite against the frozen TS CLI.
# Prereq: `pnpm install --frozen-lockfile && npm run build` (Node 22) so dist/ exists.
set -euo pipefail
here="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$here/rust" && cargo build --release --locked
cd "$here"
python3 rust/parity/chat_search_parity.py \
  --ts-cli dist/src/cli/pilotdeck.js \
  --rust-bin rust/target/release/pilotdeck \
  --node "${NODE:-node}" \
  --report "${REPORT:-/tmp/parity-chat-search.json}"
