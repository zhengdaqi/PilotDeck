//! Replays the TS op script (rust/parity/sdk/gen_session_store.mjs) against the Rust port.
use pilotdeck_sdk::session_store::*;
use serde_json::{json, Value};

fn entries(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

#[test]
fn session_store_matches_ts() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/session_store.json")).unwrap();
    let ops = fx["ops"].as_array().unwrap();
    let expected = fx["expected"].as_array().unwrap();
    let mut stores: Vec<InMemorySessionStore> = vec![];
    let mut clock: i64 = 0;
    let mut bad = vec![];
    for (i, o) in ops.iter().enumerate() {
        let s = o["s"].as_u64().unwrap_or(0) as usize;
        let key = SessionKey::from_json(&o["key"]);
        let got: Value = match o["op"].as_str().unwrap() {
            "new" => {
                stores.push(InMemorySessionStore::new());
                json!(stores.len() - 1)
            }
            "now" => {
                clock = o["v"].as_i64().unwrap();
                Value::Null
            }
            "append" => {
                stores[s].append(&key, &entries(&o["entries"]), clock);
                Value::Null
            }
            "load" => stores[s].load(&key).map(Value::from).unwrap_or(Value::Null),
            "listSessions" => Value::Array(
                stores[s]
                    .list_sessions(o["pk"].as_str().unwrap())
                    .into_iter()
                    .map(|(id, m)| json!({"sessionId": id, "mtime": m}))
                    .collect(),
            ),
            "listSessionSummaries" => Value::Array(
                stores[s].list_session_summaries(o["pk"].as_str().unwrap()).iter().map(|x| x.to_json()).collect(),
            ),
            "listSubkeys" => json!(stores[s].list_subkeys(o["pk"].as_str().unwrap(), o["sid"].as_str().unwrap())),
            "delete" => {
                stores[s].delete(&key);
                Value::Null
            }
            "import" => {
                let opts = &o["opts"];
                let c = clock;
                import_session_to_store(
                    &mut stores[s],
                    o["sid"].as_str().unwrap(),
                    &entries(&o["entries"]),
                    opts["projectKey"].as_str().unwrap(),
                    opts.get("subpath").and_then(|v| v.as_str()),
                    opts.get("batchSize").and_then(|v| v.as_f64()),
                    || c,
                );
                Value::Null
            }
            "fold" => {
                let prev = o.get("prev").filter(|p| !p.is_null()).map(SessionSummary::from_json);
                let mtime = o.get("opts").and_then(|x| x.get("mtime")).and_then(|x| x.as_i64());
                fold_session_summary(prev.as_ref(), &key, &entries(&o["entries"]), mtime).to_json()
            }
            "info" => Value::Object(summary_to_session_info(&SessionSummary::from_json(&o["summary"]))),
            other => panic!("op {other}"),
        };
        if got != expected[i] || serde_json::to_string(&got).unwrap() != serde_json::to_string(&expected[i]).unwrap() {
            bad.push(format!("#{i} {o}\n  want {}\n  got  {got}", expected[i]));
        }
    }
    eprintln!("sdk session-store oracle: {} ops, {} mismatches", ops.len(), bad.len());
    for b in bad.iter().take(5) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty());
}
