//! Port of `AgentLoop.run`/`runOrdered` for the sidecar execute path.
use crate::agent::*;
use crate::json::*;
use crate::obj;
use crate::random_id;
use crate::rt::{ErrKind, LoopErr, Rt};
use crate::status::*;

const MAX_OUTPUT_RECOVERY_LIMIT: u32 = 50;
const MAX_CONSECUTIVE_EMPTY: u32 = 3;
const MAX_JSON_SELF_CORRECT_RETRIES: u32 = 3;
const MAX_STREAM_INTERRUPTION_RECOVERIES: u32 = 2;
const MAX_UNKNOWN_FINISH_RECOVERIES: u32 = 2;
const MAX_SAME_INVALID_FINGERPRINT: u32 = 3;
const EMPTY_LENGTH_OUTPUT_RETRY_FLOOR: f64 = 4096.0;
const MAX_OUTPUT_RESUME: &str = "Output token limit hit. Resume directly - no apology, no recap of what you were doing. Pick up mid-thought if that is where the cut happened. Break remaining work into smaller pieces.";
const MAX_OUTPUT_RESUME_SENTENCE: &str = "Output token limit hit. Resume directly - no apology, no recap of what you were doing. Pick up mid-sentence if that is where the cut happened.";
const EMPTY_RETRY: &str = "Your previous response was empty (thinking only, no visible text). Please provide your answer as visible text output.";

pub struct RunResult {
    pub result: JsValue,
    pub messages: Vec<JsValue>,
}

/// Feature not migrated yet: fail loudly instead of diverging silently.
pub fn pending(feature: &str) -> LoopErr {
    LoopErr::code("RUST_SIDECAR_PENDING", format!("Rust agent-loop sidecar: {feature} is not ported yet (pending)."))
}

pub struct Run<'a> {
    pub a: &'a mut Agent,
    pub rt: &'a mut Rt,
    pub messages: Vec<JsValue>,
    pub usage: JsValue,
    pub permission_denials: Vec<JsValue>,
    pub structured_output: JsValue,
    pub final_message: JsValue,
    pub turn_count: f64,
    pub started_at: String,
    transient_ids: Vec<String>,
}

impl<'a> Run<'a> {
    pub fn new(a: &'a mut Agent, rt: &'a mut Rt) -> Self {
        let messages = a.input.messages.clone();
        Run { a, rt, messages, usage: obj! {}, permission_denials: Vec::new(), structured_output: JsValue::Undefined,
            final_message: JsValue::Undefined, turn_count: 1.0, started_at: String::new(), transient_ids: Vec::new() }
    }

    // ---------- event publication (timeline + stream events) ----------
    pub fn publish(&mut self, event: JsValue) {
        let event = self.a.timeline.event(event);
        let ty = gs(&event, "type").unwrap_or("").to_string();
        let seq = self.a.sequence;
        self.a.sequence += 1.0;
        let a = &*self.a;
        let msg = obj! {"kind" => "event", "messageId" => format!("event-{}", random_id()), "eventType" => format!("agent.{ty}"),
            "streamId" => &a.stream_id, "sequence" => seq, "runId" => &a.run_id, "operationId" => &a.operation_id,
            "requestId" => &a.request_id, "final" => false, "payload" => event};
        self.rt.send(&msg);
    }
    fn ev(&self, ty: &str) -> JsValue {
        obj! {"type" => ty, "sessionId" => &self.a.session_id, "turnId" => &self.a.turn_id}
    }
    fn ev_with(&self, ty: &str, extra: JsValue) -> JsValue {
        spread(&self.ev(ty), extra)
    }
    fn publish_ev(&mut self, ty: &str, extra: JsValue) {
        let e = self.ev_with(ty, extra);
        self.publish(e);
    }
    /// Publish an event carrying `messages[idx]`, annotating the shared message first.
    fn publish_message_event(&mut self, ty: &str, idx: usize, extra: JsValue) {
        let mut m = std::mem::replace(&mut self.messages[idx], JsValue::Undefined);
        self.a.timeline.message(&mut m);
        self.messages[idx] = m.clone();
        let mut e = self.ev(ty);
        if let JsValue::Obj(o) = &extra {
            for (k, v) in o.iter() {
                set(&mut e, k, v.clone());
            }
        }
        set(&mut e, "message", m);
        self.publish(e);
    }
    fn push_and_publish(&mut self, ty: &str, message: JsValue) {
        self.messages.push(message);
        let idx = self.messages.len() - 1;
        self.publish_message_event(ty, idx, JsValue::Undefined);
    }
    fn continued(&mut self, reason: &str) {
        self.publish_ev("turn_continued", obj! {"reason" => reason});
    }
    fn emit_status(&mut self, status: JsValue) {
        self.publish_ev("agent_status", obj! {"event" => get(&status, "event"), "detail" => get(&status, "detail")});
    }

    // ---------- results ----------
    fn turn_result(&self, ty: &str, stop: &str, errors: Option<Vec<JsValue>>, with_structured: bool, final_message: Option<JsValue>, turns: Option<f64>) -> JsValue {
        let fm = final_message.unwrap_or_else(|| self.final_message.clone());
        let mut r = obj! {"type" => ty, "stopReason" => stop, "usage" => &self.usage, "permissionDenials" => self.permission_denials.clone(),
            "turns" => turns.unwrap_or(self.turn_count), "startedAt" => &self.started_at, "finalMessage" => fm};
        if with_structured {
            set(&mut r, "structuredOutput", self.structured_output.clone());
        }
        if let Some(e) = errors {
            set(&mut r, "errors", e);
        }
        set(&mut r, "sessionId", &self.a.session_id);
        set(&mut r, "turnId", &self.a.turn_id);
        set(&mut r, "completedAt", crate::rt::iso_now());
        if let Some((spent, task, initial)) = self.a.active_budget {
            let mut b = obj! {"turnSpentUsd" => spent};
            if let Some(t) = task {
                set(&mut b, "taskBudgetUsd", t);
                set(&mut b, "taskSpentUsd", initial + spent);
            }
            set(&mut r, "budget", b);
        }
        r
    }
    fn finish(&mut self, result: JsValue) -> R<RunResult> {
        let errored = gs(&result, "type") == Some("error");
        self.capture_turn(errored)?;
        self.publish_ev("turn_completed", obj! {"result" => &result});
        Ok(RunResult { result, messages: self.messages.clone() })
    }
    fn aborted_result(&mut self, surface_status: bool) -> R<RunResult> {
        let result = self.turn_result("aborted", "aborted_streaming", None, false, None, None);
        if surface_status {
            if let Some(s) = self.abort_status() {
                self.emit_status(s);
            }
        }
        self.finish(result)
    }
    fn abort_status(&self) -> Option<JsValue> {
        let reason = self.rt.abort_reason().unwrap_or(JsValue::Null);
        if !should_surface_abort(&reason) {
            return None;
        }
        Some(turn_aborted_status(stringify_abort_reason(&reason)))
    }

    // ---------- host-backed hooks ----------
    fn capture_turn(&mut self, errored: bool) -> R<()> {
        if !Methods::has(&self.a.m.context, "capture_turn") {
            return Ok(());
        }
        let input = obj! {"sessionId" => &self.a.session_id, "turnId" => &self.a.turn_id, "messages" => self.messages.clone(), "errored" => errored};
        match self.a.context_call(self.rt, "capture_turn", input, false) {
            Ok(_) => Ok(()),
            Err(e) if matches!(e.kind, ErrKind::Aborted) => Ok(()),
            Err(_) => Ok(()),
        }
    }

    fn lifecycle(&mut self, event: &str, payload: JsValue) -> R<JsValue> {
        if !Methods::has(&self.a.m.lifecycle, "dispatch") {
            return Ok(empty_lifecycle_result());
        }
        let call = self.a.lifecycle_call(event, payload);
        let r = self.rt.call(call)?;
        if gb(&r, "ok") != Some(true) {
            return Err(module_failure(&r, "Lifecycle module failed."));
        }
        let res = get(get(&r, "payload"), "result");
        if !is_obj(res) {
            return Err(LoopErr::msg("Lifecycle module response must contain a dispatch result."));
        }
        let pa = get(res, "pendingAsyncHooks");
        if ["effects", "messages", "events", "blockingErrors", "nonBlockingErrors"].iter().any(|k| arr(get(res, k)).is_none())
            || (!is_undef(pa) && arr(pa).is_none()) {
            return Err(LoopErr::msg("Lifecycle module response has an invalid dispatch result."));
        }
        Ok(res.clone())
    }
    fn lifecycle_detached(&mut self, event: &str, payload: JsValue) {
        if Methods::has(&self.a.m.lifecycle, "dispatch") {
            let call = self.a.lifecycle_call(event, payload);
            self.rt.send_detached(&call);
        }
    }

    fn push_transient(&mut self, prompt: &str, purpose: &str) {
        let id = crate::util::uuid_v4();
        self.messages.push(obj! {"role" => "user", "content" => vec![obj! {"type" => "text", "text" => prompt}],
            "metadata" => obj! {"synthetic" => true, "transient" => true, "transientId" => &id, "purpose" => purpose}});
        self.transient_ids.push(id);
    }
    fn expire_transients(&mut self) {
        if self.transient_ids.is_empty() {
            return;
        }
        let ids = std::mem::take(&mut self.transient_ids);
        self.messages.retain(|m| {
            let md = get(m, "metadata");
            !(gs(m, "role") == Some("user") && gb(md, "transient") == Some(true)
                && gs(md, "transientId").map_or(false, |t| ids.iter().any(|x| x == t)))
        });
    }

    fn apply_steers(&mut self, pending: Vec<JsValue>) {
        for steer in pending {
            let mut message = get(&steer, "message").clone();
            self.a.timeline.message(&mut message);
            for f in ga(&steer, "allowedReadFiles") {
                if let Some(p) = str_of(f) {
                    if !self.a.allowed_read_files.iter().any(|x| x == p) {
                        self.a.allowed_read_files.push(p.to_string());
                    }
                }
            }
            self.messages.push(message);
            let idx = self.messages.len() - 1;
            self.publish_message_event("steer_applied", idx, obj! {"itemId" => get(&steer, "itemId")});
        }
    }
    fn steer_list(payload: &JsValue) -> R<Vec<JsValue>> {
        let invalid = |m: &str| LoopErr::code("INVALID_TURN_RESPONSE", m);
        let Some(items) = arr(get(payload, "messages")) else { return Err(invalid("Turn steer response must contain messages.")) };
        for item in items {
            let m = get(item, "message");
            if !is_obj(item) || gs(item, "itemId").is_none() || !is_obj(m) || !matches!(gs(m, "role"), Some("user" | "assistant")) || arr(get(m, "content")).is_none() {
                return Err(invalid("Turn steer response contains an invalid message."));
            }
            let a = get(item, "allowedReadFiles");
            if !is_undef(a) && arr(a).map_or(true, |l| l.iter().any(|p| str_of(p).is_none())) {
                return Err(invalid("Turn steer response contains invalid allowedReadFiles."));
            }
        }
        Ok(items.clone())
    }
}


// ---------------- token caps, request assembly ----------------
fn pos_finite(v: Option<f64>) -> Option<f64> {
    v.filter(|n| n.is_finite() && *n > 0.0)
}

