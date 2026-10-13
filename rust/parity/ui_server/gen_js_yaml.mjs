// Oracle for js-yaml 3.14.2 safeLoad (the loader ported in ui-server-core/src/js_yaml.rs).
// Random YAML built from syntax-heavy fragments (flow/block collections, explicit keys, quoting, escapes, block
// scalars, tags, anchors, directives, document markers, odd indentation). Records the JSON result or the error reason
// (message up to " at line"). Dropped cases: results that JSON cannot carry faithfully (circular aliases, lone
// surrogates) and results with Date-valued keys (time-zone dependent, SRC-DEFECT-013).
// Usage: node rust/parity/ui_server/gen_js_yaml.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const yaml = createRequire(import.meta.url)(path.join(root, "node_modules/.pnpm/js-yaml@3.14.2/node_modules/js-yaml"));
if (yaml.safeLoad === undefined) throw new Error("js-yaml 3.14 expected");

let seed = 314;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const scalars = ["a", "b c", "1", "0x1F", "-.inf", "1:20", "2001-12-14", "~", "true", "'q''s'", "\"d\\\"q\"", "\"\\x41\\u00e9\\U0001F600\\N\\_\\L\\P\\e\\0\"", "\"\\q\"", "\"\\xZZ\"",
  "\"line\n  two\"", "'line\n\n  three'", "\"a\\\n  b\"", "x#y", "x #c", "-x", "?x", ":x", "a:b", "@", "`", "%x", "!!str 1", "!!int 1", "!!int a", "!!bool true", "!!null", "!!float 2", "!!binary YQ==",
  "!!timestamp 2001-12-14", "!<tag:yaml.org,2002:str> v", "!<!foo> v", "!e!x v", "!foo v", "!", "! v", "&a v", "*a", "*b", "&a", "!!str &a v", "&a !!str v", "!!seq", "!!map", "!!set", "!!omap", "!!pairs",
  "!!merge <<", "<<", "é", "\t", "'", "\"", "\u0085x", "\u2028", "\ufeffz", "\\", "...", "---", "- ", "? ", ": "];
const flows = () => pick([`[${pick(scalars)}, ${pick(scalars)}]`, `{${pick(scalars)}: ${pick(scalars)}}`, `[${pick(scalars)}: ${pick(scalars)}]`, `{? ${pick(scalars)} : ${pick(scalars)}, ${pick(scalars)}}`,
  "[a, [b, {c: d}]]", "{a: [1, 2], b}", "[", "{a: 1", "[a b]", "{a: 1 b: 2}", "[,]", "[a,]", "{a,b}", "[? a]", "[a: b, c: d]", "{a: 1, a: 2}", "[\n  a,\n  b\n]", "{\na: 1,\n  b: 2}", "{a:1}", "{\"a\":1}"]);
const blocks = () => pick([`|\n  ${pick(scalars)}\n  x\n`, ">\n  a\n  b\n\n  c\n   d\n", "|-\n  t\n\n", "|+\n  t\n\n", "|2\n   t\n", "|0\n t\n", "|--\n x\n", "|1-\n  z\n", ">+1\n x\n", "| # c\n x\n", "|x\n", ">\n\n  a\n"]);
const keys = () => pick(["k", "a", "a", "\"q k\"", "'s k'", "? x", "? [a, b]", "? {x: 1}", "? |\n  lit\n", "? a\n: b", "?", ":", "<<", "1", "[a, b]", "{a: b}", "&a k", "*a", "!!str k", "!!int 3", "~", "null", "- a", "-", "2020-01-01"]);
const line = (ind) => {
  const sp = " ".repeat(ind);
  switch (rnd(9)) {
    case 0: return `${sp}${keys()}: ${pick(scalars)}`;
    case 1: return `${sp}${keys()}: ${flows()}`;
    case 2: return `${sp}${keys()}: ${blocks()}`;
    case 3: return `${sp}- ${pick([pick(scalars), flows(), `${keys()}: ${pick(scalars)}`, "- x", ""])}`;
    case 4: return `${sp}${keys()}:`;
    case 5: return `${sp}${pick(scalars)}`;
    case 6: return pick(["# c", "", "  ", "---", "...", "--- x", "%YAML 1.2", "%YAML 2.0", "%YAML 1.1 x", "%TAG !e! tag:e.com,2000:", "%TAG ! !x-", "%FOO bar", "--- !!map", "\t- x"]);
    case 7: return `${sp}${flows()}`;
    default: return `${sp}${keys()}: ${pick(["&a ", "!!map ", "!!seq ", "!!set ", "!!omap ", "!!pairs ", ""])}\n${" ".repeat(ind + pick([0, 1, 2, 4]))}${pick(["- a", "- b: 1", "x: 1", "? y", "z", "- [1]", "<<: *a", "<<: [*a, {m: 1}]"])}`;
  }
};
const safe = ["a", "b c", "1", "0x1F", "0o17", "017", "1_000", "-1:20", "1.5e3", ".NaN", "2001-12-14 21:59:43.10 -5", "~", "Null", "FALSE", "'q''s'", "\"\\u00e9\\t\\x41\"", "x#y", "-x", "é", "!!str 1",
  "!!binary YWJj", "!!float 1", "!!timestamp 2001-12-14", "\"line\n  two\"", "[1, [2, {a: b}]]", "{a: 1, b: [x]}", "{}", "[]", "{? a, b: 2}", "[a: 1]", "&n v", "*n", "!!set {a, b}", "!!omap [a: 1, b: 2]", "!!pairs [a: 1, a: 2]"];
