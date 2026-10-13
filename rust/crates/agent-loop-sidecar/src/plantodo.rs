//! Port of `parsePlanTodoSnapshot` (src/plan-todo/projection/PlanTodoProjection.ts): validates the host
//! plan/todo snapshot and rebuilds it by replaying `todoHistory`, exactly like the TS sidecar cache.
use crate::json::*;
use crate::obj;

fn type_error(m: &str) -> String {
    m.to_string()
}

/// `{ ...v }` for an arbitrary JS value.
fn js_spread(v: &JsValue) -> JsValue {
    match v {
        JsValue::Obj(o) => JsValue::Obj(o.clone()),
        JsValue::Arr(a) => {
            let mut o = JsObject::new();
            for (i, x) in a.iter().enumerate() {
                o.set(i.to_string(), x.clone());
            }
            JsValue::Obj(o)
        }
        JsValue::Str(s) => {
            let mut o = JsObject::new();
            for (i, u) in s.encode_utf16().enumerate() {
                o.set(i.to_string(), JsValue::Str(String::from_utf16_lossy(&[u])));
            }
            JsValue::Obj(o)
        }
        _ => JsValue::Obj(JsObject::new()),
    }
}

/// SameValueZero map key; objects/arrays are identity-keyed (never equal to another instance).
#[derive(PartialEq, Clone)]
enum Key {
    Undef,
    Null,
    Bool(bool),
    Num(u64),
    NaN,
    Str(String),
    Ident(usize),
}
fn key_of(todo: &JsValue, ident: usize) -> Key {
    let id = get(todo, "id");
    let v = if matches!(id, JsValue::Undefined | JsValue::Null) { get(todo, "content") } else { id };
    match v {
        JsValue::Undefined => Key::Undef,
        JsValue::Null => Key::Null,
        JsValue::Bool(b) => Key::Bool(*b),
        JsValue::Num(n) if n.is_nan() => Key::NaN,
        JsValue::Num(n) => Key::Num(if *n == 0.0 { 0 } else { n.to_bits() }),
        JsValue::Str(s) => Key::Str(s.clone()),
        _ => Key::Ident(ident),
    }
}
/// `Map` built from entries: later duplicates win; returns index of the winning entry.
fn map_get(keys: &[Key], k: &Key) -> Option<usize> {
    keys.iter().rposition(|x| x == k)
}
fn strict_ne(a: &JsValue, b: &JsValue) -> bool {
    match (a, b) {
        (JsValue::Undefined, JsValue::Undefined) | (JsValue::Null, JsValue::Null) => false,
        (JsValue::Bool(x), JsValue::Bool(y)) => x != y,
        (JsValue::Num(x), JsValue::Num(y)) => x != y,
        (JsValue::Str(x), JsValue::Str(y)) => x != y,
        _ => true,
    }
}
fn is_active(t: &JsValue) -> bool {
    matches!(gs(t, "status"), Some("pending") | Some("in_progress"))
}
fn active_todos(todos: &[JsValue]) -> Vec<JsValue> {
    todos.iter().filter(|t| is_active(t)).map(js_spread).collect()
}
fn clone_diagnostics(d: &JsValue) -> JsValue {
    let mut out = js_spread(d);
    let lw = get(d, "lastWrite");
    if truthy(lw) {
        set(&mut out, "lastWrite", js_spread(lw));
    }
    out
}
/// `value?.trim()` with the TS failure for non-string, non-nullish values.
fn opt_trim(v: &JsValue, expr: &str) -> Result<Option<String>, String> {
    match v {
        JsValue::Undefined | JsValue::Null => Ok(None),
        JsValue::Str(s) => Ok(Some(js_trim(s).to_string())),
        _ => Err(format!("{expr}?.trim is not a function")),
    }
}
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

