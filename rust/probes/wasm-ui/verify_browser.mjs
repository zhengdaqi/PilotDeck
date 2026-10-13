// Real-browser host check for the probe apps: headless Chrome via CDP (Node 22 global WebSocket).
// Usage: node verify_browser.mjs <dist-dir> [chrome-path]
import { spawn } from "node:child_process";
import http from "node:http";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const dist = path.resolve(process.argv[2] ?? "dist");
const chromePath = process.argv[3] ?? "/opt/google/chrome/chrome";
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm" };
const server = http.createServer((req, res) => {
  const p = path.join(dist, decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (!p.startsWith(dist) || !fs.existsSync(p) || fs.statSync(p).isDirectory()) { res.writeHead(404).end(); return; }
  res.writeHead(200, { "content-type": types[path.extname(p)] ?? "application/octet-stream" }).end(fs.readFileSync(p));
}).listen(0, "127.0.0.1");
await new Promise((r) => server.once("listening", r));
const port = server.address().port;
const profile = fs.mkdtempSync(path.join(os.tmpdir(), "probe-chrome-"));
const chrome = spawn(chromePath, ["--headless=new", "--no-sandbox", "--disable-gpu", `--user-data-dir=${profile}`, "--remote-debugging-port=0", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
const wsUrl = await new Promise((resolve, reject) => {
  let buf = "";
  chrome.stderr.on("data", (d) => { buf += d; const m = /DevTools listening on (ws:\/\/\S+)/.exec(buf); if (m) resolve(m[1]); });
  setTimeout(() => reject(new Error("chrome did not start")), 20000);
});
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = new WebSocket(wsUrl);
await new Promise((r) => (browser.onopen = r));
let id = 0; const pending = new Map();
browser.onmessage = (m) => { const v = JSON.parse(m.data); if (v.id && pending.has(v.id)) { pending.get(v.id)(v); pending.delete(v.id); } };
const send = (method, params = {}, sessionId) => new Promise((r) => { const i = ++id; pending.set(i, r); browser.send(JSON.stringify({ id: i, method, params, ...(sessionId ? { sessionId } : {}) })); });
const results = {};
for (const app of fs.readdirSync(dist).filter((d) => fs.existsSync(path.join(dist, d, "index.html")))) {
  const { result: { targetId } } = await send("Target.createTarget", { url: "about:blank" });
  const { result: { sessionId } } = await send("Target.attachToTarget", { targetId, flatten: true });
  await send("Page.enable", {}, sessionId);
  const t0 = Date.now();
  await send("Page.navigate", { url: `http://127.0.0.1:${port}/${app}/index.html` }, sessionId);
  const evalJs = async (expr) => (await send("Runtime.evaluate", { expression: expr, returnByValue: true }, sessionId)).result?.result?.value;
  let before = null;
  for (let i = 0; i < 100 && !before; i++) { await sleep(50); before = await evalJs("document.getElementById('groups')?.innerText || null"); }
  const mountMs = Date.now() - t0;
  const btnBefore = await evalJs("document.getElementById('toggle')?.innerText");
  await evalJs("document.getElementById('toggle').click()");
  await sleep(300);
  const after = await evalJs("document.getElementById('groups')?.innerText");
  const btnAfter = await evalJs("document.getElementById('toggle')?.innerText");
  results[app] = { mounted: !!before, mountMs, before, btnBefore, after, btnAfter };
  await send("Target.closeTarget", { targetId });
}
console.log(JSON.stringify(results, null, 2));
browser.close(); server.close();
const exited = new Promise((r) => chrome.once("exit", r));
chrome.kill();
await exited;
try { fs.rmSync(profile, { recursive: true, force: true }); } catch {}
process.exit(0);
