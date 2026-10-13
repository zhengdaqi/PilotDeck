//! Pure helpers ported from AgentLoop.ts (statuses, agent errors, model failure actions),
//! tool/execution/errorRecovery.ts (missing tool results) and small message utilities.
use crate::json::*;
use crate::obj;

pub fn agent_error(code: &str, message: &str, details: JsValue, user_hint: Option<&str>) -> JsValue {
    let mut e = obj! {"code" => code, "message" => message, "details" => details};
    if let Some(h) = user_hint.filter(|h| !h.is_empty()) {
        set(&mut e, "userHint", h);
    }
    e
}

fn i18n(key: &str, params: Option<JsValue>) -> JsValue {
    obj! {"key" => key, "params" => params}
}

pub struct Detail<'a> {
    pub message: &'a str,
    pub message_i18n: Option<JsValue>,
    pub code: JsValue,
    pub severity: Option<&'a str>,
    pub user_hint: Option<String>,
    pub user_hint_i18n: Option<JsValue>,
    pub detail: Option<JsValue>,
}

fn status_detail(d: Detail, error: bool) -> JsValue {
    let severity = if error { Some(d.severity.unwrap_or("error")) } else { None };
    let base = obj! {"message" => d.message, "messageI18n" => d.message_i18n, "code" => d.code, "severity" => severity,
        "visible" => true, "userHint" => d.user_hint, "userHintI18n" => d.user_hint_i18n, "scope" => "turn", "source" => "agent"};
    match d.detail {
        Some(x) => spread(&base, x),
        None => base,
    }
}

pub fn status(event: &str, kind: &str, text: &str, detail: JsValue) -> JsValue {
    obj! {"event" => event, "kind" => kind, "text" => text, "detail" => detail}
}

fn err_status(event: &str, text: &str, d: Detail) -> JsValue {
    let detail = status_detail(d, true);
    status(event, "error", text, detail)
}

pub struct Action {
    pub short: String,
    pub hint: String,
    pub i18n: JsValue,
    pub fix: &'static str,
}

fn action(hint: String, fix: &'static str, key: &str, params: JsValue) -> Action {
    Action { short: format!("Action: {hint}"), i18n: obj! {"key" => format!("chat:agentStatus.modelRequestFailed.actions.{key}"), "params" => params}, hint, fix }
}

