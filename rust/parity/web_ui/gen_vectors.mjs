// Shared vectors for pilotdeck-web-ui-core from the TS sources in ui/src (Node 22 --experimental-strip-types).
// The same vectors run natively (cargo test) and in a real browser (rust/web-ui selftest).
// Usage: node --experimental-strip-types rust/parity/web_ui/gen_vectors.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const m = await import(path.join(root, "ui/src/utils/workspaceFileMention.ts"));

let seed = 7;
const rnd = (n) => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed % n; };
const pick = (a) => a[rnd(a.length)];
// BMP only: TS slices by UTF-16 index and could split astral pairs, which JSON/Rust strings cannot hold.
const atoms = ["src", "a", "B", "..", ".", "", "/", "\\", "//", "C:", "c:", "D:/", "/C:/", "x y", "é", "İ", "ß", "文件", " ", "\t", "\u00a0", "\u3000", "\n", "file.ts", "Ä"];
const pathOf = () => Array.from({ length: 1 + rnd(5) }, () => pick(atoms)).join(pick(["/", "", "\\", "/"]));
const roots = ["", "/", "/repo", "/repo/", "/Repo", "C:/Work", "c:\\work\\", "/C:/Work", "//server/share", "D:/", "/repo/sub/..", "rel/root"];
const vectors = { mentionRequest: [], has: [], relative: [], canonical: [], identity: [], atOrBelow: [], insert: [] };
const reqs = [null, 1, "x", [], {}, { projectName: "p", relativePath: "a" }, { projectName: " ", relativePath: "a" }, { projectName: "p", relativePath: "\u3000" }, { projectName: "p", relativePath: 3 }, { projectName: "p" }, { projectName: "\ufeffp", relativePath: "\n" }];
for (const r of reqs) vectors.mentionRequest.push({ v: r, out: m.isWorkspaceFileMentionRequest(r) });
for (let i = 0; i < 1500; i++) {
  const p = pathOf(); const r = pick(roots); const q = pick([p, pathOf(), r + "/" + p, p.toUpperCase()]);
  vectors.relative.push({ f: q, r, out: m.getWorkspaceRelativePath(q, r) });
  vectors.canonical.push({ f: q, r, out: m.canonicalizeWorkspaceFilePath(q, r) });
  vectors.identity.push({ f: q, r, out: m.getWorkspaceFileIdentity(q, r) });
  const parent = pick([p, pathOf(), r, q.split("/").slice(0, 2).join("/")]);
  vectors.atOrBelow.push({ c: q, p: parent, r, out: m.isWorkspacePathAtOrBelow(q, parent, r) });
  const words = Array.from({ length: rnd(5) }, () => pick(["src/a.ts", "a", "x", "src/a.tsx", " ", "\t", "\u00a0", "é", "src/a.ts\n"])).join(pick([" ", "", "  "]));
  const rel = pick(["src/a.ts", "a", "a.b", "x+y", "(z)", "[q]", "a*", ""]);
  vectors.has.push({ input: words, rel, out: m.hasWorkspaceFileMention(words, rel) });
  const cur = pick([-5, 0, 1, 2, 3, 7, 1000, words.length, words.length - 1]);
  vectors.insert.push({ input: words, rel, cur, out: m.insertWorkspaceFileMention(words, rel, cur) });
}
const text = JSON.stringify(vectors) + "\n";
const target = path.join(root, "rust/crates/web-ui-core/tests/fixtures/workspace_file_mention.json");
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "vectors stable" : "vectors DIFFER"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
console.log(`wrote ${Object.values(vectors).reduce((a, v) => a + v.length, 0)} vectors`);
