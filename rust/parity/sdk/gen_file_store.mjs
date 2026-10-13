// TS oracle for FileSessionStore (packages/sdk/src/session-store.ts): op results plus the exact on-disk tree
// (paths and bytes) after each script, so the Rust store stays format-compatible with the TS one.
// Date.now is scripted. Usage: node --experimental-strip-types rust/parity/sdk/gen_file_store.mjs [--check]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const m = await import(path.join(root, "packages/sdk/src/session-store.ts"));
let clock = 0;
Date.now = () => clock;

let seed = 2026;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const pks = ["p1", "p2", "proj/é", " ", ""];
const sids = ["s1", "s2", "s 3", "\t", ""];
const subs = [undefined, undefined, undefined, "agent-1", "agent_2", "a/b", "Z", "", " "];
const key = () => { const k = { projectKey: pick(pks), sessionId: pick(sids) }; const s = pick(subs); if (s !== undefined) k.subpath = s; return k; };
const validKey = () => { const k = { projectKey: pick(["p1", "p2", "proj/é"]), sessionId: pick(["s1", "s2", "s 3"]) }; const s = pick([undefined, undefined, "agent-1", "a/b"]); if (s !== undefined) k.subpath = s; return k; };
const validEntry = () => ({ type: pick(["user", "assistant"]), ...(rnd(2) ? { uuid: pick(["u1", "u2", "u5"]) } : {}), text: "t" });
const entry = () => {
  const e = { type: pick(["user", "assistant", "session_metadata", " ", ""]) };
  if (rnd(2)) e.uuid = pick(["u1", "u2", "u3", "", "u4"]);
  if (rnd(2)) e.text = pick(["hi", "第二", "", "x\ny"]);
  if (rnd(4) === 0) e.metadata = { title: pick(["T", null]), n: pick([1, 0.5, -3]) };
  return e;
};
const entries = () => Array.from({ length: rnd(4) }, entry);

function tree(dir) {
  const out = {};
  const walk = (d, rel) => {
    for (const name of fs.readdirSync(d).sort()) {
      const p = path.join(d, name), r = rel ? rel + "/" + name : name;
      if (fs.statSync(p).isDirectory()) walk(p, r); else out[r] = fs.readFileSync(p, "utf8");
    }
  };
  if (fs.existsSync(dir)) walk(dir, "");
  return out;
}

const errOf = async (f) => { try { await f(); return null; } catch (e) { return String(e?.message ?? e); } };
const sortedDirs = (d) => (fs.existsSync(d) ? fs.readdirSync(d).filter((n) => fs.statSync(path.join(d, n)).isDirectory()).sort() : []);
async function possibleErrors(store, rootDir, op) {
  const errs = [];
  const pk = op.op === "listSubkeys" ? op.key.projectKey : op.pk;
  const keyErr = await errOf(() => store.projectDir(pk));
  if (keyErr) return [keyErr];
  const projectDir = store.projectDir(pk);
  if (op.op === "listSubkeys") {
    const ke = await errOf(() => store.sessionDir(op.key));
    if (ke) return [ke];
    const d = path.join(store.sessionDir(op.key), "subpaths");
    const files = fs.existsSync(d) ? fs.readdirSync(d).filter((f) => f.endsWith(".json")).sort() : [];
    for (const f of files) { const e = await errOf(() => store.readSnapshotAtPath(path.join(d, f))); if (e) errs.push(e); }
  } else {
    for (const c of sortedDirs(projectDir)) { const e = await errOf(() => store.readSnapshotAtPath(path.join(projectDir, c, "session.json"))); if (e) errs.push(e); }
    if (!errs.length && op.op === "listSessionSummaries") {
      for (const { sessionId } of await store.listSessions(pk)) { const e = await errOf(() => store.readSnapshot({ projectKey: pk, sessionId })); if (e) errs.push(e); }
    }
  }
  return [...new Set(errs)].map((e) => e.split(rootDir).join("<root>"));
}

