// Node 22 host check: load the wasm-bindgen (--target nodejs) output and compare with the TS SDK.
// Usage: node check.mjs <pkg-dir> <ws-url> <token>
import { createRequire } from "node:module";
import path from "node:path";
import * as ts from "../../../packages/sdk/dist/compat.js";
const require = createRequire(import.meta.url);
const [pkg, wsUrl, token] = process.argv.slice(2);
const w = require(path.resolve(pkg, "pilotdeck_sdk_wasm_probe.js"));
const out = {};
out.hookEvents = JSON.stringify(w.hookEvents()) === JSON.stringify([...ts.HOOK_EVENTS]);
const resolved = { config: { a: { b: [1, 2, { c: "x" }] }, n: null }, other: 1 };
const a = w.filterEscalatingDefaultMode(resolved), b = ts.filterEscalatingDefaultMode(resolved);
out.filterEscalatingDefaultMode = JSON.stringify(a) === JSON.stringify(b) && a !== resolved.config;
const hello = await w.gatewayHello(wsUrl, token);
out.gatewayHello = hello?.type === "hello_ok";
out.helloKeys = Object.keys(hello ?? {}).sort();
try { await w.gatewayHello(wsUrl, "wrong-token"); out.badTokenRejected = false; } catch { out.badTokenRejected = true; }
console.log(JSON.stringify(out));
process.exit(0);