pub fn model_failure_action(e: Option<&JsValue>) -> Action {
    let Some(e) = e else {
        return action("Check Settings → Model Provider and verify the selected provider, base URL, API key, model name, and timeoutMs. If the provider is slow or the network is unstable, increase timeoutMs or check provider status.".into(), "settings", "settingsDefault", obj! {});
    };
    let provider = gs(e, "provider").filter(|s| !s.is_empty());
    let model = gs(e, "model").filter(|s| !s.is_empty());
    let pl = provider.map(|p| format!(" provider \"{p}\"")).unwrap_or_else(|| " provider".into());
    let ml = model.map(|m| format!(" model \"{m}\"")).unwrap_or_else(|| " selected model".into());
    let status = gn(e, "status");
    let code = gs(e, "code").unwrap_or("");
    let pv = || gs(e, "provider").unwrap_or("the provider").to_string();
    if status == Some(401.0) || status == Some(403.0) || code == "auth_error" {
        return action(format!("Update the API key or access permissions for{pl} in Settings → Model Provider, or run pilotdeck setup."), "settings", "auth", obj! {"provider" => pv()});
    }
    if code == "model_not_found" {
        return action(format!("Choose a valid{ml} for{pl} in Settings → Model Provider, or add it under model.providers.<id>.models in pilotdeck.yaml."), "settings", "modelNotFound", obj! {"provider" => pv(), "model" => get(e, "model")});
    }
    if code == "timeout" {
        if gs(get(e, "settingsFix"), "configPath") == Some("model.providers.<id>.retry.streamIdleTimeoutMs") {
            return action(format!("Increase streamIdleTimeoutMs for{pl} in Settings → Advanced, or check local network/proxy and provider status."), "network", "streamIdleTimeout", obj! {"provider" => pv()});
        }
        return action(format!("Increase timeoutMs for{pl} in Settings → Model Provider → Advanced, or check local network/proxy and provider status."), "network", "timeout", obj! {"provider" => pv()});
    }
    if status == Some(429.0) || code == "rate_limit_error" {
        return action("Wait for the provider rate limit to reset, reduce concurrency, or switch to another provider/model in Settings.".into(), "provider", "rateLimit", obj! {});
    }
    if code == "billing" {
        return action("Top up billing/quota on the provider API side, or switch to another provider/model in Settings.".into(), "provider", "billing", obj! {});
    }
    if matches!(code, "prompt_too_long" | "context_overflow" | "context_overflow_after_emergency_compaction") {
        return action("Run /compact, start a new session, remove large attachments, or switch to a larger-context model in Settings.".into(), "prompt", "contextOverflow", obj! {});
    }
    if matches!(code, "payload_too_large" | "request_too_large") {
        return action("Reduce attachments/context size, run /compact, or start a new session before retrying.".into(), "prompt", "payloadTooLarge", obj! {});
    }
    if code == "max_output_reached" {
        return action("Increase max output tokens in Settings → Model Provider, or ask the agent to split the answer into smaller parts.".into(), "settings", "maxOutput", obj! {});
    }
    if code == "image_too_large" {
        return action("Resize or remove large images, then retry.".into(), "prompt", "imageTooLarge", obj! {});
    }
    if truthy(get(e, "retryable")) || code == "server_error" || code == "overloaded_error" {
        return action("Retry later, check provider API status, or switch to another provider/model in Settings if it repeats.".into(), "provider", "providerRetry", obj! {});
    }
    action("Check Settings → Model Provider for base URL/API key/model and timeoutMs. If settings look correct, check local network/proxy and provider API status/logs.".into(), "settings", "settingsDefault", obj! {})
}

pub fn model_request_failed_status(error: &JsValue, model_error: Option<&JsValue>) -> JsValue {
    let em = gs(error, "message").unwrap_or("");
    let provider_message = if !em.is_empty() {
        em.to_string()
    } else {
        model_error.and_then(|m| gs(m, "message")).filter(|s| !s.is_empty()).unwrap_or("The model request failed, so this turn has stopped.").to_string()
    };
    let clean = provider_message.trim();
    let clean = if clean.is_empty() { "The model request failed." } else { clean };
    let act = model_failure_action(model_error);
    let text = format!("{clean}\n\n{}", act.short);
    let me = |k: &str| model_error.map(|m| get(m, k).clone()).unwrap_or(JsValue::Undefined);
    let detail = obj! {"provider" => me("provider"), "protocol" => me("protocol"), "status" => me("status"),
        "modelErrorCode" => me("code"), "retryable" => me("retryable"), "providerMessage" => &provider_message,
        "settingsFix" => me("settingsFix"), "fixTarget" => act.fix};
    err_status("model_request_failed", &text.clone(), Detail {
        message: &text,
        message_i18n: Some(i18n("chat:agentStatus.modelRequestFailed.message", Some(obj! {"providerMessage" => &provider_message}))),
        code: get(error, "code").clone(),
        severity: None,
        user_hint: Some(act.hint),
        user_hint_i18n: Some(act.i18n),
        detail: Some(detail),
    })
}

fn hint_or(error: &JsValue, fallback: &str) -> String {
    gs(error, "userHint").map(String::from).unwrap_or_else(|| fallback.to_string())
}

