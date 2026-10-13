#!/usr/bin/env python3
"""Parity for `pilotdeck run|resume` (slice 6) against a real TS gateway + deterministic OpenAI mock
(rust/parity/mock_openai.py). Same normalizer as sdk_cli_parity."""
import argparse, json, os, shutil, signal, subprocess, sys, tempfile, time
sys.path.insert(0, os.path.dirname(__file__))
from chat_search_parity import normalize_stderr, oracle_broken  # noqa: E402
from cron_parity import CONFIG, wait_health  # noqa: E402
from sdk_cli_parity import normalize  # noqa: E402

MOCK_PORT = 18999
IDLE = 25

def steps():
    return [
        ("run-text", ["run", "hello", "world"], {}, None),
        ("run-json", ["run", "hello", "--json"], {}, None),
        ("run-stream-json", ["run", "hi", "--stream-json"], {}, None),
        ("run-json-stream-json", ["run", "both", "--json", "--stream-json"], {}, None),
        ("run-model-error", ["run", "FAIL_400", "now"], {}, None),
        ("run-model-error-json", ["run", "FAIL_400", "--json"], {}, None),
        ("run-stdin", ["run"], {}, b"  from stdin\n"),
        ("run-empty-prompt", ["run"], {}, b"   \n"),
        ("run-channel", ["run", "chan", "--channel", "feishu", "--json"], {}, None),
        ("run-project", ["run", "proj", "--project", "$PH", "--json"], {}, None),
        ("run-model-no-project", ["run", "m", "--model", "test/test"], {}, None),
        ("run-model-ok", ["run", "m2", "--model", "test/test", "--project", "$PH", "--json"], {}, None),
        ("run-model-missing", ["run", "m3", "--model", "nope", "--project", "$PH"], {}, None),
        ("run-perm-plan", ["run", "plan", "--permission-mode", "plan", "--json"], {}, None),
        ("run-perm-accept", ["run", "acc", "--permission-mode", "acceptEdits", "--json"], {}, None),
        ("run-perm-bogus", ["run", "bog", "--permission-mode", "bogus", "--json"], {}, None),
        ("resume-ok", ["resume", "$SID", "again"], {}, None),
        ("resume-json", ["resume", "$SID", "more", "--json"], {}, None),
        ("resume-no-id", ["resume"], {}, None),
        ("resume-unknown", ["resume", "cli:s_nope", "x"], {}, None),
        ("resume-no-prompt", ["resume", "$SID"], {}, b""),
        ("run-bad-token", ["run", "x", "--auth-token", "bad"], {}, None),
        ("run-refused", ["run", "x", "--gateway-url", "ws://127.0.0.1:1/ws"], {}, None),
        ("run-sigint", ["run", "SLOW", "please"], {}, "SIGINT"),
        ("sessions-after", ["sessions", "list"], {}, None),
    ]