const scripts = [];
for (let sc = 0; sc < 60; sc++) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pd-fstore-"));
  const rootDir = path.join(dir, "store");
  const store = new m.FileSessionStore({ rootDir });
  const ops = [], out = [];
  clock = 1000;
  for (let i = 0; i < 40; i++) {
    const k = rnd(13);
    let op;
    if (k < 2) { clock = pick([clock, clock + 1, clock + 40, clock - 10]); op = { op: "now", v: clock }; }
    else if (k < 5) op = { op: "append", key: rnd(3) ? validKey() : key(), entries: rnd(2) ? Array.from({ length: rnd(4) }, validEntry) : entries() };
    else if (k === 5) op = { op: "load", key: key() };
    else if (k === 6) op = { op: "listSessions", pk: pick(pks) };
    else if (k === 7) op = { op: "listSessionSummaries", pk: pick(pks) };
    else if (k === 8) op = { op: "listSubkeys", key: { projectKey: pick(pks), sessionId: pick(sids) } };
    else if (k === 9) op = { op: "delete", key: key() };
    else if (k === 10) op = { op: "export", key: key() };
    else if (k === 11) {
      const snap = rnd(6) ? { schemaVersion: 1, key: rnd(4) ? validKey() : key(), mtime: pick([0, 5, 99999, 1040, -1]), entries: rnd(3) ? Array.from({ length: rnd(4) }, validEntry) : entries() } : pick([null, 3, { schemaVersion: 2 }, { schemaVersion: 1, key: { projectKey: "p1", sessionId: "s1" }, mtime: 1, entries: [{ type: "" }] }]);
      op = { op: "import", snapshot: JSON.parse(JSON.stringify(snap)), ...(rnd(4) ? { mode: pick(["reject", "append", "replace"]) } : {}) };
    } else {
      // Corrupt one stored file (same bytes on both sides), or plant a mismatching snapshot.
      op = { op: "plant", rel: `project-${Buffer.from(pick(pks), "utf8").toString("base64url")}/session-${Buffer.from(pick(sids), "utf8").toString("base64url")}/session.json`,
        text: pick(["{bad", "null", JSON.stringify({ schemaVersion: 1, key: { projectKey: "other", sessionId: "x" }, mtime: 1, entries: [] }) + "\n"]) };
    }
    ops.push(JSON.parse(JSON.stringify(op)));
    let r;
    try {
      switch (op.op) {
        case "now": r = null; break;
        case "append": r = (await store.append(op.key, op.entries)) ?? null; break;
        case "load": r = await store.load(op.key); break;
        case "listSessions": r = await store.listSessions(op.pk); break;
        case "listSessionSummaries": r = await store.listSessionSummaries(op.pk); break;
        case "listSubkeys": r = await store.listSubkeys(op.key); break;
        case "delete": r = (await store.delete(op.key)) ?? null; break;
        case "export": r = await store.exportSession(op.key); break;
        case "import": r = await store.importSession(op.snapshot, op.mode ? { mode: op.mode } : undefined); break;
        case "plant": { const p = path.join(rootDir, op.rel); fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, op.text); r = null; break; }
      }
    } catch (e) {
      r = { __error: String(e?.message ?? e), code: e?.code ?? null };
      // Promise.all over several files rejects with whichever read fails first, so TS is nondeterministic when more
      // than one file is bad. Record every error TS could give; Rust must give one of them (it uses the first in
      // sorted directory order).
      if (["listSessions", "listSessionSummaries", "listSubkeys"].includes(op.op)) r = { __errorOneOf: await possibleErrors(store, rootDir, op), code: r.code };
    }
    out.push(JSON.parse(JSON.stringify(r ?? null).split(rootDir).join("<root>")));
  }
  scripts.push({ ops, out, tree: tree(rootDir) });
  fs.rmSync(dir, { recursive: true, force: true });
}
let ctor;
try { new m.FileSessionStore({ rootDir: "  " }); ctor = null; } catch (e) { ctor = { __error: e.message, code: e.code }; }
const text = JSON.stringify({ scripts, emptyRootError: ctor }) + "\n";
const target = path.join(root, "rust/crates/sdk/tests/fixtures/file_store.json");
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; if (!same && process.env.PD_DIFF_OUT) fs.writeFileSync(process.env.PD_DIFF_OUT, text); console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
const c = {}; for (const s of scripts) s.ops.forEach((o, i) => { const k = o.op + (s.out[i]?.__error ? ":err" : ""); c[k] = (c[k] ?? 0) + 1; });
console.log(JSON.stringify(c));
