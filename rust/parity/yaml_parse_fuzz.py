#!/usr/bin/env python3
"""Differential: rust yamlfmt::parse vs node yaml@2 parse (JSON.stringify of result; 'ERR' if parse throws)."""
import json, random, subprocess, sys, os
sys.argv += ["1"]
random.seed(int(sys.argv[1]))
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
hand = ["", "a: 1", "a: yes", "a: no", "a: on", "a: 0o17", "a: 017", "a: 0x1F", "a: 1_000", "a: 2026-10-13", "a: 12:30", "a: .inf", "a: -.Inf",
        "a: .nan", "a: ~", "a: null", "a: Null", "a: NULL", "a: True", "a: tRue", "a: 1e3", "a: +4", "a: '1'", 'a: "x\\ty"', "a: |\n  x\n  y\n",
        "a: >\n  x\n  y\n", "# c\na: 1 # d\n", "a: &x 1\nb: *x\n", "a: {b: 1, c: [1, 2]}", "a: [1, 2", "a: 1\na: 2\n", "- 1\n- 2\n", "5", "a:\n  - b: 1\n    c: 2\n",
        "a: b: c", "\ta: 1", "a: !!str 1", "a: !!int '3'", "? a\n: 1\n", "1: x\n2: y\n", "true: x", "null: x", "a: 1.0", "a: -0", "a: 0.10",
        "a: 9007199254740993", "a: 1e400", "a: 'it''s'", "a: \"\\u00e9\"", "a:\n", "a: x\n---\nb: y\n", "a: 00", "a: 0b11", "a: +.5", "a: 1.", "a: .5e2",
        "adapters:\n  wecom:\n    enabled: true\n    token: BOT\n    extra:\n      secret: s\n      allow_from:\n        - u1\n"]
gen = subprocess.run(["python3", os.path.join(os.path.dirname(__file__), "yaml_fuzz.py"), sys.argv[1], "0"], capture_output=True, text=True)
docs = list(hand)
node_strs = subprocess.run(["node", "-e", """
const {stringify}=require('yaml');const out=[];
const atoms=['a','yes','0o17','1_000','12:30','x y','#c','- d','"q"',"'s'",'1e3','null','~',''];
function r(d){const k=Math.random();if(d>2||k<0.4)return atoms[Math.floor(Math.random()*atoms.length)];if(k<0.7){const o={};for(let i=0;i<3;i++)o[atoms[Math.floor(Math.random()*atoms.length)]]=r(d+1);return o}return [r(d+1),r(d+1)]}
for(let i=0;i<800;i++)console.log(JSON.stringify(stringify({k:r(0)},{lineWidth:0,singleQuote:false})))"""], capture_output=True, text=True, cwd=ROOT).stdout.split("\n")[:-1]
docs += [json.loads(s) for s in node_strs]
inp = "\n".join(json.dumps(d) for d in docs) + "\n"
node = subprocess.run(["node", "-e", """
const {parse}=require('yaml');const ls=require('fs').readFileSync(0,'utf8').split('\\n').filter(Boolean);
for(const l of ls){let r;try{const v=parse(JSON.parse(l));r='OK '+JSON.stringify(v===undefined?null:v)}catch(e){r='ERR'}console.log(JSON.stringify(r))}"""],
    input=inp, capture_output=True, text=True, cwd=ROOT).stdout.split("\n")[:-1]
rust = subprocess.run([os.path.join(ROOT, "rust/target/release/examples/yaml_parse_probe")], input=inp, capture_output=True, text=True).stdout.split("\n")[:-1]
bad = [(d, json.loads(n), json.loads(r)) for d, n, r in zip(docs, node, rust) if n != r]
print(f"{len(docs)} docs, {len(bad)} mismatches")
for d, n, r in bad[:40]:
    print(repr(d)[:120], "\n  node:", n[:200], "\n  rust:", r[:200])
