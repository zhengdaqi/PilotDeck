//! Shared-vector runner: used by `cargo test` and by the browser self-test build (same code).
use crate::workspace_file_mention as w;
use serde_json::{json, Value};

/// Returns (checked, failures).
pub fn run_workspace_file_mention(fixture: &str) -> (usize, Vec<String>) {
    let v: Value = serde_json::from_str(fixture).expect("fixture json");
    let s = |x: &Value| x.as_str().unwrap_or("").to_string();
    let mut n = 0;
    let mut bad = vec![];
    let mut check = |kind: &str, case: &Value, got: Value| {
        n += 1;
        if case["out"] != got {
            bad.push(format!("{kind} {case} got {got}"));
        }
    };
    for c in v["mentionRequest"].as_array().unwrap() {
        check("mentionRequest", c, json!(w::is_workspace_file_mention_request(&c["v"])));
    }
    for c in v["has"].as_array().unwrap() {
        check("has", c, json!(w::has_workspace_file_mention(&s(&c["input"]), &s(&c["rel"]))));
    }
    for c in v["relative"].as_array().unwrap() {
        check("relative", c, json!(w::get_workspace_relative_path(&s(&c["f"]), &s(&c["r"]))));
    }
    for c in v["canonical"].as_array().unwrap() {
        check("canonical", c, json!(w::canonicalize_workspace_file_path(&s(&c["f"]), &s(&c["r"]))));
    }
    for c in v["identity"].as_array().unwrap() {
        check("identity", c, json!(w::get_workspace_file_identity(&s(&c["f"]), &s(&c["r"]))));
    }
    for c in v["atOrBelow"].as_array().unwrap() {
        check("atOrBelow", c, json!(w::is_workspace_path_at_or_below(&s(&c["c"]), &s(&c["p"]), &s(&c["r"]))));
    }
    for c in v["insert"].as_array().unwrap() {
        let r = w::insert_workspace_file_mention(&s(&c["input"]), &s(&c["rel"]), c["cur"].as_i64().unwrap());
        check("insert", c, json!({"input": r.input, "cursorPosition": r.cursor_position, "alreadyPresent": r.already_present}));
    }
    (n, bad)
}

/// UI batch 2 (officePreviewStatus, ime, externalFileDrop, dateUtils). `date_parse` is the host's `Date.parse`:
/// in the browser it is checked against the TS-recorded value; natively the recorded value is used.
pub fn run_batch2(fixture: &str, date_parse: Option<&dyn Fn(&str) -> f64>) -> (usize, Vec<String>) {
    use crate::{date_utils as d, external_file_drop as x, ime, office_preview as o};
    let v: Value = serde_json::from_str(fixture).expect("fixture json");
    let mut n = 0;
    let mut bad = vec![];
    let mut check = |kind: &str, case: &Value, want: &Value, got: Value| {
        n += 1;
        if *want != got {
            bad.push(format!("{kind} {case} got {got}"));
        }
    };
    let opt = |c: &Value, k: &str, u: &str| if c[u].as_bool() == Some(true) { None } else { Some(c[k].clone()) };
    for c in v["office"].as_array().unwrap() {
        let (s, b) = (opt(c, "s", "sUndef"), opt(c, "b", "bUndef"));
        check("office.norm", c, &c["norm"], json!(o::normalize_office_preview_service(s.as_ref())));
        check("office.sig", c, &c["sig"], json!(o::office_preview_renderer_signature(s.as_ref(), b.as_ref())));
    }
    for c in v["officeConfig"].as_array().unwrap() {
        check("officeConfig", c, &c["out"], json!(o::office_preview_renderer_signature_from_config(&c["c"])));
    }
    for c in v["ime"].as_array().unwrap() {
        check("ime.comp", c, &c["comp"], json!(ime::is_ime_composition_event(&c["e"])));
        check("ime.enter", c, &c["enter"], json!(ime::is_ime_enter_event(&c["e"])));
    }
    for c in v["drag"].as_array().unwrap() {
        let dt = opt(c, "dt", "dtUndef");
        check("drag", c, &c["out"], json!(x::is_external_file_drag(dt.as_ref())));
    }
    for c in v["dropPath"].as_array().unwrap() {
        check("dropPath", c, &c["out"], json!(x::resolve_external_file_drop_target_path(c["p"].as_str(), c["t"].as_str(), c["r"].as_str().unwrap())));
    }
    for c in v["timeAgo"].as_array().unwrap() {
        let recorded = c["parsed"].as_f64().unwrap_or(f64::NAN);
        let parsed = match date_parse {
            Some(p) => {
                let host = p(c["s"].as_str().unwrap());
                check("Date.parse", c, &json!(c["parsed"]), if host.is_nan() { Value::Null } else { json!(host as i64) });
                host
            }
            None => recorded,
        };
        let t = d::format_time_ago(parsed, c["now"].as_f64().unwrap());
        let keyed = match &t {
            d::TimeAgo::Key(k, cnt) => json!([k, cnt]),
            d::TimeAgo::LocaleDate(ms) => json!({"localeDate": *ms as i64}),
        };
        check("timeAgo", c, &c["out"], keyed.clone());
        check("timeAgo.plain", c, &c["plain"], d::english(&t).map(Value::String).unwrap_or(keyed));
    }
    (n, bad)
}

