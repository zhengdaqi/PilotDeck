//! Port of the default sidecar factory payload mapping.
use crate::agent::*;
use crate::json::*;
use crate::obj;
use crate::rt::{js_string, s_, LoopErr, Rt};
use crate::timeline::Timeline;

fn read_methods(host_modules: &JsValue, module: &str, allowed: &[&str]) -> Vec<String> {
    let Some(list) = arr(get(get(host_modules, module), "methods")) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for m in list {
        if let Some(s) = str_of(m) {
            if allowed.contains(&s) && !out.iter().any(|x| x == s) {
                out.push(s.to_string());
            }
        }
    }
    out
}

fn as_thinking_config(v: &JsValue) -> JsValue {
    if !is_obj(v) {
        return JsValue::Undefined;
    }
    let Some(enabled) = gb(v, "enabled") else { return JsValue::Undefined };
    let modes = ["default", "off", "minimal", "low", "medium", "high", "xhigh", "max"];
    let mut o = obj! {"enabled" => enabled};
    if let Some(m) = gs(v, "mode").filter(|m| modes.contains(m)) {
        set(&mut o, "mode", m);
    }
    if let Some(b) = as_positive_integer(get(v, "budgetTokens")) {
        set(&mut o, "budgetTokens", b);
    }
    if let Some(b) = gb(v, "preserve") {
        set(&mut o, "preserve", b);
    }
    if let Some(b) = gb(v, "splitReasoning") {
        set(&mut o, "splitReasoning", b);
    }
    o
}

fn as_tool_choice(v: &JsValue) -> JsValue {
    if let Some(s) = str_of(v).filter(|s| matches!(*s, "auto" | "none" | "required")) {
        return s_(s);
    }
    if gs(v, "type") == Some("tool") {
        if let Some(name) = as_string(get(v, "name")) {
            return obj! {"type" => "tool", "name" => name};
        }
    }
    JsValue::Undefined
}

fn as_model_override(v: &JsValue) -> Option<JsValue> {
    if !is_obj(v) {
        return None;
    }
    let provider = as_string(get(v, "provider"))?;
    let model = as_string(get(v, "model"))?;
    let th = as_thinking_config(get(v, "thinking"));
    Some(obj! {"provider" => provider, "model" => model,
        "temperature" => num(get(v, "temperature")).filter(|n| n.is_finite()),
        "speed" => num(get(v, "speed")).filter(|n| n.is_finite()),
        "thinking" => th})
}

fn as_subagent_model(v: &JsValue) -> R<Option<JsValue>> {
    if is_undef(v) {
        return Ok(None);
    }
    if !is_obj(v) {
        return Err(LoopErr::msg("agent.subagentModel must be an object."));
    }
    let provider = as_string(get(v, "provider")).ok_or_else(|| LoopErr::msg("agent.subagentModel.provider must be a non-empty string."))?;
    let model = as_string(get(v, "model")).ok_or_else(|| LoopErr::msg("agent.subagentModel.model must be a non-empty string."))?;
    let mut o = obj! {"provider" => provider, "model" => model};
    for f in ["maxContextTokens", "maxOutputTokens"] {
        if !is_undef(get(v, f)) {
            let n = as_positive_integer(get(v, f)).ok_or_else(|| LoopErr::msg(format!("agent.subagentModel.{f} must be a positive integer.")))?;
            set(&mut o, f, n);
        }
    }
    Ok(Some(o))
}

fn as_permission_rules(v: &JsValue) -> Option<JsValue> {
    if !is_obj(v) {
        return None;
    }
    let pick = |k: &str| if arr(get(v, k)).is_some() { get(v, k).clone() } else { JsValue::Undefined };
    Some(obj! {"allow" => pick("allow"), "deny" => pick("deny"), "ask" => pick("ask")})
}

fn as_string_array(v: &JsValue) -> Vec<JsValue> {
    match arr(v) {
        Some(a) if a.iter().all(|x| str_of(x).is_some()) => a.clone(),
        _ => Vec::new(),
    }
}

