#!/usr/bin/env python3
# Differential fuzz: rust v8json::parse vs Node JSON.parse (messages + values). Build first:
#   cargo build --release --example v8json_probe   (run from rust/)
import json, random, subprocess, sys
random.seed(int(sys.argv[1]) if len(sys.argv)>1 else 1)
base = ['', ' ', 'xyz', '{"a":', '[1]', '{} x', '{"x":1}', '{a:1}', 'tru', 'trux', 'nul', '-', '-a', '01', '1.', '1.e', '1e', '1e+', '"abc', '"a\\x"', '"\\u12g4"', '"\\u12', '"a\nb"', '[1,]', '[1 2]', '{"a" 1}', '{"a":1,}', '{"a":1 "b":2}', '{"a":1,"b":}', '[', '{', '{"a":1}garbage-here-and-more', 'abcdefghijklmnopqrstuvwxyz', '{"aaaaaaaaaaaaaaaaaaaaaaaa": x}', '{"k": [1, 2, 3, {"z": nul}]}', '\n\n {\n "a": 1,\n x}', '{"2":1,"1":2,"b":3,"a":4}', '{"a":1,"a":2,"b":0}', '1e400', '-0', '0.1e-5', '123456789012345678901234567890', '"\\ud83d\\ude00"', '"é中"', '"é中" x', '{"é":1 x}', '[true,false,null,-1.5E3]', '"\\/\\b\\f\\n\\r\\t"', ' \t\r\n[] ', '[,1]', '{,}', '"\u0001"', '+1', '.5', 'NaN', 'Infinity', "'a'", '{"a":"b"}}', '[[[[[[[[[[[[[[[[[[[[[[[x]]]]]]]]]]]]]]]]]]]]', 'nullx', 'true false']
chars = list('{}[]":,0123456789-+.eEtrufalsn \n\\xu"ab')
corpus = list(base)
for _ in range(3000):
    s = random.choice(base)
    s = list(s)
    for _ in range(random.randint(1,4)):
        op = random.random()
        if op < 0.4 and s: s.pop(random.randrange(len(s)))
        elif op < 0.8: s.insert(random.randint(0,len(s)), random.choice(chars))
        elif s: s[random.randrange(len(s))] = random.choice(chars)
    corpus.append(''.join(s))
inp = '\n'.join(json.dumps(c) for c in corpus) + '\n'
node = subprocess.run(['node','-e','''
const lines=require("fs").readFileSync(0,"utf8").split("\\n").filter(Boolean);
for(const l of lines){const s=JSON.parse(l);let r;try{r="OK "+JSON.stringify(JSON.parse(s))}catch(e){r="ERR "+e.message}console.log(JSON.stringify(r))}'''], input=inp, capture_output=True, text=True).stdout.splitlines()
rust = subprocess.run(['/workspace/PilotDeck/rust/target/release/examples/v8json_probe'], input=inp, capture_output=True, text=True).stdout.splitlines()
bad = [(c, json.loads(n), json.loads(r)) for c, n, r in zip(corpus, node, rust) if n != r]
print(f"{len(corpus)} cases, {len(bad)} mismatches")
for c, n, r in bad[:25]: print(repr(c), '\n  node:', n, '\n  rust:', r)