impl<'a> Run<'a> {
    fn cap(&self, p: &str, m: &str) -> Option<&TokenCap> {
        let k = cal_key(p, m);
        self.a.caps.iter().find(|(x, _)| *x == k).map(|(_, c)| c)
    }
    fn set_cap(&mut self, p: &str, m: &str, f: impl FnOnce(&mut TokenCap)) {
        let k = cal_key(p, m);
        if let Some((_, c)) = self.a.caps.iter_mut().find(|(x, _)| *x == k) {
            f(c);
        } else {
            let mut c = TokenCap::default();
            f(&mut c);
            self.a.caps.push((k, c));
        }
    }
    fn clear_attempt_cap(&mut self, p: &str, m: &str) {
        let k = cal_key(p, m);
        if let Some((_, c)) = self.a.caps.iter_mut().find(|(x, _)| *x == k) {
            c.attempt_max_output_tokens = None;
        }
    }
    fn clear_turn_scoped_caps(&mut self) {
        self.a.caps.retain_mut(|(_, c)| {
            c.requested_max_output_tokens = None;
            c.attempt_max_output_tokens = None;
            !(c.max_context_tokens.is_none() && c.session_max_output_tokens.is_none() && c.hard_max_output_tokens.is_none())
        });
    }
    /// `getModelTokenLimits`: host metadata snapshot (combined limits first).
    fn token_limits(&self, p: &str, m: &str) -> (Option<f64>, Option<f64>, bool) {
        let Some(s) = self.a.metadata_for(p, m) else { return (None, None, false) };
        let tl = get(s, "tokenLimits");
        if is_obj(tl) {
            return (gn(tl, "maxContextTokens"), gn(tl, "maxOutputTokens"), true);
        }
        let (c, o) = (gn(s, "maxContextTokens"), gn(s, "maxOutputTokens"));
        (c, o, c.is_some() || o.is_some())
    }
    fn subagent_baseline(&self, p: &str, m: &str) -> Option<(Option<f64>, Option<f64>)> {
        if !self.a.cfg.is_subagent {
            return None;
        }
        let b = self.a.cfg.subagent_model.as_ref()?;
        if gs(b, "provider") != Some(p) || gs(b, "model") != Some(m) {
            return None;
        }
        Some((gn(b, "maxContextTokens"), gn(b, "maxOutputTokens")))
    }
    fn subagent_with_model(&self) -> bool {
        self.a.cfg.is_subagent && self.a.cfg.subagent_model.is_some()
    }
    fn current_max_context(&self, p: &str, m: &str) -> f64 {
        self.cap(p, m).and_then(|c| c.max_context_tokens)
            .or_else(|| self.subagent_baseline(p, m).and_then(|b| b.0))
            .or_else(|| if self.subagent_with_model() { None } else { self.a.cfg.max_context_tokens })
            .or_else(|| self.a.metadata_for(p, m).and_then(|s| gn(s, "maxContextTokens")))
            .or_else(|| self.token_limits(p, m).0)
            .unwrap_or(1_000_000.0)
    }
    fn pre_routing_max_context(&self) -> f64 {
        if self.subagent_with_model() {
            return 1_000_000.0;
        }
        let (p, m) = (self.a.cfg.provider.clone(), self.a.cfg.model.clone());
        self.current_max_context(&p, &m)
    }
    fn current_max_output(&self, p: &str, m: &str) -> Option<f64> {
        let t = self.cap(p, m);
        let model_max = self.token_limits(p, m).1;
        let requested = t.and_then(|c| c.attempt_max_output_tokens)
            .or_else(|| t.and_then(|c| c.requested_max_output_tokens))
            .or_else(|| t.and_then(|c| c.session_max_output_tokens))
            .or_else(|| self.subagent_baseline(p, m).and_then(|b| b.1))
            .or_else(|| if self.subagent_with_model() { None } else { self.a.cfg.max_output_tokens });
        let mut c: Vec<f64> = [requested, t.and_then(|c| c.hard_max_output_tokens)].into_iter().filter_map(pos_finite).collect();
        if !c.is_empty() {
            if let Some(mm) = pos_finite(model_max) {
                c.push(mm);
            }
        }
        if c.is_empty() { None } else { Some(c.iter().map(|v| v.floor()).fold(f64::INFINITY, f64::min)) }
    }
    fn reserved_output(&self, route: Option<(&str, &str)>) -> f64 {
        match route {
            Some((p, m)) => self.current_max_output(p, m).unwrap_or(0.0),
            None => self.current_max_output(&self.a.cfg.provider, &self.a.cfg.model).unwrap_or(0.0),
        }
    }
    fn apply_token_caps(&self, request: &JsValue, p: &str, m: &str) -> JsValue {
        let max_out = self.current_max_output(p, m);
        let t = self.cap(p, m);
        let attempt = t.and_then(|c| c.attempt_max_output_tokens).or_else(|| t.and_then(|c| c.requested_max_output_tokens));
        let prepared_cap = num(get(request, "maxOutputTokens"));
        let effective = match (attempt, prepared_cap.filter(|n| n.is_finite() && *n > 0.0), max_out) {
            (None, Some(pc), Some(mo)) => Some(mo.min(pc.floor())),
            _ => max_out.or(prepared_cap),
        };
        let mut r = spread(request, obj! {"provider" => p, "model" => m});
        if let Some(e) = effective {
            set(&mut r, "maxOutputTokens", e);
        }
        r
    }
    fn calibration_for(&self, p: &str, m: &str) -> JsValue {
        let k = cal_key(p, m);
        self.a.calibration.iter().find(|(x, _)| *x == k).map(|(_, v)| v.clone()).unwrap_or(JsValue::Undefined)
    }

    // ---- tool catalog (hostCapabilityToolPort.refresh) ----
    fn refresh_tools(&mut self) -> R<()> {
        if !Methods::has(&self.a.m.capability, "list_tools") {
            return Ok(());
        }
        let call = self.a.bind("capability", format!("tool-catalog-{}", random_id()), obj! {"operation" => "list_tools"});
        let r = self.rt.call(call)?;
        if gb(&r, "ok") != Some(true) {
            return Err(LoopErr::msg(module_error_message(&r, "Host tool catalog refresh failed.")));
        }
        self.a.tools = crate::factory::read_required_tool_definitions(get(get(&r, "payload"), "tools"))?;
        Ok(())
    }

    fn context_prepare_input(&self, messages: &[JsValue]) -> R<JsValue> {
        let a = &*self.a;
        let can_prompt = a.input.can_prompt;
        let can_elicit = a.input.can_elicit && a.elicitation_available;
        let tools: Vec<&ToolDesc> = a.tools.iter()
            .filter(|t| can_prompt || !((t.name == "enter_plan_mode" || t.requires_user_interaction) && !(can_elicit && t.name == "ask_user_question")))
            .filter(|t| a.input.allow_plan_mode_tools || (t.name != "enter_plan_mode" && t.name != "exit_plan_mode"))
            .collect();
        if a.cfg.run_mode.as_deref() == Some("ask") {
            return Err(pending("ask runMode tool filtering (filterAskModeTools)"));
        }
        let (provider, model) = match &a.input.model_override {
            Some(o) => (gs(o, "provider").unwrap_or("").to_string(), gs(o, "model").unwrap_or("").to_string()),
            None => (a.cfg.provider.clone(), a.cfg.model.clone()),
        };
        let (protocol, prompt_cache) = if a.has_metadata_port {
            (a.model_protocol(&provider).map(JsValue::Str).unwrap_or(JsValue::Undefined),
             a.metadata_for(&provider, &model).map(|s| get(s, "supportsPromptCache").clone()).unwrap_or(JsValue::Undefined))
        } else {
            (JsValue::Undefined, JsValue::Undefined)
        };
        let addendum = match &a.plan_todo {
            Some(snapshot) => plan_todo_addendum(snapshot.as_ref()),
            None => None,
        };
        let plan = if a.cfg.permission_mode == "plan" { a.cfg.plan_mode_instructions.clone() } else { None };
        let parts: Vec<String> = [a.cfg.append_system_prompt.clone(), plan, addendum].into_iter().flatten().filter(|s| !s.trim().is_empty()).collect();
        Ok(obj! {"sessionId" => &a.session_id, "turnId" => &a.turn_id, "cwd" => &a.cfg.cwd,
            "runtimeContextSurface" => &a.cfg.runtime_context_surface, "provider" => &provider, "model" => &model,
            "protocol" => protocol, "supportsPromptCache" => prompt_cache, "permissionMode" => &a.cfg.permission_mode,
            "runMode" => a.cfg.run_mode.clone().unwrap_or_else(|| "agent".into()),
            "additionalWorkingDirectories" => get(&a.cfg.permission_context, "additionalWorkingDirectories"),
            "messages" => normalize_messages(messages), "tools" => tools.iter().map(|t| tool_schema(t)).collect::<Vec<_>>(),
            "maxMessages" => a.cfg.max_context_messages, "customSystemPrompt" => a.cfg.system_prompt.clone(),
            "appendSystemPrompt" => if parts.is_empty() { None } else { Some(parts.join("\n\n")) }})
    }

    /// `createModelRequest`.
    fn create_model_request(&mut self, messages: &[JsValue], emit_instructions: bool, preview_only: bool) -> R<JsValue> {
        self.refresh_tools()?;
        let mut input = self.context_prepare_input(messages)?;
        let (provider, model) = (gs(&input, "provider").unwrap_or("").to_string(), gs(&input, "model").unwrap_or("").to_string());
        set(&mut input, "previewOnly", if preview_only { JsValue::Bool(true) } else { JsValue::Undefined });
        let prepared = self.a.context_call(self.rt, "prepare_for_model", input, true)?;
        if emit_instructions {
            let has = truthy(get(&prepared, "systemPrompt"));
            self.lifecycle_detached("InstructionsLoaded", obj! {"hasSystemPrompt" => has});
            let e = obj! {"type" => "instructions_loaded", "sessionId" => &self.a.session_id, "turnId" => &self.a.turn_id, "hasSystemPrompt" => has};
            self.a.emit_host_event(self.rt, e);
        }
        let pm = ga(&prepared, "messages").to_vec();
        if pm.iter().any(has_media_reference) {
            return Err(pending("media_reference materialization (materializeMediaReferences)"));
        }
        if truthy(get(&prepared, "cachePlan")) {
            return Err(pending("cachePlan (buildCachePlan)"));
        }
        let plan = self.a.cfg.permission_mode == "plan";
        let mut msgs = pm;
        if plan {
            msgs.push(obj! {"role" => "user", "content" => vec![obj! {"type" => "text", "text" => PLAN_MODE_REMINDER}],
                "metadata" => obj! {"synthetic" => true, "purpose" => "plan_mode_reminder"}});
        }
        let a = &*self.a;
        let mo = a.input.model_override.clone().unwrap_or(JsValue::Undefined);
        let thinking = if nullish(get(&mo, "thinking")) { a.cfg.thinking.clone() } else { get(&mo, "thinking").clone() };
        let base = obj! {"provider" => &provider, "model" => &model, "messages" => Vec::<JsValue>::new(),
            "systemPrompt" => a.cfg.system_prompt.clone(), "tools" => Vec::<JsValue>::new(), "toolChoice" => &a.cfg.tool_choice,
            "maxOutputTokens" => self.current_max_output(&provider, &model).or(a.cfg.max_output_tokens),
            "speed" => get(&mo, "speed"), "thinking" => thinking, "stream" => true, "metadata" => &a.cfg.metadata};
        let sp = if nullish(get(&prepared, "systemPrompt")) { a.cfg.system_prompt.clone().map(JsValue::Str).unwrap_or(JsValue::Undefined) } else { get(&prepared, "systemPrompt").clone() };
        Ok(spread(&base, obj! {"messages" => msgs, "systemPrompt" => sp, "tools" => get(&prepared, "tools"),
            "cacheBreakpoints" => if plan { JsValue::Undefined } else { get(&prepared, "cacheBreakpoints").clone() },
            "cachePlan" => JsValue::Undefined}))
    }

    fn materialize_prepared(&mut self, prepared: &JsValue, candidate: &JsValue) -> R<JsValue> {
        if Methods::has(&self.a.m.model, "materialize_prepared_request") {
            return Err(pending("materialize_prepared_request routing"));
        }
        Ok(spread(get(prepared, "request"), obj! {"messages" => get(candidate, "messages"),
            "cacheBreakpoints" => get(candidate, "cacheBreakpoints"), "cachePlan" => get(candidate, "cachePlan"),
            "provider" => get(prepared, "provider"), "model" => get(prepared, "model")}))
    }

    fn budget_request(&mut self, messages: &[JsValue], decision: Option<(&str, &str)>, base: Option<&JsValue>, prepared: Option<&JsValue>) -> R<JsValue> {
        let candidate = self.create_model_request(messages, false, true)?;
        if let (Some(p), Some(b)) = (prepared, base) {
            let m = spread(b, obj! {"messages" => get(&candidate, "messages"), "systemPrompt" => get(&candidate, "systemPrompt"),
                "tools" => get(&candidate, "tools"), "cacheBreakpoints" => get(&candidate, "cacheBreakpoints"), "cachePlan" => get(&candidate, "cachePlan")});
            let p = p.clone();
            return self.materialize_prepared(&p, &m);
        }
        if let Some((p, m)) = decision {
            return Ok(spread(&candidate, obj! {"provider" => p, "model" => m}));
        }
        Ok(candidate)
    }

    /// `tryAutoCompact` through the host context module.
    #[allow(clippy::too_many_arguments)]
    fn try_auto_compact(&mut self, stage: &str, max_ctx: f64, reserved: f64, budget_request: JsValue, route: (&str, &str), allow_fallback: bool) -> R<JsValue> {
        let preparation = self.context_prepare_input(&self.messages.clone())?;
        let calibration = self.calibration_for(route.0, route.1);
        let mut projection = obj! {"stage" => stage, "trigger" => "auto", "maxContextTokens" => max_ctx, "reservedOutputTokens" => reserved};
        if allow_fallback {
            set(&mut projection, "allowFallbackOnFailure", true);
        }
        let input = obj! {"sessionId" => &self.a.session_id, "turnId" => &self.a.turn_id, "messages" => self.messages.clone(),
            "maxContextTokens" => max_ctx, "reservedOutputTokens" => reserved, "budgetRequest" => budget_request,
            "budgetPreparation" => preparation, "budgetCalibration" => calibration,
            "allowFallbackOnFailure" => if allow_fallback { JsValue::Bool(true) } else { JsValue::Undefined },
            "budgetProjection" => projection};
        self.a.context_call(self.rt, "try_auto_compact", input, true)
    }

