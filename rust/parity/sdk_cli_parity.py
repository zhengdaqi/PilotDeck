#!/usr/bin/env python3
"""Parity for `pilotdeck sessions …` / `pilotdeck settings …` (slice 5) against a real TS gateway.
Fixture: fresh PILOT_HOME + config, TS server, one session created through the TS `pilotdeck run`
(setup, identical for both sides). Normalizer = cron_parity.normalize + epoch-ms timestamps → <epoch>."""
import argparse, json, os, re, shutil, signal, subprocess, sys, tempfile, time
sys.path.insert(0, os.path.dirname(__file__))
from chat_search_parity import oracle_broken  # noqa: E402
from cron_parity import CONFIG, wait_health, normalize as norm_base, one  # noqa: E402

SETTLE = 2.0
EPOCH = re.compile(r"\b1[789]\d{11}\b")

def normalize(text, today):
    text = norm_base(text, today)
    text = re.sub(r"s_<uuid#\d+>", "s_<sid>", text)
    text = re.sub(r'"fileSize": \d+', '"fileSize": <n>', text)
    text = re.sub(r'"contentHash": "[0-9a-f]+"', '"contentHash": <h>', text)
    return EPOCH.sub("<epoch>", text)

def steps(proj):
    S = [
        ("sessions-list", ["sessions", "list"], {}),
        ("sessions-default", ["sessions"], {}),
        ("sessions-list-project", ["sessions", "list", "--project", "$PH"], {}),
        ("sessions-list-project-missing", ["sessions", "list", "--project", "/nonexistent/p"], {}),
        ("sessions-messages", ["sessions", "messages", "$SID"], {}),
        ("sessions-messages-flags-first", ["sessions", "messages", "--project", "$PH", "$SID"], {}),
        ("sessions-messages-noid", ["sessions", "messages"], {}),
        ("sessions-messages-unknown", ["sessions", "messages", "nope"], {}),
        ("sessions-bogus", ["sessions", "bogus"], {}),
        ("sessions-flag-first", ["sessions", "--project", "x"], {}),
        ("settings-get", ["settings", "get"], {}),
        ("settings-default", ["settings"], {}),
        ("settings-bogus", ["settings", "bogus"], {}),
        ("settings-set-empty", ["settings", "set"], {}),
        ("settings-set-token", ["settings", "set", "xyz"], {}),
        ("settings-set-eoi", ["settings", "set", '{"a":'], {}),
        ("settings-set-array", ["settings", "set", "[1]"], {}),
        ("settings-set-null", ["settings", "set", "null"], {}),
        ("settings-set-string", ["settings", "set", '"s"'], {}),
        ("settings-set-trailing", ["settings", "set", "{}", "x"], {}),
        ("settings-set-propname", ["settings", "set", "{a:1}"], {}),
        ("settings-set-long-ctx", ["settings", "set", '{"agent": {"maxOutputTokens": oops}}'], {}),
        ("settings-set-unsupported", ["settings", "set", '{"x":1}'], {}),
        ("settings-set-noop", ["settings", "set", "{}"], {}),
        ("settings-set-valid", ["settings", "set", "--json", '{"agent":{"maxOutputTokens":2048}}'], {}),
        ("settings-get-after", ["settings", "get"], {}),
        ("settings-set-clear", ["settings", "set", '{"agent":', '{"maxOutputTokens":null}}'], {}),
        ("settings-get-final", ["settings", "get"], {}),
        ("auth-bad-token", ["sessions", "list", "--auth-token", "bad"], {}),
        ("auth-env-token-bad", ["settings", "get"], {"PILOTDECK_TOKEN": "zzz"}),
        ("auth-empty-sdk-token", ["sessions", "list"], {"PILOTDECK_SDK_AUTH_TOKEN": ""}),
        ("url-refused", ["sessions", "list", "--gateway-url", "ws://127.0.0.1:1/ws"], {}),
        ("url-invalid", ["sessions", "list", "--gateway-url", "foo"], {}),
        ("url-ftp", ["sessions", "list", "--gateway-url", "ftp://127.0.0.1:18789/ws"], {}),
        ("url-http", ["sessions", "list", "--gateway-url", "http://127.0.0.1:18789/ws"], {}),
        ("url-localhost", ["sessions", "list", "--gateway-url", "ws://localhost:18789/ws"], {}),
        ("url-fragment", ["sessions", "list", "--gateway-url", "ws://127.0.0.1:18789/ws#f"], {}),
        ("url-wrong-path", ["sessions", "list", "--gateway-url", "ws://127.0.0.1:18789/nope"], {}),
        ("env-port", ["sessions", "list"], {"PILOTDECK_GATEWAY_PORT": "1"}),
        ("env-sdk-url", ["settings", "get"], {"PILOTDECK_SDK_GATEWAY_URL": "ws://127.0.0.1:1/ws", "PILOTDECK_GATEWAY_URL": "ws://127.0.0.1:18789/ws"}),
        ("env-url-empty", ["settings", "get"], {"PILOTDECK_SDK_GATEWAY_URL": ""}),
    ]
    return S

