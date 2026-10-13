//! Port of `ui/server/services/connectionTestTasks.js`, sans-IO.
//!
//! The JS module drives two kinds of promises: the connection test `run(signal)` returned by `prepare`, and
//! `persist(userId, testId)`. Here the host hands back a token for each, and reports the outcome later with
//! [`ConnectionTestTasks::run_settled`] / [`ConnectionTestTasks::persist_settled`]. The 60 s prune interval is
//! [`ConnectionTestTasks::tick`]. Task objects keep JS key order (`serde_json` with `preserve_order`).
use serde_json::{Map, Value};

const BUSY: [&str; 4] = ["testing", "savingTest", "manual", "cancelling"];

/// An `Error` with optional `status` / `code` own properties (what the JS code reads off thrown values).
#[derive(Clone, Debug, PartialEq)]
pub struct JsError {
    pub message: String,
    pub status: Option<Value>,
    pub code: Option<Value>,
}

impl JsError {
    fn fail(message: impl Into<String>, status: i64, code: &str) -> Self {
        JsError { message: message.into(), status: Some(Value::from(status)), code: Some(Value::from(code)) }
    }
    fn busy(message: &str) -> Self {
        Self::fail(message, 409, "TEST_BUSY")
    }
}

/// The callbacks given to `createConnectionTestTasks`, plus the clock and `randomUUID`.
pub trait ConnectionTestHost {
    fn now(&mut self) -> f64;
    fn new_id(&mut self) -> String;
    /// `prepare(body, userId, true)`: validation; an error is thrown to the caller of `start`.
    fn prepare(&mut self, body: &Value, user_id: &str) -> Result<(), JsError>;
    /// Calls the prepared `run(signal)`; returns a token later passed to `run_settled`.
    fn start_run(&mut self) -> u64;
    /// `persist(userId, testId)`; returns a token later passed to `persist_settled`.
    fn persist(&mut self, user_id: &str, test_id: Option<&Value>) -> u64;
    /// `getRecord(userId, testId)` -> `{ record, reason }`.
    fn get_record(&mut self, user_id: &str, test_id: Option<&Value>) -> (Option<Value>, String);
    fn apply_image(&mut self, record: &Value, body: &Value) -> Value;
    fn is_current(&mut self, user_id: &str, task: &Map<String, Value>) -> bool;
}

struct Task {
    user: String,
    /// Own enumerable keys except `controller`; `result` is `None` while undefined.
    fields: Map<String, Value>,
    result: Option<Value>,
    aborted: bool,
}

impl Task {
    fn status(&self) -> &str {
        self.fields.get("status").and_then(Value::as_str).unwrap_or("")
    }
}

pub struct ConnectionTestTasks {
    arena: Vec<Task>,
    /// `users: Map<userId, Map<key, task>>` in insertion order (re-set keeps the slot).
    users: Vec<(String, Vec<(String, usize)>)>,
    runs: Vec<(u64, usize)>,
    persists: Vec<(u64, usize)>,
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `x.prop` for the values the module reads: objects give their member, everything else `undefined`.
fn member<'v>(v: Option<&'v Value>, k: &str) -> Option<&'v Value> {
    v.and_then(|v| v.as_object()).and_then(|o| o.get(k))
}

/// V8's TypeError for `x.prop` on null/undefined.
fn read_error(v: Option<&Value>, prop: &str) -> Option<JsError> {
    let what = match v {
        None => "undefined",
        Some(Value::Null) => "null",
        _ => return None,
    };
    Some(JsError { message: format!("Cannot read properties of {what} (reading '{prop}')"), status: None, code: None })
}

fn js_key(v: Option<&Value>) -> String {
    // JSON.stringify([providerId, modelId || null]); undefined inside an array becomes null.
    v.map(Value::to_string).unwrap_or_else(|| "null".into())
}