fn is_tool_kind(v: &str) -> bool {
    matches!(v, "filesystem" | "shell" | "network" | "mcp" | "session" | "agent" | "structured_output" | "custom")
}

pub fn read_tool_descriptors(value: &JsValue) -> Vec<ToolDesc> {
    let items: &[JsValue] = arr(value).or_else(|| arr(get(value, "available"))).map(|a| a.as_slice()).unwrap_or(&[]);
    let mut out = Vec::new();
    for item in items {
        if !is_obj(item) {
            continue;
        }
        let Some(name) = as_string(get(item, "name")) else { continue };
        let kind = gs(item, "kind").filter(|k| is_tool_kind(k)).unwrap_or("custom").to_string();
        let requires_user_interaction = gb(item, "requiresUserInteraction") == Some(true);
        let mut caps: Vec<String> = Vec::new();
        for c in ga(item, "requiredRuntimeCapabilities").iter().filter_map(str_of) {
            if matches!(c, "always_on_run_context" | "plan_workflow" | "subagent_fork" | "user_interaction") && !caps.iter().any(|x| x == c) {
                caps.push(c.to_string());
            }
        }
        if kind == "agent" && !caps.iter().any(|c| c == "subagent_fork") {
            caps.push("subagent_fork".into());
        }
        if requires_user_interaction && !caps.iter().any(|c| c == "user_interaction") {
            caps.push("user_interaction".into());
        }
        let schema = coalesce(&[get(item, "inputSchema"), get(item, "input_schema")]);
        let input_schema = if is_obj(schema) && gs(schema, "type") == Some("object") { schema.clone() } else { obj! {"type" => "object"} };
        out.push(ToolDesc {
            name,
            description: as_string(get(item, "description")).unwrap_or_else(|| "Host capability".into()),
            kind,
            input_schema,
            read_only: gb(item, "readOnly") == Some(true),
            concurrency_safe: gb(item, "concurrencySafe") == Some(true),
            requires_user_interaction,
            required_runtime_capabilities: caps,
        });
    }
    out
}

pub fn read_required_tool_definitions(value: &JsValue) -> R<Vec<ToolDesc>> {
    let Some(entries) = arr(value).or_else(|| arr(get(value, "available"))) else {
        return Err(LoopErr::msg("Host tool catalog response must contain a tools array."));
    };
    let descriptors = read_tool_descriptors(&JsValue::Arr(entries.clone()));
    let mut names: Vec<&str> = descriptors.iter().map(|d| d.name.as_str()).collect();
    names.sort();
    names.dedup();
    if descriptors.len() != entries.len() || names.len() != descriptors.len() {
        return Err(LoopErr::msg("Host tool catalog response contains an invalid or duplicate tool descriptor."));
    }
    Ok(descriptors)
}

fn timeline_position(v: &JsValue) -> JsValue {
    if !is_obj(v) || gn(v, "version") != Some(1.0) || gs(v, "turnId").is_none() || gs(v, "id").is_none() {
        return JsValue::Undefined;
    }
    let (Some(order), Some(revision)) = (gn(v, "order"), gn(v, "revision")) else { return JsValue::Undefined };
    if !is_integer(order) || !is_integer(revision) {
        return JsValue::Undefined;
    }
    obj! {"version" => 1.0, "turnId" => gs(v, "turnId").unwrap(), "id" => gs(v, "id").unwrap(),
        "previousId" => gs(v, "previousId").map(String::from), "order" => order, "revision" => revision,
        "offset" => gn(v, "offset").filter(|n| is_integer(*n))}
}

fn detail_of(v: &JsValue) -> JsValue {
    match gs(v, "detail") {
        Some(d @ ("auto" | "low" | "high")) => s_(d),
        _ => JsValue::Undefined,
    }
}
fn opt_s(v: &JsValue, k: &str) -> Option<String> {
    gs(v, k).map(String::from)
}
fn is_error_true(b: &JsValue) -> JsValue {
    if gb(b, "isError") == Some(true) { JsValue::Bool(true) } else { JsValue::Undefined }
}

