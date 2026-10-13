// Oracle for ui/server/utils/frontmatter.js (gray-matter 4.0.3 + js-yaml 3.14 safeLoad) and commandParser.parseCommand.
// Records data/content (JSON.stringify, so Dates are ISO strings) or the thrown message.
// Usage: node rust/parity/ui_server/gen_frontmatter.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const { parseFrontmatter } = await import(path.join(root, "ui/server/utils/frontmatter.js"));
const { parseCommand } = await import(path.join(root, "ui/server/utils/commandParser.js"));

let seed = 2020;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const vals = ["hello", "Run the tests", "\"quoted: yes\"", "'single ''q'''", "123", "-7", "+3", "0x1F", "0x_1", "012", "09", "0b101", "0o7", "1_000", "1_", "1:30", "-1:30:05", "190:20:30.15", "1.5",
  "-0", "-0.0", "1e5", "1.0e+3", "1e400", ".5", "1.", ".inf", "-.Inf", ".NaN", "2020-01-01", "2020-1-1 10:00:00", "2001-12-14t21:59:43.10-05:00", "2001-12-14 21:59:43.1234 +8", "0099-02-30",
  "2020-13-45T25:61:61Z", "~", "null", "NULL", "True", "FALSE", "yes", "no", "on", "\"<<\"", "<<", "!!str 12", "!!int \"0x10\"", "!!int x", "!!float 1", "!!null ''", "!!bool yes", "!!binary aGVsbG8=",
  "!!binary 'aGk='", "!!timestamp 2020-01-01", "!!timestamp nope", "!foo x", "! 12", "[a, b, 1]", "[]", "{}", "{a: 1, b: [2, c]}", "[a, {b: c}]", "a: b", "\"a\\tb\\u00e9\"", "é ünïcode", "@file.md",
  "x # trailing comment", "'#not comment'", "!!set {a, b}", "!!omap [a: 1, b: 2]", "!!pairs [a: 1, a: 2]", "!!seq [1]", "!!map {a: 1}", "!!str", "&anc shared", "*anc", "!!merge <<", "|\n  line1\n  line2\n",
  ">-\n  folded\n  text\n", "|+\n  keep\n\n", "\n  - x\n  - y", "\n  k: v\n  k2: 2", "\n  - [n, 1]\n  - {m: 2}", "\t tab", "'multi\n  line'", "\"esc \\x41\""];
const keys = ["description", "model", "allowed-tools", "argument-hint", "1", "10", "02", "a b", "__proto__", "constructor", "2020-01-01", "12", "1.50", "null", "true", "\"quoted key\"", "? [k1, k2]",
  "? {x: 1}", "? [[n]]", "~", "[a, b]", "<<", "description"];
const linesFor = () => {
  const n = rnd(5);
  const out = [];
  for (let i = 0; i < n; i++) {
    const k = pick(keys);
    let v = pick(vals);
    if (k === "<<") v = pick(["*anc", "[*anc]", "{m: 1}", "1", "[{a: 1}, {b: 2}]", "[x, y]", "!!binary aGk="]);
    out.push(`${k}: ${v}`);
  }
  if (rnd(4) === 0) out.unshift(`anchor: &anc ${pick(["{a: 1, z: 2}", "[p, q]", "v", "{description: from-anchor}"])}`);
  if (rnd(6) === 0) out.push(pick(["# comment", "  # indented comment", "#", "---", "...", "- list", "  bad indent: x", "key: [unclosed", "key: {a: 1"]));
  return out.join("\n");
};
const opens = ["---\n", "---\n", "---\n", "---yaml\n", "---YAML\n", "---yml\n", "---json\n", "---js\n", "---coffee\n", "---toString\n", "--- \n", "----\n", "\ufeff---\n", "---\r\n", "  ---\n", "---  yaml  \n"];
const closes = ["\n---\n", "\n---\n", "\n---\r\n", "\n---", "\n----\nx", "\n--- trailing\n", "", "\n---\n\n"];
const bodies = ["", "Body text with $ARGUMENTS", "# Heading\n\n@file.md\n!git status", "line\n---\nnot front matter"];
const cases = [];
const record = (input) => {
  let out;
  try { const f = parseFrontmatter(input); out = { ok: JSON.parse(JSON.stringify({ data: f.data === undefined ? "__undefined" : f.data, content: f.content })) }; }
  catch (e) { out = { err: String(e?.message ?? e) }; }
  let cmd;
  try { cmd = { ok: JSON.parse(JSON.stringify(parseCommand(input))) }; } catch (e) { cmd = { err: String(e?.message ?? e) }; }
  cases.push({ input, out, cmd });
};
for (const fixed of ["", "plain", "---", "---\n", "---\n---", "---\n---\n", "---\n\n---\nbody", "---\n# c\n---\nb", "---\n#\n---\n", "---\nfoo\n---\n", "---\n- a\n---\n", "---\n0\n---\n", "---\n''\n---\n",
  "---\nfalse\n---\n", "---\n~\n---\n", "---yaml", "---a", "---ab", "---\n---\n---\n", "---\na: 1\n---\nb: 2\n---\n", "---\na: 1\n...\n---\n", "---\nkey: v\n---\r\nbody", "\ufeff", "---\u2028# x\n---",
  "---\nb: &b {a: 1, z: 2}\nobj:\n  a: 0\n  <<: *b\n---", "---\nb: &b {a: 1, z: 2}\nobj:\n  <<: *b\n  a: 5\n---", "---\nb: &b {a: 1}\nobj:\n  <<: *b\n  a: 5\n  a: 6\n---",
  "---\nb: &b {a: 1, 3: x}\nc: &c {a: 2, 1: y, q: 3}\nobj:\n  k: 0\n  <<: [*b, *c]\n  q: 9\n---", "---\nobj:\n  <<: [p, q]\n  0: z\n---", "---\nobj:\n  <<: {a: 1}\n  <<: {a: 2, b: 3}\n---",
  "---\nobj: {<<: {a: 1}, a: 2}\n---", "---\nobj: !!set {a, b: ~}\n---", "---\nobj: !!set {a: 1}\n---", "---\nobj: !!omap [a: 1, a: 2]\n---", "---\nobj: !!pairs [a: 1, b]\n---"]) record(fixed);
for (let i = 0; i < 4000; i++) record(pick(opens) + linesFor() + pick(closes) + pick(bodies));
const text = JSON.stringify(cases) + "\n";
const target = path.join(root, "rust/crates/ui-server-core/tests/fixtures/frontmatter.json");
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
const errs = {}; for (const c of cases) if (c.out.err) { const k = c.out.err.split(/[ (]/).slice(0, 3).join(" "); errs[k] = (errs[k] || 0) + 1; }
console.log("cases", cases.length, "ok", cases.filter((c) => c.out.ok).length, errs);
