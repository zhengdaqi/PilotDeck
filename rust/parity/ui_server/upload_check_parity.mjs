// HTTP parity: POST /api/projects/:projectName/files/upload/check served by the real Express handler
// (createWorkspaceUploadCheckHandler + the index.js express.json options) vs the Rust axum host.
// Both read the same randomized workspace trees (files, dirs, symlinks, dangling links, file-as-project).
// Usage: node --experimental-transform-types upload_check_parity.mjs --rust target/debug/pilotdeck-ui-server [--seed N] [--cases N]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const arg = (n, d) => { const i = process.argv.indexOf(n); return i < 0 ? d : process.argv[i + 1]; };
process.env.PILOTDECK_UPLOAD_MAX_FILES = "4";
const { register } = await import("node:module");
register("data:text/javascript," + encodeURIComponent(`
import fs from "node:fs"; import { fileURLToPath } from "node:url";
export async function resolve(spec, ctx, next) {
  try { return await next(spec, ctx); } catch (e) {
    if (!spec.endsWith(".js") || !ctx.parentURL) throw e;
    const u = new URL(spec.slice(0, -3) + ".ts", ctx.parentURL);
    if (!fs.existsSync(fileURLToPath(u))) throw e;
    return next(u.href, ctx);
  }
}`));
const express = createRequire(path.join(root, "ui/server/index.js"))("express");
const { createWorkspaceUploadCheckHandler } = await import(path.join(root, "ui/server/services/workspaceFileUpload.js"));

let seed = Number(arg("--seed", "31"));
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];

const base = fs.mkdtempSync(path.join(os.tmpdir(), "upload-check-"));
const app = express();
app.use(express.json({ limit: "50mb", type: (req) => { const ct = req.headers["content-type"] || ""; if (ct.includes("multipart/form-data")) return false; return ct.includes("json"); } }));
app.post("/api/projects/:projectName/files/upload/check", createWorkspaceUploadCheckHandler({ resolveProject: async (name) => path.join(base, name) }));
const server = await new Promise((r) => { const s = app.listen(0, "127.0.0.1", () => r(s)); });
const tsUrl = `http://127.0.0.1:${server.address().port}`;

const rust = spawn(arg("--rust"), ["--listen", "127.0.0.1:0", "--projects-base", base], { env: { ...process.env }, stdio: ["ignore", "pipe", "inherit"] });
const rustUrl = await new Promise((res, rej) => {
  let buf = "";
  rust.stdout.on("data", (d) => { buf += d; const m = buf.match(/listening (\S+)/); if (m) res(`http://${m[1]}`); });
  rust.on("exit", (c) => rej(new Error(`rust exited ${c}`)));
});

const NAMES = ["a.txt", "b", "dir", "dir/a.txt", "dir/sub/c", "link", "link/a.txt", "dangling", "f", "f/x", "new.txt", "deep/er/x"];
function makeTree() {
  fs.rmSync(path.join(base, "proj"), { recursive: true, force: true });
  fs.rmSync(path.join(base, "file"), { force: true });
  fs.rmSync(path.join(base, "plink"), { force: true });
  const p = path.join(base, "proj");
  fs.mkdirSync(p);
  if (rnd(2)) fs.writeFileSync(path.join(p, "a.txt"), "x");
  if (rnd(2)) fs.mkdirSync(path.join(p, "dir"));
  if (fs.existsSync(path.join(p, "dir")) && rnd(2)) fs.writeFileSync(path.join(p, "dir/a.txt"), "y");
  if (fs.existsSync(path.join(p, "dir")) && rnd(2)) fs.mkdirSync(path.join(p, "dir/sub"));
  if (rnd(2)) fs.symlinkSync(rnd(2) ? path.join(p, "dir") : "/tmp", path.join(p, "link"));
  if (rnd(2)) fs.symlinkSync(path.join(p, "nowhere"), path.join(p, "dangling"));
  if (rnd(2)) fs.writeFileSync(path.join(p, "f"), "z");
  fs.writeFileSync(path.join(base, "file"), "not a dir");
  fs.symlinkSync(p, path.join(base, "plink"));
}
const relPath = () => pick([...NAMES, ...NAMES, "", ".", "..", "../x", "/abs", "C:x", "a\\b.txt", "dir\\a.txt", "a//b", "./a", "a/./b", "x\u0000y", 5, null, ["a"], "\u00e9.txt", "dir/"]);
function body() {
  const b = {};
  if (rnd(4)) b.relativePaths = rnd(8) ? Array.from({ length: rnd(6) }, relPath) : pick(["a.txt", 3, null, {}]);
  if (rnd(2)) b.targetPath = pick(["", ".", "dir", "dir/sub", "link", "f", "new", "../x", "/x", 0, 5, false, "dir\\sub", "nowhere/deeper"]);
  return rnd(15) ? b : pick([[], ["a"], {}]);
}

async function hit(url, req) {
  const r = await fetch(`${url}/api/projects/${encodeURIComponent(req.project)}/files/upload/check`, {
    method: "POST", headers: req.ct === null ? {} : { "content-type": req.ct }, body: req.raw,
  });
  const text = await r.text();
  return { status: r.status, ct: r.headers.get("content-type"), body: text.split(base).join("<base>") };
}

const cases = Number(arg("--cases", "1500"));
let same = 0;
const statuses = {};
const diffs = [];
for (let i = 0; i < cases; i++) {
  if (i % 25 === 0) makeTree();
  const req = { project: pick(["proj", "proj", "proj", "plink", "file", "missing", "proj/dir"]), ct: pick(["application/json", "application/json", "application/json; charset=utf-8", "text/plain", null]), raw: JSON.stringify(body()) };
  const [a, b] = [await hit(tsUrl, req), await hit(rustUrl, req)];
  const k = `${a.status} ${(a.body.match(/"code":"([A-Z_]+)"/) || [, a.body.slice(0, 20)])[1]}`;
  statuses[k] = (statuses[k] || 0) + 1;
  if (JSON.stringify(a) === JSON.stringify(b)) same++;
  else diffs.push({ req, ts: a, rust: b });
}
console.log(`${same}/${cases} responses identical (status, content-type, body)`);
for (const d of diffs.slice(0, 8)) console.log(JSON.stringify(d));
console.log("coverage:", JSON.stringify(statuses));
rust.kill();
server.close();
fs.rmSync(base, { recursive: true, force: true });
process.exit(diffs.length ? 1 : 0);