pub fn tool_call_recovery_exhausted_status(error: &JsValue, attempts: Option<f64>, reason: Option<&str>) -> JsValue {
    let m = gs(error, "message").filter(|s| !s.is_empty()).unwrap_or("Tool-call recovery was exhausted, so this turn has stopped.").to_string();
    err_status("tool_call_recovery_exhausted", &m, Detail {
        message: &m,
        message_i18n: Some(i18n("chat:agentStatus.toolCallRecoveryExhausted.message", Some(obj! {"message" => &m}))),
        code: get(error, "code").clone(),
        severity: None,
        user_hint: Some(hint_or(error, "Retry with a shorter prompt, ask the agent to split large tool inputs into smaller steps, or switch to a model with stronger tool-calling support in Settings → Model Provider.")),
        user_hint_i18n: Some(i18n("chat:agentStatus.toolCallRecoveryExhausted.hint", None)),
        detail: Some(obj! {"attempts" => attempts, "reason" => reason}),
    })
}

pub fn tool_error_loop_status(error: &JsValue, repeated: Option<f64>) -> JsValue {
    let m = gs(error, "message").filter(|s| !s.is_empty()).unwrap_or("The agent repeatedly hit the same tool error, so this turn has stopped.").to_string();
    err_status("tool_error_loop", &m, Detail {
        message: &m, message_i18n: None, code: get(error, "code").clone(), severity: None,
        user_hint: Some(hint_or(error, "Change the request to avoid repeating the same failing tool call, grant any required permission, or switch to a model with stronger tool-calling support in Settings → Model Provider.")),
        user_hint_i18n: None, detail: Some(obj! {"repeatedFailures" => repeated}),
    })
}

pub fn lifecycle_blocked_status(error: &JsValue, stage: &str) -> JsValue {
    let m = gs(error, "message").filter(|s| !s.is_empty()).unwrap_or("A lifecycle hook blocked this turn.").to_string();
    err_status("lifecycle_blocked", &m, Detail {
        message: &m, message_i18n: None, code: get(error, "code").clone(), severity: None,
        user_hint: Some(hint_or(error, "Review the blocking lifecycle hook output or disable the hook, then retry.")),
        user_hint_i18n: None, detail: Some(obj! {"stage" => stage}),
    })
}

pub fn empty_response_status(provider: &JsValue, model: &JsValue, attempts: f64) -> JsValue {
    let t = "The model returned empty content repeatedly, so this turn has stopped. Try again later or increase max output tokens.";
    err_status("model_empty_response_exhausted", t, Detail {
        message: t, message_i18n: Some(i18n("chat:agentStatus.emptyResponse.message", None)), code: s("model_empty_response_exhausted"), severity: None,
        user_hint: Some("Increase max output tokens in Settings → Model Provider, retry with a shorter prompt, or check whether this provider/model supports the requested output format.".into()),
        user_hint_i18n: Some(i18n("chat:agentStatus.emptyResponse.hint", None)),
        detail: Some(obj! {"provider" => provider, "model" => model, "attempts" => attempts}),
    })
}

pub fn max_turns_status(max_turns: f64, error: &JsValue) -> JsValue {
    let n = crate::rt::js_string(&JsValue::Num(max_turns));
    let t = format!("Reached the maximum number of turns ({n}), so this turn has stopped. Increase maxTurns or split the task into smaller steps and try again.");
    err_status("max_turns_reached", &t.clone(), Detail {
        message: &t, message_i18n: Some(i18n("chat:agentStatus.maxTurns.message", Some(obj! {"maxTurns" => max_turns}))),
        code: get(error, "code").clone(), severity: None,
        user_hint: Some(hint_or(error, "Increase maxTurns in local config if this task legitimately needs more agent steps, or split the task into smaller prompts and try again.")),
        user_hint_i18n: Some(i18n("chat:agentStatus.maxTurns.hint", None)), detail: Some(obj! {"maxTurns" => max_turns}),
    })
}

