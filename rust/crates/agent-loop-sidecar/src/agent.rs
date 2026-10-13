//! Sidecar AgentLoop state, default factory mapping and host-module ports.
//! Ports `cli/pilotdeck-agent-loop-default-factory.ts` and the host*Port adapters.
use crate::json::*;
use crate::obj;
use crate::random_id;
use crate::rt::{js_string, Call, ErrKind, LoopErr, Rt};
use crate::timeline::Timeline;

pub type R<T> = Result<T, LoopErr>;

#[derive(Clone, Default)]
pub struct TokenCap {
    pub max_context_tokens: Option<f64>,
    pub session_max_output_tokens: Option<f64>,
    pub hard_max_output_tokens: Option<f64>,
    pub requested_max_output_tokens: Option<f64>,
    pub attempt_max_output_tokens: Option<f64>,
}

#[derive(Clone)]
pub struct ToolDesc {
    pub name: String,
    pub description: String,
    pub kind: String,
    pub input_schema: JsValue,
    pub read_only: bool,
    pub concurrency_safe: bool,
    pub requires_user_interaction: bool,
    pub required_runtime_capabilities: Vec<String>,
}

pub struct Config {
    pub provider: String,
    pub model: String,
    pub cwd: String,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub plan_mode_instructions: Option<String>,
    pub runtime_context_surface: String,
    pub max_output_tokens: Option<f64>,
    pub max_context_tokens: Option<f64>,
    pub thinking: JsValue,
    pub tool_choice: JsValue,
    pub max_context_messages: Option<f64>,
    pub stop_on_structured_output: bool,
    pub json_self_correct: bool,
    pub run_mode: Option<String>,
    pub is_subagent: bool,
    pub subagent_model: Option<JsValue>,
    pub permission_mode: String,
    pub permission_mode_before_plan: Option<String>,
    pub permission_context: JsValue,
    pub metadata: JsValue,
}

pub struct Input {
    pub messages: Vec<JsValue>,
    pub max_turns: Option<f64>,
    pub max_budget_usd: Option<f64>,
    pub task_budget_usd: Option<f64>,
    pub initial_task_budget_spent_usd: Option<f64>,
    pub run_mode: Option<String>,
    pub permission_mode: String,
    pub base_permission_mode: Option<String>,
    pub allow_plan_mode_tools: bool,
    pub can_prompt: bool,
    pub can_elicit: bool,
    pub permission_rules: Option<JsValue>,
    pub model_override: Option<JsValue>,
    pub has_drain_steer: bool,
    pub has_drain_or_close: bool,
    pub has_persist_compaction: bool,
}

#[derive(Default)]
pub struct Methods {
    pub model: Vec<String>,
    pub budget: Vec<String>,
    pub turn: Vec<String>,
    pub context: Vec<String>,
    pub lifecycle: Vec<String>,
    pub event: Vec<String>,
    pub capability: Vec<String>,
}
impl Methods {
    pub fn has(list: &[String], m: &str) -> bool {
        list.iter().any(|x| x == m)
    }
}

pub struct Agent {
    pub raw_idempotency_key: Option<String>,
    pub run_id: String,
    pub operation_id: String,
    pub idempotency_key: Option<String>,
    pub operation_deadline: Option<String>,
    pub session_id: String,
    pub turn_id: String,
    pub request_id: String,
    pub stream_id: String,
    pub sequence: f64,
    pub cfg: Config,
    pub input: Input,
    pub m: Methods,
    pub elicitation_available: bool,
    pub tools: Vec<ToolDesc>,
    pub metadata: Vec<(String, JsValue)>,
    pub has_metadata_port: bool,
    pub plan_todo: Option<Option<JsValue>>,
    pub plan_todo_seq: u32,
    pub prep_seq: u32,
    pub calibration: Vec<(String, JsValue)>,
    pub caps: Vec<(String, TokenCap)>,
    pub allowed_read_files: Vec<String>,
    pub read_file_state: JsValue,
    pub write_snapshots: JsValue,
    pub timeline: Timeline,
    pub active_budget: Option<(f64, Option<f64>, f64)>,
}

