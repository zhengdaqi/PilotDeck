#!/usr/bin/env python3
"""Differential test: Rust gateway server edge vs TS startGatewayServer, same fake Gateway.

TS oracle: node ts_server.mjs (real dist GatewayServer + fake_gateway.mjs, in-process).
Rust:      pilotdeck-gateway-server -- node bridge.mjs (same fake_gateway.mjs over the JSON-lines bridge).
Every observable byte-level outcome (WS frames, close codes/reasons, HTTP status/content-type/body)
is compared exactly after normalizing random ids/timestamps. Known, documented divergences
(SRC-DEFECT-004/005 process crashes, PENDING routes) are asserted separately and never counted as passes.
Usage: gateway_server_parity.py --rust BIN [--out FILE]
"""
import argparse, base64, json, os, re, socket, struct, subprocess, sys, tempfile, time
HERE = os.path.dirname(os.path.abspath(__file__))
TOKEN = "tok-parity-0123456789"

class Server:
    def __init__(self, argv):
        self.p = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, cwd=HERE)
        line = self.p.stdout.readline()
        if not line: raise RuntimeError("server failed: " + self.p.stderr.read())
        info = json.loads(line); m = re.match(r"http://([^:]+):(\d+)", info["url"])
        self.host, self.port = m.group(1), int(m.group(2))
    def alive(self): return self.p.poll() is None
    def stop(self):
        if self.alive(): self.p.kill()
        self.p.wait()

def http(srv, raw):
    try: s = socket.create_connection((srv.host, srv.port), timeout=5)
    except ConnectionRefusedError: return {"server_down": True}
    s.sendall(raw)
    data = b""
    try:
        while True:
            c = s.recv(65536)
            if not c: break
            data += c
            if b"\r\n\r\n" in data and parse_http(data, complete=False): break
    except socket.timeout: data += b"<timeout>"
    except ConnectionResetError: data += b"<reset>"
    s.close()
    return parse_http(data) if data and not data.startswith(b"<") else {"closed": data.decode() or "no response"}

def parse_http(data, complete=True):
    head, _, body = data.partition(b"\r\n\r\n")
    lines = head.decode("latin1").split("\r\n"); status = int(lines[0].split()[1])
    h = {}
    for l in lines[1:]:
        k, _, v = l.partition(":"); h.setdefault(k.strip().lower(), v.strip())
    if h.get("transfer-encoding") == "chunked":
        out, rest = b"", body
        while True:
            if b"\r\n" not in rest: return None if not complete else {"status": status, "error": "truncated chunked"}
            n, _, rest = rest.partition(b"\r\n"); n = int(n, 16)
            if n == 0: break
            if len(rest) < n + 2: return None if not complete else {"status": status, "error": "truncated chunked"}
            out, rest = out + rest[:n], rest[n + 2:]
        body = out
    elif "content-length" in h:
        if len(body) < int(h["content-length"]): return None if not complete else {"status": status, "error": "truncated"}
    elif not complete: return None
    return {"status": status, "content-type": h.get("content-type"), "body": body.decode("utf8", "replace")}

def get(path, headers=None, method="GET", body=b""):
    hs = "".join("%s: %s\r\n" % kv for kv in (headers or {}).items())
    return ("%s %s HTTP/1.1\r\nHost: 127.0.0.1\r\n%sContent-Length: %d\r\nConnection: close\r\n\r\n" % (method, path, hs, len(body))).encode() + body