impl Default for ConnectionTestTasks {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionTestTasks {
    pub fn new() -> Self {
        ConnectionTestTasks { arena: vec![], users: vec![], runs: vec![], persists: vec![] }
    }

    fn update(&mut self, h: &mut dyn ConnectionTestHost, t: usize, patch: &[(&str, Value)]) {
        let now = h.now();
        let task = &mut self.arena[t];
        for (k, v) in patch {
            task.fields.insert((*k).into(), v.clone());
        }
        task.fields.insert("updatedAt".into(), num(now));
    }

    fn set_result(&mut self, h: &mut dyn ConnectionTestHost, t: usize, result: Option<Value>) {
        self.arena[t].result = result;
        self.update(h, t, &[]);
    }

    fn public(&self, t: usize) -> Value {
        let task = &self.arena[t];
        let mut m = task.fields.clone();
        if let Some(r) = &task.result {
            m.insert("result".into(), r.clone());
        }
        Value::Object(m)
    }

    /// The `setInterval(prune, 60_000)` callback.
    pub fn tick(&mut self, h: &mut dyn ConnectionTestHost) {
        self.prune(h);
    }

    fn prune(&mut self, h: &mut dyn ConnectionTestHost) {
        let mut ui = 0;
        while ui < self.users.len() {
            let mut ti = 0;
            while ti < self.users[ui].1.len() {
                let t = self.users[ui].1[ti].1;
                let age = h.now() - self.arena[t].fields["updatedAt"].as_f64().unwrap_or(f64::NAN);
                let task = &mut self.arena[t];
                if matches!(task.status(), "manual" | "saveError") && age > 10.0 * 60_000.0 {
                    task.fields.insert("status".into(), "error".into());
                    task.fields.insert("code".into(), "TEST_EXPIRED".into());
                    task.fields.insert("message".into(), "Connection test has expired.".into());
                }
                if !BUSY.contains(&task.status()) && age > 60.0 * 60_000.0 {
                    self.users[ui].1.remove(ti);
                } else {
                    ti += 1;
                }
            }
            if self.users[ui].1.is_empty() {
                self.users.remove(ui);
            } else {
                ui += 1;
            }
        }
    }

    fn user_tasks(&self, user: &str) -> Vec<usize> {
        self.users.iter().find(|(u, _)| u == user).map(|(_, ts)| ts.iter().map(|(_, t)| *t).collect()).unwrap_or_default()
    }

    fn active(&self, user: &str) -> bool {
        self.user_tasks(user).into_iter().any(|t| BUSY.contains(&self.arena[t].status()))
    }

    fn lookup(&mut self, h: &mut dyn ConnectionTestHost, user: &str, id: &str) -> Result<usize, JsError> {
        self.prune(h);
        self.user_tasks(user)
            .into_iter()
            .find(|&t| self.arena[t].fields.get("id").and_then(Value::as_str) == Some(id))
            .ok_or_else(|| JsError::fail("Connection test task was not found.", 404, "TEST_NOT_FOUND"))
    }

    /// `save(userId, task)` up to its `await persist(...)`.
    fn save(&mut self, h: &mut dyn ConnectionTestHost, t: usize) {
        self.update(h, t, &[("status", "savingTest".into()), ("message", "".into()), ("code", "".into())]);
        let user = self.arena[t].user.clone();
        let test_id = member(self.arena[t].result.as_ref(), "testId").cloned();
        let token = h.persist(&user, test_id.as_ref());
        self.persists.push((token, t));
    }

    pub fn list(&mut self, h: &mut dyn ConnectionTestHost, user: &str) -> Value {
        self.prune(h);
        let mut out = vec![];
        for t in self.user_tasks(user) {
            if self.arena[t].status() != "success" || h.is_current(user, &self.arena[t].fields) {
                out.push(self.public(t));
            }
        }
        Value::Array(out)
    }

