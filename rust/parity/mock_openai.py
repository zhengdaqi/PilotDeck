#!/usr/bin/env python3
"""Deterministic OpenAI-compatible chat-completions mock (stream + non-stream) for CLI parity.
Reply text: "MOCK_REPLY::<last user text, first 40 chars>". If the last user text contains
"FAIL_400" the mock answers HTTP 400 (non-retryable)."""
import json, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

def last_user(messages):
    for m in reversed(messages or []):
        if isinstance(m, dict) and m.get("role") == "user":
            c = m.get("content")
            if isinstance(c, list):
                c = "".join(p.get("text", "") for p in c if isinstance(p, dict))
            return str(c or "")
    return ""

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        self.send_response(200); self.send_header("Content-Type", "application/json"); self.end_headers()
        self.wfile.write(b'{"data":[{"id":"test"}]}')
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))) or b"{}")
        if "/chat/completions" not in self.path:
            self.send_response(404); self.end_headers(); return
        q = last_user(body.get("messages"))
        if "FAIL_400" in q:
            data = json.dumps({"error": {"message": "mock bad request", "type": "invalid_request_error"}}).encode()
            self.send_response(400); self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data); return
        if "SLOW" in q:
            import time; time.sleep(8)
        text = "MOCK_REPLY::" + q.strip().splitlines()[-1][:40] if q.strip() else "MOCK_REPLY::"
        usage = {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        if body.get("stream"):
            self.send_response(200); self.send_header("Content-Type", "text/event-stream"); self.end_headers()
            base = {"id": "c1", "object": "chat.completion.chunk", "created": 1, "model": "test"}
            for piece in [text[:8], text[8:]]:
                self.wfile.write(b"data: " + json.dumps(dict(base, choices=[{"index": 0, "delta": {"role": "assistant", "content": piece}, "finish_reason": None}])).encode() + b"\n\n")
            self.wfile.write(b"data: " + json.dumps(dict(base, choices=[{"index": 0, "delta": {}, "finish_reason": "stop"}], usage=usage)).encode() + b"\n\n")
            self.wfile.write(b"data: [DONE]\n\n")
            return
        data = json.dumps({"id": "c1", "object": "chat.completion", "created": 1, "model": "test",
                           "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
                           "usage": usage}).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data)

if __name__ == "__main__":
    s = ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H)
    s.serve_forever()
