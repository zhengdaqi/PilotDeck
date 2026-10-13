#!/usr/bin/env python3
"""Differential: rust yamlfmt::stringify vs node yaml@2 stringify(v,{lineWidth:0,singleQuote:false}).
Build first: cargo build --release -p pilotdeck-setup --example yaml_probe (from rust/)."""
import json, random, subprocess, sys, os
random.seed(int(sys.argv[1]) if len(sys.argv) > 1 else 1)
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
atoms = ["", "a", "abc def", "true", "True", "TRUE", "yes", "no", "null", "~", "Null", "0", "12", "-3", "+4", "1.5", ".5", "1e3", "0x1F", "0o17", "0b1",
         ".inf", "-.Inf", ".nan", "a: b", "a:b", "a #b", "a#b", "#x", "- x", "-x", "-", "?", "? x", "[x]", "{x}", "x,y", "&a", "*a", "!t", "|x", ">x",
         "'q'", '"d"', "it's", 'say "hi"', "%p", "@x", "`x", " lead", "trail ", "trail:", "multi\nline", "multi\nline\n", "multi\n\n", " sp\nx",
         "tab\tx", "\ttab", "ctl\x01", "bell\x07", "esc\x1b", "nbsp\xa0", "中文", "emoji😀", "url://x:1/p?q", "C:\\path", "---", "...", "--- x",
         "wss://openws.work.weixin.qq.com", "BOT123456789", "x\r\ny", "a\u2028b", "2026-10-13", "12:30", "1_000", "0.0", "-0", "007"]
def rnd(d=0):
    r = random.random()
    if d > 3 or r < 0.5:
        c = random.random()
        if c < 0.6: return random.choice(atoms)
        if c < 0.7: return random.choice([0, 1, -1, 1.5, 1e21, 2**53, 0.1, True, False, None])
        if c < 0.8: return random.choice(atoms) + random.choice(atoms)
        return random.choice([{}, []])
    if r < 0.75:
        return {random.choice(atoms + ["k", "key", "adapters", "1", "2"]): rnd(d + 1) for _ in range(random.randint(1, 4))}
    return [rnd(d + 1) for _ in range(random.randint(1, 3))]
corpus = [{"adapters": {"wecom": {"enabled": True, "token": "B", "extra": {"secret": "s", "allow_from": ["u1", "u2"], "groups": {}}}}}]
corpus += [{"k": rnd()} for _ in range(1500)] + [rnd() for _ in range(500)]
corpus = [c for c in corpus if isinstance(c, dict)]
inp = "\n".join(json.dumps(c) for c in corpus) + "\n"
node = subprocess.run(["node", "-e", """
const {stringify}=require('yaml');const ls=require('fs').readFileSync(0,'utf8').split('\\n').filter(Boolean);
for(const l of ls){console.log(JSON.stringify(stringify(JSON.parse(l),{lineWidth:0,singleQuote:false})))}"""],
    input=inp, capture_output=True, text=True, cwd=ROOT).stdout.split("\n")[:-1]
rust = subprocess.run([os.path.join(ROOT, "rust/target/release/examples/yaml_probe")], input=inp, capture_output=True, text=True).stdout.split("\n")[:-1]
bad = [(c, json.loads(n), json.loads(r)) for c, n, r in zip(corpus, node, rust) if n != r]
print(f"{len(corpus)} cases, {len(bad)} mismatches")
for c, n, r in bad[:int(sys.argv[2]) if len(sys.argv) > 2 else 12]:
    print(json.dumps(c, ensure_ascii=False)[:200], "\n--node--\n" + n + "--rust--\n" + r)