pub fn max_output_recovery_exhausted_status(attempts: f64) -> JsValue {
    let t = "Output token recovery was exhausted, so the visible response may be incomplete. Increase max output tokens or split the task into smaller steps and try again.";
    err_status("max_output_recovery_exhausted", t, Detail {
        message: t, message_i18n: Some(i18n("chat:agentStatus.maxOutputRecoveryExhausted.message", None)), code: s("max_output_recovery_exhausted"),
        severity: Some("warning"),
        user_hint: Some("Increase max output tokens in Settings → Model Provider, or ask the agent to split the answer into smaller parts.".into()),
        user_hint_i18n: Some(i18n("chat:agentStatus.maxOutputRecoveryExhausted.hint", None)), detail: Some(obj! {"attempts" => attempts}),
    })
}

fn plain_status(event: &str, text: &str, key: &str, code: &str, hint: Option<(&str, &str)>, detail: Option<JsValue>) -> JsValue {
    let d = status_detail(Detail {
        message: text, message_i18n: Some(i18n(key, None)), code: s(code), severity: None,
        user_hint: hint.map(|h| h.0.to_string()), user_hint_i18n: hint.map(|h| i18n(h.1, None)), detail,
    }, false);
    status(event, "status", text, d)
}

pub fn structured_output_completed_status() -> JsValue {
    plain_status("structured_output_completed", "Structured output was returned, so this turn has completed.",
        "chat:agentStatus.structuredOutputCompleted.message", "structured_output_completed", None, None)
}

pub fn turn_aborted_status(reason: Option<String>) -> JsValue {
    plain_status("turn_aborted", "This turn was aborted before completion.", "chat:agentStatus.turnAborted.message", "turn_aborted",
        Some(("Retry when you are ready to continue. If this was unexpected, check whether you clicked Stop, switched sessions during a run, or lost the gateway connection.", "chat:agentStatus.turnAborted.hint")),
        Some(obj! {"reason" => reason}))
}

fn warn_status(event: &str, text: &str, key: &str, hint: &str, hint_key: &str) -> JsValue {
    err_status(event, text, Detail {
        message: text, message_i18n: Some(i18n(key, None)), code: s(event), severity: Some("warning"),
        user_hint: Some(hint.into()), user_hint_i18n: Some(i18n(hint_key, None)), detail: None,
    })
}

pub fn finish_reason_status(finish: &str, text: &str) -> Option<JsValue> {
    if text.trim().is_empty() {
        return None;
    }
    match finish {
        "content_filter" => Some(warn_status("content_filter_stop", "The response may be incomplete because the model stopped due to content filtering.",
            "chat:agentStatus.contentFilter.message",
            "Retry with a narrower request or adjust the prompt to avoid filtered content; if this seems wrong, check the provider API policy/status for the selected model.",
            "chat:agentStatus.contentFilter.hint")),
        "unknown" => Some(warn_status("unknown_finish_reason", "The model stream ended without a normal finish reason, so the response may be incomplete.",
            "chat:agentStatus.unknownFinishReason.message",
            "Retry the turn; if it repeats, check provider API status/logs for stream finish reasons and verify the selected provider/model in Settings → Model Provider.",
            "chat:agentStatus.unknownFinishReason.hint")),
        _ => None,
    }
}

/// `stringifyAbortReason`; `None` reason models an `AbortSignal` without abort.
pub fn stringify_abort_reason(reason: &JsValue) -> Option<String> {
    match reason {
        JsValue::Null => None,
        // `controller.abort()` without a reason yields a DOMException AbortError.
        JsValue::Undefined => Some("This operation was aborted".into()),
        JsValue::Str(s) => Some(s.clone()),
        other => Some(to_json(other)),
    }
}
pub fn should_surface_abort(reason: &JsValue) -> bool {
    if matches!(reason, JsValue::Null) {
        return false;
    }
    let t = stringify_abort_reason(reason).unwrap_or_default().to_lowercase();
    t.contains("timeout") || t.contains("cancel") || t.contains("abort")
}

