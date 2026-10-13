//! Port of `packages/sdk/src/session-store.ts`: `InMemorySessionStore`, `foldSessionSummary`,
//! `summaryToSessionInfo`, `importSessionToStore`. JS semantics kept: Map insertion order,
//! truthy `uuid` dedupe (SameValueZero; object uuids never match), stable mtime-desc sort.
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct SessionKey {
    pub project_key: String,
    pub session_id: String,
    pub subpath: Option<String>,
}

impl SessionKey {
    /// Reads `{projectKey, sessionId, subpath?}` (JS `?? ""` for a null/absent subpath).
    pub fn from_json(v: &Value) -> SessionKey {
        let s = |k: &str| js_string(&v[k]);
        SessionKey {
            project_key: s("projectKey"),
            session_id: s("sessionId"),
            subpath: v.get("subpath").and_then(|x| x.as_str()).map(str::to_string),
        }
    }
    fn composite(&self) -> String {
        format!("{}\0{}\0{}", self.project_key, self.session_id, self.subpath.as_deref().unwrap_or(""))
    }
}

/// Template-literal stringification for the key parts used here (strings; others best-effort).
fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// SameValueZero key for a truthy primitive; `None` for objects/arrays (identity never repeats after clone).
fn seen_key(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(format!("s{s}")),
        Value::Number(n) => Some(format!("n{}", n.as_f64().unwrap_or(0.0))),
        Value::Bool(b) => Some(format!("b{b}")),
        _ => None,
    }
}

/// Insertion-ordered map with JS `Map` semantics (set keeps position; delete then set appends).
#[derive(Clone, Debug)]
struct JsMap<V> {
    items: Vec<(String, V)>,
}
impl<V> Default for JsMap<V> {
    fn default() -> Self {
        JsMap { items: Vec::new() }
    }
}
impl<V> JsMap<V> {
    fn get(&self, k: &str) -> Option<&V> {
        self.items.iter().find(|(x, _)| x == k).map(|(_, v)| v)
    }
    fn set(&mut self, k: String, v: V) {
        match self.items.iter_mut().find(|(x, _)| *x == k) {
            Some(slot) => slot.1 = v,
            None => self.items.push((k, v)),
        }
    }
    fn delete(&mut self, k: &str) {
        self.items.retain(|(x, _)| x != k);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub session_id: String,
    pub mtime: i64,
    pub data: Map<String, Value>,
}

impl SessionSummary {
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("sessionId".into(), Value::String(self.session_id.clone()));
        m.insert("mtime".into(), self.mtime.into());
        m.insert("data".into(), Value::Object(self.data.clone()));
        Value::Object(m)
    }
    pub fn from_json(v: &Value) -> SessionSummary {
        SessionSummary {
            session_id: js_string(&v["sessionId"]),
            mtime: v["mtime"].as_i64().unwrap_or(0),
            data: v["data"].as_object().cloned().unwrap_or_default(),
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct InMemorySessionStore {
    entries: JsMap<Vec<Value>>,
    mtimes: JsMap<i64>,
    summaries: JsMap<SessionSummary>,
    last_mtime: i64,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(&self, key: &SessionKey) -> Option<Vec<Value>> {
        self.entries.get(&key.composite()).cloned()
    }

    /// `now` is the host's `Date.now()`.
    pub fn append(&mut self, key: &SessionKey, entries: &[Value], now: i64) {
        if entries.is_empty() {
            return;
        }
        let composite = key.composite();
        let mut current = self.entries.get(&composite).cloned().unwrap_or_default();
        let mut seen: Vec<String> = current
            .iter()
            .filter(|e| truthy(&e["uuid"]))
            .filter_map(|e| seen_key(&e["uuid"]))
            .collect();
        for entry in entries {
            let uuid = &entry["uuid"];
            let k = if truthy(uuid) { seen_key(uuid) } else { None };
            if let Some(k) = &k {
                if seen.contains(k) {
                    continue;
                }
            }
            current.push(entry.clone());
            if let Some(k) = k {
                seen.push(k);
            }
        }
        self.entries.set(composite.clone(), current);
        let now = now.max(self.last_mtime + 1);
        self.last_mtime = now;
        self.mtimes.set(composite.clone(), now);
        if !key.subpath.as_deref().is_some_and(|s| !s.is_empty()) {
            let previous = self.summaries.get(&composite).cloned();
            let folded = fold_session_summary(previous.as_ref(), key, entries, Some(now));
            self.summaries.set(composite, folded);
        }
    }

    pub fn list_sessions(&self, project_key: &str) -> Vec<(String, i64)> {
        let prefix = format!("{project_key}\0");
        let mut out: Vec<(String, i64)> = self
            .entries
            .items
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix) && k.ends_with('\0'))
            .map(|(k, _)| (js_slice_chars(k, prefix.len()), *self.mtimes.get(k).unwrap_or(&0)))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out
    }

    pub fn list_session_summaries(&self, project_key: &str) -> Vec<SessionSummary> {
        let prefix = format!("{project_key}\0");
        let mut out: Vec<SessionSummary> = self
            .summaries
            .items
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, s)| s.clone())
            .collect();
        out.sort_by(|a, b| b.mtime.cmp(&a.mtime));
        out
    }

    pub fn list_subkeys(&self, project_key: &str, session_id: &str) -> Vec<String> {
        let prefix = format!("{project_key}\0{session_id}\0");
        self.entries
            .items
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix) && k.len() > prefix.len())
            .map(|(k, _)| k[prefix.len()..].to_string())
            .collect()
    }

    pub fn delete(&mut self, key: &SessionKey) {
        let c = key.composite();
        self.entries.delete(&c);
        self.mtimes.delete(&c);
        self.summaries.delete(&c);
    }
}

