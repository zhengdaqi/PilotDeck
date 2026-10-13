// Oracle for ui/server/services/permissionSettings.js and mcpConfig.js.
// Pure normalizers on random JSON values, plus scripted read/write ops in a temp PILOT_HOME and project dir.
// Usage: node rust/parity/ui_server/gen_settings_files.mjs [--check]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const perm = await import(path.join(root, "ui/server/services/permissionSettings.js"));
const mcp = await import(path.join(root, "ui/server/services/mcpConfig.js"));

let seed = 3030;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const entries = ["Read", " Write ", "Bash", "Bash()", "Bash( git status )", "Bash(npm:*)", "Bash(a\nb)", "bash", "Task", "MultiEdit", "Edit", "custom_tool", "", "  ", 5, null, ["Read"], "WebSearch", "Bash(x)y", "read_file", "toString", "__proto__"];
const permValue = () => {
  if (rnd(8) === 0) return pick([null, "str", 5, [1, 2], true]);
  const o = {};
  for (const k of ["allowedTools", "askTools", "disallowedTools"]) if (rnd(4)) o[k] = rnd(6) ? Array.from({ length: rnd(6) }, () => pick(entries)) : pick(["Read", null, {}]);
  if (rnd(2)) o.skipPermissions = pick([true, false, 1, 0, "", "no", null, [], {}]);
  if (rnd(2)) o.lastUpdated = pick(["2026-01-01T00:00:00.000Z", 5, null, ""]);
  if (rnd(4) === 0) o.extra = 1;
  if (rnd(6) === 0) o.version = 7;
  return o;
};
const server = () => {
  if (rnd(10) === 0) return pick([null, "s", [], 3]);
  const s = {};
  const kind = rnd(5);
  if (kind === 0) s.command = pick(["npx", 3, ""]); else if (kind === 1) s.url = pick(["http://x", null]); else if (kind === 2) s.httpUrl = "http://h"; else if (kind === 3) { s.command = "node"; s.url = "u"; }
  if (rnd(3) === 0) s.args = pick([["-y", "pkg"], [], ["a", 1], "a", null]);
  if (rnd(3) === 0) s.env = pick([{ K: "v" }, {}, { K: 1 }, [], null, "e"]);
  if (rnd(4) === 0) s.headers = pick([{ H: "v" }, { H: null }, []]);
  if (rnd(5) === 0) s.extra = { nested: [1, { "2": "b", a: 1 }] };
  return s;
};
const mcpValue = () => {
  if (rnd(10) === 0) return pick([null, "x", 5, [1]]);
  const o = {};
  if (rnd(5) === 0) o.other = { "10": 1, "2": 2, z: [1e21, 0.1, -0] };
  if (rnd(6)) {
    if (rnd(8) === 0) o.mcpServers = pick([null, [], "s", 0]);
    else { o.mcpServers = {}; for (let i = rnd(4); i > 0; i--) Object.defineProperty(o.mcpServers, pick(["fs", "git", "", "1", "__proto__", "名"]), { value: server(), enumerable: true, writable: true, configurable: true }); }
  }
  return o;
};
const wrap = async (f) => { try { const v = await f(); return { ok: v === undefined ? "__undefined" : v }; } catch (e) { return { err: String(e?.message ?? e) }; } };
const v = { normPerm: [], normEntry: [], normMcp: [], scripts: [], virtual: [] };
const pp = await import(path.join(root, "ui/server/utils/pilotPaths.js"));
for (const r of ["", "/h", "/h/", "/h/../h", "rel", "/x/Resources/Runtime", "/x/resources/runtime/", "/x/resources/runtimes", "/rt", "/rt/a/.."])
  for (const env of [{}, { PILOTDECK_DESKTOP: "1" }, { PILOTDECK_DESKTOP: "1", PILOTDECK_DESKTOP_RUNTIME_ROOT: "/rt" }, { PILOTDECK_DESKTOP: "true" }, { PILOTDECK_DESKTOP: "1", PILOTDECK_DESKTOP_RUNTIME_ROOT: "" }])
    v.virtual.push({ r, env, cwd: process.cwd(), out: pp.isVirtualProjectPath(r, "/h", env) });
for (const e of entries) v.normEntry.push({ in: JSON.stringify(e), out: perm.normalizePermissionEntry(e) });
for (let i = 0; i < 1500; i++) { const x = permValue(); v.normPerm.push({ in: JSON.stringify(x), out: JSON.stringify(perm.normalizePermissionSettings(x)) }); }
for (let i = 0; i < 1500; i++) { const x = mcpValue(); const r = await wrap(() => mcp.normalizeMcpConfig(x)); v.normMcp.push({ in: JSON.stringify(x), out: r.ok ? { ok: JSON.stringify(r.ok) } : r }); }