// ---------- prompt-too-long classification ----------
pub fn is_prompt_too_long(e: &JsValue) -> bool {
    if gs(e, "code") == Some("prompt_too_long") || truthy(get(e, "recoverableViaCompact")) {
        return true;
    }
    let m = gs(e, "message").unwrap_or("").to_lowercase();
    m.contains("prompt is too long") || m.contains("input length and max_tokens exceed context limit") || m.contains("request too large")
}

pub fn classify_model_error(e: &JsValue) -> (&'static str, JsValue) {
    let msg = gs(e, "message").unwrap_or("");
    if is_prompt_too_long(e) {
        let hint = gs(e, "userHint").unwrap_or("Input exceeds the model context window. Try /compact to compress history or /new for a fresh session.");
        return ("prompt_too_long", agent_error("agent_prompt_too_long", msg, e.clone(), Some(hint)));
    }
    ("model_error", agent_error("agent_model_error", msg, e.clone(), gs(e, "userHint")))
}

// ---------- tool error recovery (subset reachable from createMissingToolResult) ----------
fn trim_sentence(v: &str) -> String {
    let compact = v.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = compact.chars().collect();
    if chars.len() <= 220 {
        return compact;
    }
    format!("{}...", chars[..217].iter().collect::<String>().trim_end())
}
fn lines_of(v: &str) -> Vec<&str> {
    v.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}
fn unique(values: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}
fn is_tool_error_line(l: &str) -> bool {
    l.len() >= 11 && l[..11].eq_ignore_ascii_case("TOOL_ERROR[")
}
fn regex_any(hay: &str, needles: &[&str]) -> bool {
    let h = hay.to_lowercase();
    needles.iter().any(|n| h.contains(&n.to_lowercase()))
}

/// `buildToolErrorRecovery` for `tool_execution_failed` without details.
pub fn missing_tool_recovery(tool_name: &str, message: &str, permission_mode: &str) -> (String, JsValue) {
    let code = "tool_execution_failed";
    let evidence = unique(lines_of(message).iter().map(|l| l.trim()).filter(|l| !l.is_empty() && !is_tool_error_line(l)).take(2)
        .map(trim_sentence).filter(|s| !s.is_empty()).collect());
    let first_line = lines_of(message).iter().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("").to_string();
    let summary = if let Some(e) = evidence.first() {
        trim_sentence(e)
    } else if !first_line.is_empty() {
        trim_sentence(&first_line)
    } else {
        trim_sentence(&format!("{tool_name} failed with {code}."))
    };
    let class = if regex_any(message, &["Permission denied", "EACCES", "EPERM"]) {
        "ask_user"
    } else if regex_any(message, &["No such file or directory", "ENOENT", "NameError", "ReferenceError", "SyntaxError", "TypeError", "ModuleNotFoundError", "Cannot find module"]) {
        "fix_input"
    } else if regex_any(message, &["timeout", "timed out", "time out", "ETIMEDOUT", "ESOCKETTIMEDOUT"]) {
        "retry_later"
    } else {
        "environment_issue"
    };
    let next = vec![format!("Inspect the evidence, change the approach, then retry only with corrected inputs. Current permission mode: {permission_mode}.")];
    let cleaned = lines_of(message).iter().map(|l| l.trim_end()).filter(|l| !is_tool_error_line(l.trim())).collect::<Vec<_>>().join("\n");
    let cleaned = cleaned.trim().to_string();
    let original = if cleaned.is_empty() {
        None
    } else if cleaned.chars().count() <= 2000 {
        Some(cleaned.clone())
    } else {
        let head: String = cleaned.chars().take(2000).collect();
        Some(format!("{}\n... [original error truncated; {} chars total]", head.trim_end(), cleaned.encode_utf16().count()))
    };
    let mut lines = vec![format!("TOOL_ERROR[{code}][{tool_name}][{class}]"), format!("Summary: {summary}")];
    if !evidence.is_empty() {
        lines.push("Evidence:".into());
        for e in &evidence {
            lines.push(format!("- {e}"));
        }
    }
    if let Some(o) = &original {
        lines.push("Original error:".into());
        lines.push(o.clone());
    }
    lines.push("Next actions:".into());
    for (i, a) in next.iter().enumerate() {
        lines.push(format!("{}. {a}", i + 1));
    }
    let advice = obj! {"summary" => summary, "failureClass" => class,
        "nextActions" => next.iter().map(|x| s(x)).collect::<Vec<_>>(),
        "salientEvidence" => evidence.iter().map(|x| s(x)).collect::<Vec<_>>(), "originalError" => original};
    (lines.join("\n"), advice)
}

