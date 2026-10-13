//! Fixture: rust/parity/ui_server/gen_release_service.mjs (real releaseService.js under node).
use pilotdeck_ui_server_core::release_service::*;
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
fn dec(v: &Value) -> Option<&Value> {
    if v.get("$undef").is_some() { None } else { Some(v) }
}
fn res<T: Into<Value>>(r: Result<T, ReleaseError>) -> Value {
    match r {
        Ok(v) => json!({ "v": v.into() }),
        Err(e) => json!({ "err": e.name, "message": e.message }),
    }
}
struct Host {
    response: Value,
    log: Vec<String>,
}
impl ReleaseHost for Host {
    async fn get(&mut self, url: &str) -> Result<(u16, String), ReleaseError> {
        self.log.push(format!("{url} {}", json!({ "Accept": "application/json", "User-Agent": "PilotDeck-Updater" })));
        Ok((self.response["status"].as_u64().unwrap() as u16, self.response["text"].as_str().unwrap().into()))
    }
}

#[test]
fn release_service_matches_ts() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/release_service.json")).unwrap();
    for c in fx["normalize"].as_array().unwrap() {
        assert_eq!(res(normalize_repository(dec(&c["r"]))), c["out"], "{c}");
    }
    for c in fx["versions"].as_array().unwrap() {
        assert_eq!(res(release_version(dec(&c["t"]))), c["out"], "{c}");
    }
    for c in fx["compares"].as_array().unwrap() {
        assert_eq!(res(compare_versions(dec(&c["a"]), dec(&c["b"]))), c["out"], "{c}");
    }
    for c in fx["releases"].as_array().unwrap() {
        let mut h = Host { response: c["response"].clone(), log: vec![] };
        let got = res(block_on(get_latest_release(&mut h, dec(&c["repository"]))));
        assert_eq!(json!(h.log), c["log"], "{c}");
        assert_eq!(got, c["result"], "{c}");
    }
}
