#!/usr/bin/env python3
"""Parity runner for `pilotdeck cron` (slice 3, P11: network protocol + server-side state).

For each implementation a fresh real TS gateway (`pilotdeck server`, the
production counterpart) is started on an identical fixture, then the same
scripted command sequence runs through the CLI under test. Outputs are compared
after a frozen normalizer: UUIDs → <uuid#n> by first appearance, wall-clock
ISO timestamps (current date) → <now>.

SRC-DEFECT-001: the TS CLI never exits after a successful WS connection. The
oracle is killed after ORACLE_IDLE seconds and its exit code is derived the way
`main()` sets process.exitCode (1 iff an error line was printed to stderr).
"""
import argparse, json, os, re, shutil, signal, subprocess, sys, tempfile, time, urllib.request
sys.path.insert(0, os.path.dirname(__file__))
from chat_search_parity import normalize_stderr, oracle_broken  # noqa: E402

ORACLE_IDLE = 6
CONFIG = """schemaVersion: 1
agent:
  model: test/test
  maxContextTokens: 32768
  maxOutputTokens: 4096
extension:
  builtinPluginsEnabled:
    funasr: false
    browser-use: false
telemetry:
  enabled: false
cron:
  enabled: true
  timezone: UTC
model:
  providers:
    test:
      protocol: openai
      url: http://127.0.0.1:1
      apiKey: test
      models:
        test:
          capabilities:
            supportsToolUse: true
            maxContextTokens: 32768
            maxOutputTokens: 4096
"""

STEPS = [
    ("list-empty", ["list"]),
    ("list-history-limit", ["list", "--history", "--limit", "5x"]),
    ("list-limit-nan", ["list", "--limit", "abc"]),
    ("create-cron", ["create", "--session", "cli:test", "--message", "daily hi", "--cron", "0 9 * * *"]),
    ("create-once-tz", ["create", "--session", "cli:test", "--message", "once", "--once", "2030-01-01T00:00:00Z", "--timezone", "Asia/Shanghai"]),
    ("create-channel-project", ["create", "--session", "nocolon", "--message", "m", "--cron", "*/5 * * * *", "--channel", "feishu", "--project", "/srv/p"]),
    ("create-third", ["create", "--session", ":lead", "--message", "x", "--once", "2031-06-01T08:00:00+08:00"]),
    ("create-missing-message", ["create", "--session", "cli:x", "--cron", "* * * * *"]),
    ("create-missing-schedule", ["create", "--session", "cli:x", "--message", "m"]),
    ("create-bad-cron", ["create", "--session", "cli:x", "--message", "m", "--cron", "not a cron"]),
    ("create-bad-once", ["create", "--session", "cli:x", "--message", "m", "--once", "yesterday"]),
    ("list-after-create", ["list", "--history"]),
    ("delete-first", ["delete", "$T0"]),
    ("delete-stop-running", ["delete", "--task", "$T1", "--stop-running"]),
    ("delete-missing", ["delete", "00000000-0000-4000-8000-000000000000"]),
    ("delete-no-id", ["delete"]),
    ("stop-task", ["stop", "$T2"]),
    ("stop-run", ["stop", "--run", "no-such-run"]),
    ("stop-no-args", ["stop"]),
    ("bad-subcommand", ["pause"]),
    ("list-final", ["list", "--history"]),
]

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")

def normalize(text, today):
    seen = {}
    def u(m):
        v = m.group(0)
        if v.startswith("00000000-"):
            return v
        seen.setdefault(v, f"<uuid#{len(seen)}>")
        return seen[v]
    text = UUID.sub(u, text)
    return re.sub(today + r"T\d\d:\d\d:\d\d\.\d{3}Z", "<now>", text)

def wait_health(timeout=60):
    end = time.time() + timeout
    while time.time() < end:
        try:
            if urllib.request.urlopen("http://127.0.0.1:18789/health", timeout=1).status == 200:
                return True
        except Exception:
            time.sleep(0.5)
    return False