def run_impl(cmd, node, ts_cli, base):
    if os.path.exists(base):
        shutil.rmtree(base)
    ph, proj = os.path.join(base, "ph"), os.path.join(base, "proj")
    os.makedirs(ph); os.makedirs(proj)
    open(os.path.join(ph, "pilotdeck.yaml"), "w").write(CONFIG)
    env = {"PATH": os.environ.get("PATH", ""), "HOME": base, "PILOT_HOME": ph, "TZ": "UTC", "LANG": "C.UTF-8"}
    out = [("no-server", one(cmd, ["sessions", "list"], proj, env, False)),
           ("no-token", one(cmd, ["settings", "get"], proj, dict(env, PILOT_HOME=os.path.join(base, "none")), False))]
    server = subprocess.Popen([node, ts_cli, "server"], cwd=proj, env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, start_new_session=True)
    try:
        if not wait_health():
            raise SystemExit("gateway did not start")
        time.sleep(1)
        subprocess.run([node, ts_cli, "run", "hello there"], cwd=proj, env=env, capture_output=True, timeout=120)
        listed = subprocess.run([node, ts_cli, "sessions", "list"], cwd=proj, env=env, capture_output=True, timeout=60)
        sid = json.loads(listed.stdout)[0]["sessionId"]
        for name, argv, extra in steps(proj):
            argv = [sid if a == "$SID" else ph if a == "$PH" else a for a in argv]
            out.append((name, one(cmd, argv, proj, dict(env, **extra), False)))
            if name.startswith("settings-set"):
                time.sleep(SETTLE)  # let the server's config watcher settle (version counter is timing-sensitive)
    finally:
        os.killpg(server.pid, signal.SIGTERM)
        try:
            server.wait(10)
        except subprocess.TimeoutExpired:
            os.killpg(server.pid, signal.SIGKILL)
        time.sleep(1)
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ts-cli", required=True); ap.add_argument("--rust-bin", required=True)
    ap.add_argument("--node", default="node"); ap.add_argument("--report")
    a = ap.parse_args()
    ts_cli = os.path.abspath(a.ts_cli)
    base = os.path.join(tempfile.mkdtemp(prefix="pd-sdk-"), "fx")
    today = time.strftime("%Y-%m-%d", time.gmtime())
    t = run_impl([a.node, ts_cli], a.node, ts_cli, base)
    r = run_impl([os.path.abspath(a.rust_bin)], a.node, ts_cli, base)
    results, failed = [], 0
    for (sid, tr), (_, rr) in zip(t, r):
        if oracle_broken({"code": tr["code"], "stderr": tr["stderr"].encode()}):
            failed += 1; results.append({"id": sid, "ok": False, "oracle_unavailable": True}); continue
        tn = {k: normalize(tr[k], today) if isinstance(tr[k], str) else tr[k] for k in ("code", "stdout", "stderr")}
        rn = {k: normalize(rr[k], today) if isinstance(rr[k], str) else rr[k] for k in ("code", "stdout", "stderr")}
        ok = tn == rn and not rr["hung"] and not tr["hung"]
        failed += not ok
        results.append({"id": sid, "ok": ok, "exit": tn["code"], "stderr": tn["stderr"][:200], "stdout_len": len(tn["stdout"])})
        if not ok:
            print(f"FAIL {sid}\n  ts={tn!r:.1500}\n  rs={rn!r:.1500}")
    nc = [{"id": "nc-normalizer-keeps-content", "detected": normalize('{"a":"x"}', today) != normalize('{"a":"y"}', today)},
          {"id": "nc-oracle-has-session", "detected": any('"sessionId"' in x[1]["stdout"] for x in t)},
          {"id": "nc-oracle-settings-changed", "detected": dict(t)["settings-get-after"]["stdout"] != dict(t)["settings-get"]["stdout"]}]
    nc_ok = all(x["detected"] for x in nc)
    n = len(steps("")) + 2
    summary = {"cases_planned": n, "cases_executed": len(results), "passed": len(results) - failed, "failed": failed,
               "negative_controls": nc, "results": results}
    if a.report:
        json.dump(summary, open(a.report, "w"), indent=2)
    shutil.rmtree(os.path.dirname(base), ignore_errors=True)
    print(f"parity: {summary['passed']}/{n} passed, negative controls {'ok' if nc_ok else 'FAILED'}")
    sys.exit(0 if failed == 0 and nc_ok and len(results) == n else 1)

if __name__ == "__main__":
    main()
