// Oracle for ui/server/utils/commandParser.js: replaceArguments, isPathSafe, validateCommand (shell-quote), sanitizeOutput,
// processFileIncludes. For the fixed source defects (SRC-DEFECT-010/011/012, ADR-0004) a small JS reference of the
// fixed behaviour is recorded next to the TS output. The Rust port must equal the reference; any case where TS differs
// must be one of the listed defects (checked by the Rust test).
// Usage: node rust/parity/ui_server/gen_command_parser.mjs [--check]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const m = await import(path.join(root, "ui/server/utils/commandParser.js"));

let seed = 1010;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const wrap = async (f) => { try { return { ok: await f() }; } catch (e) { return { err: String(e?.message ?? e) }; } };

// Reference (fixed) replaceArguments: single pass, literal values.
const refReplace = (content, args) => {
  if (!content) return content;
  const a = Array.isArray(args) ? args : args ? [args] : [];
  return content.replace(/\$ARGUMENTS|\$([1-9])/g, (mm, d) => (d ? a[d - 1] || "" : a.join(" ")));
};
const v = { replace: [], pathSafe: [], validate: [], sanitize: [], includes: null };
const tmplAtoms = ["run ", "$ARGUMENTS", "$1", "$2", "$9", "$10", "$", "$$", "$ARG", " and ", "\n", "x"];
const argAtoms = ["a", "b c", "", "$1", "$&", "$'", "$`", "$$", "$ARGUMENTS", "price $5", "é"];
for (let i = 0; i < 1500; i++) {
  const content = Array.from({ length: rnd(6) }, () => pick(tmplAtoms)).join("");
  const args = rnd(5) ? Array.from({ length: rnd(4) }, () => pick(argAtoms)) : pick(["single", ""]);
  v.replace.push({ content, args, ts: m.replaceArguments(content, args), ref: refReplace(content, args) });
}
const bases = ["/base", "/base/", "/", "/a/b"];
const fps = ["a.md", "../x", "./a", "", ".", "sub/../a", "sub/../../x", "/etc/passwd", "/base/a", "..a", "a/..", "sub/./b"];
for (const b of bases) for (const f of fps) v.pathSafe.push({ f, b, out: m.isPathSafe(f, b) });
const cmdAtoms = ["ls", "echo", "git", "/bin/ls", "rm", "task-master", "./node", "ls/", " ", "  ", "-la", "'a b'", "\"x y\"", "\"$HOME\"", "$X", "${Y}", "${}", "${", "$?", "$constructor",
  "$toString", "a#b", "#c", "&&", "|", ";", ">", "<(", "*.js", "a?", "\\*", "\\;", "'it''s'", "\"a\\\"b\"", "\"a\\nb\"", "x;y", "`id`", "$(id)", "a{b}", "[x]", "\\\\", "\"\\$\"", "\t", "é", "'unterminated", "\""];
for (let i = 0; i < 3000; i++) {
  const s = Array.from({ length: 1 + rnd(5) }, () => pick(cmdAtoms)).join(pick([" ", "", " "]));
  v.validate.push({ s, out: await wrap(() => m.validateCommand(s)) });
}
for (const s of ["", "plain", "a\u0000b\u0007c", "tab\tnl\nCR\r", "\u007fdel", "\u0085nel\u009f", "emoji😀", "\u001b[31mred\u001b[0m"]) v.sanitize.push({ s, out: m.sanitizeOutput(s) });

// Includes: a temp tree; the same tree is rebuilt by the Rust test from the recorded spec.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pd-incl-"));
const base = path.join(dir, "base"), outside = path.join(dir, "outside");
const files = { "a.md": "A", "b.md": "B @a.md", "dollar.md": "cost $& $' $` $$ end", "d1.md": "@d2.md", "d2.md": "@d3.md", "d3.md": "@d4.md", "d4.md": "four",
  "nl.md": "line1\n@a.md\nline3", "sub/c.md": "C", "twice.md": "@a.md @a.md", "empty.md": "" };
const links = [["link_out", "../outside/secret.txt"], ["link_in", "a.md"], ["dangling", "../outside/nope.txt"], ["dirlink", "../outside"]];
fs.mkdirSync(path.join(base, "sub"), { recursive: true }); fs.mkdirSync(outside);
fs.writeFileSync(path.join(outside, "secret.txt"), "SECRET");
for (const [f, c] of Object.entries(files)) fs.writeFileSync(path.join(base, f), c);
for (const [l, t] of links) fs.symlinkSync(t, path.join(base, l));
const contents = ["", "no includes", "@a.md", " @a.md", "x @a.md y", "x\n@a.md", "x\t@a.md", "@b.md", "@dollar.md", "@d1.md", "@d2.md", "@nl.md", "@sub/c.md", "@../outside/secret.txt",
  "@link_out", "@link_in", "@dangling", "@dirlink/secret.txt", "@missing.md", "@sub", "@twice.md", "@a.md @a.md", "mail a@b.com", "@empty.md", "x\r\n@a.md", "@", "@a.md\n@b.md"];
v.includes = { files, links, outside: { "secret.txt": "SECRET" }, cases: [] };
for (const c of contents) {
  const r = await wrap(() => m.processFileIncludes(c, base));
  v.includes.cases.push({ c, ts: JSON.parse(JSON.stringify(r).split(dir).join("<dir>")) });
}
fs.rmSync(dir, { recursive: true, force: true });

const text = JSON.stringify(v) + "\n";
const target = path.join(root, "rust/crates/ui-server-core/tests/fixtures/command_parser.json");
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
const errs = new Set(v.validate.filter((x) => x.out.err).map((x) => x.out.err.slice(0, 40)));
console.log("replace diffs (TS vs fixed):", v.replace.filter((x) => x.ts !== x.ref).length, "validate allowed:", v.validate.filter((x) => x.out.ok?.allowed).length, "errors:", [...errs].join(" | "));
for (const c of v.includes.cases) console.log(JSON.stringify(c.c), "=>", JSON.stringify(c.ts));
