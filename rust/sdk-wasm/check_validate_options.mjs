// Replays the TS validateOptions fixture (rust/parity/sdk/gen_validate_options.mjs) through the wasm npm wrapper.
import { readFileSync } from "node:fs";
import { validateOptions } from "./npm/index.js";
const fx = JSON.parse(readFileSync(new URL("../crates/sdk/tests/fixtures/validate_options.json", import.meta.url), "utf8"));
// Decode the fixture markers back into real JS values, so the wrapper's own encoder is exercised too.
const dec = (v) => {
  if (Array.isArray(v)) return v.map(dec);
  if (v && typeof v === "object") {
    const k = Object.keys(v);
    if (k.length === 1 && k[0] === "$fn") return () => {};
    if (k.length === 1 && k[0] === "$undef") return undefined;
    if (k.length === 1 && k[0] === "$num") return Number(v.$num);
    return Object.fromEntries(Object.entries(v).map(([a, b]) => [a, dec(b)]));
  }
  return v;
};
let bad = 0;
for (const c of fx.cases) {
  let got;
  try { validateOptions(dec(c.options)); got = "ok"; } catch (e) { got = e instanceof TypeError ? `TypeError: ${e.message}` : `${e.code}: ${e.message}`; }
  if (got !== c.result && bad++ < 5) console.log(JSON.stringify(c.options), "\n want", c.result, "\n got ", got);
}
console.log(`sdk-wasm validateOptions: ${fx.cases.length} cases, ${bad} mismatches`);
process.exit(bad ? 1 : 0);