pub fn missing_tool_result(call: &JsValue, message: &str, permission_mode: &str) -> JsValue {
    let ts = crate::rt::iso_now();
    let name = gs(call, "name").unwrap_or("");
    let (text, advice) = missing_tool_recovery(name, message, permission_mode);
    obj! {"type" => "error", "toolCallId" => get(call, "id"), "toolName" => name,
        "error" => obj! {"code" => "tool_execution_failed", "message" => message},
        "content" => vec![obj! {"type" => "text", "text" => text}],
        "metadata" => obj! {"recovery" => advice}, "startedAt" => &ts, "completedAt" => &ts}
}

// ---------- message helpers ----------
pub fn text_from_message(m: &JsValue) -> String {
    ga(m, "content").iter().filter(|b| gs(b, "type") == Some("text")).map(|b| gs(b, "text").unwrap_or("").to_string()).collect::<Vec<_>>().join("\n")
}
pub fn without_thinking(m: &JsValue) -> JsValue {
    let content: Vec<JsValue> = ga(m, "content").iter().filter(|b| gs(b, "type") != Some("thinking")).cloned().collect();
    spread(m, obj! {"content" => content})
}
pub fn safe_final_text(m: &JsValue, tool_calls: usize) -> Option<JsValue> {
    if tool_calls > 0 {
        return None;
    }
    let t = without_thinking(m);
    if text_from_message(&t).trim().is_empty() { None } else { Some(t) }
}

pub fn merge_usage(a: &JsValue, b: &JsValue) -> JsValue {
    if !is_obj(b) {
        return a.clone();
    }
    let add = |k: &str| match (gn(a, k), gn(b, k)) {
        (None, None) => JsValue::Undefined,
        (x, y) => JsValue::Num(x.unwrap_or(0.0) + y.unwrap_or(0.0)),
    };
    obj! {"inputTokens" => add("inputTokens"), "outputTokens" => add("outputTokens"), "cacheReadTokens" => add("cacheReadTokens"),
        "cacheWriteTokens" => add("cacheWriteTokens"), "totalTokens" => add("totalTokens"), "nativeCost" => add("nativeCost")}
}

pub fn strip_trailing_error_pair(messages: &mut Vec<JsValue>) {
    if let Some(last) = messages.last() {
        if gs(last, "role") == Some("user") && ga(last, "content").iter().all(|b| gs(b, "type") == Some("tool_result")) {
            messages.pop();
        }
    }
    if messages.last().map_or(false, |m| gs(m, "role") == Some("assistant")) {
        messages.pop();
    }
}

pub fn strip_images(messages: &mut [JsValue]) {
    let ph = || obj! {"type" => "text", "text" => "[Image removed: multimodal processor error recovery]"};
    for m in messages.iter_mut() {
        let content: Vec<JsValue> = ga(m, "content").iter().map(|b| {
            if gs(b, "type") == Some("image") {
                ph()
            } else if gs(b, "type") == Some("tool_result") && ga(b, "content").iter().any(|c| gs(c, "type") == Some("image")) {
                let inner: Vec<JsValue> = ga(b, "content").iter().map(|c| if gs(c, "type") == Some("image") { ph() } else { c.clone() }).collect();
                spread(b, obj! {"content" => inner})
            } else {
                b.clone()
            }
        }).collect();
        *m = spread(m, obj! {"content" => content});
    }
}

