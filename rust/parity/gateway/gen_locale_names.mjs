// Oracle: String.prototype.localeCompare on file-name-like strings (Node ICU). Usage: node gen_locale_names.mjs [--check]
import fs from "node:fs";
let seed = 5;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t ^= t + Math.imul(t ^ (t >>> 7), 61 | t); return (((t ^ (t >>> 14)) >>> 0) % n); };
const alpha = [..."aAbBeEzZ019._- ", "é", "É", "è", "ê", "ë", "à", "ç", "ñ", "ö", "ü", "ß", "e\u0301", "中", "文", "日"];
const word = () => Array.from({ length: 1 + rnd(6) }, () => alpha[rnd(alpha.length)]).join("");
const cases = [];
for (let i = 0; i < 20000; i++) { const a = word(), b = rnd(4) ? word() : a.normalize("NFD"); cases.push([a, b, Math.sign(a.localeCompare(b))]); }
const p = new URL("../../crates/gateway-server/tests/fixtures/locale_names.json", import.meta.url);
const text = JSON.stringify(cases) + "\n";
if (process.argv.includes("--check")) { if (fs.readFileSync(p, "utf8") !== text) { console.error("drift"); process.exit(1); } console.log("ok"); }
else { fs.writeFileSync(p, text); console.log("wrote", cases.length); }