fn content_blocks(b: &JsValue) -> Vec<JsValue> {
    if !is_obj(b) {
        return vec![];
    }
    let timeline = timeline_position(get(b, "timeline"));
    let t = gs(b, "type").unwrap_or("");
    let src_ok = matches!(gs(b, "source"), Some("base64" | "url"));
    let nnf = |k: &str| as_non_negative_finite(get(b, k));
    let block = match t {
        "text" if gs(b, "text").is_some() => {
            obj! {"type" => "text", "text" => gs(b, "text").unwrap(), "blockId" => opt_s(b, "blockId"), "timeline" => timeline}
        }
        "thinking" if gs(b, "text").is_some() => {
            obj! {"type" => "thinking", "text" => gs(b, "text").unwrap(), "blockId" => opt_s(b, "blockId"),
                "signature" => opt_s(b, "signature"), "reasoningContent" => opt_s(b, "reasoningContent"), "timeline" => timeline}
        }
        "tool_call" if gs(b, "id").is_some() && gs(b, "name").is_some() => {
            let input = if nullish(get(b, "input")) { obj! {} } else { get(b, "input").clone() };
            obj! {"type" => "tool_call", "id" => gs(b, "id").unwrap(), "name" => gs(b, "name").unwrap(),
                "input" => input, "raw" => get(b, "raw"), "timeline" => timeline}
        }
        "tool_result" if gs(b, "toolCallId").is_some() => {
            let content: Vec<JsValue> = if let Some(a) = arr(get(b, "content")) {
                a.iter().flat_map(content_blocks).collect()
            } else if let Some(s) = gs(b, "content") {
                vec![obj! {"type" => "text", "text" => s}]
            } else {
                vec![]
            };
            obj! {"type" => "tool_result", "toolCallId" => gs(b, "toolCallId").unwrap(), "content" => content,
                "isError" => is_error_true(b), "raw" => get(b, "raw"), "timeline" => timeline}
        }
        "image" if src_ok && gs(b, "data").is_some() && gs(b, "mimeType").is_some() => {
            obj! {"type" => "image", "source" => gs(b, "source").unwrap(), "mimeType" => gs(b, "mimeType").unwrap(),
                "data" => gs(b, "data").unwrap(), "bytes" => nnf("bytes"), "detail" => detail_of(b), "timeline" => timeline}
        }
        "pdf" if gs(b, "source") == Some("base64") && gs(b, "data").is_some() && gs(b, "mimeType") == Some("application/pdf") && nnf("bytes").is_some() => {
            obj! {"type" => "pdf", "source" => "base64", "data" => gs(b, "data").unwrap(), "mimeType" => "application/pdf",
                "bytes" => nnf("bytes"), "pages" => as_positive_integer(get(b, "pages")), "timeline" => timeline}
        }
        "audio" if src_ok && gs(b, "data").is_some() && gs(b, "mimeType").is_some() => {
            obj! {"type" => "audio", "source" => gs(b, "source").unwrap(), "data" => gs(b, "data").unwrap(), "mimeType" => gs(b, "mimeType").unwrap(),
                "bytes" => nnf("bytes"), "durationSeconds" => nnf("durationSeconds"), "timeline" => timeline}
        }
        "tool_result_reference" if gs(b, "toolCallId").is_some() && gs(b, "path").is_some() && nnf("originalBytes").is_some()
            && gs(b, "preview").is_some() && gb(b, "hasMore").is_some() => {
            obj! {"type" => "tool_result_reference", "toolCallId" => gs(b, "toolCallId").unwrap(), "path" => gs(b, "path").unwrap(),
                "originalBytes" => nnf("originalBytes"), "preview" => gs(b, "preview").unwrap(), "hasMore" => gb(b, "hasMore").unwrap(),
                "isError" => is_error_true(b), "readFilePath" => opt_s(b, "readFilePath"), "mimeType" => opt_s(b, "mimeType"),
                "reason" => opt_s(b, "reason"), "timeline" => timeline}
        }
        "media_reference" if gs(b, "path").is_some() && nnf("originalBytes").is_some() && gs(b, "preview").is_some()
            && gb(b, "hasMore").is_some() && gs(b, "mimeType").is_some() && matches!(gs(b, "mediaType"), Some("image" | "pdf" | "audio")) => {
            obj! {"type" => "media_reference", "path" => gs(b, "path").unwrap(), "originalBytes" => nnf("originalBytes"),
                "preview" => gs(b, "preview").unwrap(), "hasMore" => gb(b, "hasMore").unwrap(), "mimeType" => gs(b, "mimeType").unwrap(),
                "mediaType" => gs(b, "mediaType").unwrap(), "toolCallId" => opt_s(b, "toolCallId"),
                "pages" => as_positive_integer(get(b, "pages")), "detail" => detail_of(b), "reason" => opt_s(b, "reason"), "timeline" => timeline}
        }
        "image_url" => {
            let url = gs(get(b, "image_url"), "url").unwrap_or("");
            // /^data:([^;]+);base64,(.+)$/
            let Some(rest) = url.strip_prefix("data:") else { return vec![] };
            let Some(semi) = rest.find(';') else { return vec![] };
            let (mime, tail) = rest.split_at(semi);
            let Some(data) = tail.strip_prefix(";base64,") else { return vec![] };
            if mime.is_empty() || mime.contains('\n') || data.is_empty() || data.contains('\n') || data.contains('\r') {
                return vec![];
            }
            obj! {"type" => "image", "source" => "base64", "mimeType" => mime, "data" => data}
        }
        _ => return vec![],
    };
    vec![block]
}

