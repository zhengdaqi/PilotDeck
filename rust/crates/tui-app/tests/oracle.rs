//! Differential test against the Ink oracle fixture (rust/parity/tui/gen_tui_app_fixture.mjs).
use pilotdeck_tui_app::{App, AppProps, Key, TuiGateway};
use serde_json::{json, Value};
use std::path::PathBuf;

struct Fake {
    spec: Value,
    files_dir: String,
    calls: Vec<Value>,
}
impl Fake {
    fn log(&mut self, m: &str, p: &Value) {
        self.calls.push(json!({"method": m, "params": p}));
    }
    fn subst(&self, v: &Value) -> Value {
        match v {
            Value::String(s) if self.spec["files"].get(s).is_some() => Value::from(format!("{}/{}.txt", self.files_dir, s.replace('@', ""))),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.subst(x)).collect()),
            Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), self.subst(x))).collect()),
            x => x.clone(),
        }
    }
}
impl TuiGateway for Fake {
    fn list_sessions(&mut self, p: Value) -> Result<Value, String> {
        self.log("listSessions", &p);
        let lim = p.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
        let s: Vec<Value> = self.spec["sessions"].as_array().unwrap().iter().take(lim).cloned().collect();
        Ok(json!({ "sessions": s }))
    }
    fn new_session(&mut self, p: Value) -> Result<Value, String> {
        self.log("newSession", &p);
        Ok(json!({"sessionKey": self.spec["newSessionKey"]}))
    }
    fn resume_session(&mut self, p: Value) {
        self.log("resumeSession", &p);
    }
    fn read_session_messages(&mut self, p: Value) -> Result<Value, String> {
        self.log("readSessionMessages", &p);
        let k = p["sessionKey"].as_str().unwrap_or("");
        match self.spec["messages"].get(k) {
            Some(m) => Ok(json!({"messages": m})),
            None => Err(format!("no session {k}")),
        }
    }
    fn submit_turn(&mut self, p: Value, on: &mut dyn FnMut(&Value)) -> Result<(), String> {
        self.log("submitTurn", &p);
        let msg = p["message"].as_str().unwrap_or("").to_string();
        if self.spec["throwTurn"] == Value::from(msg.clone()) {
            return Err(format!("{msg} failed"));
        }
        let evs = match self.spec["turns"].get(&msg) {
            Some(e) => self.subst(e),
            None => json!([{"type": "turn_started"}, {"type": "assistant_text_delta", "text": format!("echo: {msg}")}, {"type": "turn_completed"}]),
        };
        for e in evs.as_array().unwrap() {
            on(e);
        }
        Ok(())
    }
    fn permission_decide(&mut self, p: Value) {
        self.log("permissionDecide", &p);
    }
    fn abort_turn(&mut self, p: Value) {
        self.log("abortTurn", &p);
    }
}

fn read_perms(home: &PathBuf) -> Value {
    match std::fs::read_to_string(home.join("permissions.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        Some(mut v) => {
            v.as_object_mut().map(|o| o.remove("lastUpdated"));
            v
        }
        None => Value::Null,
    }
}

fn snapshot(app: &App) -> Value {
    json!({
        "state": serde_json::to_value(&app.state).unwrap(),
        "textInput": app.text_input.map(|t| json!({"cursorOffset": t.cursor_offset, "cursorWidth": t.cursor_width})),
        "viewerOffset": app.viewer_offset,
    })
}

#[test]
fn tui_app_matches_ink_oracle() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("tests/fixtures/tui_app.json")).unwrap()).unwrap();
    let spec: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("../../parity/tui/tui_app_spec.json")).unwrap()).unwrap();
    let files_dir = fx["filesDir"].as_str().unwrap().to_string();
    std::fs::create_dir_all(&files_dir).unwrap();
    for (k, n) in spec["fake"]["files"].as_object().unwrap() {
        let body: Vec<String> = (1..=n.as_u64().unwrap()).map(|i| format!("line {i}")).collect();
        std::fs::write(format!("{files_dir}/{}.txt", k.replace('@', "")), body.join("\n")).unwrap();
    }
    let mut failures = vec![];
    let mut checked = 0;
    for (sc, spec_sc) in fx["scenarios"].as_array().unwrap().iter().zip(spec["scenarios"].as_array().unwrap()) {
        let name = sc["name"].as_str().unwrap();
        let home = std::env::temp_dir().join(format!("pd-tui-app-rs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        if let Some(p) = spec_sc.get("permissions") {
            std::fs::write(home.join("permissions.json"), p.to_string()).unwrap();
        }
        if let Some(p) = spec_sc.get("permissionsRaw").and_then(Value::as_str) {
            std::fs::write(home.join("permissions.json"), p).unwrap();
        }
        let mut gw = Fake { spec: spec["fake"].clone(), files_dir: files_dir.clone(), calls: vec![] };
        let props = AppProps {
            connection: "in_process".into(),
            project_key: sc["props"]["projectKey"].as_str().map(str::to_string),
            session_key: sc["props"]["sessionKey"].as_str().map(str::to_string),
            cwd: fx["cwd"].as_str().unwrap().into(),
            pilot_home: home.clone(),
            columns: fx["columns"].as_u64().unwrap() as u16,
            rows: fx["rows"].as_u64().unwrap() as u16,
        };
        let mut app = App::new(props, &mut gw);
        let mut cmp = |label: String, want: &Value, got: &Value, failures: &mut Vec<String>| {
            checked += 1;
            if want != got {
                failures.push(format!("{name} {label}\n  want {want}\n  got  {got}"));
            }
        };
        let init = snapshot(&app);
        for k in ["state", "textInput", "viewerOffset"] {
            cmp(format!("initial.{k}"), &sc["initial"][k], &init[k], &mut failures);
        }
        cmp("initialPermissions".into(), &sc["initialPermissions"], &read_perms(&home), &mut failures);
        gw.calls.clear();
        for (i, st) in sc["steps"].as_array().unwrap().iter().enumerate() {
            let writes: Vec<&str> = st["writes"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
            let sb0 = app.scrollback.len();
            if writes == ["\u{3}"] {
                app.ctrl_c();
            }
            for e in st["events"].as_array().unwrap() {
                let key: Key = serde_json::from_value(e["key"].clone()).unwrap();
                app.handle(&mut gw, e["input"].as_str().unwrap(), &key);
            }
            let label = format!("step {i} {:?}", st["bytes"].as_str().unwrap());
            if st.get("exited").is_some() {
                cmp(format!("{label} exited"), &Value::Bool(true), &Value::Bool(app.exited), &mut failures);
            } else {
                cmp(format!("{label} exited"), &Value::Bool(false), &Value::Bool(app.exited), &mut failures);
                let snap = snapshot(&app);
                for k in ["state", "textInput", "viewerOffset"] {
                    cmp(format!("{label} {k}"), &st[k], &snap[k], &mut failures);
                }
            }
            cmp(format!("{label} calls"), &st["calls"], &Value::Array(std::mem::take(&mut gw.calls)), &mut failures);
            cmp(format!("{label} scrollback"), &st["scrollback"], &json!(app.scrollback[sb0..]), &mut failures);
            cmp(format!("{label} permissions"), &st["permissions"], &read_perms(&home), &mut failures);
        }
        cmp("exited".into(), &sc["exited"], &Value::Bool(app.exited), &mut failures);
        let _ = std::fs::remove_dir_all(&home);
    }
    eprintln!("tui-app oracle: {checked} comparisons, {} mismatches", failures.len());
    for f in failures.iter().take(15) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty());
}