    /// Apply a compacted result (messages, calibration reset, durable boundary).
    fn apply_compacted(&mut self, compact: &JsValue) -> R<()> {
        self.messages = ga(compact, "messages").to_vec();
        self.a.calibration.clear();
        let result = get(compact, "result");
        if !self.a.input.has_persist_compaction || !truthy(result) {
            return Ok(());
        }
        let snap = get(compact, "snapshot");
        let cid = get(result, "compactionId").clone();
        let mut extra = obj! {"tier" => get(compact, "tier"),
            "summarySucceeded" => is_undef(get(result, "error")) && !is_undef(get(result, "summaryMessage"))};
        if gb(result, "cacheReset") == Some(true) {
            set(&mut extra, "cacheReset", true);
        }
        if truthy(get(compact, "error")) {
            set(&mut extra, "finalBudgetTokens", get(snap, "maxContextTokens").clone());
            set(&mut extra, "finalUsedTokens", get(snap, "tokens").clone());
            set(&mut extra, "finalBudgetRatio", get(snap, "ratio").clone());
            set(&mut extra, "error", get(compact, "error").clone());
        }
        let summary_generated = match get(result, "summaryGenerated") {
            JsValue::Undefined | JsValue::Null => !is_undef(get(result, "summaryMessage")),
            v => truthy(v),
        };
        let checkpoint = match get(result, "checkpointMerged") {
            JsValue::Undefined | JsValue::Null => gb(result, "cacheReset") == Some(true),
            v => truthy(v),
        };
        let mut meta = obj! {"compactionId" => &cid, "trigger" => get(result, "trigger"), "preTokens" => get(result, "preTokens"),
            "postTokens" => get(snap, "tokens"), "messagesSummarized" => get(result, "messagesSummarized")};
        if !is_undef(get(result, "targetPostTokens")) {
            set(&mut meta, "targetTokens", get(result, "targetPostTokens").clone());
        }
        set(&mut meta, "summaryGenerated", summary_generated);
        set(&mut meta, "checkpointMerged", checkpoint);
        set(&mut meta, "finalRatio", get(snap, "ratio").clone());
        set(&mut meta, "extra", extra);
        if truthy(&cid) {
            let p = self.a.timeline.position(&format!("compact:{}", crate::rt::js_string(&cid)), false);
            set(&mut meta, "timeline", p);
        }
        let boundary = obj! {"kind" => "compact", "subtype" => "compact_boundary", "compactMetadata" => meta};
        let marked: Vec<JsValue> = self.messages.iter().map(|m| {
            let md = if is_obj(get(m, "metadata")) { get(m, "metadata").clone() } else { obj! {} };
            spread(m, obj! {"metadata" => spread(&md, obj! {"compactReplacement" => true, "compactSnapshotId" => &cid})})
        }).collect();
        match self.a.turn_call_with(self.rt, "persist_compaction", obj! {"boundary" => boundary, "messages" => marked}) {
            Ok(_) => Ok(()),
            Err(e) => Err(LoopErr { kind: ErrKind::Fatal, code: None, message: e.message }),
        }
    }
}

pub const PLAN_MODE_REMINDER: &str = "Plan mode is active.\nRead first using read-only tools, then write or refine plan markdown only under `.pilotdeck/plans/`.\nDo not make implementation changes while planning.\nWhen the plan is ready for user review, call `exit_plan_mode` with the plan file path.";

fn plan_todo_addendum(snapshot: Option<&JsValue>) -> Option<String> {
    let s = snapshot?;
    if !truthy(get(s, "approvedPlan")) {
        return None;
    }
    if truthy(get(s, "requiresInitialization")) {
        return Some(["You are executing an approved plan.",
            "Before using any non-read-only tool, you MUST call `todo_write` with a markdown checklist derived from the approved plan.",
            "Represent completed items as `- [x]` and remaining items as `- [ ]`."].join("\n"));
    }
    let n = gn(s, "toolCallsSinceLastTodoWrite").unwrap_or(0.0);
    if n >= 10.0 {
        return Some(format!("You haven't updated the todo list in a while ({} tool calls since last update). Consider calling `todo_write` to reflect your current progress. This is a gentle reminder - ignore if not applicable.",
            pilotdeck_skills::jsjson::js_number_to_string(n)));
    }
    None
}

fn has_tool_call(m: &JsValue) -> bool {
    ga(m, "content").iter().any(|b| gs(b, "type") == Some("tool_call"))
}

/// `normalizeMessagesForModelRequest` + `cloneMessages`.
pub fn normalize_messages(messages: &[JsValue]) -> Vec<JsValue> {
    let mut out: Vec<JsValue> = Vec::new();
    for raw in messages {
        let content = ga(raw, "content").to_vec();
        let message = spread(raw, obj! {"content" => content.clone()});
        if let Some(last) = out.last() {
            if gs(last, "role") == Some("assistant") && gs(&message, "role") == Some("assistant") && !has_tool_call(last) && !has_tool_call(&message) {
                let mut merged = ga(last, "content").to_vec();
                merged.extend(content);
                let (lm, mm) = (get(last, "metadata"), get(&message, "metadata"));
                let md = if is_undef(lm) && is_undef(mm) {
                    JsValue::Undefined
                } else {
                    let base = if is_obj(lm) { lm.clone() } else { obj! {} };
                    if is_obj(mm) { spread(&base, mm.clone()) } else { base }
                };
                *out.last_mut().unwrap() = obj! {"role" => "assistant", "content" => merged, "metadata" => md};
                continue;
            }
        }
        if gs(&message, "role") == Some("assistant") && content.is_empty() {
            continue;
        }
        out.push(message);
    }
    out
}

fn has_media_reference(m: &JsValue) -> bool {
    ga(m, "content").iter().any(|b| gs(b, "type") == Some("media_reference")
        || ga(b, "content").iter().any(|c| gs(c, "type") == Some("media_reference")))
}

// ---------------- model + tool ports ----------------
fn invalid_model(code: &str, m: &str) -> LoopErr {
    LoopErr::code(code, m)
}
fn pos_int(v: &JsValue) -> Option<f64> {
    num(v).filter(|n| is_safe_int(*n) && *n > 0.0)
}

impl<'a> Run<'a> {
    fn model_context(&self) -> JsValue {
        let a = &*self.a;
        let mo = a.input.model_override.as_ref().map(|o| obj! {"provider" => get(o, "provider"), "model" => get(o, "model")});
        obj! {"sessionId" => &a.session_id, "turnId" => &a.turn_id, "runId" => &a.run_id, "operationId" => &a.operation_id,
            "idempotencyKey" => a.raw_idempotency_key.clone(), "operationDeadline" => a.operation_deadline.clone(),
            "modelOverride" => mo}
    }
    fn model_call(&self, request_id: String, payload: JsValue, abortable: bool) -> crate::rt::Call {
        let mut c = self.a.model_call(request_id, payload, abortable);
        c.idempotency_key = self.a.raw_idempotency_key.clone();
        c
    }

    /// `modelPort.prepare`.
    fn prepare(&mut self, request: &JsValue) -> R<(JsValue, String)> {
        let request = json_clone(request);
        let fallback = obj! {"request" => &request, "provider" => get(&request, "provider"), "model" => get(&request, "model")};
        let preparation_id = self.a.next_preparation_id();
        if !Methods::has(&self.a.m.model, "prepare") {
            return Ok((fallback, preparation_id));
        }
        let payload = obj! {"operation" => "prepare", "request" => &request, "preparationId" => &preparation_id, "context" => self.model_context()};
        let call = self.model_call(format!("model-prepare-{}", random_id()), payload, true);
        let r = self.rt.call(call)?;
        if gb(&r, "ok") != Some(true) {
            return Err(model_module_failure(&r, "Model prepare failed"));
        }
        let v = get(get(&r, "payload"), "prepared");
        if !is_obj(v) {
            return Err(invalid_model("INVALID_MODEL_PREPARATION", "Model prepare response must contain a prepared invocation."));
        }
        if gs(v, "provider").is_none() || gs(v, "model").is_none() || !is_obj(get(v, "request")) {
            return Err(invalid_model("INVALID_MODEL_PREPARATION", "Model prepare response contains an invalid prepared invocation."));
        }
        let prepared = obj! {"request" => json_clone(get(v, "request")), "provider" => get(v, "provider"), "model" => get(v, "model"),
            "maxContextTokens" => pos_int(get(v, "maxContextTokens")), "maxOutputTokens" => pos_int(get(v, "maxOutputTokens"))};
        if self.a.has_metadata_port {
            let md = get(get(&r, "payload"), "metadata").clone();
            let (p, m) = (gs(&prepared, "provider").unwrap().to_string(), gs(&prepared, "model").unwrap().to_string());
            self.a.apply_prepared_metadata(&md, Some((&p, &m)))?;
        }
        Ok((prepared, preparation_id))
    }

    fn close_stream(&mut self, preparation_id: &str) {
        if !Methods::has(&self.a.m.model, "close_stream") {
            return;
        }
        let mut call = self.model_call(format!("model-close-stream-{}", random_id()), obj! {"operation" => "close_stream", "preparationId" => preparation_id}, false);
        call.record_failure = false;
        let _ = self.rt.call(call);
    }

    // ---- tool execution (hostCapabilityToolPort.executeAll) ----
    fn tool_context(&self, output_truncated: bool) -> JsValue {
        let a = &*self.a;
        let pc = spread(&a.cfg.permission_context, obj! {"cwd" => &a.cfg.cwd, "canPrompt" => a.input.can_prompt});
        obj! {"sessionId" => &a.session_id, "turnId" => &a.turn_id, "cwd" => &a.cfg.cwd, "permissionMode" => &a.cfg.permission_mode,
            "permissionContext" => pc, "runMode" => a.cfg.run_mode.clone().unwrap_or_else(|| "agent".into()),
            "outputTruncated" => if output_truncated { JsValue::Bool(true) } else { JsValue::Undefined }}
    }
    fn execution_context(&self) -> JsValue {
        obj! {"runId" => &self.a.run_id, "turnId" => &self.a.turn_id, "operationDeadline" => self.a.operation_deadline.clone()}
    }
    fn tool_call_binding(&self, request_id: String, payload: JsValue) -> crate::rt::Call {
        let mut c = self.a.bind("capability", request_id, payload);
        c.abortable = true;
        c
    }
    fn cancelled(&mut self) -> LoopErr {
        self.rt.abort(s("tool_cancelled"));
        LoopErr::msg("Tool execution cancelled.")
    }

    fn execute_all(&mut self, calls: &[JsValue], ctx: JsValue) -> R<Vec<JsValue>> {
        if Methods::has(&self.a.m.capability, "execute_batch") && !calls.is_empty() {
            let payload = obj! {"operation" => "execute_batch",
                "calls" => calls.iter().map(|c| obj! {"name" => get(c, "name"), "arguments" => get(c, "input"), "toolCallId" => get(c, "id")}).collect::<Vec<_>>(),
                "context" => ctx, "execution" => self.execution_context()};
            let call = self.tool_call_binding(format!("tool-batch-{}", random_id()), payload);
            let r = match self.rt.call(call) {
                Ok(r) => r,
                Err(e) => {
                    if matches!(e.kind, ErrKind::Aborted) || self.rt.aborted() {
                        return Err(self.cancelled());
                    }
                    return Ok(calls.iter().map(|c| transport_failure_result(c, &e)).collect());
                }
            };
            if gb(&r, "ok") != Some(true) {
                return Ok(calls.iter().map(|c| module_failure_result(c, &r)).collect());
            }
            let results = ga(get(&r, "payload"), "results");
            if arr(get(get(&r, "payload"), "results")).is_none() || results.len() != calls.len() {
                return Err(LoopErr::msg("Capability batch response must contain one result for every tool call."));
            }
            let mut out = Vec::new();
            for (i, (res, c)) in results.iter().zip(calls).enumerate() {
                if !is_obj(res) || matches!(res, JsValue::Obj(o) if o.get("type").is_none()) {
                    return Err(LoopErr::msg(format!("Capability batch result {i} is invalid.")));
                }
                if get(res, "toolCallId") != get(c, "id") {
                    return Err(LoopErr::msg(format!("Capability batch result {i} does not match tool call {}.", crate::rt::js_string(get(c, "id")))));
                }
                out.push(res.clone());
            }
            return Ok(out);
        }
        // Per-call path: concurrency-safe calls are issued together, then the rest in order.
        let safe = |c: &JsValue| self.a.tools.iter().find(|t| Some(t.name.as_str()) == gs(c, "name")).map_or(false, |t| t.concurrency_safe);
        let concurrent: Vec<usize> = (0..calls.len()).filter(|i| safe(&calls[*i])).collect();
        let sequential: Vec<usize> = (0..calls.len()).filter(|i| !safe(&calls[*i])).collect();
        let mut slots: Vec<Option<JsValue>> = vec![None; calls.len()];
        let mut sent = Vec::new();
        for &i in &concurrent {
            if self.rt.aborted() {
                return Err(LoopErr::msg("Tool execution cancelled."));
            }
            let call = self.single_tool_call(&calls[i], &ctx);
            let id = self.rt.send_call(&call);
            sent.push((i, id, call));
        }
        let mut first_err: Option<LoopErr> = None;
        for (i, id, call) in sent {
            let r = self.rt.wait(&id, &call);
            match self.finish_single(&calls[i], r) {
                Ok(v) => slots[i] = Some(v),
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }
        if let Some(e) = first_err {
            return Err(e);
        }
        for &i in &sequential {
            if self.rt.aborted() {
                return Err(LoopErr::msg("Tool execution cancelled."));
            }
            let call = self.single_tool_call(&calls[i], &ctx);
            let r = self.rt.call(call);
            slots[i] = Some(self.finish_single(&calls[i], r)?);
        }
        Ok(slots.into_iter().map(|s| s.unwrap_or(JsValue::Undefined)).collect())
    }
    fn single_tool_call(&self, c: &JsValue, ctx: &JsValue) -> crate::rt::Call {
        let payload = obj! {"name" => get(c, "name"), "arguments" => get(c, "input"), "toolCallId" => get(c, "id"),
            "context" => ctx, "execution" => self.execution_context()};
        self.tool_call_binding(format!("tool-{}", random_id()), payload)
    }
    fn finish_single(&mut self, c: &JsValue, r: R<JsValue>) -> R<JsValue> {
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                if matches!(e.kind, ErrKind::Aborted) || self.rt.aborted() {
                    return Err(self.cancelled());
                }
                return Ok(transport_failure_result(c, &e));
            }
        };
        if self.rt.aborted() {
            return Err(LoopErr::msg("Tool execution cancelled."));
        }
        let ok = gb(&r, "ok") == Some(true);
        let payload = get(&r, "payload");
        let err = get(&r, "error");
        let code_src = if nullish(get(&r, "code")) { get(err, "code") } else { get(&r, "code") };
        let code = if nullish(code_src) { String::new() } else { crate::rt::js_string(code_src).to_uppercase() };
        let msg = if nullish(get(err, "message")) { String::new() } else { crate::rt::js_string(get(err, "message")) };
        let cancel_word = |t: &str| cancel_re(t);
        if !ok && (["CANCELLED", "ABORTED", "TOOL_ABORTED"].contains(&code.as_str()) || cancel_word(&format!("{code} {msg}"))) {
            return Err(self.cancelled());
        }
        if ok && is_obj(payload) && gs(payload, "type") == Some("error") && is_obj(get(payload, "error")) {
            let pe = get(payload, "error");
            let pc = if nullish(get(pe, "code")) { String::new() } else { crate::rt::js_string(get(pe, "code")).to_uppercase() };
            let pm = if nullish(get(pe, "message")) { String::new() } else { crate::rt::js_string(get(pe, "message")) };
            if ["CANCELLED", "ABORTED", "TOOL_ABORTED"].contains(&pc.as_str()) || cancel_word(&pm) {
                return Err(self.cancelled());
            }
        }
        if ok && is_obj(payload) && matches!(payload, JsValue::Obj(o) if o.get("type").is_some()) {
            return Ok(payload.clone());
        }
        Ok(module_failure_result(c, &r))
    }
}

