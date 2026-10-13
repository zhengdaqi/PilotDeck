//! Fixture: rust/parity/ui_server/gen_web_update_command.mjs (real webUpdateCommand.js under node).
use pilotdeck_ui_server_core::model_probe::FetchFailure;
use pilotdeck_ui_server_core::web_update_command::*;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
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
const UUID: &str = "11111111-2222-4333-8444-555555555555";

struct Body {
    chunks: VecDeque<Vec<u8>>,
    error: bool,
}
impl BodyStream for Body {
    async fn next_chunk(&mut self) -> Option<Result<Vec<u8>, String>> {
        match self.chunks.pop_front() {
            Some(c) => Some(Ok(c)),
            None if self.error => {
                self.error = false;
                Some(Err("stream reset".into()))
            }
            None => None,
        }
    }
    async fn cancel(&mut self) -> Result<(), String> {
        // ReadableStream (highWaterMark 1) pulls ahead: once the queue is drained the pending error is stored.
        if self.chunks.is_empty() && self.error { Err("stream reset".into()) } else { Ok(()) }
    }
}
struct Host {
    s: Value,
    queues: HashMap<String, VecDeque<Value>>,
    project: String,
    now: f64,
    log: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    update_id: Option<String>,
}
fn bytes(v: &Value) -> Vec<u8> {
    v.as_array().unwrap().iter().map(|b| b.as_u64().unwrap() as u8).collect()
}
impl UpdateHost for Host {
    type Body = Body;
    async fn fetch(&mut self, url: &str, init: FetchInit<'_>) -> Result<(u16, Body), FetchFailure> {
        let route = url.split_once(":8080/").or_else(|| url.split_once(":3001/")).or_else(|| url.split_once(":1000/")).map(|(_, r)| r.to_string()).unwrap_or_else(|| url.rsplit_once('/').unwrap().1.to_string());
        let mut body = init.body.clone();
        if route == "api/update/apply" {
            let b = body.clone().unwrap();
            let id = serde_json::from_str::<Value>(&b).unwrap()["updateId"].as_str().unwrap().to_string();
            body = Some(b.replace(&id, "$ID"));
            self.update_id = Some(id);
        }
        self.log.borrow_mut().push(format!("fetch {url} {} {} {} {}", init.method.unwrap_or("-"), init.headers.map(|h| Value::Object(h.clone()).to_string()).unwrap_or("-".into()), body.unwrap_or("-".into()), init.redirect));
        let mut r = if route == "health" {
            self.s["health"].clone()
        } else {
            let q = self.queues.get_mut(route.trim_start_matches("api/update/"));
            q.and_then(|q| q.pop_front()).unwrap_or_else(|| json!({ "status": 500, "chunks": [b"{}".to_vec()] }))
        };
        if route == "api/update/status" {
            let text = String::from_utf8(bytes(&r["chunks"][0])).unwrap().replace("\"$ID\"", &json!(self.update_id.clone()).to_string());
            r["chunks"] = json!([text.into_bytes()]);
        }
        if let Some(t) = r.get("throw") {
            return Err(FetchFailure::Error { message: t["message"].as_str().unwrap().into(), code: None, cause: t.get("causeCode").map(|c| json!({ "code": c })) });
        }
        let chunks = r.get("chunks").and_then(Value::as_array).map(|a| a.iter().map(bytes).collect()).unwrap_or_default();
        Ok((r["status"].as_u64().unwrap() as u16, Body { chunks, error: r.get("streamError") == Some(&json!(true)) }))
    }
    fn now_ms(&mut self) -> f64 {
        self.now
    }
    async fn sleep(&mut self, ms: u64) {
        self.log.borrow_mut().push(format!("sleep {ms}"));
        self.now += self.s["clockStep"].as_f64().unwrap();
    }
    fn random_uuid(&mut self) -> String {
        UUID.into()
    }
    fn realpath_project_root(&mut self) -> Result<String, String> {
        Ok(self.project.clone())
    }
    async fn headers(&mut self) -> Result<Map<String, Value>, UpdateError> {
        Ok(json!({ "Authorization": "Bearer T" }).as_object().unwrap().clone())
    }
    fn offline_created(&mut self) {
        self.log.borrow_mut().push("offline".into());
    }
    async fn offline_check(&mut self) -> Result<Value, UpdateError> {
        self.log.borrow_mut().push("offline.check".into());
        Ok(self.s["offline"]["check"].clone())
    }
    async fn offline_apply(&mut self, target: &Value, output: &mut dyn FnMut(Value)) -> Result<(), UpdateError> {
        self.log.borrow_mut().push(format!("offline.apply {target}"));
        output(json!("offline applying"));
        if self.s["offline"]["applyThrows"] == json!(true) {
            return Err(UpdateError::Failure { reason: json!("applyFailed"), message: "offline failed".into() });
        }
        Ok(())
    }
}

#[test]
fn web_update_command_matches_ts() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/web_update_command.json")).unwrap();
    for r in fx["reasons"].as_array().unwrap() {
        assert_eq!(json!(describe_update_reason(&r["r"])), r["out"]);
    }
    for c in fx["cases"].as_array().unwrap() {
        let s = &c["s"];
        let queues = s["routes"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_array().unwrap().iter().cloned().collect())).collect();
        let mut h = Host { s: s.clone(), queues, project: c["project"].as_str().unwrap().into(), now: 1_000_000.0, log: Default::default(), update_id: None };
        let log = h.log.clone();
        let result = block_on(async {
            let client = create_command_client(&mut h, s["env"]["SERVER_PORT"].as_str()).await?;
            let mut out = |m: Value| log.borrow_mut().push(format!("out {m}"));
            run_web_update_command(&mut h, &client, s["checkOnly"] == json!(true), s["restart"] == json!(true), &mut out).await
        });
        let got = match result {
            Ok(code) => json!({ "code": code }),
            Err(UpdateError::Failure { reason, message }) => json!({ "reason": reason, "message": message }),
            Err(UpdateError::Plain { name, message }) => json!({ "plain": name, "message": message }),
        };
        assert_eq!(json!(*h.log.borrow()), c["log"], "{}", json!(s));
        assert_eq!(got, c["result"], "{}", json!(s));
    }
}

