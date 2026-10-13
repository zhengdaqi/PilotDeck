// Oracle for ui/server/services/releaseService.js: normalizeRepository, releaseVersion, compareVersions and
// getLatestRelease (scripted fetchImpl; manifests mutated field by field from a valid base).
// Usage: node gen_release_service.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.join(root, "rust/crates/ui-server-core/tests/fixtures/release_service.json");
const rs = await import(path.join(root, "ui/server/services/releaseService.js"));

let seed = 3838;
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];
const U = { $undef: 1 };
const enc = (v) => (v === undefined ? U : v);
const call = (fn) => { try { return { v: fn() }; } catch (e) { return { err: e.name, message: e.message }; } };

const repos = [undefined, null, 5, "", " OpenBMB/PilotDeck ", "https://github.com/a/b", "https://github.com/a/b.git", "https://github.com/a/b.git/", "https://github.com/a/b/", "a/b/", "a/b.git",
  "HTTPS://github.com/a/b", "http://github.com/a/b", "a/b/c", "a", "./b", "../b", "a/..", "...a/b", "a-b.c_d/e.f-g", "a/b\n", "a/ b", "é/b", "a/b.git.git", "a/.git", "https://github.com/https://github.com/a/b", "\ta/b\u00a0", "a//b", "a/b//"];
const normalize = repos.map((r) => ({ r: enc(r), out: call(() => rs.normalizeRepository(r)) }));
const tags = [undefined, null, "", "v2026.10.13", "v2026.10.13-r1", "v2026.10.13-r2", "v2026.10.13-r0", "v2026.10.13-r01", "v2026.02.29", "v2024.02.29", "v2026.02.28", "v2026.13.01", "v2026.00.10", "v2026.04.31",
  "v2026.04.30", "v2026.12.31", "v2026.01.00", "v0000.02.29", "v0004.02.29", "v0099.02.29", "v0100.02.29", "v2000.02.29", "v1900.02.29", "v2026.10.13-r9007199254740991", "v2026.10.13-r9007199254740992", "v2026.10.13-r99999999999999999999",
  "2026.10.13", "v2026.10.13 ", "v2026.1.13", "v2026.10.13-r", "V2026.10.13", 20261013, ["v2026.10.13"]];
const versions = tags.map((t) => ({ t: enc(t), out: call(() => rs.releaseVersion(t)) }));
const VS = [undefined, null, "", "1.2.3", "1.2.4", "1.10.0", "2026.1013.0", "2026.1013.1", "0.0.0", "01.2.3", "1.2", "1.2.3.4", "1.2.x", " 1.2.3", "9007199254740991.0.0", "9007199254740992.0.0", "99999999999999999999.0.0", 5];
const compares = [];
for (let i = 0; i < 400; i++) { const a = pick(VS), b = pick(VS); compares.push({ a: enc(a), b: enc(b), out: call(() => rs.compareVersions(a, b)) }); }

const sha40 = "0123456789abcdef0123456789abcdef01234567";
const sha256 = "A".repeat(10) + "b".repeat(54);
const sha512 = "Ab+/".repeat(21) + "xy==";
function baseManifest(repo) {
  return { schemaVersion: 1, repository: repo, tag: "v2026.10.13-r2", version: "2026.1013.1", date: "2026-10-13", sourceSha: sha40,
    assets: [{ name: "PilotDeck-2026.10.13.dmg", sha256, sha512, size: 1234, platform: "darwin", arch: "arm64" }, { name: "PilotDeck-setup-x64.exe", sha256: sha256.toLowerCase(), sha512, size: 99, platform: "win32", arch: "x64", extra: true }] };
}
const MUT = [
  (m) => { m.schemaVersion = pick([2, "1", undefined]); }, (m) => { m.repository = pick(["other/repo", undefined, "OpenBMB/pilotdeck"]); },
  (m) => { m.tag = pick(["v2026.10.13", "v2026.02.30", "bad", undefined, 5, ["v2026.10.13-r2"]]); }, (m) => { m.version = pick(["2026.1013.0", "2026.1013.2", undefined]); },
  (m) => { m.date = pick(["2026-10-14", "2026.10.13", undefined]); }, (m) => { m.sourceSha = pick([sha40.toUpperCase(), sha40.slice(1), undefined, sha40 + "\n"]); },
  (m) => { m.assets = pick([[], {}, undefined, "x", null]); },
  (m) => { m.assets.push(null); }, (m) => { m.assets.push({ ...m.assets[0] }); }, (m) => { m.assets[1].name = m.assets[0].name; },
  (m) => { m.assets[0].name = pick(["..", ".", "...", "a b", "Pilot Deck setup (x64).exe", "a/b", "", undefined, 5, "ok.dmg", "%.zip", "é.zip"]); },
  (m) => { m.assets[0].sha256 = pick(["A".repeat(64), "g".repeat(64), "a".repeat(63), undefined, "a".repeat(65)]); },
  (m) => { m.assets[0].size = pick([0, -1, 1.5, "12", 9007199254740991, 9007199254740992, undefined, null]); },
  (m) => { m.assets[0].sha512 = pick(["a".repeat(86) + "==", "a".repeat(87) + "==", "a".repeat(85) + "===", "a".repeat(86) + "=", "-".repeat(86) + "==", undefined]); },
  (m) => { m.assets[0].platform = pick([5, undefined, ""]); }, (m) => { m.assets[0].arch = pick([null, "", "x"]); },
  (m) => { m.assets[0].downloadUrl = "stale"; }, (m) => { delete m.assets[1].extra; m.assets[1].sha256 = "F".repeat(64); },
];
const releases = [];
for (let i = 0; i < 1500; i++) {
  const repository = rnd(3) ? undefined : pick(["https://github.com/a/b.git", "bad repo", "x/y"]);
  const repo = (() => { try { return rs.normalizeRepository(repository); } catch { return "OpenBMB/PilotDeck"; } })();
  const m = baseManifest(repo);
  const n = rnd(4) === 0 ? 0 : 1 + rnd(2);
  for (let k = 0; k < n; k++) { try { pick(MUT)(m); } catch {} }
  const response = rnd(10) === 0 ? pick([{ status: 404, text: "" }, { status: 500, text: "{}" }, { status: 200, text: "not json" }, { status: 200, text: "null" }, { status: 302, text: "" }]) : { status: 200, text: JSON.stringify(m) };
  const log = [];
  const fetchImpl = async (url, init) => { log.push(`${url} ${JSON.stringify(init.headers)}`); return new Response(response.text, { status: response.status }); };
  let result;
  try { result = { v: await rs.getLatestRelease({ ...(repository === undefined ? {} : { repository }), fetchImpl }) }; }
  catch (e) { result = { err: e.name, message: e.name === "SyntaxError" ? null : e.message }; }
  releases.push({ repository: enc(repository), response, log, result });
}
const text = JSON.stringify({ normalize, versions, compares, releases }) + "\n";
if (process.argv.includes("--check")) console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
else { fs.writeFileSync(out, text); console.log(`normalize ${normalize.length}, versions ${versions.length}, compares ${compares.length}, releases ${releases.length}`); }