class WS:
    def __init__(self, srv, path="/ws"):
        try: self.s = socket.create_connection((srv.host, srv.port), timeout=3)
        except ConnectionRefusedError: raise RuntimeError("server down")
        key = base64.b64encode(os.urandom(16)).decode()
        self.s.sendall(("GET %s HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: %s\r\nSec-WebSocket-Version: 13\r\n\r\n" % (path, key)).encode())
        self.buf = b""
        while b"\r\n\r\n" not in self.buf:
            c = self.s.recv(4096)
            if not c: self.status = "closed"; return
            self.buf += c
        head, _, self.buf = self.buf.partition(b"\r\n\r\n"); self.status = head.split(b"\r\n")[0].decode()
        import hashlib
        want = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        self.accept_ok = ("Sec-WebSocket-Accept: " + want).encode() in head
    def send(self, opcode, payload, masked=True):
        if isinstance(payload, str): payload = payload.encode()
        n = len(payload); hdr = bytes([0x80 | opcode])
        mb = 0x80 if masked else 0
        hdr += bytes([mb | n]) if n < 126 else (bytes([mb | 126]) + struct.pack(">H", n) if n < 65536 else bytes([mb | 127]) + struct.pack(">Q", n))
        if masked:
            mask = os.urandom(4); hdr += mask; payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self.s.sendall(hdr + payload)
    def text(self, v): self.send(1, v if isinstance(v, str) else json.dumps(v))
    def recv(self, timeout=3):
        self.s.settimeout(timeout)
        while True:
            f = self._frame()
            if f: return f
            try: c = self.s.recv(65536)
            except socket.timeout: return {"timeout": True}
            except ConnectionResetError: return {"eof": "reset"}
            if not c: return {"eof": True}
            self.buf += c
    def _frame(self):
        b = self.buf
        if len(b) < 2: return None
        op, n, off = b[0] & 0xF, b[1] & 0x7F, 2
        if n == 126:
            if len(b) < 4: return None
            n, off = struct.unpack(">H", b[2:4])[0], 4
        elif n == 127:
            if len(b) < 10: return None
            n, off = struct.unpack(">Q", b[2:10])[0], 10
        if len(b) < off + n: return None
        p, self.buf = b[off:off + n], b[off + n:]
        if op == 1: return {"text": json.loads(p.decode())}
        if op == 8: return {"close": struct.unpack(">H", p[:2])[0] if len(p) >= 2 else None, "reason": p[2:].decode()}
        if op == 10: return {"pong": p.decode()}
        return {"opcode": op}
    def until_quiet(self, timeout=0.6):
        out = []
        while True:
            f = self.recv(timeout)
            if "timeout" in f: break
            out.append(f)
            if "eof" in f or "close" in f: break
        return out
    def close(self): self.s.close()

def hello(ws, client="web", token=TOKEN, version="1.2"):
    ws.text({"type": "hello", "protocolVersion": version, "clientName": client, "clientVersion": "0.0.1", "token": token})
    return ws.recv()

def request(ws, rid, method, params=None, has_params=True):
    f = {"type": "request", "id": rid, "method": method}
    if has_params: f["params"] = params
    ws.text(f)
    frames = []
    while True:
        r = ws.recv(5)
        frames.append(r)
        if "timeout" in r or "eof" in r or "close" in r: break
        t = r.get("text", {})
        if t.get("type") == "response" and t.get("id") == rid: break
        if t.get("type") == "event" and t.get("id") == rid and t.get("final"): break
    return frames

WIRE = ["upload_create", "upload_get", "upload_part", "upload_complete", "upload_cancel", "steer_turn", "cancel_steer", "abort_turn",
  "run_get", "run_events", "run_reattach", "permission_list", "memory_list", "memory_wipe", "snapshot_list", "snapshot_get",
  "snapshot_restore", "manager_sessions", "manager_browsers", "native_archive_manifest", "native_archive_entries",
  "native_archive_artifact", "list_sessions", "resume_session", "sop_status", "sop_resume", "new_session", "close_project_sessions",
  "close_session", "delete_session", "export_session_transcript", "restore_session_transcript", "rename_session", "tag_session",
  "record_agent_status_message", "describe_server", "project_files_list", "commands_list", "model_catalog_list", "session_model_get",
  "session_model_set", "session_model_clear", "active_turn_snapshot", "mcp_server_status", "set_mcp_servers", "mcp_server_reconnect",
  "mcp_server_toggle", "set_mcp_permission_mode_override", "project_file_read", "set_permission_mode", "apply_flag_settings",
  "update_settings", "resolve_settings", "set_session_thinking", "output_styles_list", "set_output_style", "reload_output_styles",
  "usage_snapshot", "model_usage_snapshot", "rewind_files", "background_task_stop", "background_tasks", "hook_async_result",
  "seed_read_state", "supported_agents", "reconnect_interaction", "cron_create", "cron_list", "cron_update", "cron_delete", "cron_stop",
  "cron_run_now", "elicitation_respond", "user_dialog_list", "user_dialog_claim", "user_dialog_release", "user_dialog_respond",
  "permission_decide", "grant_session_permission", "read_session_messages", "read_subagent_messages", "fork_session",
  "replace_last_turn", "finalize_last_turn_replacement", "list_projects", "describe_project", "reload_config", "prepare_weixin_login",
  "reload_extensions", "skill_list", "skill_read", "skill_write", "skill_create", "skill_delete", "skill_import", "skill_validate",
  "skill_scan", "always_on_apply", "always_on_abort", "always_on_rerun_plan", "frobnicate"]
