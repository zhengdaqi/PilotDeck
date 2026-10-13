// Replays the TS-generated session-store fixture through the wasm npm wrapper (Node).
import { readFileSync } from "node:fs";
import { drive } from "../parity/sdk/drive.mjs";
import * as sdk from "./npm/index.js";
const fx = JSON.parse(readFileSync(new URL("../crates/sdk/tests/fixtures/session_store.json", import.meta.url), "utf8"));
let clock = 0; Date.now = () => clock;
const got = await drive({ ...sdk, setNow: (v) => { clock = v; } }, fx.ops);
let bad = 0;
got.forEach((g, i) => { if (JSON.stringify(g) !== JSON.stringify(fx.expected[i])) { if (bad++ < 5) console.log("#" + i, JSON.stringify(fx.ops[i]), "\n want", JSON.stringify(fx.expected[i]), "\n got ", JSON.stringify(g)); } });
console.log(`sdk-wasm npm wrapper: ${got.length} ops, ${bad} mismatches`);
process.exit(bad ? 1 : 0);