pub fn is_missing_reasoning_content_error(e: &JsValue) -> bool {
    let m = gs(e, "message").unwrap_or("").to_lowercase();
    let word = |w: &str| {
        m.match_indices(w).any(|(i, _)| {
            let before = m[..i].chars().last().map_or(true, |c| !(c.is_alphanumeric() || c == '_'));
            let after = m[i + w.len()..].chars().next().map_or(true, |c| !(c.is_alphanumeric() || c == '_'));
            before && after
        })
    };
    let thinking_mode = m.split("thinking").skip(1).any(|rest| rest.starts_with(char::is_whitespace) && rest.trim_start().starts_with("mode"));
    let pass_back = ["pass", "passed"].iter().any(|p| m.split(p).skip(1).any(|rest| rest.starts_with(char::is_whitespace) && rest.trim_start().starts_with("back")));
    word("reasoning_content") && thinking_mode && pass_back
}

pub fn add_empty_reasoning_markers(messages: &mut [JsValue]) {
    let n = messages.len();
    for (i, m) in messages.iter_mut().enumerate() {
        let content = ga(m, "content").to_vec();
        if i + 1 == n && gs(m, "role") == Some("assistant") && content.is_empty() {
            continue;
        }
        let replayable = content.iter().any(|b| gs(b, "type") == Some("thinking") && {
            let rc = get(b, "reasoningContent");
            let src = if is_undef(rc) { gs(b, "text").unwrap_or("") } else { str_of(rc).unwrap_or("") };
            !src.is_empty() || str_of(rc) == Some("")
        });
        if gs(m, "role") != Some("assistant") || replayable {
            continue;
        }
        let mut nc = vec![obj! {"type" => "thinking", "text" => "", "reasoningContent" => ""}];
        nc.extend(content);
        *m = spread(m, obj! {"content" => nc});
    }
}

pub fn stream_interruption_prompt(phase: &str, active: &[JsValue]) -> String {
    match phase {
        "tool_call" => {
            let tools: Vec<String> = active.iter().map(|c| gs(c, "name").filter(|n| !n.is_empty()).unwrap_or("unknown").to_string()).collect();
            let label = if tools.is_empty() { String::new() } else { format!(" ({})", tools.iter().take(3).cloned().collect::<Vec<_>>().join(", ")) };
            [format!("The previous model stream disconnected while generating a tool call{label}. No incomplete tool call was executed."),
             "Continue the original task from the current workspace state. Inspect relevant files before writing.".into(),
             "Do not retry the same large atomic write. Create or extend the artifact through small focused write_file or edit_file calls, keeping each tool call well under 8K output tokens.".into()].join("\n")
        }
        "reasoning" => "The previous model stream disconnected during reasoning. Continue the original task directly from the current workspace state; do not repeat analysis or recap.".into(),
        "text" => "The previous model stream disconnected mid-response. Continue exactly where the visible response ended; do not repeat prior text or recap.".into(),
        _ => "The previous model stream disconnected before producing a response. Continue the original task directly from the current workspace state.".into(),
    }
}

pub fn unknown_finish_prompt(has_tool_calls: bool) -> String {
    if has_tool_calls {
        return ["The previous response ended without a recognized finish reason after generating tool calls. No tool call was executed.",
            "Continue the original task from the current workspace state. Inspect relevant files before acting.",
            "Do not repeat the same large atomic write. Use small focused write_file or edit_file calls."].join("\n");
    }
    "The previous response ended without a recognized finish reason. Continue exactly where the visible response ended; do not repeat prior text or recap.".into()
}

pub const CIRCUIT_BREAKER_GRACE_PROMPT: &str = "Your last several tool calls all failed input validation with the same error. This may indicate a tool-side issue rather than a problem with your approach. Options: (1) try a different tool or different parameters, (2) explain the situation in text without calling tools, (3) if you believe the tool should work, try once more with corrected input.";
