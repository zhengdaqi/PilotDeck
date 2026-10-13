//! Fixture: rust/parity/ui_server/gen_network_fetch.mjs (real src/network/fetch.ts under node).
use pilotdeck_ui_server_core::model_probe::FetchFailure;
use pilotdeck_ui_server_core::network_fetch::*;
use serde_json::{json, Value};
use std::future::Future;

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}
fn num(v: &Value) -> f64 {
    match v {
        Value::Object(o) => o["$num"].as_str().unwrap().parse::<f64>().unwrap_or(f64::NAN),
        v => v.as_f64().unwrap(),
    }
}
fn retry_of(r: &Value) -> RetryOptions {
    RetryOptions {
        max_retries: r.get("maxRetries").map(num),
        base_delay_ms: r.get("baseDelayMs").map(num),
        max_delay_ms: r.get("maxDelayMs").map(num),
        retry_on_post: r.get("retryOnPost") == Some(&json!(true)),
        retry_statuses: r.get("retryStatuses").map(|a| a.as_array().unwrap().iter().map(num).collect()),
    }
}
struct Host {
    script: Vec<Value>,
    k: usize,
    randoms: Vec<f64>,
    ri: usize,
    now: f64,
    log: Vec<String>,
}
impl FetchHost for Host {
    async fn fetch(&mut self, method: Option<&str>) -> Result<HttpResponse, FetchFailure> {
        self.log.push(format!("fetch {}", method.unwrap_or("-")));
        let s = self.script.get(self.k).cloned();
        self.k += 1;
        let Some(s) = s else { return Ok(HttpResponse { status: 404, status_text: "Not Found".into(), retry_after: None, text: String::new() }) };
        if let Some(e) = s.get("throw") {
            return Err(FetchFailure::Error { message: e["message"].as_str().unwrap().into(), code: e.get("code").cloned(), cause: e.get("cause").cloned() });
        }
        let status = s["status"].as_u64().unwrap() as u16;
        let st = match status { 200 => "OK", 201 => "Created", 204 => "No Content", 400 => "Bad Request", 404 => "Not Found", 408 => "Request Timeout", 409 => "Conflict", 425 => "Too Early", 429 => "Too Many Requests", 500 => "Internal Server Error", 502 => "Bad Gateway", 503 => "Service Unavailable", 504 => "Gateway Timeout", _ => "I'm a teapot" };
        let text = if status == 204 { String::new() } else { s["text"].as_str().unwrap().to_string() };
        Ok(HttpResponse { status, status_text: st.into(), retry_after: s["retryAfter"].as_str().map(|v| v.trim_matches([' ', '\t', '\r', '\n']).to_string()), text })
    }
    async fn sleep(&mut self, ms: f64) {
        self.log.push(format!("sleep {}", delay_label(ms)));
    }
    fn random(&mut self) -> f64 {
        let r = self.randoms[self.ri % self.randoms.len()];
        self.ri += 1;
        r
    }
    fn now_ms(&mut self) -> f64 {
        self.now
    }
}

#[test]
fn network_fetch_matches_ts() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/network_fetch.json")).unwrap();
    let now = fx["now"].as_f64().unwrap();
    for c in fx["cases"].as_array().unwrap() {
        let retry = retry_of(&c["retry"]);
        let mut h = Host { script: c["script"].as_array().unwrap().clone(), k: 0, randoms: c["randoms"].as_array().unwrap().iter().map(num).collect(), ri: 0, now, log: vec![] };
        let method = c["method"].as_str();
        let err = |e: pilotdeck_ui_server_core::model_probe::NetworkFetchError| json!({ "err": { "name": "NetworkFetchError", "code": e.code, "message": e.message } });
        let result = if c["json"] == json!(true) {
            let exp: Option<Vec<u16>> = c["expected"].as_array().map(|a| a.iter().map(|x| x.as_u64().unwrap() as u16).collect());
            match block_on(network_fetch_json(&mut h, "https://h/x", method, &retry, exp.as_deref())) {
                Ok((r, j)) => json!({ "ok": { "status": r.status, "json": j, "text": r.text } }),
                Err(e) => err(e),
            }
        } else {
            match block_on(network_fetch(&mut h, method, &retry)) {
                Ok(r) => json!({ "ok": { "status": r.status, "text": r.text } }),
                Err(e) => err(e),
            }
        };
        assert_eq!(json!(h.log), c["log"], "{c}");
        assert_eq!(result, c["result"], "{c}");
    }
    for c in fx["backoff"].as_array().unwrap() {
        let r = c["random"].as_f64().unwrap();
        let got = jittered_backoff(c["attempt"].as_f64().unwrap(), &retry_of(&c["retry"]), c["header"].as_str(), now, &mut || r);
        assert_eq!(got, c["out"].as_f64().unwrap(), "{c}");
    }
    for c in fx["codes"].as_array().unwrap() {
        assert_eq!(json!(is_retryable_network_code(c[0].as_str().unwrap())), c[1]);
    }
}
