//! Port of `ui/server/services/modelReferences.js`: find, plan removal of, and rename model references in a
//! PilotDeck config. Values are JSON; JS object key order (index keys first) is reproduced with `js_order`.
//! Errors are V8 TypeError messages (strict-mode ES module semantics).
use crate::js_object::{create_property_error, field, is_record, js_order, truthy};
use pilotdeck_web_ui_core::js::trim;
use serde_json::{json, Map, Value};

pub const REFERENCE_ACTIONS: [&str; 4] = ["replace", "inherit", "remove", "clear"];

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => trim(s).to_string(),
        _ => String::new(),
    }
}

pub fn parse_model_ref(v: Option<&Value>) -> Option<(String, String)> {
    match v {
        Some(Value::String(s)) => {
            let raw = trim(s);
            let slash = raw.find('/')?;
            if slash > 0 && slash < raw.len() - 1 {
                Some((raw[..slash].to_string(), raw[slash + 1..].to_string()))
            } else {
                None
            }
        }
        Some(Value::Object(_)) => {
            let id = text(field(v, "id"));
            let p = Some(text(field(v, "provider"))).filter(|x| !x.is_empty()).unwrap_or_else(|| text(field(v, "providerId")));
            let m = Some(text(field(v, "model"))).filter(|x| !x.is_empty()).unwrap_or_else(|| text(field(v, "modelId")));
            if !p.is_empty() && !m.is_empty() {
                Some((p, m))
            } else if !id.is_empty() {
                parse_model_ref(Some(&Value::String(id)))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn reference_kind(path: &str) -> &'static str {
    if path.starts_with("router.") {
        "router"
    } else if path.starts_with("memory.") {
        "memory"
    } else if path.starts_with("agent.") {
        "agent"
    } else {
        "model"
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Loc {
    AgentModel,
    SubagentDefault,
    MemoryModel,
    Scenario(String),
    Fallback(String, usize),
    Judge,
    Tier(String),
    Pricing(String),
    Baseline,
}

#[derive(Clone, Debug)]
struct Slot {
    path: String,
    value: String,
    kind: &'static str,
    provider_id: String,
    model_id: String,
    action: &'static str,
    raw: Value,
    loc: Loc,
    router: bool,
    token_saver: bool,
    final_action: &'static str,
}

fn collect(config: &Value) -> Vec<Slot> {
    let mut slots = vec![];
    let mut add = |path: String, value: Option<&Value>, action: &'static str, loc: Loc, router: bool, token_saver: bool| {
        if let Some((p, m)) = parse_model_ref(value) {
            slots.push(Slot {
                kind: reference_kind(&path),
                path,
                value: format!("{p}/{m}"),
                provider_id: p,
                model_id: m,
                action,
                raw: value.cloned().unwrap_or(Value::Null),
                loc,
                router,
                token_saver,
                final_action: action,
            });
        }
    };
    let c = Some(config);
    let agent = field(c, "agent");
    add("agent.model".into(), field(agent, "model"), "replace", Loc::AgentModel, false, false);
    add("agent.subagents.default".into(), field(field(agent, "subagents"), "default"), "inherit", Loc::SubagentDefault, false, false);
    add("memory.model".into(), field(field(c, "memory"), "model"), "inherit", Loc::MemoryModel, false, false);
    let router = field(c, "router");
    if let Some(Value::Object(sc)) = field(router, "scenarios") {
        for (name, value) in sc {
            let action = if name == "default" { "replace" } else { "remove" };
            add(format!("router.scenarios.{name}"), Some(value), action, Loc::Scenario(name.clone()), true, false);
        }
    }
    if let Some(Value::Object(fb)) = field(router, "fallback") {
        for (name, values) in fb {
            if let Value::Array(vals) = values {
                for (i, value) in vals.iter().enumerate() {
                    add(format!("router.fallback.{name}.{i}"), Some(value), "remove", Loc::Fallback(name.clone(), i), false, false);
                }
            }
        }
    }
    let ts = field(router, "tokenSaver");
    add("router.tokenSaver.judge".into(), field(ts, "judge"), "replace", Loc::Judge, true, true);
    if let Some(Value::Object(tiers)) = field(ts, "tiers") {
        for (name, tier) in tiers {
            add(format!("router.tokenSaver.tiers.{name}.model"), field(Some(tier), "model"), "replace", Loc::Tier(name.clone()), true, true);
        }
    }
    let stats = field(router, "stats");
    if let Some(Value::Object(pricing)) = field(stats, "modelPricing") {
        for key in pricing.keys() {
            let k = Value::String(key.clone());
            add(format!("router.stats.modelPricing.{key}"), Some(&k), "remove", Loc::Pricing(key.clone()), false, false);
        }
    }
    add("router.stats.baselineModel".into(), field(stats, "baselineModel"), "remove", Loc::Baseline, false, false);
    slots
}

fn matches(slot: &Slot, provider_id: &str, model_id: &str) -> bool {
    (provider_id.is_empty() || slot.provider_id == provider_id) && (model_id.is_empty() || slot.model_id == model_id)
}

/// `findModelReferences(config, { providerId, modelId })`.
pub fn find_model_references(config: &Value, provider_id: &str, model_id: &str) -> Value {
    let config = js_order(config.clone());
    Value::Array(
        collect(&config)
            .into_iter()
            .filter(|s| matches(s, provider_id, model_id))
            .map(|s| json!({"path": s.path, "value": s.value, "kind": s.kind}))
            .collect(),
    )
}

fn null_read(prop: &str) -> String {
    format!("TypeError: Cannot read properties of null (reading '{prop}')")
}

fn replacement_value(value: &Value, replacement: &str) -> Result<Value, String> {
    let Value::Object(o) = value else { return Ok(Value::String(replacement.into())) };
    let r = parse_model_ref(Some(&Value::String(replacement.into())));
    let mut next = o.clone();
    if o.contains_key("id") {
        next.insert("id".into(), Value::String(replacement.into()));
    }
    for (k, provider) in [("provider", true), ("providerId", true), ("model", false), ("modelId", false)] {
        if o.contains_key(k) {
            let (p, m) = r.clone().ok_or_else(|| null_read(if provider { "providerId" } else { "modelId" }))?;
            next.insert(k.into(), Value::String(if provider { p } else { m }));
        }
    }
    Ok(Value::Object(next))
}

fn remaining_model_refs(config: &Value, provider_id: &str, model_id: &str) -> Vec<String> {
    let mut refs = vec![];
    let Some(Value::Object(providers)) = field(field(Some(config), "model"), "providers") else { return refs };
    for (id, provider) in providers {
        if id == provider_id && model_id.is_empty() {
            continue;
        }
        let Some(Value::Object(models)) = field(Some(provider), "models") else { continue };
        for model in models.keys() {
            if id == provider_id && model == model_id {
                continue;
            }
            refs.push(format!("{id}/{model}"));
        }
    }
    refs
}

enum Provider<'a> {
    Missing,
    Value(&'a Value),
    /// `providers["__proto__"]` without an own key: `Object.prototype`, a record with no `models`.
    ObjectPrototype,
}

fn lookup_provider<'a>(config: &'a Value, id: &str) -> Provider<'a> {
    match field(field(Some(config), "model"), "providers") {
        Some(Value::Object(m)) => match m.get(id) {
            Some(v) => Provider::Value(v),
            None if id == "__proto__" => Provider::ObjectPrototype,
            None => Provider::Missing,
        },
        Some(Value::Array(a)) => match crate::js_object::array_index(id) {
            Some(i) if (i as usize) < a.len() => Provider::Value(&a[i as usize]),
            _ => Provider::Missing,
        },
        _ => Provider::Missing,
    }
}

fn obj_at<'a>(root: &'a mut Value, path: &[&str]) -> Option<&'a mut Map<String, Value>> {
    let mut cur = root;
    for p in path {
        cur = cur.as_object_mut()?.get_mut(*p)?;
    }
    cur.as_object_mut()
}