/// `/\bcancel(?:led|lation)?\b/i`
fn cancel_re(t: &str) -> bool {
    let l = t.to_lowercase();
    let b = l.as_bytes();
    let wc = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while let Some(off) = l[i..].find("cancel") {
        let s0 = i + off;
        if s0 == 0 || !wc(b[s0 - 1]) {
            let mut e = s0 + 6;
            for suf in ["lation", "led", ""] {
                if l[e..].starts_with(suf) {
                    let end = e + suf.len();
                    if end == b.len() || !wc(b[end]) {
                        return true;
                    }
                }
            }
            e = s0 + 1;
            i = e;
            continue;
        }
        i = s0 + 1;
    }
    false
}

const TOOL_ERROR_CODES: &[&str] = &["tool_not_found", "tool_unavailable", "invalid_tool_input", "permission_denied", "permission_cancelled",
    "permission_required", "tool_execution_failed", "tool_aborted", "tool_timeout", "result_too_large", "path_not_allowed", "file_not_found",
    "file_conflict", "unsupported_tool", "setup_required", "plan_mode_violation", "ask_mode_violation"];

fn module_failure_result(c: &JsValue, r: &JsValue) -> JsValue {
    let code = gs(r, "code").filter(|x| TOOL_ERROR_CODES.contains(x)).unwrap_or("tool_execution_failed");
    let err = get(r, "error");
    let message = if nullish(get(err, "message")) { "Capability module failed.".to_string() } else { crate::rt::js_string(get(err, "message")) };
    let mut e = obj! {"code" => code, "message" => &message};
    if truthy(get(r, "code")) || truthy(err) {
        let mut d = obj! {};
        if truthy(get(r, "code")) {
            set(&mut d, "moduleCode", get(r, "code").clone());
        }
        if truthy(err) {
            set(&mut d, "moduleError", err.clone());
        }
        set(&mut e, "details", d);
    }
    let ts = crate::rt::iso_now();
    obj! {"type" => "error", "toolCallId" => get(c, "id"), "toolName" => get(c, "name"), "error" => e,
        "content" => vec![obj! {"type" => "text", "text" => &message}], "startedAt" => &ts, "completedAt" => &ts}
}

fn transport_failure_result(c: &JsValue, e: &LoopErr) -> JsValue {
    let code = e.code.clone().unwrap_or_else(|| "MODULE_TRANSPORT_UNAVAILABLE".into());
    let ts = crate::rt::iso_now();
    obj! {"type" => "error", "toolCallId" => get(c, "id"), "toolName" => get(c, "name"),
        "error" => obj! {"code" => if code == "MODULE_TIMEOUT" { "tool_timeout" } else { "tool_execution_failed" }, "message" => &e.message,
            "details" => obj! {"moduleCode" => &code, "outcome" => "result_unknown", "retryability" => "unsafe"}},
        "content" => vec![obj! {"type" => "text", "text" => &e.message}], "startedAt" => &ts, "completedAt" => &ts}
}

// ---------------- stream assembler (assembleModelMessage.ts) ----------------
struct Assembler {
    response_id: String,
    content: Vec<JsValue>,
    text: String,
    model: Option<JsValue>,
    thinking: String,
    reasoning: String,
    signature: Option<JsValue>,
    usage: JsValue,
    finish: Option<String>,
    has_end: bool,
    error: Option<JsValue>,
    tool_calls: Vec<JsValue>,
    repaired: bool,
}

impl Assembler {
    fn new() -> Self {
        Assembler { response_id: crate::util::uuid_v4(), content: Vec::new(), text: String::new(), model: None, thinking: String::new(),
            reasoning: String::new(), signature: None, usage: obj! {}, finish: None, has_end: false, error: None, tool_calls: Vec::new(), repaired: false }
    }
    fn block_id(&self, kind: &str) -> String {
        let n = self.content.iter().filter(|b| gs(b, "type") == Some(kind)).count();
        format!("{}:{kind}:{n}", self.response_id)
    }
    fn flush(&mut self) {
        if !self.thinking.is_empty() || !self.reasoning.is_empty() || self.signature.is_some() {
            let mut b = obj! {"type" => "thinking", "text" => &self.thinking, "blockId" => self.block_id("thinking")};
            if !self.reasoning.is_empty() {
                set(&mut b, "reasoningContent", &self.reasoning);
            }
            if let Some(sig) = self.signature.take() {
                set(&mut b, "signature", sig);
            }
            self.content.push(b);
            self.thinking.clear();
            self.reasoning.clear();
        }
        if !self.text.is_empty() {
            let b = obj! {"type" => "text", "text" => &self.text, "blockId" => self.block_id("text")};
            self.content.push(b);
            self.text.clear();
        }
    }
    fn apply(&mut self, e: &JsValue) {
        match gs(e, "type").unwrap_or("") {
            "request_started" => self.model = Some(get(e, "model").clone()),
            "text_delta" => {
                if !self.thinking.is_empty() || !self.reasoning.is_empty() || self.signature.is_some() {
                    self.flush();
                }
                self.text.push_str(&crate::rt::js_string(get(e, "text")));
            }
            "thinking_delta" => {
                if !self.text.is_empty() {
                    self.flush();
                }
                self.thinking.push_str(&crate::rt::js_string(get(e, "text")));
                if !is_undef(get(e, "reasoningContent")) {
                    self.reasoning.push_str(&crate::rt::js_string(get(e, "reasoningContent")));
                }
                if let Some(sig) = gs(e, "signature").filter(|x| !x.is_empty()) {
                    self.signature = Some(s(sig));
                }
            }
            "tool_call_end" => {
                self.flush();
                let tc = get(e, "toolCall").clone();
                self.tool_calls.push(tc.clone());
                self.content.push(spread(&obj! {"type" => "tool_call"}, tc));
                if truthy(get(e, "wasRepaired")) {
                    self.repaired = true;
                }
            }
            "message_end" => {
                self.flush();
                self.finish = gs(e, "finishReason").map(String::from);
                self.has_end = true;
            }
            "usage" => {
                let u = get(e, "usage");
                let add = |k: &str, a: &JsValue| match (gn(a, k), gn(u, k)) {
                    (None, None) => JsValue::Undefined,
                    (x, y) => JsValue::Num(x.unwrap_or(0.0) + y.unwrap_or(0.0)),
                };
                let a = self.usage.clone();
                self.usage = obj! {"inputTokens" => add("inputTokens", &a), "outputTokens" => add("outputTokens", &a),
                    "cacheReadTokens" => add("cacheReadTokens", &a), "cacheWriteTokens" => add("cacheWriteTokens", &a),
                    "totalTokens" => add("totalTokens", &a), "nativeCost" => add("nativeCost", &a)};
            }
            "error" => {
                self.flush();
                self.error = Some(get(e, "error").clone());
                self.finish = Some("error".into());
            }
            _ => {}
        }
    }
    fn assemble(&mut self) -> Assembled {
        self.flush();
        if !self.tool_calls.is_empty() {
            let mut used: Vec<String> = Vec::new();
            let mut ids = Vec::new();
            for (i, tc) in self.tool_calls.iter().enumerate() {
                let raw = gs(tc, "id").map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
                let base = raw.unwrap_or_else(|| format!("call_{i}"));
                let mut id = base.clone();
                let mut n = 2;
                while used.contains(&id) {
                    id = format!("{base}_{n}");
                    n += 1;
                }
                used.push(id.clone());
                ids.push(id);
            }
            for (tc, id) in self.tool_calls.iter_mut().zip(&ids) {
                if gs(tc, "id") != Some(id.as_str()) {
                    *tc = spread(tc, obj! {"id" => id.as_str()});
                }
            }
            let mut k = 0;
            for b in self.content.iter_mut() {
                if gs(b, "type") == Some("tool_call") {
                    if let Some(id) = ids.get(k) {
                        *b = spread(b, obj! {"id" => id.as_str()});
                    }
                    k += 1;
                }
            }
        }
        let mut message = obj! {"role" => "assistant", "content" => self.content.clone()};
        if let Some(m) = &self.model {
            if truthy(m) {
                set(&mut message, "metadata", obj! {"model" => m});
            }
        }
        let has_usage = matches!(&self.usage, JsValue::Obj(o) if o.iter().any(|(_, v)| !is_undef(v)));
        Assembled {
            message,
            finish: self.finish.clone().unwrap_or_else(|| if self.error.is_some() { "error".into() } else { "unknown".into() }),
            has_end: self.has_end,
            usage: if has_usage { self.usage.clone() } else { JsValue::Undefined },
            error: self.error.clone(),
            repaired: self.repaired,
        }
    }
}

struct Assembled {
    message: JsValue,
    finish: String,
    has_end: bool,
    usage: JsValue,
    error: Option<JsValue>,
    repaired: bool,
}

fn collect_tool_calls(m: &JsValue) -> Vec<JsValue> {
    ga(m, "content").iter().filter(|b| gs(b, "type") == Some("tool_call"))
        .map(|b| obj! {"id" => get(b, "id"), "name" => get(b, "name"), "input" => get(b, "input"), "raw" => get(b, "raw")}).collect()
}

fn is_fatal_compaction(e: &LoopErr) -> bool {
    matches!(e.kind, ErrKind::Fatal) || e.code.as_deref() == Some("COMPACTION_BUDGET_CONTRACT_INVALID")
}

fn tool_result_block(r: &JsValue) -> JsValue {
    let content: Vec<JsValue> = ga(r, "content").iter().map(|c| match gs(c, "type") {
        Some("image") => obj! {"type" => "image", "source" => "base64", "data" => get(c, "data"), "mimeType" => get(c, "mimeType"), "bytes" => get(c, "bytes"), "detail" => get(c, "detail")},
        Some("pdf") => obj! {"type" => "pdf", "source" => "base64", "data" => get(c, "data"), "mimeType" => get(c, "mimeType"), "bytes" => get(c, "bytes"), "pages" => get(c, "pages")},
        _ => obj! {"type" => "text", "text" => content_to_text(c)},
    }).collect();
    let sanitize = |c: &JsValue| match gs(c, "type") {
        Some("image" | "pdf" | "audio") => {
            let mut x = c.clone();
            remove(&mut x, "data");
            x
        }
        _ => c.clone(),
    };
    let mut raw = spread(r, obj! {"content" => ga(r, "content").iter().map(sanitize).collect::<Vec<_>>()});
    if is_obj(r) && !is_undef(get(r, "supplementalMessages")) && truthy(get(r, "supplementalMessages")) {
        let sm: Vec<JsValue> = ga(r, "supplementalMessages").iter()
            .map(|m| spread(m, obj! {"content" => ga(m, "content").iter().map(sanitize).collect::<Vec<_>>()})).collect();
        set(&mut raw, "supplementalMessages", sm);
    }
    obj! {"type" => "tool_result", "toolCallId" => get(r, "toolCallId"),
        "isError" => if gs(r, "type") == Some("error") { JsValue::Bool(true) } else { JsValue::Undefined },
        "content" => if content.is_empty() { vec![obj! {"type" => "text", "text" => "Tool completed with no output."}] } else { content },
        "raw" => raw}
}