MODES = ["structured", "dialog", "dialog_nodetails", "plain", "value", "object", "numcode", "skill_manager", "skill_validation",
  "status_404", "status_409", "status_700", "status_arr", "undefined_result"]

def ws_cases(srv, variant):
    out = {}
    w = WS(srv); out["hello_bad_token"] = [hello(w, token="nope"), w.recv()]; w.close()
    w = WS(srv); out["hello_bad_version"] = [hello(w, version="9.9")]; w.close()
    w = WS(srv); w.text({"type": "request", "id": "1", "method": "list_sessions"}); out["no_hello"] = w.until_quiet(); w.close()
    w = WS(srv); w.text("{nope"); out["invalid_json"] = w.until_quiet(); w.close()
    w = WS(srv); w.text({"type": "hello", "protocolVersion": "1.2", "clientName": "web", "token": TOKEN}); out["hello_missing_field"] = w.until_quiet(); w.close()
    w = WS(srv); out["upgrade_accept_header_valid"] = [w.status, w.accept_ok]; out["hello_ok"] = [hello(w)]
    w.send(9, "ping-data"); out["ping"] = [w.recv()]
    for i, m in enumerate(WIRE):
        out["req_%s" % m] = request(w, "r%d" % i, m, {"sessionKey": "s1", "requestId": "q1", "projectKey": "p1", "x": i})
        out["req_noparams_%s" % m] = request(w, "n%d" % i, m, None, has_params=False)
    for m in ["list_sessions", "steer_turn", "sop_status", "sop_resume", "skill_list", "describe_project"]:
        for mode in MODES:
            out["mode_%s_%s" % (m, mode)] = request(w, "m-%s-%s" % (m, mode), m, {"mode": mode})
    for mode in ["events", "permission", "noterminal", "error_event", "throw_mid", "throw_now"]:
        out["submit_%s" % mode] = request(w, "t-" + mode, "submit_turn", {"sessionKey": "st-" + mode, "projectKey": "p1", "mode": mode})
    out["submit_no_session"] = request(w, "t-nos", "submit_turn", {"mode": "events"})
    out["submit_string_params"] = request(w, "t-str", "submit_turn", "ab")
    out["permission_decide_null"] = request(w, "pd-null", "permission_decide", None)
    out["active_turn_noparams"] = request(w, "ats", "active_turn_snapshot", None)
    out["after_auth_invalid_frame"] = request(w, "x", None, None) if False else None
    w.text({"type": "request", "method": "list_sessions"}); out["after_auth_invalid_frame"] = w.until_quiet(); w.close(); time.sleep(0.8)
    w = WS(srv); hello(w, client="sdk")
    out["sdk_permission_decide_control"] = request(w, "sdk1", "permission_decide", {"sessionKey": "s-sdk", "requestId": "q"})
    request(w, "sdk-r", "reconnect_interaction", {"sessionKey": "s-sdk"})
    out["sdk_permission_decide_owned"] = request(w, "sdk2", "permission_decide", {"sessionKey": "s-sdk", "requestId": "q"})
    w.send(8, struct.pack(">H", 1000)); out["client_close"] = w.until_quiet(); w.close(); time.sleep(0.8)
    # close cleanup: in-flight hang turn + interaction session + preserved session, then abrupt close
    w = WS(srv); hello(w)
    w.text({"type": "request", "id": "h1", "method": "submit_turn", "params": {"sessionKey": "s-hang", "mode": "hang"}})
    out["hang_first_event"] = [w.recv()]
    request(w, "ri1", "reconnect_interaction", {"sessionKey": "s-int"})
    request(w, "ri2", "reconnect_interaction", {"sessionKey": "keep"})
    # close handshake initiated by the client (TS only observes 'close' once both sides ended; see SRC-DEFECT-006)
    w.send(8, struct.pack(">H", 1000)); w.until_quiet(); w.close(); time.sleep(1.5)
    w = WS(srv); hello(w); out["log_after_close"] = request(w, "log", "list_sessions", None, has_params=False); w.close()
    w = WS(srv, "/nows"); out["upgrade_wrong_path"] = [w.status]; w.close()
    return out