/// `planModelRemoval(config, { providerId, modelId }, { replacement })`. `Err` is a thrown TypeError message.
pub fn plan_model_removal(config: &Value, provider_id: &str, model_id: &str, replacement: &str) -> Result<Value, String> {
    let config = js_order(config.clone());
    let pid = trim(provider_id).to_string();
    let mid = trim(model_id).to_string();
    let mut target = Map::new();
    target.insert("providerId".into(), Value::String(pid.clone()));
    if !mid.is_empty() {
        target.insert("modelId".into(), Value::String(mid.clone()));
    }
    let mut plan = Map::new();
    plan.insert("target".into(), Value::Object(target));
    plan.insert("replacement".into(), json!(""));
    plan.insert("replacementOptions".into(), json!([]));
    plan.insert("requiresReplacement".into(), json!(false));
    plan.insert("changes".into(), json!([]));
    let blocked = |mut plan: Map<String, Value>, code: &str, message: String| {
        plan.insert("blocked".into(), json!({"code": code, "message": message}));
        plan.insert("config".into(), Value::Null);
        Ok(Value::Object(plan))
    };
    let provider = lookup_provider(&config, &pid);
    let provider_models = match &provider {
        Provider::Value(v @ Value::Object(_)) => field(Some(v), "models"),
        Provider::ObjectPrototype => None,
        _ => return blocked(plan, "NOT_FOUND", format!("Provider \"{pid}\" is not configured.")),
    };
    if pid.is_empty() {
        return blocked(plan, "NOT_FOUND", format!("Provider \"{pid}\" is not configured."));
    }
    if !mid.is_empty() && !matches!(provider_models, Some(Value::Object(m)) if m.contains_key(&mid)) {
        return blocked(plan, "NOT_FOUND", format!("Model \"{mid}\" is not configured for provider \"{pid}\"."));
    }

    let mut next = config.clone();
    let options = remaining_model_refs(&next, &pid, &mid);
    let mut slots: Vec<Slot> = collect(&next).into_iter().filter(|s| matches(s, &pid, &mid)).collect();
    let requires = slots.iter().any(|s| s.action == "replace");
    let chosen = trim(replacement).to_string();
    plan.insert("replacement".into(), json!(if requires { chosen.as_str() } else { "" }));
    plan.insert("replacementOptions".into(), json!(options));
    plan.insert("requiresReplacement".into(), json!(requires));

    let router = field(Some(&next), "router");
    let router_enabled = is_record(router) && field(router, "enabled") != Some(&Value::Bool(false));
    let ts_enabled = router_enabled && field(field(router, "tokenSaver"), "enabled") != Some(&Value::Bool(false));
    for slot in &mut slots {
        let mut action = slot.action;
        if action == "replace" && chosen.is_empty() && options.is_empty() {
            let active = if slot.token_saver { ts_enabled } else if slot.router { router_enabled } else { true };
            if slot.path == "agent.model" {
                action = "clear";
            } else if !active {
                action = "remove";
            }
        }
        slot.final_action = action;
    }
    plan.insert(
        "changes".into(),
        Value::Array(
            slots
                .iter()
                .map(|s| {
                    let mut c = json!({"path": s.path, "value": s.value, "kind": s.kind, "action": s.final_action});
                    if s.final_action == "replace" && !chosen.is_empty() {
                        c["to"] = json!(chosen);
                    }
                    c
                })
                .collect(),
        ),
    );
    let unresolved = slots.iter().any(|s| s.final_action == "replace");
    if unresolved && chosen.is_empty() {
        return if options.is_empty() {
            blocked(plan, "ROUTER_REQUIRES_MODEL", "Smart routing is enabled and needs at least one other model.".into())
        } else {
            blocked(plan, "REPLACEMENT_REQUIRED", "Choose a replacement model for the references that need one.".into())
        };
    }
    if unresolved && !options.contains(&chosen) {
        return blocked(plan, "REPLACEMENT_INVALID", format!("Replacement \"{chosen}\" is not an available model."));
    }

    let mut removed_fallback: Vec<(String, usize)> = vec![];
    for slot in &slots {
        match (slot.final_action, &slot.loc) {
            ("replace", loc) => {
                let v = replacement_value(&slot.raw, &chosen)?;
                let (path, key): (Vec<&str>, &str) = match loc {
                    Loc::AgentModel => (vec!["agent"], "model"),
                    Loc::Scenario(n) => (vec!["router", "scenarios"], n.as_str()),
                    Loc::Judge => (vec!["router", "tokenSaver"], "judge"),
                    Loc::Tier(n) => (vec!["router", "tokenSaver", "tiers", n.as_str()], "model"),
                    _ => unreachable!("replace on a slot without set()"),
                };
                if let Some(o) = obj_at(&mut next, &path) {
                    o.insert(key.into(), v);
                }
            }
            ("clear", _) => {
                if let Some(o) = obj_at(&mut next, &["agent"]) {
                    o.insert("model".into(), json!(""));
                }
            }
            ("inherit", Loc::SubagentDefault) => {
                if let Some(o) = obj_at(&mut next, &["agent", "subagents"]) {
                    o.insert("default".into(), json!("inherit"));
                }
            }
            ("inherit", Loc::MemoryModel) => {
                if let Some(o) = obj_at(&mut next, &["memory"]) {
                    o.shift_remove("model");
                }
            }
            (_, loc) => {
                let (path, key): (Vec<&str>, String) = match loc {
                    Loc::Scenario(n) => (vec!["router", "scenarios"], n.clone()),
                    Loc::Fallback(n, i) => {
                        removed_fallback.push((n.clone(), *i));
                        continue;
                    }
                    Loc::Judge => (vec!["router", "tokenSaver"], "judge".into()),
                    Loc::Tier(n) => (vec!["router", "tokenSaver", "tiers", n.as_str()], "model".into()),
                    Loc::Pricing(k) => (vec!["router", "stats", "modelPricing"], k.clone()),
                    Loc::Baseline => (vec!["router", "stats"], "baselineModel".into()),
                    _ => unreachable!("remove on a slot without remove()"),
                };
                if let Some(o) = obj_at(&mut next, &path) {
                    o.shift_remove(&key);
                }
            }
        }
    }
    let mut touched: Vec<String> = vec![];
    for (n, _) in &removed_fallback {
        if !touched.contains(n) {
            touched.push(n.clone());
        }
    }
    if let Some(fb) = obj_at(&mut next, &["router", "fallback"]) {
        for name in touched {
            let Some(Value::Array(values)) = fb.get(&name) else { continue };
            let kept: Vec<Value> = values
                .iter()
                .enumerate()
                .filter(|(i, _)| !removed_fallback.contains(&(name.clone(), *i)))
                .map(|(_, v)| v.clone())
                .collect();
            if kept.is_empty() {
                fb.shift_remove(&name);
            } else {
                fb.insert(name, Value::Array(kept));
            }
        }
    }
    if !mid.is_empty() {
        let providers = next.get_mut("model").and_then(|m| m.get_mut("providers"));
        let provider = match providers {
            Some(Value::Object(m)) => m.get_mut(&pid),
            Some(Value::Array(a)) => crate::js_object::array_index(&pid).and_then(|i| a.get_mut(i as usize)),
            _ => None,
        };
        if let Some(models) = provider.and_then(|p| p.get_mut("models")).and_then(|m| m.as_object_mut()) {
            models.shift_remove(&mid);
        }
    } else {
        match next.get_mut("model").and_then(|m| m.get_mut("providers")) {
            Some(Value::Object(m)) => {
                m.shift_remove(&pid);
            }
            Some(Value::Array(a)) => {
                if let Some(i) = crate::js_object::array_index(&pid) {
                    if let Some(slot) = a.get_mut(i as usize) {
                        *slot = Value::Null; // `delete arr[i]` leaves a hole; JSON.stringify writes null
                    }
                }
            }
            _ => {}
        }
    }
    plan.insert("blocked".into(), Value::Null);
    plan.insert("config".into(), next);
    Ok(js_order(Value::Object(plan)))
}