fn content_to_text(c: &JsValue) -> JsValue {
    let len = |k: &str| crate::rt::js_string(get(c, k)).encode_utf16().count();
    match gs(c, "type") {
        Some("text") => get(c, "text").clone(),
        Some("json") => s(&to_json(get(c, "value"))),
        Some("image") => s(&format!("[Image: {}, {} base64 characters]", crate::rt::js_string(get(c, "mimeType")), len("data"))),
        Some("pdf") => s(&format!("[PDF: {}, {} base64 characters{}]", crate::rt::js_string(get(c, "mimeType")), len("data"),
            if truthy(get(c, "pages")) { format!(", {} pages", crate::rt::js_string(get(c, "pages"))) } else { String::new() })),
        Some("file") => s(&format!("[File: {}{}{}]", crate::rt::js_string(get(c, "path")),
            if truthy(get(c, "mimeType")) { format!(", {}", crate::rt::js_string(get(c, "mimeType"))) } else { String::new() },
            if truthy(get(c, "description")) { format!(", {}", crate::rt::js_string(get(c, "description"))) } else { String::new() })),
        _ => JsValue::Undefined,
    }
}

/// `projectToolResults`.
fn project_tool_results(results: &[JsValue]) -> Vec<JsValue> {
    let mut out = vec![obj! {"role" => "user", "content" => results.iter().map(tool_result_block).collect::<Vec<_>>()}];
    for r in results {
        for m in ga(r, "supplementalMessages") {
            let blocks: Vec<JsValue> = ga(m, "content").iter().map(|c| match gs(c, "type") {
                Some("image") | Some("pdf") => tool_result_block(&obj! {"content" => vec![c.clone()]}).pipe_first(),
                Some("text") => obj! {"type" => "text", "text" => get(c, "text")},
                _ => obj! {"type" => "text", "text" => to_json(c)},
            }).collect();
            out.push(obj! {"role" => "user", "content" => blocks,
                "metadata" => obj! {"synthetic" => true, "purpose" => "tool_result_supplemental", "toolCallId" => get(r, "toolCallId")}});
        }
    }
    out
}

trait PipeFirst {
    fn pipe_first(self) -> JsValue;
}
impl PipeFirst for JsValue {
    fn pipe_first(self) -> JsValue {
        ga(&self, "content").first().cloned().unwrap_or(JsValue::Undefined)
    }
}

fn recovery_of(r: &JsValue) -> JsValue {
    let rec = get(get(r, "metadata"), "recovery");
    if is_obj(rec) { rec.clone() } else { JsValue::Undefined }
}
fn failure_key(r: &JsValue) -> String {
    let rec = recovery_of(r);
    let class = if nullish(get(&rec, "failureClass")) { "unknown".to_string() } else { crate::rt::js_string(get(&rec, "failureClass")) };
    format!("{}::{}::{}", crate::rt::js_string(get(r, "toolName")), crate::rt::js_string(get(get(r, "error"), "code")), class)
}
fn sorted_js(mut v: Vec<String>) -> Vec<String> {
    // Array.prototype.sort default: UTF-16 code unit order.
    v.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    v
}

// ---------------- runOrdered ----------------
impl<'a> Run<'a> {
    fn err_result(&self, stop: &str, error: JsValue, with_structured: bool) -> JsValue {
        self.turn_result("error", stop, Some(vec![error]), with_structured, None, None)
    }
    fn stop_failure(&mut self, message: &str) -> R<()> {
        self.lifecycle("StopFailure", obj! {"error" => message})?;
        self.publish_ev("stop_failure", obj! {"error" => message});
        Ok(())
    }
    /// Push a durable assistant message and publish it.
    fn push_assistant(&mut self, m: JsValue) {
        self.push_and_publish("assistant_message", m);
        self.final_message = self.messages.last().cloned().unwrap_or(JsValue::Undefined);
    }
    fn last_assistant(&self) -> JsValue {
        self.messages.iter().rev().find(|m| gs(m, "role") == Some("assistant")).cloned().unwrap_or(JsValue::Undefined)
    }