pub fn create_plan_todo_snapshot() -> JsValue {
    obj! {
        "requiresInitialization" => false, "toolCallsSinceLastTodoWrite" => 0usize,
        "todos" => Vec::<JsValue>::new(), "activeTodos" => Vec::<JsValue>::new(), "todoHistory" => Vec::<JsValue>::new(),
        "todoDiagnostics" => obj! {"writeCount" => 0usize, "todoCount" => 0usize, "activeCount" => 0usize,
            "completedCount" => 0usize, "cancelledCount" => 0usize, "largeRewriteCount" => 0usize,
            "deletedOpenItemCount" => 0usize, "completedWithoutActiveCount" => 0usize},
    }
}

fn snapshot_for_plan(plan: &JsValue) -> Result<JsValue, String> {
    let mut s = create_plan_todo_snapshot();
    let approved = opt_trim(plan, "plan")?.filter(|p| !p.is_empty());
    if let Some(p) = &approved {
        set(&mut s, "approvedPlan", p.as_str());
    }
    set(&mut s, "requiresInitialization", approved.is_some());
    Ok(s)
}

fn apply_todo_write(state: &JsValue, item: &JsValue) -> Result<JsValue, String> {
    let next: Vec<JsValue> = ga(item, "todos").iter().map(js_spread).collect();
    let prev = ga(state, "todos");
    let prev_keys: Vec<Key> = prev.iter().enumerate().map(|(i, t)| key_of(t, i)).collect();
    let next_keys: Vec<Key> = next.iter().enumerate().map(|(i, t)| key_of(t, usize::MAX / 2 + i)).collect();
    let removed: Vec<&JsValue> = prev.iter().zip(&prev_keys).filter(|(_, k)| map_get(&next_keys, k).is_none()).map(|(t, _)| t).collect();
    let added = next_keys.iter().filter(|k| map_get(&prev_keys, k).is_none()).count();
    let changed = next.iter().zip(&next_keys).filter(|(t, k)| match map_get(&prev_keys, k) {
        Some(i) => {
            let p = &prev[i];
            strict_ne(get(p, "content"), get(t, "content")) || strict_ne(get(p, "status"), get(t, "status"))
                || strict_ne(get(p, "priority"), get(t, "priority"))
        }
        None => false,
    }).count();
    let deleted_open = removed.iter().filter(|t| is_active(t)).count();
    let prev_active = ga(state, "activeTodos").len();
    let preserved = next_keys.iter().filter(|k| map_get(&prev_keys, k).is_some()).count();
    let large = !prev.is_empty() && !next.is_empty() && (preserved as f64) < (prev.len() as f64 / 2.0).ceil();
    let act = active_todos(&next);
    let all_completed = !next.is_empty() && act.is_empty()
        && next.iter().all(|t| matches!(gs(t, "status"), Some("completed") | Some("cancelled")));
    let reason = opt_trim(get(item, "reason"), "entry.reason")?.filter(|r| !r.is_empty());
    let mode = get(item, "mode").clone();
    let merge = get(item, "merge").clone();
    let mut last_write = obj! {"mode" => &mode, "merge" => &merge};
    if let Some(r) = &reason {
        set(&mut last_write, "reason", r.as_str());
    }
    for (k, v) in [("addedCount", JsValue::Num(added as f64)), ("removedCount", JsValue::Num(removed.len() as f64)),
        ("changedCount", JsValue::Num(changed as f64)), ("deletedOpenItemCount", JsValue::Num(deleted_open as f64)),
        ("largeRewrite", JsValue::Bool(large)), ("allCompleted", JsValue::Bool(all_completed))] {
        set(&mut last_write, k, v);
    }
    let sd = get(state, "todoDiagnostics");
    let add_num = |k: &str, d: f64| -> JsValue { JsValue::Num(gn(sd, k).unwrap_or(f64::NAN) + d) };
    let history = ga(state, "todoHistory");
    let count = |st: &str| next.iter().filter(|t| gs(t, "status") == Some(st)).count();
    let diagnostics = obj! {
        "writeCount" => history.len() + 1, "todoCount" => next.len(), "activeCount" => act.len(),
        "completedCount" => count("completed"), "cancelledCount" => count("cancelled"),
        "largeRewriteCount" => add_num("largeRewriteCount", if large { 1.0 } else { 0.0 }),
        "deletedOpenItemCount" => add_num("deletedOpenItemCount", deleted_open as f64),
        "completedWithoutActiveCount" => add_num("completedWithoutActiveCount", if all_completed && prev_active > 0 { 1.0 } else { 0.0 }),
        "lastWrite" => last_write,
    };
    let mut base = obj! {};
    let ap = get(state, "approvedPlan");
    if truthy(ap) {
        set(&mut base, "approvedPlan", ap);
    }
    set(&mut base, "requiresInitialization", false);
    set(&mut base, "toolCallsSinceLastTodoWrite", 0usize);
    let markdown = get(item, "markdown");
    if !is_undef(markdown) {
        set(&mut base, "lastMarkdown", markdown);
    }
    set(&mut base, "todos", next.clone());
    set(&mut base, "activeTodos", act);
    set(&mut base, "todoHistory", history.to_vec());
    set(&mut base, "todoDiagnostics", diagnostics.clone());
    let mut entry = obj! {"createdAt" => get(item, "createdAt"), "mode" => &mode, "merge" => &merge};
    if let Some(r) = &reason {
        set(&mut entry, "reason", r.as_str());
    }
    if !is_undef(markdown) {
        set(&mut entry, "markdown", markdown);
    }
    set(&mut entry, "todos", next.iter().map(js_spread).collect::<Vec<_>>());
    set(&mut entry, "diagnostics", clone_diagnostics(&diagnostics));
    let mut h = history.to_vec();
    h.push(entry);
    set(&mut base, "todoHistory", h);
    Ok(base)
}