def run_impl(cmd, node, ts_cli, base, oracle):
    if os.path.exists(base):
        shutil.rmtree(base)
    ph, proj = os.path.join(base, "ph"), os.path.join(base, "proj")
    os.makedirs(ph); os.makedirs(proj)
    open(os.path.join(ph, "pilotdeck.yaml"), "w").write(CONFIG)
    env = {"PATH": os.environ.get("PATH", ""), "HOME": base, "PILOT_HOME": ph, "TZ": "UTC", "LANG": "C.UTF-8"}
    out = []
    # step 0: no server running
    out.append(("no-server", one(cmd, ["cron", "list"], proj, env, oracle)))
    server = subprocess.Popen([node, ts_cli, "server"], cwd=proj, env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, start_new_session=True)
    try:
        if not wait_health():
            raise SystemExit("gateway did not start")
        time.sleep(1)
        other = dict(env, PILOT_HOME=os.path.join(base, "no-token-home"))
        out.append(("no-token", one(cmd, ["cron", "list"], proj, other, oracle)))
        tasks = []
        for sid, argv in STEPS:
            argv = [tasks[int(a[2:])] if a.startswith("$T") and int(a[2:]) < len(tasks) else a for a in argv]
            r = one(cmd, ["cron", *argv], proj, env, oracle)
            if sid.startswith("create") and r["code"] == 0:
                try:
                    tasks.append(json.loads(r["stdout"])["task"]["taskId"])
                except Exception:
                    pass
            out.append((sid, r))
    finally:
        os.killpg(server.pid, signal.SIGTERM)
        try:
            server.wait(10)
        except subprocess.TimeoutExpired:
            os.killpg(server.pid, signal.SIGKILL)
        time.sleep(1)
    return out

def one(cmd, argv, cwd, env, oracle):
    try:
        p = subprocess.run(cmd + argv, cwd=cwd, env=env, capture_output=True, timeout=ORACLE_IDLE if oracle else 30)
        code, so, se, hung = p.returncode, p.stdout, p.stderr, False
    except subprocess.TimeoutExpired as e:
        so, se, hung = e.stdout or b"", e.stderr or b"", True
        code = None
    se = normalize_stderr(se)
    if hung:
        code = 1 if se.strip() else 0
    return {"code": code, "stdout": so.decode("utf-8", "replace"), "stderr": se.decode("utf-8", "replace"), "hung": hung}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ts-cli", required=True); ap.add_argument("--rust-bin", required=True)
    ap.add_argument("--node", default="node"); ap.add_argument("--report")
    a = ap.parse_args()
    ts_cli = os.path.abspath(a.ts_cli)
    base = os.path.join(tempfile.mkdtemp(prefix="pd-cron-"), "fx")
    today = time.strftime("%Y-%m-%d", time.gmtime())
    t = run_impl([a.node, ts_cli], a.node, ts_cli, base, True)
    r = run_impl([os.path.abspath(a.rust_bin)], a.node, ts_cli, base, False)
    results, failed = [], 0
    for (sid, tr), (_, rr) in zip(t, r):
        if oracle_broken({"code": 1, "stderr": tr["stderr"].encode()}) and tr["code"] != 0:
            failed += 1; results.append({"id": sid, "ok": False, "oracle_unavailable": True}); continue
        tn = {k: normalize(tr[k], today) if isinstance(tr[k], str) else tr[k] for k in ("code", "stdout", "stderr")}
        rn = {k: normalize(rr[k], today) if isinstance(rr[k], str) else rr[k] for k in ("code", "stdout", "stderr")}
        ok = tn == rn and not rr["hung"]
        failed += not ok
        results.append({"id": sid, "ok": ok, "exit": tn["code"], "oracle_hung": tr["hung"], "rust_hung": rr["hung"]})
        if not ok:
            print(f"FAIL {sid}\n  ts={tn!r:.1200}\n  rs={rn!r:.1200}")
    nc = [{"id": "nc-normalizer-keeps-content", "detected": normalize('{"a":"x"}', today) != normalize('{"a":"y"}', today)},
          {"id": "nc-uuid-identity", "detected": normalize("11111111-1111-4111-8111-111111111111 22222222-2222-4222-8222-222222222222", today)
           != normalize("11111111-1111-4111-8111-111111111111 11111111-1111-4111-8111-111111111111", today)},
          {"id": "nc-oracle-nonempty", "detected": any(x[1]["stdout"] for x in t)}]
    nc_ok = all(x["detected"] for x in nc)
    n = len(STEPS) + 2
    summary = {"cases_planned": n, "cases_executed": len(results), "passed": len(results) - failed, "failed": failed,
               "skipped": 0, "negative_controls": nc, "results": results,
               "oracle_hangs": sum(1 for x in t if x[1]["hung"])}
    if a.report:
        json.dump(summary, open(a.report, "w"), indent=2)
    shutil.rmtree(os.path.dirname(base), ignore_errors=True)
    print(f"parity: {summary['passed']}/{n} passed (oracle hung in {summary['oracle_hangs']} steps, SRC-DEFECT-001), negative controls {'ok' if nc_ok else 'FAILED'}")
    sys.exit(0 if failed == 0 and nc_ok and len(results) == n else 1)

if __name__ == "__main__":
    main()