def one(cmd, argv, cwd, env, stdin):
    if stdin == "SIGINT":
        p = subprocess.Popen(cmd + argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        time.sleep(4)
        p.send_signal(signal.SIGINT)
        try:
            so, se = p.communicate(timeout=30)
        except subprocess.TimeoutExpired:
            p.kill(); so, se = p.communicate()
        code = p.returncode
        time.sleep(9)  # let the slow mock request drain so the next step starts clean
    else:
        try:
            p = subprocess.run(cmd + argv, cwd=cwd, env=env, input=stdin if stdin is not None else b"",
                               capture_output=True, timeout=IDLE)
            so, se, code = p.stdout, p.stderr, p.returncode
        except subprocess.TimeoutExpired as e:
            # SRC-DEFECT-002: TS never exits after a run-initialization error (query transport left open).
            so, se = e.stdout or b"", e.stderr or b""
            code = 1 if normalize_stderr(se).strip() else 0
            return {"code": code, "stdout": so.decode("utf-8", "replace"), "stderr": normalize_stderr(se).decode("utf-8", "replace"), "hung": True}
    return {"code": code, "stdout": so.decode("utf-8", "replace"), "stderr": normalize_stderr(se).decode("utf-8", "replace"), "hung": False}

def run_impl(cmd, node, ts_cli, base):
    if os.path.exists(base):
        shutil.rmtree(base)
    ph, proj = os.path.join(base, "ph"), os.path.join(base, "proj")
    os.makedirs(ph); os.makedirs(proj)
    open(os.path.join(ph, "pilotdeck.yaml"), "w").write(CONFIG.replace("http://127.0.0.1:1", f"http://127.0.0.1:{MOCK_PORT}/v1"))
    env = {"PATH": os.environ.get("PATH", ""), "HOME": base, "PILOT_HOME": ph, "TZ": "UTC", "LANG": "C.UTF-8"}
    mock = subprocess.Popen([sys.executable, os.path.join(os.path.dirname(__file__), "mock_openai.py"), str(MOCK_PORT)], start_new_session=True)
    server = subprocess.Popen([node, ts_cli, "server"], cwd=proj, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    out = []
    try:
        if not wait_health():
            raise SystemExit("gateway did not start")
        time.sleep(1)
        subprocess.run([node, ts_cli, "run", "seed"], cwd=proj, env=env, capture_output=True, timeout=120)
        sid = json.loads(subprocess.run([node, ts_cli, "sessions", "list"], cwd=proj, env=env, capture_output=True, timeout=60).stdout)[0]["sessionId"]
        for name, argv, extra, stdin in steps():
            argv = [sid if a == "$SID" else ph if a == "$PH" else a for a in argv]
            out.append((name, one(cmd, argv, proj, dict(env, **extra), stdin)))
    finally:
        for p in (server, mock):
            os.killpg(p.pid, signal.SIGTERM)
            try:
                p.wait(10)
            except subprocess.TimeoutExpired:
                os.killpg(p.pid, signal.SIGKILL)
        time.sleep(1)
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ts-cli", required=True); ap.add_argument("--rust-bin", required=True)
    ap.add_argument("--node", default="node"); ap.add_argument("--report")
    a = ap.parse_args()
    ts_cli = os.path.abspath(a.ts_cli)
    base = os.path.join(tempfile.mkdtemp(prefix="pd-run-"), "fx")
    today = time.strftime("%Y-%m-%d", time.gmtime())
    t = run_impl([a.node, ts_cli], a.node, ts_cli, base)
    r = run_impl([os.path.abspath(a.rust_bin)], a.node, ts_cli, base)
    results, failed = [], 0
    for (sid, tr), (_, rr) in zip(t, r):
        if oracle_broken({"code": tr["code"], "stderr": tr["stderr"].encode()}):
            failed += 1; results.append({"id": sid, "ok": False, "oracle_unavailable": True}); continue
        tn = {k: normalize(v, today) if isinstance(v, str) else v for k, v in tr.items() if k != "hung"}
        rn = {k: normalize(v, today) if isinstance(v, str) else v for k, v in rr.items() if k != "hung"}
        ok = tn == rn and not rr["hung"]
        failed += not ok
        results.append({"id": sid, "ok": ok, "exit": tn["code"], "stdout_head": tn["stdout"][:160], "stderr": tn["stderr"][:200], "oracle_hung": tr["hung"]})
        if not ok:
            print(f"FAIL {sid}\n  ts={tn!r:.1500}\n  rs={rn!r:.1500}")
    nc = [{"id": "nc-oracle-completed", "detected": "MOCK_REPLY::hello world" in dict(t)["run-text"]["stdout"]},
          {"id": "nc-oracle-failed-path", "detected": dict(t)["run-model-error"]["code"] == 1},
          {"id": "nc-normalizer-keeps-content", "detected": normalize("MOCK_REPLY::a", today) != normalize("MOCK_REPLY::b", today)}]
    nc_ok = all(x["detected"] for x in nc)
    n = len(steps())
    hangs = [x for x, v in t if v["hung"]]
    summary = {"oracle_hangs_SRC_DEFECT_002": hangs, "cases_planned": n, "cases_executed": len(results), "passed": len(results) - failed, "failed": failed,
               "negative_controls": nc, "results": results}
    if a.report:
        json.dump(summary, open(a.report, "w"), indent=2)
    shutil.rmtree(os.path.dirname(base), ignore_errors=True)
    print(f"parity: {summary['passed']}/{n} passed, negative controls {'ok' if nc_ok else 'FAILED'}")
    sys.exit(0 if failed == 0 and nc_ok and len(results) == n else 1)

if __name__ == "__main__":
    main()