/// `Map` lookups for rewriteModelReferences (last duplicate wins, like `new Map(entries)`).
pub struct Renames {
    pub providers: Vec<(String, Value)>,
    pub models: Vec<(String, Value)>,
}

fn map_get<'a>(m: &'a [(String, Value)], k: &str) -> Option<&'a Value> {
    m.iter().rev().find(|(x, _)| x == k).map(|(_, v)| v)
}

fn rename_ref(value: Option<&Value>, r: &Renames) -> Option<Value> {
    let Some((p, m)) = parse_model_ref(value) else { return value.cloned() };
    let rp = match map_get(&r.providers, &p) {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => p.clone(),
    };
    let rm = match map_get(&r.models, &format!("{p}/{m}")).and_then(|v| field(Some(v), "modelId")) {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => m.clone(),
    };
    let next_id = format!("{rp}/{rm}");
    match value {
        Some(Value::String(_)) => Some(Value::String(next_id)),
        Some(Value::Object(o)) => {
            let mut n = o.clone();
            for (k, v) in [("id", &next_id), ("provider", &rp), ("providerId", &rp), ("model", &rm), ("modelId", &rm)] {
                if o.contains_key(k) {
                    n.insert(k.into(), Value::String(v.clone()));
                }
            }
            Some(Value::Object(n))
        }
        _ => value.cloned(),
    }
}