    pub fn start(&mut self, h: &mut dyn ConnectionTestHost, user: &str, body: &Value, model_id: Option<&Value>) -> Result<Value, JsError> {
        self.prune(h);
        if self.active(user) {
            return Err(JsError::busy("Another connection test is in progress."));
        }
        h.prepare(body, user)?;
        let model = model_id.filter(|m| truthy(Some(m)));
        let mut fields = Map::new();
        fields.insert("id".into(), h.new_id().into());
        let provider = member(Some(body), "providerId");
        if let Some(p) = provider {
            fields.insert("providerId".into(), p.clone());
        }
        if let Some(m) = model {
            fields.insert("modelId".into(), m.clone());
        }
        fields.insert("status".into(), "testing".into());
        fields.insert("message".into(), "".into());
        fields.insert("code".into(), "".into());
        fields.insert("updatedAt".into(), num(h.now()));
        let t = self.arena.len();
        self.arena.push(Task { user: user.into(), fields, result: Some(Value::Null), aborted: false });
        // `result` sits between `status` and `message` on the task but publicTask moves it last anyway.
        let key = format!("[{},{}]", js_key(provider), js_key(model));
        let ui = match self.users.iter().position(|(u, _)| u == user) {
            Some(i) => i,
            None => {
                self.users.push((user.into(), vec![]));
                self.users.len() - 1
            }
        };
        let tasks = &mut self.users[ui].1;
        match tasks.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = t,
            None => tasks.push((key, t)),
        }
        let token = h.start_run();
        self.runs.push((token, t));
        Ok(self.public(t))
    }

    /// The `run(signal)` promise settled: `Ok(result)` (None = undefined) or a rejection.
    pub fn run_settled(&mut self, h: &mut dyn ConnectionTestHost, token: u64, outcome: Result<Option<Value>, JsError>) {
        let Some(i) = self.runs.iter().position(|(k, _)| *k == token) else { return };
        let (_, t) = self.runs.remove(i);
        let caught: Option<JsError> = match outcome {
            Err(e) => Some(e),
            Ok(_) if self.arena[t].aborted => None,
            Ok(result) => self.after_run(h, t, result).err(),
        };
        if let Some(e) = caught {
            if !self.arena[t].aborted {
                let code = e.code.filter(|c| truthy(Some(c))).unwrap_or_else(|| "TEST_FAILED".into());
                self.update(h, t, &[("status", "error".into()), ("message", e.message.into()), ("code", code)]);
            }
        }
        // `finally`. On the save path JS runs it once persist settles, but the task cannot have been aborted
        // by then (cancel throws while savingTest and only aborts a testing task), so running it now is the same.
        if self.arena[t].aborted {
            self.update(h, t, &[("status", "cancelled".into()), ("message", "".into()), ("code", "".into())]);
        }
    }

    fn after_run(&mut self, h: &mut dyn ConnectionTestHost, t: usize, result: Option<Value>) -> Result<(), JsError> {
        self.set_result(h, t, result.clone());
        let user = self.arena[t].user.clone();
        if self.arena[t].fields.contains_key("modelId") {
            if !h.is_current(&user, &self.arena[t].fields) {
                self.arena[t].result = Some(Value::Null);
                self.update(
                    h,
                    t,
                    &[
                        ("status", "error".into()),
                        ("code", "CONFIGURATION_MISMATCH".into()),
                        ("message", "The model connection configuration changed during testing.".into()),
                    ],
                );
            } else {
                if let Some(e) = read_error(result.as_ref(), "status") {
                    return Err(e);
                }
                let failed = member(result.as_ref(), "status").and_then(Value::as_str) == Some("failed");
                self.update(h, t, &[("status", if failed { "error" } else { "success" }.into())]);
            }
            return Ok(());
        }
        if let Some(e) = read_error(result.as_ref(), "status") {
            return Err(e);
        }
        let r = result.as_ref();
        if member(r, "status").and_then(Value::as_str) == Some("passed") {
            self.save(h, t);
        } else if truthy(member(r, "manualInputRequired")) {
            self.update(h, t, &[("status", "manual".into())]);
        } else {
            let err = member(r, "error");
            let msg = member(err, "message").filter(|m| truthy(Some(m))).cloned().unwrap_or_else(|| "Connection failed.".into());
            let code = member(err, "code").filter(|c| truthy(Some(c))).cloned().unwrap_or_else(|| "TEST_FAILED".into());
            self.update(h, t, &[("status", "error".into()), ("message", msg), ("code", code)]);
        }
        Ok(())
    }
}