    pub fn run(&mut self) -> R<RunResult> {
        // ---- per-turn setup ----
        self.clear_turn_scoped_caps();
        match &self.a.input.run_mode {
            Some(m) => self.a.cfg.run_mode = Some(m.clone()),
            None => {
                if self.a.cfg.run_mode.is_none() {
                    self.a.cfg.run_mode = Some("agent".into());
                }
            }
        }
        if self.a.cfg.run_mode.as_deref() == Some("ask") {
            return Err(pending("ask runMode"));
        }
        {
            let pm = self.a.input.permission_mode.clone();
            if !pm.is_empty() {
                if pm == "plan" && self.a.cfg.permission_mode != "plan" {
                    self.a.cfg.permission_mode_before_plan = Some(self.a.input.base_permission_mode.clone().unwrap_or_else(|| self.a.cfg.permission_mode.clone()));
                }
                self.a.cfg.permission_mode = pm.clone();
                set(&mut self.a.cfg.permission_context, "mode", pm);
            }
            if let Some(rules) = self.a.input.permission_rules.clone() {
                for k in ["allow", "deny", "ask"] {
                    let mut target = get(get(&self.a.cfg.permission_context, "rules"), k).clone();
                    let Some(list) = arr(&target).cloned() else {
                        return Err(LoopErr::msg("Cannot read properties of undefined (reading 'filter')"));
                    };
                    let mut next: Vec<JsValue> = list.into_iter().filter(|r| gs(r, "source") != Some("user")).collect();
                    next.extend(ga(&rules, k).iter().filter(|r| gs(r, "source") == Some("user")).cloned());
                    target = JsValue::Arr(next);
                    let mut rs = get(&self.a.cfg.permission_context, "rules").clone();
                    set(&mut rs, k, target);
                    set(&mut self.a.cfg.permission_context, "rules", rs);
                }
            }
        }
        self.started_at = crate::rt::iso_now();
        let tracks_budget = self.a.input.max_budget_usd.is_some() || self.a.input.task_budget_usd.is_some();
        let mut spent = self.a.input.initial_task_budget_spent_usd.unwrap_or(0.0);
        let mut turn_spent = 0.0;
        self.a.active_budget = if tracks_budget {
            Some((0.0, self.a.input.task_budget_usd, self.a.input.initial_task_budget_spent_usd.unwrap_or(0.0)))
        } else {
            None
        };
        let mut has_attempted_compact = false;
        let mut has_attempted_output_retry = false;
        let mut has_attempted_empty_retry = false;
        let mut max_output_recovery = 0u32;
        let mut consecutive_empty = 0u32;
        let mut json_self_correct = 0u32;
        let mut reasoning_retry = false;
        let mut image_strip = false;
        let mut stream_recoveries = 0u32;
        let mut unknown_recoveries = 0u32;
        let mut last_invalid_fp: Option<String> = None;
        let mut same_invalid = 0u32;
        let mut grace_used = false;
        let mut last_failure_fp: Option<String> = None;
        let has_compaction = Methods::has(&self.a.m.context, "try_auto_compact");

        loop {
            if self.rt.aborted() {
                return self.aborted_result(true);
            }
            if self.a.input.has_drain_steer {
                let p = self.a.turn_call(self.rt, "drain_steer")?;
                let steers = Self::steer_list(&p)?;
                self.apply_steers(steers);
            }

            // ---- pre-route compaction ----
            let mut pending_budget = JsValue::Undefined;
            let pre_ctx = self.pre_routing_max_context();
            if has_compaction {
                let attempt: R<Option<RunResult>> = (|| {
                    let reserved = self.reserved_output(None);
                    let msgs = self.messages.clone();
                    let br = self.budget_request(&msgs, None, None, None)?;
                    let route = (gs(&br, "provider").unwrap_or("").to_string(), gs(&br, "model").unwrap_or("").to_string());
                    let compact = self.try_auto_compact("pre_route", pre_ctx, reserved, br, (&route.0, &route.1), false)?;
                    if gs(&compact, "type") == Some("compacted") {
                        self.apply_compacted(&compact)?;
                        self.continued("auto_compact");
                        if truthy(get(&compact, "error")) {
                            return self.context_overflow(&compact).map(Some);
                        }
                    }
                    pending_budget = get(&compact, "snapshot").clone();
                    Ok(None)
                })();
                match attempt {
                    Ok(Some(done)) => return Ok(done),
                    Ok(None) => {}
                    Err(e) if is_fatal_compaction(&e) => return Err(e),
                    Err(e) => log_compact_failure("pre-routing", &self.a.session_id, &self.a.turn_id, &e),
                }
            }

            let msgs = self.messages.clone();
            let mut request = self.create_model_request(&msgs, true, false)?;
            if self.rt.aborted() {
                return self.aborted_result(true);
            }
            self.lifecycle_detached("PreModelRequest", obj! {"provider" => get(&request, "provider"), "model" => get(&request, "model")});
            self.publish_ev("model_request_started", obj! {"model" => get(&request, "model"), "provider" => get(&request, "provider")});

            let (mut prepared, preparation_id) = self.prepare(&request)?;
            let decision = (gs(&prepared, "provider").unwrap_or("").to_string(), gs(&prepared, "model").unwrap_or("").to_string());
            let (rp, rm) = (decision.0.clone(), decision.1.clone());
            let routed_max_output = gn(&prepared, "maxOutputTokens").or_else(|| self.token_limits(&rp, &rm).1);

            let mut emitted_budget = false;
            if has_compaction {
                let routed_ctx = gn(&prepared, "maxContextTokens").unwrap_or_else(|| self.current_max_context(&rp, &rm));
                if routed_ctx != pre_ctx {
                    let attempt: R<Option<RunResult>> = (|| {
                        let reserved = self.reserved_output(Some((&rp, &rm)));
                        let msgs = self.messages.clone();
                        let br = self.budget_request(&msgs, Some((&rp, &rm)), Some(&request), Some(&prepared))?;
                        let compact = self.try_auto_compact("routed", routed_ctx, reserved, br, (&rp, &rm), false)?;
                        if gs(&compact, "type") == Some("compacted") {
                            self.apply_compacted(&compact)?;
                            let msgs = self.messages.clone();
                            let r2 = self.create_model_request(&msgs, true, false)?;
                            request = self.apply_token_caps(&r2, &rp, &rm);
                            let materialized = self.materialize_prepared(&prepared, &request)?;
                            set(&mut prepared, "request", json_clone(&materialized));
                            self.continued("auto_compact");
                            if truthy(get(&compact, "error")) {
                                self.publish_ev("context_budget", obj! {"snapshot" => get(&compact, "snapshot")});
                                return self.context_overflow(&compact).map(Some);
                            }
                        }
                        self.publish_ev("context_budget", obj! {"snapshot" => get(&compact, "snapshot")});
                        emitted_budget = true;
                        Ok(None)
                    })();
                    match attempt {
                        Ok(Some(done)) => return Ok(done),
                        Ok(None) => {}
                        Err(e) if is_fatal_compaction(&e) => return Err(e),
                        Err(e) => log_compact_failure("post-routing", &self.a.session_id, &self.a.turn_id, &e),
                    }
                }
            }
            let capped = self.apply_token_caps(get(&prepared, "request"), &rp, &rm);
            set(&mut prepared, "request", capped.clone());
            request = capped;
            self.clear_attempt_cap(&rp, &rm);
            if !is_undef(&pending_budget) && !emitted_budget {
                self.publish_ev("context_budget", obj! {"snapshot" => pending_budget.clone()});
            }

            let calibration_request = get(&prepared, "request").clone();
            let input_estimate = self.a.estimate_request_input(self.rt, &calibration_request)?;
            let fingerprint = crate::util::sha256_hex(to_json(&calibration_request).as_bytes());
            let mut asm = Assembler::new();
            let mut executed: Option<(JsValue, JsValue, JsValue)> = None;

            // ---- stream ----
            let stream_result: R<()> = (|| {
                if Methods::has(&self.a.m.model, "stream_next") {
                    let mut done = false;
                    let mut broke = false;
                    let res: R<()> = (|| {
                        while !done {
                            let payload = obj! {"operation" => "stream_next", "request" => json_clone(get(&prepared, "request")),
                                "preparationId" => &preparation_id, "context" => self.model_context()};
                            let call = self.model_call(format!("model-stream-next-{}", random_id()), payload, true);
                            let r = self.rt.call(call)?;
                            if gb(&r, "ok") != Some(true) {
                                return Err(model_module_failure(&r, "Model stream failed"));
                            }
                            let p = get(&r, "payload");
                            let (Some(events), Some(d)) = (arr(get(p, "events")).cloned(), gb(p, "done")) else {
                                return Err(invalid_model("INVALID_MODEL_RESPONSE", "Model stream_next response must contain events and done."));
                            };
                            for event in events {
                                if self.on_model_event(&event, &mut asm, &mut executed) {
                                    broke = true;
                                    return Ok(());
                                }
                            }
                            done = d;
                        }
                        Ok(())
                    })();
                    let _ = broke;
                    if !done {
                        self.close_stream(&preparation_id);
                    }
                    return res;
                }
                let payload = obj! {"operation" => "stream", "request" => get(&prepared, "request"),
                    "preparationId" => &preparation_id, "context" => self.model_context()};
                let call = self.model_call(format!("model-{}", random_id()), payload, true);
                let r = self.rt.call(call)?;
                if gb(&r, "ok") != Some(true) {
                    return Err(model_module_failure(&r, "Model module failed"));
                }
                let Some(events) = arr(get(get(&r, "payload"), "events")).cloned() else {
                    return Err(invalid_model("INVALID_MODEL_RESPONSE", "Model module response must contain canonical events."));
                };
                for event in events {
                    if self.on_model_event(&event, &mut asm, &mut executed) {
                        break;
                    }
                }
                Ok(())
            })();

            if let Err(error) = stream_result {
                if self.rt.aborted() {
                    let partial = asm.assemble();
                    let tcs = collect_tool_calls(&partial.message);
                    if let Some(safe) = safe_final_text(&partial.message, tcs.len()) {
                        self.messages.push(safe);
                        self.expire_transients();
                        self.usage = merge_usage(&self.usage, &partial.usage.clone());
                        let idx = self.messages.len() - 1;
                        self.publish_message_event("assistant_message", idx, JsValue::Undefined);
                        self.final_message = self.messages[idx].clone();
                    }
                    return self.aborted_result(false);
                }
                let model_error = match &error.kind { ErrKind::Model(c) => Some(c.clone()), _ => None };
                let msg = model_error.as_ref().and_then(|m| gs(m, "message").map(String::from)).unwrap_or(error.message.clone());
                self.stop_failure(&msg)?;
                let err = agent_error("agent_model_error", &msg, model_error.clone().unwrap_or(JsValue::Undefined),
                    model_error.as_ref().and_then(|m| gs(m, "userHint")));
                let result = self.err_result("model_error", err.clone(), false);
                match self.abort_status() {
                    Some(st) => self.emit_status(st),
                    None => self.emit_status(model_request_failed_status(&err, model_error.as_ref())),
                }
                self.publish_ev("turn_failed", obj! {"error" => &err});
                self.capture_turn(true)?;
                self.publish_ev("turn_completed", obj! {"result" => &result});
                return Ok(RunResult { result, messages: self.messages.clone() });
            }

            if self.rt.aborted() {
                let partial = asm.assemble();
                let tcs = collect_tool_calls(&partial.message);
                if let Some(safe) = safe_final_text(&partial.message, tcs.len()) {
                    self.messages.push(safe);
                    self.expire_transients();
                    self.usage = merge_usage(&self.usage, &partial.usage.clone());
                    let idx = self.messages.len() - 1;
                    self.publish_message_event("assistant_message", idx, JsValue::Undefined);
                    self.final_message = self.messages[idx].clone();
                }
                return self.aborted_result(true);
            }

            let assembled = asm.assemble();
            if let Some((ep, em, ef)) = &executed {
                if ep == get(&calibration_request, "provider") && em == get(&calibration_request, "model") && str_of(ef) == Some(fingerprint.as_str()) {
                    self.record_calibration(&calibration_request, &assembled.usage, input_estimate);
                }
            }
            let assistant = assembled.message.clone();
            let tool_calls = collect_tool_calls(&assistant);
            let budget_usage = if !tracks_budget { assembled.usage.clone() } else { self.resolve_budget_usage(&assembled.usage, input_estimate)? };
            self.usage = merge_usage(&self.usage, &budget_usage);
            self.final_message = assistant.clone();
            self.expire_transients();

            let budget_provider = executed.as_ref().map(|e| e.0.clone()).unwrap_or_else(|| get(&prepared, "provider").clone());
            let budget_model = executed.as_ref().map(|e| e.1.clone()).unwrap_or_else(|| get(&prepared, "model").clone());
            let cost = self.estimate_cost(&budget_usage, &crate::rt::js_string(&budget_provider), &crate::rt::js_string(&budget_model))?;
            if let Some(c) = cost {
                spent += c;
                turn_spent += c;
                if let Some(b) = self.a.active_budget.as_mut() {
                    b.0 = turn_spent;
                }
            }
            let max_turn_reached = self.a.input.max_budget_usd.map_or(false, |m| turn_spent >= m);
            let task_reached = self.a.input.task_budget_usd.map_or(false, |t| spent >= t);
            if max_turn_reached || task_reached {
                let task_terminal = task_reached && !max_turn_reached;
                let limit = if task_terminal { self.a.input.task_budget_usd.unwrap() } else { self.a.input.max_budget_usd.unwrap() };
                let code = if task_terminal { "agent_task_budget_reached" } else { "agent_max_budget_reached" };
                let name = if task_terminal { "taskBudget.total" } else { "maxBudgetUsd" };
                let message = format!("Reached Gateway-owned {name} (${}) after spending ${}.", to_fixed6(limit), to_fixed6(spent));
                let details = obj! {"maxBudgetUsd" => self.a.input.max_budget_usd, "taskBudgetUsd" => self.a.input.task_budget_usd,
                    "spentBudgetUsd" => spent, "turnSpentBudgetUsd" => turn_spent, "lastInvocationCostUsd" => cost,
                    "provider" => &budget_provider, "model" => &budget_model};
                let hint = if task_terminal { "Increase taskBudget.total or start a new SDK session with a larger budget." } else { "Increase maxBudgetUsd or start a new turn with a larger budget." };
                let error = agent_error(code, &message, details.clone(), Some(hint));
                match safe_final_text(&assistant, tool_calls.len()) {
                    Some(m) => self.push_assistant(m),
                    None => self.final_message = JsValue::Undefined,
                }
                self.stop_failure(&message)?;
                let ev = if task_terminal { "task_budget_reached" } else { "max_budget_reached" };
                self.emit_status(status(ev, "error", &message, details));
                let result = self.err_result(if task_terminal { "task_budget" } else { "max_budget" }, error.clone(), true);
                self.publish_ev("turn_failed", obj! {"error" => &error});
                self.capture_turn(true)?;
                self.publish_ev("turn_completed", obj! {"result" => &result});
                return Ok(RunResult { result, messages: self.messages.clone() });
            }

            // ---- stream interruption ----
            let interruption = assembled.error.as_ref().map(|e| get(e, "streamInterruption").clone()).filter(truthy);
            if let Some(si) = interruption {
                let phase = gs(&si, "phase").unwrap_or("").to_string();
                if stream_recoveries < MAX_STREAM_INTERRUPTION_RECOVERIES {
                    stream_recoveries += 1;
                    let structured = !tool_calls.is_empty() || phase == "tool_call";
                    if structured {
                        self.final_message = JsValue::Undefined;
                    }
                    if phase == "text" && !structured {
                        let partial = without_thinking(&assistant);
                        if !text_from_message(&partial).trim().is_empty() {
                            self.push_assistant(partial);
                        }
                    }
                    let eff_phase = if structured { "tool_call" } else { phase.as_str() };
                    let active = ga(&si, "activeToolCalls").to_vec();
                    self.push_transient(&stream_interruption_prompt(eff_phase, &active), "stream_interruption_recovery");
                    self.continued("model_error");
                    continue;
                }
                let msg = format!("Stream interruption recovery exhausted after {MAX_STREAM_INTERRUPTION_RECOVERIES} attempts ({phase}).");
                let error = agent_error("agent_model_error", &msg, assembled.error.clone().unwrap_or(JsValue::Undefined),
                    Some("The model stream repeatedly disconnected. Retry the turn or switch providers."));
                return self.exhausted(&assistant, &tool_calls, error, assembled.error.clone());
            }
            stream_recoveries = 0;

            if assembled.error.is_none() && assembled.has_end && assembled.finish == "unknown" {
                if unknown_recoveries < MAX_UNKNOWN_FINISH_RECOVERIES {
                    unknown_recoveries += 1;
                    let partial = without_thinking(&assistant);
                    if tool_calls.is_empty() && !text_from_message(&partial).trim().is_empty() {
                        self.push_assistant(partial);
                    }
                    self.push_transient(&unknown_finish_prompt(!tool_calls.is_empty()), "unknown_finish_recovery");
                    self.continued("model_error");
                    continue;
                }
                let msg = format!("Unknown finish reason recovery exhausted after {MAX_UNKNOWN_FINISH_RECOVERIES} attempts.");
                let error = agent_error("agent_model_error", &msg, JsValue::Undefined,
                    Some("The provider repeatedly ended the stream without a recognized finish reason. Retry the turn or switch providers."));
                return self.exhausted(&assistant, &tool_calls, error, None);
            }
            unknown_recoveries = 0;

            if assembled.repaired && matches!(assembled.finish.as_str(), "length" | "tool_call" | "stop") {
                return Err(pending("repaired-truncated tool call recovery (LargeFileRepair)"));
            }

            if assembled.error.is_none() && tool_calls.is_empty() && text_from_message(&assistant).is_empty() {
                // Empty response handled before durable emission.
                if max_output_recovery > 0 {
                    consecutive_empty += 1;
                    if consecutive_empty < MAX_CONSECUTIVE_EMPTY && max_output_recovery < MAX_OUTPUT_RECOVERY_LIMIT {
                        max_output_recovery += 1;
                        self.empty_length_bump(&assembled.finish, &decision, routed_max_output);
                        self.push_transient(MAX_OUTPUT_RESUME_SENTENCE, "max_output_recovery");
                        self.continued("model_error");
                        continue;
                    }
                    self.final_message = self.last_assistant();
                    let st = empty_response_status(get(&request, "provider"), get(&request, "model"), consecutive_empty as f64);
                    self.emit_status(st);
                    let result = self.turn_result("success", "completed", None, false, None, None);
                    self.capture_turn(true)?;
                    self.publish_ev("turn_completed", obj! {"result" => &result});
                    return Ok(RunResult { result, messages: self.messages.clone() });
                }
                if !has_attempted_empty_retry {
                    has_attempted_empty_retry = true;
                    max_output_recovery += 1;
                    self.empty_length_bump(&assembled.finish, &decision, routed_max_output);
                    self.push_transient(EMPTY_RETRY, "empty_response_retry");
                    self.continued("model_error");
                    continue;
                }
                let st = empty_response_status(get(&request, "provider"), get(&request, "model"), 2.0);
                self.emit_status(st);
                let fm = self.last_assistant();
                let result = self.turn_result("success", "completed", None, false, Some(fm), None);
                self.capture_turn(true)?;
                self.publish_ev("turn_completed", obj! {"result" => &result});
                return Ok(RunResult { result, messages: self.messages.clone() });
            }

            self.push_assistant(assistant.clone());

            if let Some(model_err) = assembled.error.clone() {
                if !reasoning_retry && is_missing_reasoning_content_error(&model_err) {
                    reasoning_retry = true;
                    add_empty_reasoning_markers(&mut self.messages);
                    self.continued("model_error");
                    continue;
                }
                if !tool_calls.is_empty() {
                    let pm = self.a.cfg.permission_mode.clone();
                    let results: Vec<JsValue> = tool_calls.iter().map(|c| missing_tool_result(c, "Model error interrupted tool execution.", &pm)).collect();
                    let projected = project_tool_results(&results);
                    let start = self.messages.len();
                    self.messages.extend(projected);
                    self.publish_message_event("tool_results_projected", start, JsValue::Undefined);
                    for i in start..self.messages.len() {
                        let mut m = std::mem::replace(&mut self.messages[i], JsValue::Undefined);
                        self.a.timeline.message(&mut m);
                        self.messages[i] = m;
                    }
                }
                if self.a.cfg.json_self_correct && gs(&model_err, "code") == Some("invalid_tool_arguments") && json_self_correct < MAX_JSON_SELF_CORRECT_RETRIES {
                    json_self_correct += 1;
                    self.push_transient("Your previous tool call contained invalid JSON in the arguments and could not be parsed. Please retry with valid JSON. Common issues: missing quotes around keys/values, trailing commas, unescaped special characters in strings.", "json_self_correct");
                    self.continued("model_error");
                    continue;
                }
                let reactive = self.try_reactive_recover(&model_err, has_attempted_compact);
                let rtype = reactive.as_ref().and_then(|r| gs(r, "type").map(String::from)).unwrap_or_default();
                if rtype == "adjust_output_and_retry" && !has_attempted_output_retry {
                    has_attempted_output_retry = true;
                    let r = reactive.unwrap();
                    let (tp, tm) = error_target(&model_err, &decision);
                    let previous = self.current_max_output(&tp, &tm);
                    let n = gn(&r, "maxOutputTokens");
                    let attempt = gs(&r, "scope") == Some("attempt");
                    self.set_cap(&tp, &tm, |c| if attempt { c.attempt_max_output_tokens = n } else { c.hard_max_output_tokens = n });
                    if tp != decision.0 || tm != decision.1 {
                        self.set_cap(&decision.0, &decision.1, |c| c.attempt_max_output_tokens = n);
                    }
                    strip_trailing_error_pair(&mut self.messages);
                    self.publish_ev("token_cap_adjusted", obj! {"provider" => &tp, "model" => &tm, "cap" => "output", "previous" => previous, "next" => n, "reason" => get(&r, "reason")});
                    self.continued("model_error");
                    continue;
                }
                if rtype == "compact_and_retry" && !has_attempted_compact {
                    let r = reactive.unwrap();
                    let (tp, tm) = error_target(&model_err, &decision);
                    if let Some(n) = gn(&r, "maxContextTokens") {
                        let previous = self.current_max_context(&tp, &tm);
                        self.set_cap(&tp, &tm, |c| c.max_context_tokens = Some(n));
                        self.publish_ev("token_cap_adjusted", obj! {"provider" => &tp, "model" => &tm, "cap" => "context", "previous" => previous, "next" => n, "reason" => get(&r, "reason")});
                    }
                    if let Some(n) = gn(&r, "maxOutputTokens") {
                        let previous = self.current_max_output(&tp, &tm);
                        self.set_cap(&tp, &tm, |c| c.attempt_max_output_tokens = Some(n));
                        if tp != decision.0 || tm != decision.1 {
                            self.set_cap(&decision.0, &decision.1, |c| c.attempt_max_output_tokens = Some(n));
                        }
                        self.publish_ev("token_cap_adjusted", obj! {"provider" => &tp, "model" => &tm, "cap" => "output", "previous" => previous, "next" => n, "reason" => get(&r, "reason")});
                    }
                    strip_trailing_error_pair(&mut self.messages);
                    if has_compaction {
                        let attempt: R<Option<RunResult>> = (|| {
                            let max_ctx = self.current_max_context(&tp, &tm);
                            let reserved = self.reserved_output(Some((&tp, &tm)));
                            let base = spread(&request, obj! {"provider" => &tp, "model" => &tm});
                            let msgs = self.messages.clone();
                            let br = self.budget_request(&msgs, Some((&tp, &tm)), Some(&base), None)?;
                            let compact = self.try_auto_compact("recovery", max_ctx, reserved, br, (&tp, &tm), true)?;
                            if gs(&compact, "type") == Some("compacted") {
                                self.apply_compacted(&compact)?;
                                if truthy(get(&compact, "error")) {
                                    return self.context_overflow(&compact).map(Some);
                                }
                                Ok(None)
                            } else {
                                Err(pending("truncateHeadPreservingCheckpoint"))
                            }
                        })();
                        match attempt {
                            Ok(Some(done)) => return Ok(done),
                            Ok(None) => {}
                            Err(e) if is_fatal_compaction(&e) => return Err(e),
                            Err(e) if e.code.as_deref() == Some("RUST_SIDECAR_PENDING") => return Err(e),
                            Err(_) => return Err(pending("truncateHeadPreservingCheckpoint")),
                        }
                    } else {
                        return Err(pending("truncateHeadPreservingCheckpoint"));
                    }
                    has_attempted_compact = true;
                    self.continued("model_error");
                    continue;
                }
                if rtype == "truncate_head_and_retry" {
                    return Err(pending("truncateHeadPreservingCheckpoint"));
                }
                if rtype == "strip_images_and_retry" && !image_strip {
                    image_strip = true;
                    strip_trailing_error_pair(&mut self.messages);
                    strip_images(&mut self.messages);
                    self.a.calibration.clear();
                    self.continued("model_error");
                    continue;
                }
                if gs(&model_err, "code") == Some("max_output_reached") {
                    if !has_attempted_output_retry {
                        has_attempted_output_retry = true;
                        if let Some(next) = output_retry_bump(self.current_max_output(&decision.0, &decision.1), routed_max_output) {
                            strip_trailing_error_pair(&mut self.messages);
                            let previous = self.current_max_output(&decision.0, &decision.1);
                            self.set_cap(&decision.0, &decision.1, |c| c.requested_max_output_tokens = Some(next));
                            self.publish_ev("token_cap_adjusted", obj! {"provider" => &decision.0, "model" => &decision.1, "cap" => "output", "previous" => previous, "next" => next, "reason" => "max-output-retry-bump"});
                            self.continued("model_error");
                            continue;
                        }
                    }
                    if max_output_recovery < MAX_OUTPUT_RECOVERY_LIMIT {
                        max_output_recovery += 1;
                        self.push_transient(MAX_OUTPUT_RESUME, "max_output_recovery");
                        self.continued("model_error");
                        continue;
                    }
                }
                let (stop, cerr) = classify_model_error(&model_err);
                self.lifecycle("StopFailure", obj! {"error" => &model_err})?;
                let text = match &model_err { JsValue::Str(s) => s.clone(), v => to_json(v) };
                self.publish_ev("stop_failure", obj! {"error" => text});
                let result = self.err_result(stop, cerr.clone(), false);
                self.emit_status(model_request_failed_status(&cerr, Some(&model_err)));
                self.publish_ev("turn_failed", obj! {"error" => &cerr});
                self.capture_turn(true)?;
                self.publish_ev("turn_completed", obj! {"result" => &result});
                return Ok(RunResult { result, messages: self.messages.clone() });
            }

            if tool_calls.is_empty() {
                let text = text_from_message(&assistant);
                if text.is_empty() {
                    self.messages.pop();
                    if max_output_recovery > 0 {
                        consecutive_empty += 1;
                        if consecutive_empty < MAX_CONSECUTIVE_EMPTY && max_output_recovery < MAX_OUTPUT_RECOVERY_LIMIT {
                            max_output_recovery += 1;
                            self.empty_length_bump(&assembled.finish, &decision, routed_max_output);
                            self.push_transient(MAX_OUTPUT_RESUME_SENTENCE, "max_output_recovery");
                            self.continued("model_error");
                            continue;
                        }
                        self.final_message = self.last_assistant();
                        let st = empty_response_status(get(&request, "provider"), get(&request, "model"), consecutive_empty as f64);
                        self.emit_status(st);
                        let result = self.turn_result("success", "completed", None, false, None, None);
                        self.capture_turn(true)?;
                        self.publish_ev("turn_completed", obj! {"result" => &result});
                        return Ok(RunResult { result, messages: self.messages.clone() });
                    } else if !has_attempted_empty_retry {
                        has_attempted_empty_retry = true;
                        max_output_recovery += 1;
                        self.empty_length_bump(&assembled.finish, &decision, routed_max_output);
                        self.push_transient(EMPTY_RETRY, "empty_response_retry");
                        self.continued("model_error");
                        continue;
                    } else {
                        let st = empty_response_status(get(&request, "provider"), get(&request, "model"), 2.0);
                        self.emit_status(st);
                    }
                }
                if assembled.finish == "length" {
                    consecutive_empty = 0;
                    if max_output_recovery < MAX_OUTPUT_RECOVERY_LIMIT {
                        max_output_recovery += 1;
                        self.push_transient(MAX_OUTPUT_RESUME_SENTENCE, "max_output_recovery");
                        self.continued("model_error");
                        continue;
                    }
                    self.emit_status(max_output_recovery_exhausted_status(max_output_recovery as f64));
                }
                let can_continue = self.a.input.max_turns.map_or(true, |m| self.turn_count < m);
                if can_continue && self.a.input.has_drain_or_close {
                    let p = self.a.turn_call(self.rt, "drain_or_close_steer")?;
                    if gb(&p, "closed").is_none() {
                        return Err(LoopErr::code("INVALID_TURN_RESPONSE", "Turn drain response must contain closed."));
                    }
                    let steers = Self::steer_list(&p)?;
                    if !steers.is_empty() {
                        self.apply_steers(steers);
                        self.turn_count += 1.0;
                        self.continued("user_steer");
                        continue;
                    }
                }
                let hooks = self.lifecycle("Stop", obj! {"stopHookActive" => false, "lastAssistantMessage" => text.as_str()})?;
                self.publish_ev("stop_requested", obj! {});
                self.messages.extend(ga(&hooks, "messages").iter().cloned());
                if let Some(block) = ga(&hooks, "effects").iter().find(|e| gs(e, "type") == Some("block")) {
                    let error = agent_error("agent_unsupported_feature", gs(block, "reason").unwrap_or(""), JsValue::Undefined, None);
                    let result = self.err_result("tool_error", error.clone(), true);
                    self.emit_status(lifecycle_blocked_status(&error, "stop"));
                    self.publish_ev("turn_failed", obj! {"error" => &error});
                    self.capture_turn(true)?;
                    self.publish_ev("turn_completed", obj! {"result" => &result});
                    return Ok(RunResult { result, messages: self.messages.clone() });
                }
                if let Some(st) = finish_reason_status(&assembled.finish, &text) {
                    self.emit_status(st);
                }
                let result = self.turn_result("success", "completed", None, true, None, None);
                self.capture_turn(false)?;
                self.publish_ev("turn_completed", obj! {"result" => &result});
                return Ok(RunResult { result, messages: self.messages.clone() });
            }

            // ---- tool path ----
            let detected = self.ev_with("tool_calls_detected", obj! {"calls" => tool_calls.clone()});
            self.publish(detected);
            if self.rt.aborted() {
                return self.aborted_result(false);
            }
            let ctx = self.tool_context(assembled.finish == "length" || assembled.repaired);
            let pm = self.a.cfg.permission_mode.clone();
            let results = match self.execute_all(&tool_calls, ctx).and_then(|r| { self.observe_tool_results()?; Ok(r) }) {
                Ok(r) => r,
                Err(e) if e.is_terminal_tool_error() => return Err(e),
                Err(e) => tool_calls.iter().map(|c| missing_tool_result(c, &e.message, &pm)).collect(),
            };
            if self.rt.aborted() {
                return self.aborted_result(false);
            }
            // ensureToolResultPairing
            let mut pool: Vec<Option<JsValue>> = results.into_iter().map(Some).collect();
            let mut paired: Vec<JsValue> = Vec::new();
            for c in &tool_calls {
                let pos = pool.iter().position(|r| r.as_ref().map_or(false, |r| get(r, "toolCallId") == get(c, "id")));
                paired.push(match pos {
                    Some(i) => pool[i].take().unwrap(),
                    None => missing_tool_result(c, "Tool execution did not produce a result.", &pm),
                });
            }
            // repeated failure detection
            let keys = sorted_js(paired.iter().filter(|r| gs(r, "type") == Some("error")).map(failure_key).collect());
            let fp = if keys.is_empty() { None } else { Some(keys.join("\n")) };
            let mut repeated: Vec<String> = Vec::new();
            {
                let mut seen: Vec<&String> = Vec::new();
                for k in &keys {
                    if seen.contains(&k) { if !repeated.contains(k) { repeated.push(k.clone()); } } else { seen.push(k); }
                }
                if fp.is_some() && fp == last_failure_fp {
                    for k in &keys {
                        if !repeated.contains(k) { repeated.push(k.clone()); }
                    }
                }
            }
            if !repeated.is_empty() {
                paired = paired.into_iter().map(|r| annotate_repeated(r, &repeated)).collect();
            }
            last_failure_fp = fp;
            for r in &paired {
                if gs(r, "type") == Some("error") && matches!(gs(get(r, "error"), "code"), Some("permission_denied" | "permission_required" | "permission_cancelled")) {
                    self.permission_denials.push(obj! {"toolCallId" => get(r, "toolCallId"), "toolName" => get(r, "toolName"), "errorCode" => get(get(r, "error"), "code")});
                }
            }
            for r in &paired {
                if gs(r, "type") == Some("success") && truthy(get(get(r, "metadata"), "structuredOutput")) {
                    self.structured_output = get(r, "data").clone();
                }
                let data = if gs(r, "type") == Some("success") { get(r, "data") } else { &JsValue::Undefined };
                if let Some(req) = gs(data, "requestedMode").filter(|m| matches!(*m, "default" | "plan" | "bypassPermissions")) {
                    let mut eff = req.to_string();
                    if req == "plan" && self.a.cfg.permission_mode != "plan" {
                        self.a.cfg.permission_mode_before_plan = Some(self.a.cfg.permission_mode.clone());
                    } else if self.a.cfg.permission_mode == "plan" && req != "plan" {
                        if let Some(b) = self.a.cfg.permission_mode_before_plan.take() {
                            eff = b;
                        }
                    }
                    self.a.cfg.permission_mode = eff.clone();
                    set(&mut self.a.cfg.permission_context, "mode", eff.as_str());
                    self.publish_ev("mode_change_requested", obj! {"mode" => eff});
                }
                self.publish_ev("tool_result", obj! {"result" => r});
            }
            let projected = project_tool_results(&paired);
            let mut appended = projected.clone();
            if Methods::has(&self.a.m.context, "apply_tool_results") {
                let supplemental: Vec<JsValue> = {
                    let mut out = Vec::new();
                    let mut idx = 1;
                    for r in &paired {
                        for _ in 0..ga(r, "supplementalMessages").len() {
                            if idx < projected.len() {
                                out.push(obj! {"toolCallId" => get(r, "toolCallId"), "message" => &projected[idx]});
                                idx += 1;
                            }
                        }
                    }
                    out
                };
                let input = obj! {"sessionId" => &self.a.session_id, "turnId" => &self.a.turn_id, "toolResultMessage" => &projected[0],
                    "supplementalMessages" => supplemental, "messages" => self.messages.clone()};
                match self.a.context_call(self.rt, "apply_tool_results", input, false) {
                    Ok(applied) => {
                        let mut msgs = ga(&applied, "messages").to_vec();
                        let am = get(&applied, "appendedMessages");
                        appended = if nullish(am) { projected.clone() } else { ga(&applied, "appendedMessages").to_vec() };
                        if !nullish(am) && appended.len() <= msgs.len() {
                            msgs.truncate(msgs.len() - appended.len());
                            msgs.extend(appended.iter().cloned());
                        }
                        self.messages = msgs;
                    }
                    Err(_) => self.messages.extend(projected.iter().cloned()),
                }
            } else {
                self.messages.extend(projected.iter().cloned());
            }
            // appended messages are the tail of `messages` (shared instances).
            let n = appended.len();
            let base = self.messages.len().saturating_sub(n);
            for (k, m) in appended.into_iter().enumerate() {
                let idx = base + k;
                if idx < self.messages.len() && self.messages[idx] == m {
                    self.publish_message_event("tool_results_projected", idx, JsValue::Undefined);
                } else {
                    let mut m = m;
                    self.a.timeline.message(&mut m);
                    let e = self.ev_with("tool_results_projected", obj! {"message" => m});
                    self.publish(e);
                }
            }

            if let Some(block) = paired.iter().find_map(|r| {
                let b = get(get(get(r, "metadata"), "lifecycle"), "blocked");
                if is_obj(b) && gs(b, "reason").is_some() { Some(gs(b, "reason").unwrap().to_string()) } else { None }
            }) {
                let error = agent_error("agent_unsupported_feature", &block, JsValue::Undefined, None);
                let result = self.err_result("tool_error", error.clone(), true);
                self.emit_status(lifecycle_blocked_status(&error, "tool_lifecycle"));
                self.publish_ev("turn_failed", obj! {"error" => &error});
                self.capture_turn(true)?;
                self.publish_ev("turn_completed", obj! {"result" => &result});
                return Ok(RunResult { result, messages: self.messages.clone() });
            }

            let all_invalid = !paired.is_empty() && paired.iter().all(|r| gs(r, "type") == Some("error") && gs(get(r, "error"), "code") == Some("invalid_tool_input"));
            if all_invalid {
                let fp = sorted_js(paired.iter().map(|r| format!("{}::{}", crate::rt::js_string(get(r, "toolName")), crate::rt::js_string(get(get(r, "error"), "message")))).collect()).join("\n");
                if Some(&fp) == last_invalid_fp.as_ref() {
                    same_invalid += 1;
                } else {
                    same_invalid = 1;
                    last_invalid_fp = Some(fp);
                    grace_used = false;
                }
                if same_invalid >= MAX_SAME_INVALID_FINGERPRINT {
                    if !grace_used {
                        grace_used = true;
                        self.push_transient(CIRCUIT_BREAKER_GRACE_PROMPT, "circuit_breaker_grace");
                        self.continued("model_error");
                        continue;
                    }
                    let error = agent_error("agent_tool_error_loop",
                        &format!("Terminated: {same_invalid} consecutive turns with identical tool input validation failures (same tool + same error). The model appears stuck in a loop."),
                        JsValue::Undefined, Some("The model is repeatedly producing invalid tool calls. Consider switching to a more capable model via settings."));
                    let result = self.err_result("tool_error", error.clone(), true);
                    self.emit_status(tool_error_loop_status(&error, Some(same_invalid as f64)));
                    self.publish_ev("turn_failed", obj! {"error" => &error});
                    self.capture_turn(true)?;
                    self.publish_ev("turn_completed", obj! {"result" => &result});
                    return Ok(RunResult { result, messages: self.messages.clone() });
                }
            } else {
                same_invalid = 0;
                last_invalid_fp = None;
                grace_used = false;
                if !paired.iter().any(|r| gs(r, "type") == Some("error")) {
                    last_failure_fp = None;
                }
                max_output_recovery = 0;
                consecutive_empty = 0;
                has_attempted_output_retry = false;
                has_attempted_empty_retry = false;
                image_strip = false;
            }

            if self.a.cfg.stop_on_structured_output && !is_undef(&self.structured_output) {
                let result = self.turn_result("success", "completed", None, true, None, None);
                self.emit_status(structured_output_completed_status());
                self.capture_turn(false)?;
                self.publish_ev("turn_completed", obj! {"result" => &result});
                return Ok(RunResult { result, messages: self.messages.clone() });
            }

            let next = self.turn_count + 1.0;
            if let Some(max) = self.a.input.max_turns.filter(|m| next > *m) {
                let error = agent_error("agent_max_turns_reached", &format!("Reached maximum number of turns ({}).", pilotdeck_skills::jsjson::js_number_to_string(max)),
                    JsValue::Undefined, Some("Max turn limit reached. Increase maxTurns in config or break the task into smaller steps."));
                let result = self.turn_result("max_turns", "max_turns", Some(vec![error.clone()]), true, None, Some(next));
                self.emit_status(max_turns_status(max, &error));
                self.capture_turn(false)?;
                self.publish_ev("turn_completed", obj! {"result" => &result});
                return Ok(RunResult { result, messages: self.messages.clone() });
            }
            self.turn_count = next;
            self.continued("next_turn");
        }
    }
}

