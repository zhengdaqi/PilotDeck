#!/usr/bin/env python3
"""Paced-stdin parity for `pilotdeck gateway setup` (TS oracle vs Rust)."""
import argparse, os, subprocess, tempfile, time, json, re
def run(cmd, home, lines, extra_env, files):
    os.makedirs(os.path.join(home, ".pilotdeck"), exist_ok=True)
    for rel, txt in files.items():
        p = os.path.join(home, rel); os.makedirs(os.path.dirname(p), exist_ok=True); open(p, "w").write(txt)
    env = dict(os.environ, HOME=home, PILOTDECK_PROXY="http://127.0.0.1:9", NO_COLOR="1", **extra_env)
    p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, cwd=home)
    for l in lines:
        time.sleep(0.6)
        try: p.stdin.write(l.encode()); p.stdin.flush()
        except BrokenPipeError: break
    time.sleep(0.6)
    try: p.stdin.close()
    except BrokenPipeError: pass
    try: p.wait(timeout=40)
    except subprocess.TimeoutExpired: p.kill(); p.wait(); return ("TIMEOUT",)
    out, err = p.stdout.read(), p.stderr.read()
    state = {}
    for root, _, fs in os.walk(os.path.join(home, ".pilotdeck")):
        for f in fs:
            fp = os.path.join(root, f); state[os.path.relpath(fp, home)] = open(fp, errors="replace").read()
    norm = lambda s: re.sub(r"\d{4}-\d\d-\d\dT[\d:.]+Z", "<TS>", s.decode(errors="replace"))
    # Contract normalization (same as chat_search_parity): drop Node runtime ExperimentalWarning banner.
    err = re.sub(rb"^\(node:\d+\) ExperimentalWarning: [^\n]*\n(\(Use `node --trace-warnings \.\.\.` to show where the warning was created\)\n)?", b"", err, flags=re.M)
    return (p.returncode, norm(out), norm(err), state)
YAML_EXIST = "adapters:\n  wecom:\n    enabled: true\n    token: bot-1\n    extra:\n      secret: s\n  feishu:\n    enabled: false\n    extra:\n      app_id: '017'\nother: [1, \"a: b\", null]\n"
CASES = [
 ("quit", [], ["q\n"], {}),
 ("eof-menu", [], [], {}),
 ("invalid-choice", [], ["9\n", "q\n"], {}),
 ("wecom-manual-open", ["wecom"], ["2\n", "botid\n", "secret\n", "\n", "1\n", "1\n"], {}),
 ("wecom-manual-allowlist", ["wecom"], ["2\n", "botid\n", "secret\n", "wss://x/y\n", "2\n", "u1, u2\n", "2\n", "g1\n"], {}),
 ("wecom-qr-deadproxy", ["wecom"], ["1\n"], {}),
 ("wecom-existing-keep", ["wecom"], ["n\n"], {".pilotdeck/pilotdeck.yaml": YAML_EXIST}),
 ("wecom-existing-redo", ["wecom"], ["y\n", "2\n", "b2\n", "s2\n", "\n", "1\n", "1\n"], {".pilotdeck/pilotdeck.yaml": YAML_EXIST}),
 ("feishu-deadproxy", ["feishu"], ["cli_a\n", "sec\n", "\n"], {}),
 ("feishu-eof-mid", ["feishu"], ["cli_a\n"], {}),
 ("weixin-deadproxy", ["weixin"], ["\n", "\n"], {}),
 ("menu-all-eof", [], ["4\n", "\n"], {}),
 ("bad-yaml-root", ["wecom"], ["2\n", "b\n", "s\n", "\n", "1\n", "1\n"], {".pilotdeck/pilotdeck.yaml": "- a\n- b\n"}),
 ("adapters-is-seq", ["wecom"], ["2\n", "b\n", "s\n", "\n", "1\n", "1\n"], {".pilotdeck/pilotdeck.yaml": "adapters:\n  - x\nk: 1\n"}),
 ("scalar-root", ["wecom"], ["2\n", "b\n", "s\n", "\n", "1\n", "1\n"], {".pilotdeck/pilotdeck.yaml": "hello\n"}),
 ("weixin-existing-creds", ["weixin"], ["n\n"], {".pilotdeck/weixin-credentials.json": "{\"token\":\"t\"}"}),
 ("feishu-existing", ["feishu"], ["lark\n", "\n", "\n"], {".pilotdeck/pilotdeck.yaml": YAML_EXIST.replace("app_id: '017'", "appId: cli_x\n    appSecret: zz")}),
 ("unknown-platform", ["slack"], ["q\n"], {}),
]
ap = argparse.ArgumentParser(); ap.add_argument("--ts-cli", required=True); ap.add_argument("--rust-bin", required=True)
ap.add_argument("--node", default="node"); ap.add_argument("--report", required=True); a = ap.parse_args()
res = []; ok = 0
for name, args, lines, files in CASES:
    t = run([a.node, a.ts_cli, "gateway", "setup"] + args, tempfile.mkdtemp(), lines, {}, files)
    r = run([a.rust_bin, "gateway", "setup"] + args, tempfile.mkdtemp(), lines, {}, files)
    same = t == r; ok += same
    res.append({"case": name, "match": same, **({} if same else {"ts": t, "rust": r})})
    print(("PASS " if same else "FAIL ") + name, flush=True)
json.dump({"passed": ok, "total": len(CASES), "results": res}, open(a.report, "w"), ensure_ascii=False, indent=1)
print(f"{ok}/{len(CASES)}")
