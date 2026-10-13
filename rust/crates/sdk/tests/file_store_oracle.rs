//! Replays rust/parity/sdk/gen_file_store.mjs scripts against the Rust FileSessionStore: op results and the
//! exact on-disk tree (paths + bytes) must match TS. `__errorOneOf` records TS's Promise.all nondeterminism.
use pilotdeck_sdk::file_store::FileSessionStore;
use pilotdeck_sdk::snapshot::StoreError;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;

fn tree(dir: &Path, rel: &str, out: &mut Map<String, Value>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut names: Vec<String> = rd.flatten().map(|d| d.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for n in names {
        let p = dir.join(&n);
        let r = if rel.is_empty() { n.clone() } else { format!("{rel}/{n}") };
        if p.is_dir() {
            tree(&p, &r, out);
        } else {
            out.insert(r, json!(fs::read_to_string(&p).unwrap()));
        }
    }
}

#[test]
fn file_store_matches_ts() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/file_store.json")).unwrap();
    let base = std::env::temp_dir().join(format!("pd-rs-fstore-{}", std::process::id()));
    let mut bad = vec![];
    let mut n = 0;
    for (si, sc) in fx["scripts"].as_array().unwrap().iter().enumerate() {
        let root = base.join(format!("s{si}")).join("store");
        let _ = fs::remove_dir_all(&root);
        let rs = root.to_string_lossy().into_owned();
        let mut store = FileSessionStore::new(&rs).unwrap();
        let mut clock = 1000.0;
        for (i, (o, want)) in sc["ops"].as_array().unwrap().iter().zip(sc["out"].as_array().unwrap()).enumerate() {
            n += 1;
            let ents = |v: &Value| v.as_array().cloned().unwrap_or_default();
            let r: Result<Value, StoreError> = match o["op"].as_str().unwrap() {
                "now" => { clock = o["v"].as_f64().unwrap(); Ok(Value::Null) }
                "append" => store.append(&o["key"], &ents(&o["entries"]), clock).map(|_| Value::Null),
                "load" => store.load(&o["key"]).map(|x| x.map(Value::from).unwrap_or(Value::Null)),
                "listSessions" => store.list_sessions(&o["pk"]).map(|v| Value::Array(v.into_iter().map(|(s, m)| json!({"sessionId": s, "mtime": m})).collect())),
                "listSessionSummaries" => store.list_session_summaries(&o["pk"]).map(|v| Value::Array(v.iter().map(|s| s.to_json()).collect())),
                "listSubkeys" => store.list_subkeys(&o["key"]).map(|v| json!(v)),
                "delete" => store.delete(&o["key"]).map(|_| Value::Null),
                "export" => store.export_session(&o["key"]).map(|x| x.unwrap_or(Value::Null)),
                "import" => store.import_session(&o["snapshot"], o["mode"].as_str(), clock),
                "plant" => {
                    let p = root.join(o["rel"].as_str().unwrap());
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(&p, o["text"].as_str().unwrap()).unwrap();
                    Ok(Value::Null)
                }
                x => panic!("{x}"),
            };
            let ok = match (&r, want.get("__errorOneOf")) {
                (Err(e), Some(Value::Array(one_of))) => {
                    let m = e.message().replace(&rs, "<root>");
                    one_of.contains(&json!(m)) && want["code"] == json!(e.code())
                }
                (Err(e), _) => *want == json!({"__error": e.message().replace(&rs, "<root>"), "code": e.code()}),
                (Ok(v), _) => serde_json::to_string(v).unwrap() == serde_json::to_string(want).unwrap(),
            };
            if !ok {
                bad.push(format!("script {si} op {i} {o}\n  want {want}\n  got  {r:?}"));
            }
        }
        n += 1;
        let mut t = Map::new();
        tree(&root, "", &mut t);
        if Value::Object(t.clone()) != sc["tree"] {
            bad.push(format!("script {si} on-disk tree differs\n  want {}\n  got  {}", sc["tree"], Value::Object(t)));
        }
    }
    let _ = fs::remove_dir_all(&base);
    let ctor = FileSessionStore::new("  ").err().map(|e| json!({"__error": e.message(), "code": e.code()})).unwrap_or(Value::Null);
    if ctor != fx["emptyRootError"] {
        bad.push(format!("ctor {ctor}"));
    }
    eprintln!("sdk file-store oracle: {n} checks (ops + trees), {} mismatches", bad.len());
    for b in bad.iter().take(5) {
        eprintln!("{}", &b[..b.len().min(1500)]);
    }
    assert!(bad.is_empty());
}