impl ConnectionTestTasks {
    /// A `persist(...)` promise settled.
    pub fn persist_settled(&mut self, h: &mut dyn ConnectionTestHost, token: u64, outcome: Result<(), JsError>) {
        let Some(i) = self.persists.iter().position(|(k, _)| *k == token) else { return };
        let (_, t) = self.persists.remove(i);
        match outcome {
            Ok(()) => self.update(h, t, &[("status", "success".into())]),
            Err(e) => {
                let expired = e.code.as_ref().and_then(Value::as_str) == Some("TEST_EXPIRED");
                let code = e.code.filter(|c| truthy(Some(c))).unwrap_or_else(|| "TEST_SAVE_FAILED".into());
                self.update(h, t, &[("status", if expired { "error" } else { "saveError" }.into()), ("message", e.message.into()), ("code", code)]);
            }
        }
    }

    pub fn retry(&mut self, h: &mut dyn ConnectionTestHost, user: &str, id: &str) -> Result<Value, JsError> {
        let t = self.lookup(h, user, id)?;
        if self.active(user) || self.arena[t].status() != "saveError" {
            return Err(JsError::busy("Another connection test is in progress."));
        }
        self.save(h, t);
        Ok(self.public(t))
    }

    pub fn confirm(&mut self, h: &mut dyn ConnectionTestHost, user: &str, id: &str, body: &Value) -> Result<Value, JsError> {
        let t = self.lookup(h, user, id)?;
        if self.arena[t].status() != "manual" {
            return Err(JsError::busy("This task is not waiting for image capabilities."));
        }
        let test_id = member(self.arena[t].result.as_ref(), "testId").cloned();
        let (record, reason) = h.get_record(user, test_id.as_ref());
        let Some(record) = record else {
            self.update(h, t, &[("status", "error".into()), ("code", "TEST_EXPIRED".into()), ("message", "Connection test has expired.".into())]);
            return Err(JsError::fail(reason, 410, "TEST_EXPIRED"));
        };
        let result = h.apply_image(&record, body);
        self.set_result(h, t, Some(result));
        self.save(h, t);
        Ok(self.public(t))
    }

    pub fn acknowledge(&mut self, h: &mut dyn ConnectionTestHost, user: &str, id: &str) -> Result<Value, JsError> {
        let t = self.lookup(h, user, id)?;
        if !truthy(self.arena[t].fields.get("modelId")) || BUSY.contains(&self.arena[t].status()) {
            return Err(JsError::busy("This model test has not finished."));
        }
        self.update(h, t, &[("acknowledged", true.into())]);
        Ok(self.public(t))
    }

    pub fn cancel(&mut self, h: &mut dyn ConnectionTestHost, user: &str, id: &str) -> Result<Value, JsError> {
        let t = self.lookup(h, user, id)?;
        match self.arena[t].status() {
            "savingTest" => return Err(JsError::busy("Test results are being saved.")),
            "testing" => {
                self.update(h, t, &[("status", "cancelling".into())]);
                self.arena[t].aborted = true;
            }
            "manual" | "saveError" => self.update(h, t, &[("status", "cancelled".into())]),
            _ => {}
        }
        Ok(self.public(t))
    }
}

fn num(f: f64) -> Value {
    if f.fract() == 0.0 && f.abs() < 9.0e15 {
        Value::from(f as i64)
    } else {
        Value::from(f)
    }
}