def http_cases(srv, static, web_gateway=True):
    a = {"Authorization": "Bearer " + TOKEN}
    cases = {"health": get("/health"), "local_token": get("/auth/local-token"), "nope": get("/nope"),
      "dot_health": get("/a/../health"), "pct_dot_health": get("/%2e%2E/health"), "query_health": get("/health?x=1#f"),
      "authority_health": get("//evil/health"), "abs_health": get("http://other.example/health"),
      "web_noauth": get("/api/web/projects"), "web_projects": get("/api/web/projects", a),
      "web_lower_bearer": get("/api/web/projects", {"Authorization": "bearer   " + TOKEN}),
      "web_bad_token": get("/api/web/projects", {"Authorization": "Bearer x"}),
      "web_post_projects": get("/api/web/projects", a, "POST"), "web_project": get("/api/web/projects/p1", a),
      "web_project_slash": get("/api/web/projects/p1/", a), "web_project_enc": get("/api/web/projects/p%2F1", a),
      "web_project_missing": get("/api/web/projects/missing", a), "web_project_bad_pct": get("/api/web/projects/%zz", a), "web_other": get("/api/web/other", a),
      "web_project_sub": get("/api/web/projects/p1/unknown", a), "web_base": get("/api/web/", a),
      "approval_noauth": get("/api/module-host/approvals/status"), "approval_get": get("/api/module-host/approvals/status", a),
      "approval_post": get("/api/module-host/approvals/resume", a, "POST", b"{}"),
      "host_noauth": get("/api/module-host/call", None, "POST"), "host_get": get("/api/module-host/describe", a),
      "host_post": get("/api/module-host/describe", a, "POST", b"{}"), "feishu": get("/feishu/webhook", None, "POST", b"{}")}
    if not web_gateway:
        # minimal Gateway lacks listProjects/describeProject: TS crashes the process (SRC-DEFECT-005), see divergences
        cases = {k: v for k, v in cases.items() if k not in ("web_projects", "web_lower_bearer", "web_project", "web_project_slash", "web_project_enc", "web_project_missing")}
    if static:
        for p in ["/", "/app.js", "/style.css", "/x.svg", "/data.json", "/LICENSE", "/missing/route", "/../../etc/passwd", "/sub/", "/a%20b.txt"]:
            cases["static " + p] = get(p)
    return {k: http(srv, v) for k, v in cases.items()}

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
ISO = re.compile(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z")
def norm(v):
    s = json.dumps(v, sort_keys=False)
    seen = {}
    s = UUID.sub(lambda m: seen.setdefault(m.group(0), "<uuid%d>" % len(seen)), s)
    return json.loads(ISO.sub("<ts>", s))

def divergence_cases(rust_srv_factory, ts_srv_factory):
    """Documented divergences: asserted, reported, never counted as parity passes."""
    res = {}
    for name, action in [("SRC-DEFECT-004 unmasked client frame", lambda srv: (lambda w: (hello(w), w.send(1, '{"a":1}', masked=False), time.sleep(0.5)))(WS(srv))),
                         ("SRC-DEFECT-005 decodeURIComponent URIError before try in /api/web files route", lambda srv: (http(srv, get("/api/web/projects/%zz/files/tree", {"Authorization": "Bearer " + TOKEN})), time.sleep(0.5)))]:
        t, r = ts_srv_factory(), rust_srv_factory()
        try:
            action(t); action(r)
            res[name] = {"ts_server_alive": t.alive(), "rust_server_alive": r.alive(),
                         "rust_health_after": http(r, get("/health")).get("status") if r.alive() else None}
        finally: t.stop(); r.stop()
    def fin_only(srv):
        w = WS(srv); hello(w)
        w.text({"type": "request", "id": "h1", "method": "submit_turn", "params": {"sessionKey": "s-fin", "mode": "hang"}}); w.recv()
        w.s.shutdown(socket.SHUT_WR); time.sleep(1.0)
        v = WS(srv); hello(v); log = request(v, "log", "list_sessions", None, has_params=False)[0]["text"]["result"]["calls"]; v.close(); w.close()
        return [c[0] for c in log if c[0] in ("disconnectInteraction", "abortTurn")]
    t, r = ts_srv_factory(), rust_srv_factory()
    try: res["SRC-DEFECT-006 client FIN without close frame (half-open socket, no close cleanup in TS)"] = {"ts_cleanup_calls": fin_only(t), "rust_cleanup_calls": fin_only(r)}
    finally: t.stop(); r.stop()
    t, r = ts_srv_factory(), rust_srv_factory()
    try:
        a = {"Authorization": "Bearer " + TOKEN}
        res["PENDING /api/web project files routes"] = {"ts": http(t, get("/api/web/projects/p1/files/tree", a)).get("status"),
                                                       "rust": http(r, get("/api/web/projects/p1/files/tree", a))}
    finally: t.stop(); r.stop()
    return res

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--rust", required=True); ap.add_argument("--out"); a = ap.parse_args()
    static = tempfile.mkdtemp(prefix="gw-static-")
    for n, c in [("index.html", "<html>index</html>"), ("app.js", "console.log(1)"), ("style.css", "a{}"), ("x.svg", "<svg/>"),
                 ("data.json", "{}"), ("LICENSE", "license"), ("a%20b.txt", "pct")]:
        open(os.path.join(static, n), "w").write(c)
    os.makedirs(os.path.join(static, "sub"), exist_ok=True)
    total = fails = 0; lines = []
    for variant in ["full", "minimal"]:
        for with_static in ([True, False] if variant == "full" else [False]):
            ts_argv = ["node", "ts_server.mjs", variant, TOKEN] + ([static] if with_static else [])
            rs_argv = [a.rust, "--port", "0", "--token", TOKEN] + (["--static-assets", static] if with_static else []) + ["--", "node", "bridge.mjs", variant]
            results = {}
            for name, argv in [("ts", ts_argv), ("rust", rs_argv)]:
                srv = Server(argv)
                try: results[name] = {"http": http_cases(srv, with_static, variant == "full"), "ws": ws_cases(srv, variant) if not with_static else {}}
                except Exception as e:
                    raise RuntimeError("%s server (%s): %s; stderr tail: %s" % (name, variant, e, srv.p.stderr.read()[-1500:] if not srv.alive() else "alive"))
                finally: srv.stop()
            for kind in ["http", "ws"]:
                for k in results["ts"][kind]:
                    total += 1
                    tv, rv = norm(results["ts"][kind][k]), norm(results["rust"][kind].get(k))
                    ok = tv == rv
                    fails += not ok
                    tag = "%s%s/%s/%s" % (variant, "+static" if with_static else "", kind, k)
                    lines.append(("PASS " if ok else "FAIL ") + tag)
                    if not ok:
                        sys.path.insert(0, os.path.dirname(HERE))
                        from agent_loop_replay_parity import first_diff
                        lines.append("  first diff: %s" % (first_diff(tv, rv),))
                        lines.append("  TS:   %s\n  RUST: %s" % (json.dumps(tv)[:600], json.dumps(rv)[:600]))
    div = divergence_cases(lambda: Server([a.rust, "--port", "0", "--token", TOKEN, "--", "node", "bridge.mjs", "full"]),
                           lambda: Server(["node", "ts_server.mjs", "full", TOKEN]))
    lines.append("%d/%d parity cases passed" % (total - fails, total))
    lines.append("DOCUMENTED DIVERGENCES (not counted): " + json.dumps(div, indent=1))
    text = "\n".join(lines); print(text)
    if a.out: open(a.out, "w").write(text + "\n")
    sys.exit(1 if fails else 0)

if __name__ == "__main__": main()