// ---------- small value helpers ----------
pub fn as_string(v: &JsValue) -> Option<String> {
    match v {
        JsValue::Str(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}
pub fn as_optional_string(v: &JsValue) -> Option<String> {
    match v {
        JsValue::Str(s) => Some(s.clone()),
        _ => None,
    }
}
pub fn is_integer(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0
}
pub fn as_positive_integer(v: &JsValue) -> Option<f64> {
    num(v).filter(|n| is_integer(*n) && *n > 0.0)
}
pub fn as_positive_finite(v: &JsValue) -> Option<f64> {
    num(v).filter(|n| n.is_finite() && *n > 0.0)
}
pub fn as_non_negative_finite(v: &JsValue) -> Option<f64> {
    num(v).filter(|n| n.is_finite() && *n >= 0.0)
}
pub fn bool_opt(v: &JsValue) -> Option<bool> {
    match v {
        JsValue::Bool(b) => Some(*b),
        _ => None,
    }
}
/// `a ?? b ?? c`
pub fn coalesce<'a>(vals: &[&'a JsValue]) -> &'a JsValue {
    for v in vals {
        if !matches!(v, JsValue::Undefined | JsValue::Null) {
            return v;
        }
    }
    &UNDEF
}
pub fn nullish(v: &JsValue) -> bool {
    matches!(v, JsValue::Undefined | JsValue::Null)
}
pub fn is_permission_mode(v: &JsValue) -> bool {
    matches!(str_of(v), Some("default") | Some("plan") | Some("bypassPermissions"))
}
pub fn is_safe_int(n: f64) -> bool {
    is_integer(n) && n.abs() <= 9007199254740991.0
}
pub fn cal_key(provider: &str, model: &str) -> String {
    format!("{provider}\u{0}{model}")
}
pub fn module_error_message(resp: &JsValue, fallback: &str) -> String {
    match get(get(resp, "error"), "message") {
        JsValue::Undefined | JsValue::Null => match get(resp, "code") {
            JsValue::Undefined | JsValue::Null => fallback.to_string(),
            c => js_string(c),
        },
        other => js_string(other),
    }
}
pub fn module_failure(resp: &JsValue, fallback: &str) -> LoopErr {
    LoopErr { kind: ErrKind::Generic, code: gs(resp, "code").map(String::from), message: module_error_message(resp, fallback) }
}
pub fn tool_schema(t: &ToolDesc) -> JsValue {
    obj! {"name" => &t.name, "description" => &t.description, "inputSchema" => &t.input_schema}
}
pub fn empty_lifecycle_result() -> JsValue {
    obj! {"effects" => Vec::<JsValue>::new(), "messages" => Vec::<JsValue>::new(), "events" => Vec::<JsValue>::new(),
        "blockingErrors" => Vec::<JsValue>::new(), "nonBlockingErrors" => Vec::<JsValue>::new()}
}

fn uuid() -> String {
    random_id()
}

impl Agent {
    /// Module call bound to the sidecar binding (`sidecarModuleBinding`).
    pub fn bind(&self, module: &'static str, request_id: String, payload: JsValue) -> Call {
        Call { run_id: self.run_id.clone(), operation_id: self.operation_id.clone(), idempotency_key: self.idempotency_key.clone(),
            request_id, module, abortable: false, record_failure: true, payload }
    }

    // ---- plan/todo port (hostPlanTodoPort) ----
    pub fn plan_todo_read(&mut self, rt: &mut Rt, session_id: &str, turn_id: &str) -> R<()> {
        self.plan_todo_seq += 1;
        let call = self.bind("capability", format!("plan-todo-{}-{}", uuid(), self.plan_todo_seq),
            obj! {"operation" => "plan_todo", "method" => "read", "sessionId" => session_id, "turnId" => turn_id});
        let r = rt.call(call)?;
        if gb(&r, "ok") != Some(true) {
            return Err(module_failure(&r, "Plan/todo module failed."));
        }
        let snapshot = crate::plantodo::parse_plan_todo_snapshot(get(get(&r, "payload"), "snapshot")).map_err(LoopErr::msg)?;
        self.plan_todo = Some(Some(snapshot));
        Ok(())
    }