// ---------------- loop helpers ----------------
fn to_fixed6(v: f64) -> String {
    format!("{v:.6}")
}
fn log_compact_failure(stage: &str, session: &str, turn: &str, e: &LoopErr) {
    eprintln!("[agent:auto-compact] {stage} failed sessionId={session} turnId={turn}: {}", e.message);
}
fn error_target(e: &JsValue, decision: &(String, String)) -> (String, String) {
    let p = gs(e, "provider").filter(|x| !x.is_empty()).map(String::from).unwrap_or_else(|| decision.0.clone());
    let m = gs(e, "model").filter(|x| !x.is_empty()).map(String::from).unwrap_or_else(|| decision.1.clone());
    (p, m)
}
fn output_retry_bump(current: Option<f64>, cap: Option<f64>) -> Option<f64> {
    let c = current.filter(|c| c.is_finite() && *c > 0.0)?;
    if let Some(m) = cap {
        if !m.is_finite() || m <= 0.0 || c >= m {
            return None;
        }
    }
    let d = c * 2.0;
    if !d.is_finite() || d <= c {
        return None;
    }
    Some(cap.map_or(d, |m| d.min(m)))
}
fn clamp_output(requested: f64, cap: Option<f64>) -> Option<f64> {
    if !requested.is_finite() || requested <= 0.0 {
        return None;
    }
    let n = requested.floor();
    match cap.filter(|m| m.is_finite() && *m > 0.0) {
        Some(m) => Some(n.min(m.floor())),
        None => Some(n),
    }
}
fn annotate_repeated(r: JsValue, repeated: &[String]) -> JsValue {
    if gs(&r, "type") != Some("error") || !repeated.contains(&failure_key(&r)) {
        return r;
    }
    let rec = recovery_of(&r);
    let reason = gs(&rec, "avoidRetryReason").map(String::from)
        .unwrap_or_else(|| "The same tool, error code, and recovery class repeated. Retrying unchanged is likely to fail again.".into());
    let suffix = format!("\n\nRepeated failure: {reason}\nChange at least one of the tool, parameters, path, scope, permission path, or explain the blocker in text.");
    let content = ga(&r, "content").to_vec();
    let new_content = match content.first() {
        None => vec![obj! {"type" => "text", "text" => suffix.trim_start()}],
        Some(f) if gs(f, "type") != Some("text") => {
            let mut v = vec![obj! {"type" => "text", "text" => suffix.trim_start()}];
            v.extend(content.iter().cloned());
            v
        }
        Some(f) => {
            let mut v = vec![spread(f, obj! {"text" => format!("{}{}", crate::rt::js_string(get(f, "text")), suffix)})];
            v.extend(content.iter().skip(1).cloned());
            v
        }
    };
    let new_rec = if is_obj(&rec) {
        spread(&rec, obj! {"avoidRetryReason" => &reason, "repeatedFailure" => true})
    } else {
        obj! {"avoidRetryReason" => &reason, "repeatedFailure" => true}
    };
    let md = if is_obj(get(&r, "metadata")) { get(&r, "metadata").clone() } else { obj! {} };
    spread(&r, obj! {"content" => new_content, "metadata" => spread(&md, obj! {"recovery" => new_rec})})
}