/// `key.slice(prefix.length, -1)` where the prefix is a byte-prefix of `key` and `key` ends with NUL.
fn js_slice_chars(k: &str, start: usize) -> String {
    k[start..k.len() - 1].to_string()
}

fn as_record(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}

fn field<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    // JS `asRecord(x)[name]` / `x.name`: only objects carry own fields in our JSON domain.
    v.as_object().and_then(|m| m.get(name))
}

fn extract_text(entry: &Value) -> Option<String> {
    if let Some(Value::String(s)) = field(entry, "text") {
        return Some(s.clone());
    }
    if let Some(message) = field(entry, "message") {
        if truthy(message) {
            if let Some(Value::String(s)) = field(message, "text") {
                return Some(s.clone());
            }
        }
    }
    if let Some(Value::Array(messages)) = field(entry, "messages") {
        for item in messages {
            let content = match as_record(item).and_then(|r| r.get("content")) {
                Some(Value::Array(c)) => c.as_slice(),
                _ => &[],
            };
            let text = content.iter().find_map(|b| match field(b, "text") {
                Some(Value::String(s)) => Some(s.clone()),
                _ => None,
            });
            if let Some(t) = text {
                if !t.is_empty() {
                    return Some(t);
                }
            }
        }
    }
    None
}

pub fn fold_session_summary(
    previous: Option<&SessionSummary>,
    key: &SessionKey,
    entries: &[Value],
    mtime: Option<i64>,
) -> SessionSummary {
    let mut data = previous.map(|p| p.data.clone()).unwrap_or_default();
    for entry in entries {
        if data.get("createdAt").is_none() {
            if let Some(v @ Value::String(_)) = field(entry, "timestamp") {
                data.insert("createdAt".into(), v.clone());
            }
        }
        if data.get("cwd").is_none() {
            if let Some(v @ Value::String(_)) = field(entry, "cwd") {
                data.insert("cwd".into(), v.clone());
            }
        }
        if field(entry, "type") == Some(&Value::String("session_metadata".into())) {
            if let Some(meta) = field(entry, "metadata").filter(|m| truthy(m)).and_then(as_record) {
                for f in ["title", "aiTitle", "tag", "firstPrompt", "lastPrompt", "gitBranch"] {
                    if let Some(v) = meta.get(f) {
                        data.insert(f.into(), v.clone());
                    }
                }
            }
        }
        if let Some(text) = extract_text(entry).filter(|t| !t.is_empty()) {
            if data.get("firstPrompt").is_none() {
                data.insert("firstPrompt".into(), Value::String(text.clone()));
            }
            data.insert("lastPrompt".into(), Value::String(text.clone()));
            data.insert("summaryHint".into(), Value::String(text));
        }
    }
    SessionSummary {
        session_id: key.session_id.clone(),
        mtime: mtime.or(previous.map(|p| p.mtime)).unwrap_or(0),
        data,
    }
}

pub fn summary_to_session_info(summary: &SessionSummary) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("sessionId".into(), Value::String(summary.session_id.clone()));
    m.insert("sessionKey".into(), Value::String(summary.session_id.clone()));
    m.insert("lastModified".into(), summary.mtime.into());
    for (k, v) in &summary.data {
        m.insert(k.clone(), v.clone());
    }
    m
}

/// JS `Math.max(1, Math.floor(batchSize ?? 500))` for finite JSON numbers.
pub fn import_batch_size(batch_size: Option<f64>) -> usize {
    (batch_size.unwrap_or(500.0).floor()).max(1.0) as usize
}

pub fn import_session_to_store(
    store: &mut InMemorySessionStore,
    session_id: &str,
    entries: &[Value],
    project_key: &str,
    subpath: Option<&str>,
    batch_size: Option<f64>,
    mut now: impl FnMut() -> i64,
) {
    let size = import_batch_size(batch_size);
    let key = SessionKey {
        project_key: project_key.into(),
        session_id: session_id.into(),
        subpath: subpath.map(str::to_string),
    };
    for chunk in entries.chunks(size) {
        store.append(&key, chunk, now());
    }
}
