// Oracle for src/network/fetch.ts networkFetch / networkFetchJson / jitteredBackoff (retry loop, Retry-After,
// jitter, method gating, error normalization). Scripted fetchImpl; Math.random from a recorded sequence;
// Date.now fixed; setTimeout records the delay and fires at once. Run with TZ=UTC.
// Usage: env TZ=UTC node --experimental-transform-types gen_network_fetch.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.join(root, "rust/crates/ui-server-core/tests/fixtures/network_fetch.json");
const net = await import(path.join(root, "src/network/fetch.ts"));

let seed = 3636;
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];
const NOW = Date.UTC(2026, 9, 13, 7, 0, 0);
Date.now = () => NOW;
let randoms = [], ri = 0;
Math.random = () => randoms[ri++ % randoms.length];
let log = [];
globalThis.setTimeout = (fn, ms) => { log.push(`sleep ${ms}`); queueMicrotask(fn); return 0; };
globalThis.clearTimeout = () => {};

const RETRY_AFTER = [null, "", " ", "0", "1", "2.5", " 3 ", "-1", "abc", "1e1", "0x10", "Infinity", "120",
  "Tue, 13 Oct 2026 07:00:05 GMT", "Tue, 13 Oct 2026 06:59:00 GMT", "Tue, 13 Oct 2026 07:10:00 GMT", "Wed, 14 Oct 2026 07:00:00 GMT"];
const ERRS = [{ message: "getaddrinfo ENOTFOUND h" }, { message: "x", code: "ECONNRESET" }, { message: "self signed certificate" }, { message: "This operation was aborted" },
  { message: "boom" }, { message: "fetch failed", cause: { code: "ETIMEDOUT" } }, { message: "proxy" }];
const STATUSES = [200, 201, 204, 400, 404, 408, 409, 425, 429, 500, 502, 503, 504, 418];
const STATUS_TEXT = { 200: "OK", 201: "Created", 204: "No Content", 400: "Bad Request", 404: "Not Found", 408: "Request Timeout", 409: "Conflict", 425: "Too Early", 429: "Too Many Requests", 500: "Internal Server Error", 502: "Bad Gateway", 503: "Service Unavailable", 504: "Gateway Timeout", 418: "I'm a teapot" };
const BODIES = ['{"a":1}', "", "not json", "x".repeat(600), "é".repeat(300) + "😀".repeat(200), "[1,2]", "null"];

const cases = [];
for (let i = 0; i < 1500; i++) {
  const retry = {};
  if (rnd(5)) retry.maxRetries = pick([0, 1, 2, 3, 1.5, -1, NaN]);
  if (rnd(2)) retry.baseDelayMs = pick([0, 1, 100, 500, 1000, 7]);
  if (rnd(2)) retry.maxDelayMs = pick([0, 50, 1000, 5000, 30000]);
  if (rnd(2)) retry.retryOnPost = pick([true, false]);
  if (rnd(5) === 0) retry.retryStatuses = pick([[], [418], [500, 503], [200]]);
  const method = pick([undefined, "GET", "get", "HEAD", "OPTIONS", "POST", "post", "PUT", "DELETE", "Patch"]);
  const script = Array.from({ length: 1 + rnd(5) }, () => rnd(4) === 0 ? { throw: pick(ERRS) } : { status: pick(STATUSES), retryAfter: pick(RETRY_AFTER), text: pick(BODIES) });
  const json = rnd(3) === 0;
  const expected = json && rnd(3) === 0 ? pick([[200], [404, 500], [], [418]]) : undefined;
  randoms = Array.from({ length: 8 }, () => rnd(1000000) / 1000000); ri = 0;
  log = [];
  let k = 0;
  const fetchImpl = async (input, init) => {
    log.push(`fetch ${init.method ?? "-"}`);
    const s = script[k++];
    if (!s) return new Response("", { status: 404, statusText: "Not Found" });
    if (s.throw) { const e = s.throw; throw Object.assign(new Error(e.message), e.code ? { code: e.code } : {}, e.cause ? { cause: e.cause } : {}); }
    const headers = s.retryAfter === null ? {} : { "retry-after": s.retryAfter };
    return new Response([204].includes(s.status) ? null : s.text, { status: s.status, statusText: STATUS_TEXT[s.status], headers });
  };
  const init = method === undefined ? {} : { method };
  let result;
  try {
    if (json) {
      const r = await net.networkFetchJson("https://h/x", init, { retry, fetchImpl, ...(expected ? { expectedStatuses: expected } : {}) });
      result = { ok: { status: r.response.status, json: r.json, text: r.text } };
    } else {
      const r = await net.networkFetch("https://h/x", init, { retry, fetchImpl });
      result = { ok: { status: r.status, text: await r.text() } };
    }
  } catch (e) {
    result = { err: { name: e.name, code: e.code, message: e.message } };
  }
  cases.push({ retry: JSON.parse(JSON.stringify(retry, (k, v) => (typeof v === "number" && !Number.isFinite(v) ? { $num: String(v) } : v))), method: method ?? null, json, expected: expected ?? null, script, randoms, log, result });
}
const backoff = [];
for (let i = 0; i < 400; i++) {
  const retry = {};
  if (rnd(2)) retry.baseDelayMs = pick([0, 1, 3, 100, 1000, 2500]);
  if (rnd(2)) retry.maxDelayMs = pick([0, 10, 1000, 30000, 100000]);
  const attempt = pick([0, 1, 2, 3, 5, 10, 40]);
  const header = pick([...RETRY_AFTER, undefined]);
  randoms = [rnd(1000000) / 1000000]; ri = 0;
  backoff.push({ retry, attempt, header: header ?? null, random: randoms[0], out: net.jitteredBackoff(attempt, retry, header) });
}
const codes = ["network_timeout", "network_dns_error", "network_connection_reset", "network_connection_refused", "network_tls_error", "network_proxy_error", "network_rate_limited", "network_server_error", "network_abort", "network_fetch_failed"].map((c) => [c, net.isRetryableNetworkCode(c)]);
const text = JSON.stringify({ now: NOW, cases, backoff, codes }) + "\n";
if (process.argv.includes("--check")) console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
else { fs.writeFileSync(out, text); console.log(`cases ${cases.length}, backoff ${backoff.length}`); }