impl<'a> Run<'a> {
    /// Returns true when the stream loop must stop (canonical `error` event).
    fn on_model_event(&mut self, event: &JsValue, asm: &mut Assembler, executed: &mut Option<(JsValue, JsValue, JsValue)>) -> bool {
        let ty = gs(event, "type").unwrap_or("").to_string();
        if ty == "request_started" {
            *executed = Some((get(event, "provider").clone(), get(event, "model").clone(), get(event, "requestFingerprint").clone()));
        }
        let block_id = match ty.as_str() {
            "text_delta" => Some(asm.block_id("text")),
            "thinking_delta" => Some(asm.block_id("thinking")),
            _ => None,
        };
        let mut e = self.ev_with("model_event", obj! {"event" => event});
        if let Some(b) = block_id {
            set(&mut e, "blockId", b);
        }
        self.publish(e);
        asm.apply(event);
        ty == "error"
    }

    fn record_calibration(&mut self, request: &JsValue, usage: &JsValue, estimate: Option<f64>) {
        let mut total = 0.0;
        for k in ["inputTokens", "cacheReadTokens", "cacheWriteTokens"] {
            if let Some(t) = gn(usage, k).filter(|t| t.is_finite() && *t > 0.0) {
                total += t;
            }
        }
        let Some(est) = estimate.filter(|e| *e > 0.0) else { return };
        if total <= 0.0 {
            return;
        }
        let (p, m) = (gs(request, "provider").unwrap_or(""), gs(request, "model").unwrap_or(""));
        let v = obj! {"provider" => p, "model" => m, "actualInputTokens" => total.ceil(), "estimatedInputTokens" => est};
        let k = cal_key(p, m);
        if let Some(e) = self.a.calibration.iter_mut().find(|(x, _)| *x == k) {
            e.1 = v;
        } else {
            self.a.calibration.push((k, v));
        }
    }

    fn resolve_budget_usage(&mut self, usage: &JsValue, _estimate: Option<f64>) -> R<JsValue> {
        if matches!(usage, JsValue::Obj(o) if o.iter().any(|(_, v)| num(v).map_or(false, |n| n.is_finite()))) {
            return Ok(usage.clone());
        }
        Err(pending("countTokens fallback in resolveBudgetUsage"))
    }

    fn estimate_cost(&mut self, usage: &JsValue, p: &str, m: &str) -> R<Option<f64>> {
        let router = self.a.estimate_usage_cost(self.rt, usage, p, m)?;
        if let Some(c) = router.filter(|c| c.is_finite() && *c >= 0.0) {
            return Ok(Some(c));
        }
        Ok(gn(usage, "nativeCost").filter(|c| c.is_finite() && *c >= 0.0))
    }

    fn empty_length_bump(&mut self, finish: &str, decision: &(String, String), routed_max_output: Option<f64>) {
        if finish != "length" {
            return;
        }
        let previous = self.current_max_output(&decision.0, &decision.1);
        let next = clamp_output((previous.unwrap_or(0.0) * 2.0).max(EMPTY_LENGTH_OUTPUT_RETRY_FLOOR), routed_max_output);
        if let Some(n) = next {
            if Some(n) != previous {
                self.set_cap(&decision.0, &decision.1, |c| c.requested_max_output_tokens = Some(n));
                self.publish_ev("empty_output_recovery", obj! {"provider" => &decision.0, "model" => &decision.1,
                    "finishReason" => finish, "previousMaxOutputTokens" => previous, "nextMaxOutputTokens" => n});
            }
        }
    }

    fn try_reactive_recover(&mut self, error: &JsValue, has_attempted_compact: bool) -> Option<JsValue> {
        if !Methods::has(&self.a.m.context, "recover_from_model_error") {
            return None;
        }
        let input = obj! {"sessionId" => &self.a.session_id, "turnId" => &self.a.turn_id, "error" => error,
            "messages" => self.messages.clone(), "hasAttemptedCompact" => has_attempted_compact};
        self.a.context_call(self.rt, "recover_from_model_error", input, false).ok()
    }

    fn observe_tool_results(&mut self) -> R<()> {
        if self.a.plan_todo.is_some() {
            let (sid, tid) = (self.a.session_id.clone(), self.a.turn_id.clone());
            self.a.plan_todo_read(self.rt, &sid, &tid)?;
        }
        Ok(())
    }

    /// `contextOverflowAfterEmergency` plus its terminal events.
    fn context_overflow(&mut self, compact: &JsValue) -> R<RunResult> {
        let ce = get(compact, "error").clone();
        let msg = if truthy(&ce) { crate::rt::js_string(&ce) } else { "context_overflow_after_emergency_compaction".into() };
        let error = agent_error("agent_context_recovery_failed", &msg,
            obj! {"code" => &ce, "snapshot" => get(compact, "snapshot"), "diagnostics" => get(get(compact, "result"), "diagnostics")},
            Some("The context is still too large after emergency compaction. Start a new session or reduce the request and retry."));
        self.lifecycle("StopFailure", obj! {"error" => &msg})?;
        let result = self.err_result("prompt_too_long", error.clone(), true);
        self.publish_ev("stop_failure", obj! {"error" => &msg});
        self.emit_status(model_request_failed_status(&error, None));
        self.publish_ev("turn_failed", obj! {"error" => &error});
        self.capture_turn(true)?;
        self.publish_ev("turn_completed", obj! {"result" => &result});
        Ok(RunResult { result, messages: self.messages.clone() })
    }

    /// Recovery-exhausted terminal (stream interruption / unknown finish).
    fn exhausted(&mut self, assistant: &JsValue, tool_calls: &[JsValue], error: JsValue, model_error: Option<JsValue>) -> R<RunResult> {
        match safe_final_text(assistant, tool_calls.len()) {
            Some(m) => self.push_assistant(m),
            None => self.final_message = JsValue::Undefined,
        }
        let msg = gs(&error, "message").unwrap_or("").to_string();
        self.stop_failure(&msg)?;
        let result = self.err_result("model_error", error.clone(), true);
        self.emit_status(model_request_failed_status(&error, model_error.as_ref()));
        self.publish_ev("turn_failed", obj! {"error" => &error});
        self.capture_turn(true)?;
        self.publish_ev("turn_completed", obj! {"result" => &result});
        Ok(RunResult { result, messages: self.messages.clone() })
    }
}