fn canonical_message_metadata(v: &JsValue) -> JsValue {
    if !is_obj(v) {
        return JsValue::Undefined;
    }
    let mut m = JsObject::new();
    let put_s = |m: &mut JsObject, k: &str| if let Some(s) = gs(v, k) { m.set(k, s_(s)); };
    put_s(&mut m, "model");
    for k in ["synthetic", "transient"] {
        if let Some(b) = gb(v, k) { m.set(k, JsValue::Bool(b)); }
    }
    put_s(&mut m, "transientId");
    put_s(&mut m, "toolCallId");
    if let Some(b) = gb(v, "compactReplacement") { m.set("compactReplacement", JsValue::Bool(b)); }
    put_s(&mut m, "compactSnapshotId");
    put_s(&mut m, "purpose");
    put_s(&mut m, "queueItemId");
    let fc = get(v, "forkCarryover");
    if let Some(src) = gs(fc, "sourceSessionId") {
        m.set("forkCarryover", obj! {"sourceSessionId" => src, "sourceTurnId" => opt_s(fc, "sourceTurnId")});
    }
    if m.is_empty() { JsValue::Undefined } else { JsValue::Obj(m) }
}

fn to_canonical_messages(raw: &JsValue) -> Vec<JsValue> {
    if !is_obj(raw) {
        return vec![];
    }
    let role = if gs(raw, "role") == Some("assistant") { "assistant" } else { "user" };
    let mut content = Vec::new();
    if let Some(s) = gs(raw, "content").filter(|s| !s.is_empty()) {
        content.push(obj! {"type" => "text", "text" => s});
    }
    content.extend(ga(raw, "content").iter().flat_map(content_blocks));
    content.extend(ga(raw, "images").iter().flat_map(content_blocks));
    if content.is_empty() {
        return vec![];
    }
    vec![obj! {"role" => role, "content" => content, "metadata" => canonical_message_metadata(get(raw, "metadata"))}]
}

fn build_initial_messages(task: &JsValue, raw: &JsValue, explicit: bool) -> Vec<JsValue> {
    let messages: Vec<JsValue> = arr(raw).map(|a| a.iter().flat_map(to_canonical_messages).collect()).unwrap_or_default();
    if explicit || !messages.is_empty() {
        return messages;
    }
    let prompt = as_string(coalesce(&[get(task, "prompt"), get(task, "instruction"), get(task, "description")]));
    let text = prompt.unwrap_or_else(|| to_json(task));
    vec![obj! {"role" => "user", "content" => vec![obj! {"type" => "text", "text" => text}]}]
}

fn merge_metadata(sources: &[&JsValue]) -> JsValue {
    let mut out: Option<JsValue> = None;
    for s in sources {
        if is_obj(s) {
            out = Some(spread(&out.unwrap_or_else(|| obj! {}), (*s).clone()));
        }
    }
    out.unwrap_or(JsValue::Undefined)
}