    // ---- model metadata port ----
    pub fn metadata_refresh(&mut self, rt: &mut Rt, provider: &str, model: &str) -> R<()> {
        let call = self.bind("model", format!("model-metadata-{provider}-{model}"),
            obj! {"operation" => "get_metadata", "provider" => provider, "model" => model});
        let r = rt.call(call)?;
        if gb(&r, "ok") != Some(true) {
            let m = match get(get(&r, "error"), "message") {
                JsValue::Str(s) => s.clone(),
                _ => gs(&r, "code").map(String::from).unwrap_or_else(|| "Host model metadata lookup failed.".into()),
            };
            return Err(LoopErr::msg(m));
        }
        let snap = crate::factory::read_metadata_snapshot(get(get(&r, "payload"), "metadata"))?;
        if gs(&snap, "provider") != Some(provider) || gs(&snap, "model") != Some(model) {
            return Err(LoopErr::msg("Host model metadata response does not match the requested route."));
        }
        self.store_metadata(snap);
        Ok(())
    }
    pub fn apply_prepared_metadata(&mut self, value: &JsValue, expected: Option<(&str, &str)>) -> R<()> {
        let snap = crate::factory::read_metadata_snapshot(value)?;
        if let Some((p, m)) = expected {
            if gs(&snap, "provider") != Some(p) || gs(&snap, "model") != Some(m) {
                return Err(LoopErr::msg("Host model metadata response does not match the prepared route."));
            }
        }
        self.store_metadata(snap);
        Ok(())
    }
    fn store_metadata(&mut self, snap: JsValue) {
        let key = cal_key(gs(&snap, "provider").unwrap_or(""), gs(&snap, "model").unwrap_or(""));
        if let Some(e) = self.metadata.iter_mut().find(|(k, _)| *k == key) {
            e.1 = snap;
        } else {
            self.metadata.push((key, snap));
        }
    }
    pub fn metadata_for(&self, provider: &str, model: &str) -> Option<&JsValue> {
        let key = cal_key(provider, model);
        self.metadata.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
    pub fn model_protocol(&self, provider: &str) -> Option<String> {
        self.metadata.iter().find(|(_, s)| gs(s, "provider") == Some(provider) && gs(s, "protocol").is_some())
            .and_then(|(_, s)| gs(s, "protocol").map(String::from))
    }

    // ---- turn callbacks (hostTurnCallbacks) ----
    pub fn turn_call(&self, rt: &mut Rt, operation: &str) -> R<JsValue> {
        self.turn_call_with(rt, operation, obj! {})
    }
    pub fn turn_call_with(&self, rt: &mut Rt, operation: &str, payload: JsValue) -> R<JsValue> {
        let call = self.bind("turn", format!("turn-{operation}-{}", uuid()), spread(&obj! {"operation" => operation}, payload));
        let r = rt.call(call)?;
        if gb(&r, "ok") != Some(true) {
            return Err(module_failure(&r, &format!("Turn module {operation} failed.")));
        }
        Ok(if is_obj(get(&r, "payload")) { get(&r, "payload").clone() } else { obj! {} })
    }

    // ---- context runtime (hostContextRuntime) ----
    pub fn context_call(&self, rt: &mut Rt, operation: &str, input: JsValue, abortable: bool) -> R<JsValue> {
        let mut call = self.bind("context", format!("context-{operation}-{}", uuid()), obj! {"operation" => operation, "input" => input});
        call.record_failure = operation == "prepare_for_model";
        call.abortable = abortable;
        let r = rt.call(call)?;
        if gb(&r, "ok") != Some(true) {
            return Err(module_failure(&r, &format!("Context module {operation} failed.")));
        }
        match get(&r, "payload") {
            JsValue::Obj(o) if o.get("result").is_some() => Ok(o.get("result").unwrap().clone()),
            _ => Err(LoopErr::msg(format!("Context module {operation} returned no result."))),
        }
    }

    // ---- budget port (hostModelBudgetPort) ----
    pub fn budget_call(&self, rt: &mut Rt, operation: &str, mut payload: JsValue) -> R<JsValue> {
        let mut full = obj! {"operation" => operation};
        if let JsValue::Obj(o) = &mut payload {
            full = spread(&full, JsValue::Obj(std::mem::take(o)));
        }
        let call = self.bind("budget", format!("budget-{operation}-{}", uuid()), full);
        let r = rt.call(call)?;
        if gb(&r, "ok") != Some(true) {
            return Err(module_failure(&r, &format!("Budget module {operation} failed.")));
        }
        Ok(if is_obj(get(&r, "payload")) { get(&r, "payload").clone() } else { obj! {} })
    }
    pub fn estimate_request_input(&self, rt: &mut Rt, request: &JsValue) -> R<Option<f64>> {
        if !Methods::has(&self.m.budget, "estimate_request_input") {
            return Ok(None);
        }
        let p = self.budget_call(rt, "estimate_request_input", obj! {"request" => json_clone(request)})?;
        non_negative(get(&p, "tokens"), "Budget input estimate").map(Some)
    }
    pub fn estimate_usage_cost(&self, rt: &mut Rt, usage: &JsValue, provider: &str, model: &str) -> R<Option<f64>> {
        if !Methods::has(&self.m.budget, "estimate_usage_cost") {
            return Ok(None);
        }
        let p = self.budget_call(rt, "estimate_usage_cost", obj! {"usage" => usage, "provider" => provider, "model" => model})?;
        if nullish(get(&p, "costUsd")) {
            return Ok(None);
        }
        non_negative(get(&p, "costUsd"), "Budget usage cost").map(Some)
    }

    // ---- lifecycle runtime ----
    pub fn lifecycle_call(&self, event: &str, payload: JsValue) -> Call {
        self.bind("lifecycle", format!("lifecycle-dispatch-{}", uuid()), obj! {"operation" => "dispatch", "event" => event, "payload" => payload})
    }

    // ---- event bridge (chained emits) ----
    pub fn emit_host_event(&self, rt: &mut Rt, event: JsValue) {
        if Methods::has(&self.m.event, "emit") {
            let mut call = self.bind("event", format!("agent-event-{}", uuid()), obj! {"operation" => "emit", "event" => event});
            call.record_failure = false;
            rt.emit_agent_event(call);
        }
    }

    // ---- model invoker (hostModelInvokerPort) ----
    pub fn model_call(&self, request_id: String, payload: JsValue, abortable: bool) -> Call {
        let mut c = self.bind("model", request_id, payload);
        c.abortable = abortable;
        c
    }
    pub fn next_preparation_id(&mut self) -> String {
        self.prep_seq += 1;
        format!("prepared-{}-{}", uuid(), self.prep_seq)
    }
}

fn non_negative(v: &JsValue, label: &str) -> R<f64> {
    match num(v) {
        Some(n) if n.is_finite() && n >= 0.0 => Ok(n),
        _ => Err(LoopErr::code("INVALID_BUDGET_RESPONSE", format!("{label} must be a non-negative finite number."))),
    }
}

/// Model module failure: a canonical error becomes a `ModelProviderError`.
pub fn model_module_failure(resp: &JsValue, fallback: &str) -> LoopErr {
    let canonical = get(get(resp, "error"), "canonical");
    if gs(canonical, "code").is_some() && gs(canonical, "message").is_some() {
        return LoopErr { kind: ErrKind::Model(canonical.clone()), code: gs(canonical, "code").map(String::from),
            message: gs(canonical, "message").unwrap().to_string() };
    }
    module_failure(resp, fallback)
}
