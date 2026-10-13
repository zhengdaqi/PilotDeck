#!/usr/bin/env python3
"""Parity for small CLI entries (slice 4): `update`, `gateway` usage, `tui` without a TTY.
`update` keeps delegating to the bundled scripts/update-web.mjs (Node), as TS does."""
import argparse, json, os, shutil, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from chat_search_parity import normalize_stderr, oracle_broken  # noqa: E402

def run(cmd, cwd, env):
    p = subprocess.run(cmd, cwd=cwd, env=env, stdin=subprocess.DEVNULL, capture_output=True, timeout=120)
    return {"code": p.returncode, "stdout": normalize_stderr(p.stdout), "stderr": normalize_stderr(p.stderr)}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ts-cli", required=True); ap.add_argument("--rust-bin", required=True)
    ap.add_argument("--node", default="node"); ap.add_argument("--report", required=True)
    a = ap.parse_args()
    repo = os.path.abspath(os.path.join(os.path.dirname(a.ts_cli), "../../.."))
    tmp = tempfile.mkdtemp()
    # Installation without the Git Web updater: copy dist only (node_modules linked).
    bare = os.path.join(tmp, "bare"); os.makedirs(bare)
    shutil.copytree(os.path.join(repo, "dist"), os.path.join(bare, "dist"), symlinks=True)
    os.symlink(os.path.join(repo, "node_modules"), os.path.join(bare, "node_modules"))
    shutil.copy(os.path.join(repo, "package.json"), bare)
    os.symlink(os.path.join(repo, "packages"), os.path.join(bare, "packages"))
    env = dict(os.environ, PILOT_HOME=os.path.join(tmp, "home"))
    cases = []
    for name, args in [("update-bad-arg", ["update", "--bogus"]), ("update-check-devtree", ["update", "--check"]),
                       ("update-mixed", ["update", "--check", "x"]),
                       ("gateway-none", ["gateway"]), ("gateway-bad", ["gateway", "nope"]),
                       ("tui-notty", ["tui"]), ("help", ["help"]), ("help-long", ["--help", "x"]), ("help-h", ["-h"])]:
        cases.append((name, [a.node, a.ts_cli] + args, [a.rust_bin] + args, dict(env), tmp))
    cases.append(("update-no-updater", [a.node, os.path.join(bare, "dist/src/cli/pilotdeck.js"), "update"],
                  [a.rust_bin, "update"], dict(env, PILOTDECK_INSTALL_ROOT=os.path.join(bare, "dist/src/runtime")), tmp))
    results, ok = [], True
    for name, ts, rs, e, cwd in cases:
        o = run(ts, cwd, e)
        if oracle_broken(o): print(f"BLOCKED {name}: oracle broken"); sys.exit(3)
        r = run(rs, cwd, e)
        same = o == r
        ok &= same
        results.append({"case": name, "pass": same, "oracle": {k: (v.decode(errors="replace") if isinstance(v, bytes) else v) for k, v in o.items()},
                        **({} if same else {"rust": {k: (v.decode(errors="replace") if isinstance(v, bytes) else v) for k, v in r.items()}})})
        print(("PASS " if same else "FAIL ") + name)
    # negative control: comparator must detect a different exit code
    nc = run([a.rust_bin, "gateway"], tmp, env); nc["code"] = 0
    nc_ok = nc != run([a.node, a.ts_cli, "gateway"], tmp, env)
    json.dump({"cases": results, "negative_control_detected": nc_ok}, open(a.report, "w"), indent=2)
    print(f"parity: {sum(r['pass'] for r in results)}/{len(results)} passed, negative control {'ok' if nc_ok else 'FAILED'}")
    sys.exit(0 if ok and nc_ok else 1)

main()