fn parse_seed_state(v: &JsValue) -> R<(Vec<String>, JsValue, JsValue)> {
    let (mut allowed, mut read, mut write) = (Vec::new(), obj! {}, obj! {});
    if nullish(v) {
        return Ok((allowed, read, write));
    }
    if !is_obj(v) {
        return Err(LoopErr::msg("Invalid sidecar seedState: expected an object."));
    }
    let a = get(v, "allowedReadFiles");
    if !is_undef(a) {
        match arr(a) {
            Some(list) if list.iter().all(|x| str_of(x).is_some()) => allowed = list.iter().map(|x| str_of(x).unwrap().to_string()).collect(),
            _ => return Err(LoopErr::msg("Invalid sidecar seedState.allowedReadFiles.")),
        }
    }
    let r = get(v, "readFileState");
    if !is_undef(r) {
        let JsValue::Obj(entries) = r else { return Err(LoopErr::msg("Invalid sidecar seedState.readFileState.")) };
        for (path, e) in entries.iter() {
            if !is_obj(e) || gn(e, "mtimeMs").is_none() || !matches!(gs(e, "kind"), Some("text" | "image" | "pdf" | "notebook")) {
                return Err(LoopErr::msg(format!("Invalid readFileState entry: {path}.")));
            }
            set(&mut read, path, obj! {"mtimeMs" => gn(e, "mtimeMs"), "kind" => gs(e, "kind").unwrap(),
                "offset" => gn(e, "offset"), "limit" => gn(e, "limit"), "pages" => opt_s(e, "pages")});
        }
    }
    let w = get(v, "writeSnapshots");
    if !is_undef(w) {
        let JsValue::Obj(entries) = w else { return Err(LoopErr::msg("Invalid sidecar seedState.writeSnapshots.")) };
        for (path, e) in entries.iter() {
            if !is_obj(e) || gs(e, "absolutePath").is_none() || gn(e, "mtimeMs").is_none() || gs(e, "contentHash").is_none() {
                return Err(LoopErr::msg(format!("Invalid writeSnapshots entry: {path}.")));
            }
            set(&mut write, path, obj! {"absolutePath" => gs(e, "absolutePath").unwrap(), "mtimeMs" => gn(e, "mtimeMs"),
                "contentHash" => gs(e, "contentHash").unwrap(), "offset" => gn(e, "offset"), "limit" => gn(e, "limit")});
        }
    }
    Ok((allowed, read, write))
}

fn parse_model_state(v: &JsValue) -> R<(Vec<(String, JsValue)>, Vec<(String, TokenCap)>)> {
    let (mut cal, mut caps) = (Vec::new(), Vec::new());
    if nullish(v) {
        return Ok((cal, caps));
    }
    if !is_obj(v) {
        return Err(LoopErr::msg("Invalid sidecar modelState: expected an object."));
    }
    let read_arr = |val: &JsValue, field: &str| -> R<Vec<JsValue>> {
        if is_undef(val) { return Ok(vec![]); }
        arr(val).cloned().ok_or_else(|| LoopErr::msg(format!("Invalid sidecar {field}: expected an array.")))
    };
    let nes = |val: &JsValue, field: &str| -> R<String> { as_string(val).ok_or_else(|| LoopErr::msg(format!("Invalid {field}."))) };
    let pos = |val: &JsValue, field: &str| -> R<f64> {
        num(val).filter(|n| n.is_finite() && *n > 0.0).ok_or_else(|| LoopErr::msg(format!("Invalid {field}.")))
    };
    for e in read_arr(get(v, "tokenCalibration"), "modelState.tokenCalibration")? {
        if !is_obj(&e) { return Err(LoopErr::msg("Invalid sidecar modelState calibration entry.")); }
        let p = nes(get(&e, "provider"), "modelState calibration provider")?;
        let m = nes(get(&e, "model"), "modelState calibration model")?;
        let a = pos(get(&e, "actualInputTokens"), "modelState calibration actualInputTokens")?;
        let es = pos(get(&e, "estimatedInputTokens"), "modelState calibration estimatedInputTokens")?;
        cal.push((cal_key(&p, &m), obj! {"provider" => p, "model" => m, "actualInputTokens" => a, "estimatedInputTokens" => es}));
    }
    for e in read_arr(get(v, "tokenCaps"), "modelState.tokenCaps")? {
        if !is_obj(&e) { return Err(LoopErr::msg("Invalid sidecar modelState token cap entry.")); }
        let p = nes(get(&e, "provider"), "modelState token cap provider")?;
        let m = nes(get(&e, "model"), "modelState token cap model")?;
        let opt = |f: &str| -> R<Option<f64>> {
            if is_undef(get(&e, f)) { Ok(None) } else { pos(get(&e, f), &format!("modelState token cap {f}")).map(Some) }
        };
        caps.push((cal_key(&p, &m), TokenCap {
            max_context_tokens: opt("maxContextTokens")?,
            session_max_output_tokens: opt("sessionMaxOutputTokens")?,
            hard_max_output_tokens: opt("hardMaxOutputTokens")?,
            ..Default::default()
        }));
    }
    Ok((cal, caps))
}