/// UI batch 3 (reloadOnChunkError, frameBatcher, uploadFormData, desktopUpdates).
pub fn run_batch3(fixture: &str) -> (usize, Vec<String>) {
    use crate::chunk_reload::{self as c, Thrown};
    use crate::frame_batcher::{FrameBatcher, FrameHost};
    use crate::js_value::number_to_string as ns;
    use crate::upload_form_data as u;
    let v: Value = serde_json::from_str(fixture).expect("fixture json");
    let mut n = 0;
    let mut bad = vec![];
    let mut check = |kind: &str, case: &Value, want: &Value, got: Value| {
        n += 1;
        if *want != got {
            bad.push(format!("{kind} {case} got {got}"));
        }
    };
    let thrown = |e: &Value| -> Thrown {
        match e {
            Value::Object(o) if o.contains_key("$undef") => Thrown::Value(None),
            Value::Object(o) if o.contains_key("$error") => {
                Thrown::Error { name: o["$error"]["name"].as_str().unwrap().into(), message: o["$error"]["message"].as_str().unwrap().into() }
            }
            v => Thrown::Value(Some(v.clone())),
        }
    };
    for case in v["chunk"].as_array().unwrap() {
        check("chunk", case, &case["out"], json!(c::is_dynamic_import_load_error(&thrown(&case["e"]))));
    }
    struct Win {
        store: Option<String>,
        st: Value,
        log: Vec<String>,
    }
    impl c::ReloadWindow for Win {
        fn get_item(&mut self, k: &str) -> Result<Option<String>, ()> {
            self.log.push(format!("get {k}"));
            if self.st["getThrows"] == true {
                return Err(());
            }
            Ok(self.store.clone())
        }
        fn set_item(&mut self, k: &str, val: &str) -> Result<(), ()> {
            self.log.push(format!("set {k} {val}"));
            if self.st["setThrows"] == true {
                return Err(());
            }
            self.store = Some(val.into());
            Ok(())
        }
        fn remove_item(&mut self, k: &str) -> Result<(), ()> {
            self.log.push(format!("remove {k}"));
            if self.st["removeThrows"] == true {
                return Err(());
            }
            self.store = None;
            Ok(())
        }
        fn reload(&mut self) {
            self.log.push("reload".into());
        }
    }
    for case in v["reload"].as_array().unwrap() {
        let e = thrown(&case["e"]);
        let mut w = Win { store: case["st"]["marker"].as_str().map(str::to_string), st: case["st"].clone(), log: vec![] };
        let r1 = c::reload_once_for_dynamic_import_error(&e, &mut w);
        let r2 = c::reload_once_for_dynamic_import_error(&e, &mut w);
        c::clear_dynamic_import_reload_marker(&mut w);
        let r3 = c::reload_once_for_dynamic_import_error(&e, &mut w);
        check("reload", case, &json!({ "out": case["out"], "log": case["log"] }), json!({ "out": [r1, r2, r3], "log": w.log }));
    }
    struct Frames {
        seq: u64,
        live: Vec<u64>,
        log: Vec<String>,
    }
    impl FrameHost for Frames {
        fn request_animation_frame(&mut self) -> u64 {
            self.seq += 1;
            self.live.push(self.seq);
            self.log.push(format!("raf {}", self.seq));
            self.seq
        }
        fn cancel_animation_frame(&mut self, id: u64) {
            self.live.retain(|x| *x != id);
            self.log.push(format!("caf {id}"));
        }
    }
    for case in v["frame"].as_array().unwrap() {
        let mut h = Frames { seq: 0, live: vec![], log: vec![] };
        let mut b: FrameBatcher<Value> = FrameBatcher::new();
        let mut applied: Vec<String> = vec![];
        for op in case["ops"].as_array().unwrap() {
            h.log.push(format!("> {op}"));
            let mut apply = |x: Value| applied.push(format!("apply {x}"));
            match op["op"].as_str().unwrap() {
                "schedule" => b.schedule(&mut h, op["v"].clone()),
                "flush" => b.flush(&mut h, &mut apply),
                "cancel" => b.cancel(&mut h),
                _ => {
                    let id = op["id"].as_u64().unwrap();
                    if h.live.contains(&id) {
                        h.live.retain(|x| *x != id);
                        b.flush(&mut h, &mut apply);
                    }
                }
            }
            h.log.append(&mut applied);
        }
        check("frame", case, &case["log"], json!(h.log));
    }
    for case in v["progress"].as_array().unwrap() {
        let known = case["known"].as_f64().unwrap();
        let mut log = vec![format!("auth {}", u::authorization_header(case["isPlatform"] == true, case["stored"].as_str()).unwrap_or_else(|| "none".into()))];
        for ev in case["events"].as_array().unwrap() {
            if let Some((a, b2, c2)) = u::progress_event(ev["lengthComputable"] == true, ev["loaded"].as_f64().unwrap(), ev["total"].as_f64().unwrap(), known) {
                log.push(format!("progress {} {} {}", ns(a), ns(b2), ns(c2)));
            }
        }
        if case["onload"] == true {
            let (a, b2, c2) = u::upload_complete(known);
            log.push(format!("progress {} {} {}", ns(a), ns(b2), ns(c2)));
            if case["transferredCb"] == true {
                log.push("transferred".into());
            }
        }
        let r = u::response(case["status"].as_u64().unwrap() as u16, case["text"].as_str().unwrap(), case["token"].as_str());
        log.push(format!("resolve {}", json!({ "ok": r.ok, "status": r.status, "body": r.body })));
        let token = r.store_token.or_else(|| case["stored"].as_str().map(str::to_string));
        log.push(format!("token {}", token.unwrap_or_else(|| "none".into())));
        check("upload", case, &case["log"], json!(log));
    }
    for case in v["desktop"].as_array().unwrap() {
        let keys: Option<Vec<&str>> = case["keys"].as_array().map(|a| a.iter().map(|k| k.as_str().unwrap()).collect());
        let got = match crate::desktop_updates::desktop_updates(keys.as_deref()) {
            Ok(()) => "ok",
            Err(e) => e,
        };
        check("desktop", case, &case["out"], json!(got));
    }
    (n, bad)
}
