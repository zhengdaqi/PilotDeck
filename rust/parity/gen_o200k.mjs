// Converts js-tiktoken's o200k_base ranks (the tokenizer behind src/context/budget/tokenizer.ts
// countTokens) into a compact binary for the Rust sidecar: repeated [u32 LE rank][u8 len][bytes].
// usage: node rust/parity/gen_o200k.mjs [--check]
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(import.meta.url);
const ranks = require("../../node_modules/js-tiktoken/dist/ranks/o200k_base.cjs");
const r = ranks.default ?? ranks;
const chunks = [];
for (const line of r.bpe_ranks.split("\n").filter(Boolean)) {
  const [, offsetStr, ...tokens] = line.split(" ");
  const offset = Number.parseInt(offsetStr, 10);
  tokens.forEach((token, i) => {
    const bytes = Buffer.from(token, "base64");
    const head = Buffer.alloc(5);
    head.writeUInt32LE(offset + i, 0);
    head.writeUInt8(bytes.length, 4);
    chunks.push(head, bytes);
  });
}
const out = Buffer.concat(chunks);
const path = new URL("../crates/agent-loop-sidecar/src/o200k_base.bin", import.meta.url);
if (process.argv.includes("--check")) {
  if (!fs.readFileSync(path).equals(out)) { console.error("o200k_base.bin is stale"); process.exit(1); }
  console.log("o200k_base.bin up to date");
} else fs.writeFileSync(path, out);
console.error("pat_str:", r.pat_str);