pub fn read_metadata_snapshot(v: &JsValue) -> R<JsValue> {
    let (Some(provider), Some(model)) = (as_string(get(v, "provider")), as_string(get(v, "model"))) else {
        return Err(LoopErr::msg("Host model metadata response must include provider and model."));
    };
    let req_pos = |val: &JsValue, field: &str| -> R<f64> {
        num(val).filter(|n| is_safe_int(*n) && *n > 0.0).ok_or_else(|| LoopErr::msg(format!("Host model metadata response contains an invalid {field}.")))
    };
    let opt_pos = |val: &JsValue, field: &str| -> R<Option<f64>> { if is_undef(val) { Ok(None) } else { req_pos(val, field).map(Some) } };
    let max_ctx = opt_pos(get(v, "maxContextTokens"), "maxContextTokens")?;
    let max_out = opt_pos(get(v, "maxOutputTokens"), "maxOutputTokens")?;
    let tl = get(v, "tokenLimits");
    let token_limits = if is_obj(tl) {
        let c = req_pos(get(tl, "maxContextTokens"), "tokenLimits.maxContextTokens")?;
        let o = opt_pos(get(tl, "maxOutputTokens"), "tokenLimits.maxOutputTokens")?;
        obj! {"maxContextTokens" => c, "maxOutputTokens" => o}
    } else {
        JsValue::Undefined
    };
    let protocol = get(v, "protocol");
    if !is_undef(protocol) && !matches!(str_of(protocol), Some("anthropic" | "openai" | "openai-responses" | "google")) {
        return Err(LoopErr::msg("Host model metadata response contains an invalid protocol."));
    }
    let spc = get(v, "supportsPromptCache");
    if !is_undef(spc) && bool_opt(spc).is_none() {
        return Err(LoopErr::msg("Host model metadata response contains an invalid prompt-cache capability."));
    }
    Ok(obj! {"provider" => provider, "model" => model, "maxContextTokens" => max_ctx, "maxOutputTokens" => max_out,
        "tokenLimits" => token_limits, "protocol" => protocol, "supportsPromptCache" => bool_opt(spc)})
}

fn or_default(v: &JsValue, d: &str) -> String {
    if nullish(v) { d.to_string() } else { js_string(v) }
}