/// `container.key = value` in strict mode: primitives throw; arrays/undefined values are invisible in JSON.
fn assign(container: &mut Value, key: &str, value: Option<Value>) -> Result<(), String> {
    match container {
        Value::Object(o) => {
            if let Some(v) = value {
                o.insert(key.into(), v);
            }
            Ok(())
        }
        Value::Array(_) => Ok(()),
        other => Err(create_property_error(key, other)),
    }
}

/// `rewriteModelReferences(config, { providerRenames, modelRenames })`, returning the mutated config.
pub fn rewrite_model_references(config: &Value, r: &Renames) -> Result<Value, String> {
    let mut config = js_order(config.clone());
    if let Some(agent) = config.get_mut("agent").filter(|a| truthy(Some(a))) {
        let v = rename_ref(field(Some(agent), "model"), r);
        assign(agent, "model", v)?;
        if let Some(sub) = agent.get_mut("subagents").filter(|s| truthy(Some(s))) {
            let v = rename_ref(field(Some(sub), "default"), r);
            assign(sub, "default", v)?;
        }
    }
    if let Some(memory) = config.get_mut("memory").filter(|m| truthy(Some(m))) {
        let v = rename_ref(field(Some(memory), "model"), r);
        assign(memory, "model", v)?;
    }
    if let Some(router) = config.get_mut("router").filter(|x| x.is_object()) {
        if let Some(Value::Object(sc)) = router.get_mut("scenarios") {
            for (_, v) in sc.iter_mut() {
                *v = rename_ref(Some(v), r).unwrap_or(Value::Null);
            }
        }
        if let Some(Value::Object(fb)) = router.get_mut("fallback") {
            for (_, v) in fb.iter_mut() {
                if let Value::Array(a) = v {
                    for x in a.iter_mut() {
                        *x = rename_ref(Some(x), r).unwrap_or(Value::Null);
                    }
                }
            }
        }
        if let Some(ts) = router.get_mut("tokenSaver").filter(|t| truthy(Some(t))) {
            let v = rename_ref(field(Some(ts), "judge"), r);
            assign(ts, "judge", v)?;
            if let Some(Value::Object(tiers)) = ts.get_mut("tiers") {
                for (_, tier) in tiers.iter_mut() {
                    if tier.is_object() {
                        let v = rename_ref(field(Some(tier), "model"), r);
                        assign(tier, "model", v)?;
                    }
                }
            }
        }
        if let Some(stats) = router.get_mut("stats").filter(|s| s.is_object()) {
            if let Some(Value::Object(pricing)) = stats.get("modelPricing") {
                let mut out = Map::new();
                for (k, v) in pricing {
                    let nk = match rename_ref(Some(&Value::String(k.clone())), r) {
                        Some(Value::String(s)) => s,
                        _ => k.clone(),
                    };
                    out.insert(nk, v.clone());
                }
                stats["modelPricing"] = Value::Object(out);
            }
            if stats.get("baselineModel").is_some() {
                let v = rename_ref(stats.get("baselineModel"), r);
                assign(stats, "baselineModel", v)?;
            }
        }
    }
    Ok(js_order(config))
}

/// `configuredModelIds(config)` as `[[providerId, [modelId...]], ...]`.
pub fn configured_model_ids(config: &Value) -> Value {
    let config = js_order(config.clone());
    let Some(Value::Object(providers)) = field(field(Some(&config), "model"), "providers") else { return json!([]) };
    Value::Array(
        providers
            .iter()
            .map(|(id, p)| {
                let models: Vec<&String> = match field(Some(p), "models") {
                    Some(Value::Object(m)) => m.keys().collect(),
                    _ => vec![],
                };
                json!([id, models])
            })
            .collect(),
    )
}
