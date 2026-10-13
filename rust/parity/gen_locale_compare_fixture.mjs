// Generates a fixture of String.prototype.localeCompare results (node's ICU, default locale)
// for random ASCII identifier-like pairs. Used by tests/locale_compare.rs.
import fs from "node:fs";
let seed = 12345;
const rnd = (n) => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed % n; };
const alphabets = ["abcdefghijklmnopqrstuvwxyz_", "abcXYZ_-.019", " !\"#$%&'()*+,-./0123456789:;<=>?@ABCZ[\\]^_`abz{|}~"];
const pairs = [];
for (let i = 0; i < 20000; i++) {
  const al = alphabets[i % alphabets.length];
  const mk = () => Array.from({ length: 1 + rnd(8) }, () => al[rnd(al.length)]).join("");
  let a = mk(), b = i % 4 === 0 ? a.slice(0, rnd(a.length + 1)) + mk() : mk();
  if (i % 7 === 0) b = a.toUpperCase();
  pairs.push([a, b, Math.sign(a.localeCompare(b))]);
}
const fixturePath = new URL("../crates/agent-loop-sidecar/tests/fixtures/locale_compare.json", import.meta.url);
const fixtureText = JSON.stringify(pairs);
if (process.argv.includes("--check")) {
  if (fs.readFileSync(fixturePath, "utf8") !== fixtureText) { console.error("locale_compare.json is stale"); process.exit(1); }
  console.log("locale_compare.json up to date");
} else fs.writeFileSync(fixturePath, fixtureText);
