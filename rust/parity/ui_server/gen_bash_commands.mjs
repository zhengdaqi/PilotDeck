// Oracle for commandParser.processBashCommands (real processes, cwd = a temp dir with known files).
// Usage: node rust/parity/ui_server/gen_bash_commands.mjs [--check]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const m = await import(path.join(root, "ui/server/utils/commandParser.js"));
const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "pd-bash-")));
const files = { "a.txt": "alpha\nbeta $& gamma\n", "b.md": "# B\n", "ctl.txt": "x\u0001y\u007fz\ttab\n" };
for (const [f, c] of Object.entries(files)) fs.writeFileSync(path.join(dir, f), c);
const contents = ["", "no commands", "!echo hi", "!echo hi\n!pwd", "before\n!echo one two\nafter", "!ls", "!cat a.txt", "!cat ctl.txt", "!grep beta a.txt", "!grep zzz a.txt", "!ls missing-file",
  "!rm -rf /", "!echo a && echo b", "!echo $HOME", "!echo 'a b'  \"c\"", "! echo spaced", "!", "x!echo not-at-line-start", "!echo cr\r\n!echo next", "!echo first\n!echo first", "!cat a.txt\n!echo $& literal",
  "!echo '$&'", "!echo \"$'\"", "!find . -name b.md", "!whoami-not", "!/bin/echo path", "!echo é😀", "line\n!echo end", "!echo trailing\n", "!grep -c a a.txt b.md"];
const cases = [];
for (const c of contents) {
  let out;
  try { out = { ok: await m.processBashCommands(c, { cwd: dir }) }; } catch (e) { out = { err: String(e?.message ?? e) }; }
  cases.push({ c, ts: JSON.parse(JSON.stringify(out).split(dir).join("<dir>")) });
}
fs.rmSync(dir, { recursive: true, force: true });
const text = JSON.stringify({ files, cases }) + "\n";
const target = path.join(root, "rust/crates/ui-server-core/tests/fixtures/bash_commands.json");
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
for (const c of cases) console.log(JSON.stringify(c.c), "=>", JSON.stringify(c.ts));
