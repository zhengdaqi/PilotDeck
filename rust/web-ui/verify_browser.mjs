// Real-browser host check for the Leptos web UI (runs shared vectors in Chrome): headless Chrome via CDP (Node 22 global WebSocket).
// Usage: node verify_browser.mjs [dist-dir] [chrome-path]
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
const { result: { targetId } } = await send("Target.createTarget", { url: "about:blank" });
const { result: { sessionId } } = await send("Target.attachToTarget", { targetId, flatten: true });
await send("Page.enable", {}, sessionId);
await send("Page.navigate", { url: `http://127.0.0.1:${port}/index.html` }, sessionId);
const evalJs = async (expr) => (await send("Runtime.evaluate", { expression: expr, returnByValue: true }, sessionId)).result?.result?.value;
let st = null;
for (let i = 0; i < 200 && !st; i++) { await sleep(50); st = await evalJs("document.getElementById('selftest')?.innerText || null"); }
const results = { userAgent: await evalJs("navigator.userAgent"), selftest: st && JSON.parse(st), detail: await evalJs("document.getElementById('selftest-detail')?.innerText") };
await evalJs("document.getElementById('mention').click()"); await sleep(100);
results.afterClick1 = await evalJs("JSON.stringify(document.getElementById('composer').value)");
await evalJs("document.getElementById('mention').click()"); await sleep(100);
results.afterClick2 = await evalJs("JSON.stringify(document.getElementById('composer').value)");
results.status = await evalJs("document.getElementById('status').innerText");
const key = (opts) => evalJs(`(()=>{const t=document.getElementById('ime-composer');t.value=${JSON.stringify(opts.value)};t.dispatchEvent(new KeyboardEvent('keydown',${JSON.stringify({ bubbles: true, cancelable: true, ...opts.init })}));})()`);
await key({ value: "hello", init: { key: "Enter" } });
await key({ value: "composing", init: { key: "Enter", isComposing: true } });
await key({ value: "shiftless a", init: { key: "a" } });
await sleep(100);
results.sent = await evalJs("[...document.querySelectorAll('#sent li')].map(x=>x.innerText)");
results.timeAgo = await evalJs("document.getElementById('time-ago').innerText");
await evalJs(`(()=>{const i=document.getElementById('office-service');i.value=' LibreOffice ';i.dispatchEvent(new Event('input',{bubbles:true}));})()`);
await sleep(100);
results.officeNormalized = await evalJs("document.getElementById('office-normalized').innerText");
console.log(JSON.stringify(results, null, 2));
const ok = results.selftest?.checked > 0 && results.selftest.mismatches === 0 && results.status === "already present"
  && JSON.stringify(results.sent) === JSON.stringify(["hello"]) && results.timeAgo === "3 hours ago" && results.officeNormalized === "libreoffice";
browser.close(); server.close();
const exited = new Promise((r) => chrome.once("exit", r));
chrome.kill();
await exited;
try { fs.rmSync(profile, { recursive: true, force: true }); } catch {}
process.exit(ok ? 0 : 1);