/// Build the AgentLoop for one execute request, issuing the factory's host
/// calls (plan/todo read, model metadata) in the TS order.
pub fn create(rt: &mut Rt, request: &JsValue, stream_id: &str) -> R<Agent> {
    let payload = if is_obj(get(request, "payload")) { get(request, "payload").clone() } else { obj! {} };
    let rec = |k: &str| if is_obj(get(&payload, k)) { get(&payload, k).clone() } else { obj! {} };
    let agent = rec("agent");
    let task = rec("task");
    let context_override = rec("contextOverride");
    let execution_context = get(&payload, "executionContext").clone();
    let host_modules = get(&payload, "hostModules").clone();
    let interaction = get(&payload, "interactionCapabilities").clone();
    let m = Methods {
        model: read_methods(&host_modules, "model", &["prepare", "materialize_prepared_request", "stream", "stream_next", "close_stream", "get_metadata"]),
        budget: read_methods(&host_modules, "budget", &["estimate_request_input", "evaluate_request_budget", "estimate_usage_cost"]),
        turn: read_methods(&host_modules, "turn", &["drain_steer", "drain_or_close_steer", "persist_compaction"]),
        context: read_methods(&host_modules, "context", &["prepare_for_model", "apply_tool_results", "recover_from_model_error", "capture_turn", "try_auto_compact"]),
        lifecycle: read_methods(&host_modules, "lifecycle", &["dispatch"]),
        event: read_methods(&host_modules, "event", &["emit"]),
        capability: read_methods(&host_modules, "capability", &["execute", "execute_batch", "list_tools", "plan_todo"]),
    };
    let has_override_messages = matches!(&context_override, JsValue::Obj(o) if o.get("messages").is_some());
    let operation_id = gs(request, "operationId").unwrap_or("").to_string();
    let session_id = or_default(get(request, "sessionId"), &operation_id);
    let turn_id = or_default(get(request, "turnId"), &operation_id);
    let cwd_v = coalesce(&[get(&agent, "cwd"), get(&payload, "cwd")]);
    let cwd = if nullish(cwd_v) {
        std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
    } else {
        js_string(cwd_v)
    };
    let pci = rec("permissionContext");
    let permission_mode = if is_permission_mode(get(&pci, "mode")) {
        gs(&pci, "mode").unwrap().to_string()
    } else if is_permission_mode(get(&agent, "permissionMode")) {
        gs(&agent, "permissionMode").unwrap().to_string()
    } else {
        "default".to_string()
    };
    let can_prompt = gb(&pci, "canPrompt") == Some(true);
    let bypass_available = gb(&pci, "bypassAvailable") == Some(true);
    let pick = |k: &str| coalesce(&[get(&agent, k), get(&payload, k)]).clone();
    let run_mode = match str_of(&pick("runMode")) {
        Some(m @ ("agent" | "plan" | "ask")) => Some(m.to_string()),
        _ => None,
    };
    let subagent_model = as_subagent_model(&pick("subagentModel"))?;
    let rules = as_permission_rules(get(&pci, "rules"));
    let empty = || JsValue::Arr(Vec::new());
    let mut rule_set = obj! {"allow" => empty(), "deny" => empty(), "ask" => empty()};
    if let Some(r) = &rules {
        rule_set = spread(&rule_set, r.clone());
    }
    let permission_context = obj! {"mode" => &permission_mode, "canPrompt" => can_prompt, "bypassAvailable" => bypass_available,
        "cwd" => &cwd, "additionalWorkingDirectories" => as_string_array(get(&pci, "additionalWorkingDirectories")), "rules" => rule_set};
    let surface = match gs(&agent, "runtimeContextSurface") {
        Some(s @ ("system_prompt" | "user_message")) => s.to_string(),
        _ => "system_prompt".to_string(),
    };
    let cfg = Config {
        provider: or_default(&pick("provider"), "default"),
        model: or_default(&pick("model"), "default"),
        cwd: cwd.clone(),
        system_prompt: as_optional_string(coalesce(&[get(&context_override, "systemPrompt"), get(&agent, "systemPrompt"), get(&payload, "systemPrompt")])),
        append_system_prompt: as_string(&pick("appendSystemPrompt")),
        plan_mode_instructions: as_string(&pick("planModeInstructions")),
        runtime_context_surface: surface,
        max_output_tokens: as_positive_integer(&pick("maxOutputTokens")),
        max_context_tokens: as_positive_integer(&pick("maxContextTokens")),
        thinking: as_thinking_config(&pick("thinking")),
        tool_choice: as_tool_choice(&pick("toolChoice")),
        max_context_messages: as_positive_integer(&pick("maxContextMessages")),
        stop_on_structured_output: bool_opt(&pick("stopOnStructuredOutput")) == Some(true),
        json_self_correct: bool_opt(&pick("jsonSelfCorrect")) == Some(true),
        run_mode: run_mode.clone(),
        is_subagent: bool_opt(&pick("isSubagent")) == Some(true),
        subagent_model,
        permission_mode: permission_mode.clone(),
        permission_mode_before_plan: if is_permission_mode(get(&agent, "permissionModeBeforePlan")) { opt_s(&agent, "permissionModeBeforePlan") } else { None },
        permission_context,
        metadata: merge_metadata(&[get(&agent, "metadata"), &execution_context, get(&context_override, "metadata")]),
    };
    let tools_src = if is_undef(get(&context_override, "tools")) { get(&payload, "tools") } else { get(&context_override, "tools") };
    let tools = read_tool_descriptors(tools_src);
    let has = |l: &[String], x: &str| Methods::has(l, x);
    let input = Input {
        messages: build_initial_messages(&task, if has_override_messages { get(&context_override, "messages") } else { get(&payload, "messages") }, has_override_messages),
        max_turns: as_positive_integer(&pick("maxTurns")),
        max_budget_usd: as_positive_finite(get(&payload, "maxBudgetUsd")),
        task_budget_usd: as_positive_finite(get(&payload, "taskBudgetUsd")),
        initial_task_budget_spent_usd: as_non_negative_finite(get(&payload, "initialTaskBudgetSpentUsd")),
        run_mode,
        permission_mode,
        base_permission_mode: if is_permission_mode(get(&payload, "basePermissionMode")) { opt_s(&payload, "basePermissionMode") } else { None },
        allow_plan_mode_tools: gb(&payload, "allowPlanModeTools") == Some(true),
        can_prompt,
        can_elicit: gb(&payload, "canElicit") == Some(true),
        permission_rules: rules,
        model_override: None,
        has_drain_steer: has(&m.turn, "drain_steer"),
        has_drain_or_close: has(&m.turn, "drain_or_close_steer"),
        has_persist_compaction: has(&m.turn, "persist_compaction"),
    };
    let mut a = Agent {
        run_id: gs(request, "runId").unwrap_or("").to_string(),
        operation_id,
        idempotency_key: gs(request, "idempotencyKey").filter(|s| !s.is_empty()).map(String::from),
        operation_deadline: opt_s(request, "operationDeadline"),
        session_id,
        turn_id: turn_id.clone(),
        request_id: gs(request, "requestId").unwrap_or("").to_string(),
        stream_id: stream_id.to_string(),
        sequence: 0.0,
        cfg,
        input,
        elicitation_available: gb(&interaction, "elicitationAvailable") == Some(true),
        has_metadata_port: has(&m.model, "get_metadata"),
        m,
        tools,
        metadata: Vec::new(),
        plan_todo: None,
        plan_todo_seq: 0,
        prep_seq: 0,
        calibration: Vec::new(),
        caps: Vec::new(),
        allowed_read_files: Vec::new(),
        read_file_state: obj! {},
        write_snapshots: obj! {},
        timeline: Timeline::new(&turn_id),
        active_budget: None,
    };
    if has(&a.m.capability, "plan_todo") {
        a.plan_todo = Some(None);
        let (sid, tid) = (a.session_id.clone(), a.turn_id.clone());
        a.plan_todo_read(rt, &sid, &tid)?;
    }
    a.input.model_override = as_model_override(&pick("modelOverride"));
    if a.has_metadata_port {
        let (p, md) = match &a.input.model_override {
            Some(o) => (gs(o, "provider").unwrap().to_string(), gs(o, "model").unwrap().to_string()),
            None => (a.cfg.provider.clone(), a.cfg.model.clone()),
        };
        a.metadata_refresh(rt, &p, &md)?;
    }
    let (allowed, read, write) = parse_seed_state(get(&payload, "seedState"))?;
    a.allowed_read_files = allowed;
    a.read_file_state = read;
    a.write_snapshots = write;
    let (cal, caps) = parse_model_state(get(&payload, "modelState"))?;
    a.calibration = cal;
    a.caps = caps;
    Ok(a)
}