// Scripts: real files under a temp dir; Date pinned so lastUpdated is deterministic.
const RealDate = Date;
const fixedIso = "2026-10-13T06:00:00.000Z";
for (let si = 0; si < 120; si++) {
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "pd-settings-")));
  const home = path.join(dir, "home"); const proj = path.join(dir, "proj");
  fs.mkdirSync(proj);
  const env = { PILOT_HOME: home };
  process.env.PILOT_HOME = home;
  delete process.env.PILOTDECK_DESKTOP;
  const ops = []; const outs = [];
  for (let k = 0; k < 2 + rnd(6); k++) {
    const r = rnd(9);
    let op;
    if (r === 0) op = { op: "plantPerm", text: pick(["{bad", "", JSON.stringify(permValue()), "null", "[]"]) };
    else if (r === 1) op = { op: "readPerm" };
    else if (r === 2) op = { op: "writePerm", updates: JSON.stringify(pick([permValue(), null, "ab", { allowedTools: ["Bash(ls)"] }])) };
    else if (r === 3) op = { op: "plantMcp", scope: pick(["global", "project"]), text: pick(["{bad", "", JSON.stringify(mcpValue()), "{\"mcpServers\":{\"a\":{\"command\":\"x\"}}}"]) };
    else if (r === 4) op = { op: "readMcp", scope: pick(["global", "project", "other"]), proj: pick(["P", "P", "", "HOME", "FILE"]) };
    else if (r === 5) op = { op: "writeMcp", scope: pick(["global", "project"]), raw: pick(["{bad", JSON.stringify(mcpValue()), "{\"mcpServers\":{\"a\":{\"url\":\"u\"}}}", "[]"]), proj: pick(["P", "", "HOME", "NEW"]) };
    else if (r === 6) op = { op: "listMcp", proj: pick(["P", "", "HOME"]) };
    else if (r === 7) op = { op: "mcpDir", scope: pick(["global", "project"]) };
    else op = { op: "readPerm" };
    ops.push(op);
    const projOf = (p) => (p === "P" ? proj : p === "HOME" ? home : p === "FILE" ? path.join(dir, "afile") : p === "NEW" ? path.join(dir, "newproj") : "");
    if (op.proj === "FILE") fs.writeFileSync(path.join(dir, "afile"), "x");
    let out;
    if (op.op === "plantPerm") { fs.mkdirSync(home, { recursive: true }); fs.writeFileSync(path.join(home, "permissions.json"), op.text); out = { ok: null }; }
    else if (op.op === "readPerm") out = { ok: perm.readPermissionSettings(env) };
    else if (op.op === "writePerm") { globalThis.Date = class extends RealDate { toISOString() { return fixedIso; } }; out = await wrap(() => perm.writePermissionSettings(JSON.parse(op.updates), env)); globalThis.Date = RealDate; }
    else if (op.op === "plantMcp") { const f = op.scope === "global" ? path.join(home, "mcp.json") : path.join(proj, ".pilotdeck", "mcp.json"); fs.mkdirSync(path.dirname(f), { recursive: true }); fs.rmSync(f, { force: true, recursive: true }); fs.writeFileSync(f, op.text); out = { ok: null }; }
    else if (op.op === "mcpDir") { const f = op.scope === "global" ? path.join(home, "mcp.json") : path.join(proj, ".pilotdeck", "mcp.json"); fs.rmSync(f, { force: true, recursive: true }); fs.mkdirSync(f, { recursive: true }); out = { ok: null }; }
    else if (op.op === "readMcp") out = await wrap(() => mcp.readMcpConfigFile(op.scope, projOf(op.proj) || undefined));
    else if (op.op === "writeMcp") out = await wrap(() => mcp.writeMcpConfigFile(op.scope, op.raw, projOf(op.proj) || undefined));
    else if (op.op === "listMcp") out = await wrap(() => mcp.listMcpConfigFiles(projOf(op.proj) || undefined));
    outs.push(JSON.parse(JSON.stringify(out).split(dir).join("<dir>")));
  }
  const tree = {};
  const walk = (p, rel) => { for (const n of fs.readdirSync(p).sort()) { const f = path.join(p, n); const st = fs.statSync(f); if (st.isDirectory()) walk(f, rel + n + "/"); else tree[rel + n] = { text: fs.readFileSync(f, "utf8"), mode: (st.mode & 0o777).toString(8) }; } };
  walk(dir, "");
  v.scripts.push({ ops, outs, tree });
  fs.rmSync(dir, { recursive: true, force: true });
}
const text = JSON.stringify(v) + "\n";
const target = path.join(root, "rust/crates/ui-server-core/tests/fixtures/settings_files.json");
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
const errs = {}; for (const s of v.scripts) for (const o of s.outs) if (o.err) { const k = o.err.slice(0, 30); errs[k] = (errs[k] || 0) + 1; }
console.log("normMcp errors", v.normMcp.filter((x) => x.out.err).length, "/", v.normMcp.length, "script errs", errs);