fn is_safe_nonneg_int(v: &JsValue) -> bool {
    matches!(v, JsValue::Num(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0 && *n >= 0.0)
}

pub fn parse_plan_todo_snapshot(value: &JsValue) -> Result<JsValue, String> {
    if !is_obj(value) {
        return Err(type_error("plan/todo checkpoint must be an object."));
    }
    let ri = get(value, "requiresInitialization");
    let tc = get(value, "toolCallsSinceLastTodoWrite");
    let diag = get(value, "todoDiagnostics");
    let diag_ok = matches!(diag, JsValue::Obj(_) | JsValue::Arr(_));
    if !matches!(ri, JsValue::Bool(_)) || !is_safe_nonneg_int(tc) || arr(get(value, "todos")).is_none()
        || arr(get(value, "activeTodos")).is_none() || arr(get(value, "todoHistory")).is_none() || !diag_ok {
        return Err(type_error("plan/todo checkpoint is invalid."));
    }
    let ap = get(value, "approvedPlan");
    let mut state = if truthy(ap) { snapshot_for_plan(ap)? } else { create_plan_todo_snapshot() };
    for h in ga(value, "todoHistory") {
        if !matches!(h, JsValue::Obj(_) | JsValue::Arr(_)) {
            return Err(type_error("plan/todo checkpoint history is invalid."));
        }
        if arr(get(h, "todos")).is_none() || !matches!(gs(h, "mode"), Some("markdown") | Some("structured")) {
            return Err(type_error("plan/todo checkpoint history is invalid."));
        }
        state = apply_todo_write(&state, h)?;
    }
    set(&mut state, "requiresInitialization", ri);
    set(&mut state, "toolCallsSinceLastTodoWrite", tc);
    let lm = get(value, "lastMarkdown");
    if !is_undef(lm) {
        set(&mut state, "lastMarkdown", lm);
    }
    set(&mut state, "todoDiagnostics", clone_diagnostics(diag));
    Ok(state)
}