const skeys = ["a", "b", "c", "a", "1", "02", "\"k\"", "'k'", "<<", "~", "true", "1.5", "[x, y]", "{m: 1}", "__proto__", "&k kk", "!!str 7"];
const node = (d, ind) => {
  const sp = " ".repeat(ind);
  const r = d > 2 ? rnd(2) : rnd(7);
  if (r <= 1) return " " + pick(safe);
  if (r === 2) return pick([" |\n", " >-\n", " |+\n", " >\n"]) + `${sp}  ${pick(["t", "a b", "  more", ""])}\n${sp}  ${pick(["u", "", "x y"])}`;
  if (r <= 4) return "\n" + Array.from({ length: 1 + rnd(3) }, () => `${sp}- ${pick(["", "&s ", "!!seq ", ""])}`.trimEnd() + node(d + 1, ind + 2)).join("\n");
  return "\n" + Array.from({ length: 1 + rnd(3) }, () => {
    const k = pick(skeys);
    if (k === "<<") return `${sp}<<: ${pick(["*n", "[*n, {z: 9}]", "{m: 1}", "*s", "*k"])}`;
    if (rnd(8) === 0) return `${sp}? ${pick(skeys)}\n${sp}:` + node(d + 1, ind + 2);
    return `${sp}${k}:` + node(d + 1, ind + 2);
  }).join("\n");
};
const lone = /\\ud[89a-f][0-9a-f]{2}/i; // well-formed JSON.stringify escapes lone surrogates
const hasDateKey = (v) => v && typeof v === "object" && !(v instanceof Date) && !Buffer.isBuffer(v) && (Object.keys(v).some((k) => / GMT[+-]\d{4}/.test(k)) || Object.values(v).some(hasDateKey));
const cases = [];
const seen = new Set();
for (let i = 0; cases.length < 6000 && i < 60000; i++) {
  const n = 1 + rnd(6);
  const ls = [];
  let ind = 0;
  for (let j = 0; j < n; j++) { ls.push(line(ind)); ind = pick([0, 0, ind, ind + 2, Math.max(0, ind - 2), 1]); }
  let input = ls.join(pick(["\n", "\n", "\r\n", "\r"])) + pick(["", "\n", "\n\n", " ", "\t"]);
  if (rnd(2) === 0) input = `anchor: &n {p: 1, q: [2]}` + node(0, 0).replace(/^\n?/, "\n").replace(/^\n *- /, "\nlist:\n- ") + pick(["", "\n"]);
  if (rnd(10) === 0) input = pick(["", " ", "\n", "\ufeff", "  a", "\u0000x"]) + input;
  if (seen.has(input)) continue;
  seen.add(input);
  let out;
  try {
    const v = yaml.safeLoad(input);
    if (hasDateKey(v)) continue;
    const s = v === undefined ? "\"__undefined\"" : JSON.stringify(v);
    if (lone.test(s)) continue;
    out = { ok: JSON.parse(s) };
  } catch (e) {
    if (!(e instanceof yaml.YAMLException)) continue; // circular JSON etc.
    out = { err: String(e.message).split(" at line ")[0] };
  }
  cases.push({ input, out });
}
const target = path.join(root, "rust/crates/ui-server-core/tests/fixtures/js_yaml.json");
const text = JSON.stringify(cases) + "\n";
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
const errs = cases.filter((c) => c.out.err).length;
console.log(`wrote ${cases.length} cases (${errs} errors) to ${path.relative(root, target)}`);
