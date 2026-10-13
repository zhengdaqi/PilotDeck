// UI batch 2 vectors from the real TS: officePreviewStatus (pure fns), ime, externalFileDrop (pure fns), dateUtils.
// Type-only imports that strip-types cannot erase (`import { TFunction } from 'i18next'`) and the React hook import
// are removed from temporary copies; everything else runs as written.
// Usage: node --experimental-strip-types rust/parity/web_ui/gen_batch2.mjs [--check]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const u = path.join(root, "ui/src/utils");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "pd-batch2-"));
const copy = (name, edit = (s) => s) => { fs.writeFileSync(path.join(tmp, name), edit(fs.readFileSync(path.join(u, name), "utf8"))); };
copy("workspaceFileMention.ts");
copy("ime.ts");
copy("dateUtils.ts", (s) => s.replace("import { TFunction } from 'i18next';", "type TFunction = any;"));
copy("externalFileDrop.ts", (s) => s.replace("import { useEffect } from 'react';", "const useEffect = () => {};").replace("from './workspaceFileMention'", "from './workspaceFileMention.ts'"));
copy("officePreviewStatus.ts", (s) => s.replace("import { api } from './api';", "const api = {} as any;"));
const imp = (n) => import(pathToFileURL(path.join(tmp, n)).href);
const office = await imp("officePreviewStatus.ts"), ime = await imp("ime.ts"), drop = await imp("externalFileDrop.ts"), date = await imp("dateUtils.ts");
fs.rmSync(tmp, { recursive: true, force: true });

let seed = 77;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const v = { office: [], officeConfig: [], ime: [], drag: [], dropPath: [], timeAgo: [] };
const vals = [undefined, null, "", "libreoffice", " LibreOffice ", "LIBREOFFICE\n", "builtin", "libre office", 0, 1, 1.5, -2, true, false, ["libreoffice"], [" libreoffice", null], [["LibreOffice"]], {}, { a: 1 }, "/usr/bin/soffice", "  C:\\Program Files\\soffice.exe\t", "a\u0001b\"\\", "\u00a0x\u3000", "é\u2028"];
for (let i = 0; i < 1500; i++) {
  const s = pick(vals), b = pick(vals);
  v.office.push({ s: s ?? null, sUndef: s === undefined, b: b ?? null, bUndef: b === undefined, norm: office.normalizeOfficePreviewService(s), sig: office.officePreviewRendererSignature(s, b) });
}
const cfgs = [null, 1, "x", [], {}, { webui: null }, { webui: [] }, { webui: {} }, { webui: { officePreview: [] } }, { webui: { officePreview: {} } }];
for (let i = 0; i < 300; i++) {
  const c = rnd(3) ? { webui: { officePreview: { ...(rnd(2) ? { service: pick(vals) } : {}), ...(rnd(2) ? { binaryPath: pick(vals) } : {}) } } } : pick(cfgs);
  const cc = JSON.parse(JSON.stringify(c ?? null));
  v.officeConfig.push({ c: cc, out: office.officePreviewRendererSignatureFromConfig(cc) });
}
const evVals = [undefined, null, 229, "229", 13, 0, true, false, 1, "", "x"];
for (let i = 0; i < 1500; i++) {
  const e = {};
  for (const k of ["key", "keyCode", "which", "isComposing"]) if (rnd(2)) e[k] = k === "key" ? pick(["Enter", "enter", "a", null, 13]) : pick(evVals);
  if (rnd(2)) e.nativeEvent = rnd(6) ? Object.fromEntries(["keyCode", "which", "isComposing"].filter(() => rnd(2)).map((k) => [k, pick(evVals)])) : pick([null, 0, "s", true]);
  const ej = JSON.parse(JSON.stringify(e));
  v.ime.push({ e: ej, comp: ime.isImeCompositionEvent(ej), enter: ime.isImeEnterEvent(ej) });
}
const typesVals = [["Files"], ["text/plain"], ["text/plain", "Files"], [], "Files", null, 0, 5, true, { length: 1, 0: "Files" }, { length: 2, 1: "Files" }, { length: "1", 0: "Files" }, { length: 1.9, 0: "Files" }, { 0: "Files" }, { length: -1, 0: "Files" }, ["files"]];
for (const dt of [undefined, null, {}, ...typesVals.map((t) => ({ types: t }))]) {
  const dj = dt === undefined ? undefined : JSON.parse(JSON.stringify(dt));
  v.drag.push({ dt: dj ?? null, dtUndef: dj === undefined, out: drop.isExternalFileDrag({ dataTransfer: dj }) });
}
const roots = ["/repo", "/repo/", "C:/Work", "c:\\work", "/", ""];
const paths = ["/repo/src/a.ts", "/repo/src", "/repo", "/repo/a.ts", "/other/x", "C:/Work/a/b.txt", "c:\\work\\x\\y", "src/a.ts", "", "/repo/src/deep/x/y.md"];
for (let i = 0; i < 1000; i++) {
  const p = pick([...paths, null]), t = pick(["file", "directory", null]), r = pick(roots);
  v.dropPath.push({ p, t, r, out: drop.resolveExternalFileDropTargetPath(p, t, r) });
}
const now = new Date("2026-10-13T06:00:00.000Z");
const ds = ["2026-10-13T06:00:00.000Z", "2026-10-13T05:59:01Z", "2026-10-13T05:59:00Z", "2026-10-13T05:58:00Z", "2026-10-13T05:58:59.999Z", "2026-10-13T05:00:00Z", "2026-10-13T05:00:00.001Z",
  "2026-10-13T04:00:00Z", "2026-10-12T06:00:00Z", "2026-10-12T06:00:00.001Z", "2026-10-11T06:00:00Z", "2026-10-06T06:00:01Z", "2026-10-06T06:00:00Z", "2025-01-01", "2026-10-14T00:00:00Z",
  "not a date", "", "2026-13-01", "2026-10-13T05:30:00+08:00", "2026-10-13"];
for (let i = 0; i < 400; i++) {
  const s = rnd(3) ? pick(ds) : new Date(now.getTime() - rnd(20 * 86400) * 1000 - rnd(1000)).toISOString();
  const parsed = Date.parse(s);
  const keyed = date.formatTimeAgo(s, now, (k, o) => JSON.stringify([k, o?.count ?? null]));
  const plain = date.formatTimeAgo(s, now, null);
  const locale = Number.isNaN(parsed) ? null : new Date(parsed).toLocaleDateString();
  const isLocale = keyed === locale && !keyed.startsWith("[");
  v.timeAgo.push({ s, parsed: Number.isNaN(parsed) ? null : parsed, now: now.getTime(), out: isLocale ? { localeDate: parsed } : JSON.parse(keyed), plain: isLocale ? { localeDate: parsed } : plain });
}
const text = JSON.stringify(v) + "\n";
const target = path.join(root, "rust/crates/web-ui-core/tests/fixtures/batch2.json");
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "vectors stable" : "vectors DIFFER"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
console.log("wrote", Object.fromEntries(Object.entries(v).map(([k, a]) => [k, a.length])));
