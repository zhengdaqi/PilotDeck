// Fixture of countTokens (js-tiktoken o200k_base, encode(text, [], [])) on random mixed-script text.
import { Tiktoken } from "js-tiktoken/lite";
import o200k from "js-tiktoken/ranks/o200k_base";
import fs from "node:fs";
const enc = new Tiktoken(o200k);
let seed = 7;
const rnd = (n) => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed % n; };
const atoms = ["hello", "World", "HTTPServer", " ", "  ", "   ", "\t", "\n", "\r\n", "\n\n", " \n ", "'s", "'LL", "'re", "don't", "123", "4567890",
  "3.14", "!", "?!", "...", "//", "/*", "->", "=>", "{", "}", "(", ")", "\"", "`", "_", "-", "Ünïcödé", "naïve", "中文字符", "日本語のテキスト",
  "한국어", "Привет", "مرحبا", "😀", "👍🏽", "🇨🇳", "\u00a0", "\u3000", "\u2028", "\ufeff", "\u0085", "x\u0301", "ǅ", "ʰ", "<|endoftext|>",
  "camelCaseIdent", "snake_case_ident", "SCREAMING", "e.g.", "a/b/c", "\\n", "#", "@user", "$x", "%", "^", "&&", "||", "~", "Ⅻ", "½", "²"];
const cases = [];
for (let i = 0; i < 4000; i++) {
  const n = 1 + rnd(14);
  let s = "";
  for (let j = 0; j < n; j++) s += atoms[rnd(atoms.length)];
  cases.push([s, enc.encode(s, [], []).length]);
}
const long = Array.from({ length: 400 }, (_, i) => atoms[(i * 7) % atoms.length]).join("");
cases.push([long, enc.encode(long, [], []).length]);
const fixturePath = new URL("../crates/agent-loop-sidecar/tests/fixtures/count_tokens.json", import.meta.url);
const fixtureText = JSON.stringify(cases);
if (process.argv.includes("--check")) {
  if (fs.readFileSync(fixturePath, "utf8") !== fixtureText) { console.error("count_tokens.json is stale"); process.exit(1); }
  console.log("count_tokens.json up to date");
} else fs.writeFileSync(fixturePath, fixtureText);
