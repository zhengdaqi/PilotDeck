//! Port of `src/gateway/server/GatewayWsConnection.ts`.
use crate::backend::{GatewayBackend, GwError, GwResult};
use crate::ws::WsSender;
use crate::{get, iso_now, js_spread, obj, s, stringify_compact, truthy, JsObject, JsValue, PILOTDECK_GATEWAY_PROTOCOL_VERSION};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

static CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct State {
    authed: bool,
    client_name: Option<JsValue>,
    /// insertion-ordered sets keyed by the JSON form of the session key value
    in_flight: Vec<(String, JsValue)>,
    interaction: Vec<(String, JsValue)>,
}

fn set_add(set: &mut Vec<(String, JsValue)>, v: &JsValue) {
    let k = stringify_compact(v);
    if !set.iter().any(|(x, _)| *x == k) {
        set.push((k, v.clone()));
    }
}
fn set_has(set: &[(String, JsValue)], v: &JsValue) -> bool {
    let k = stringify_compact(v);
    set.iter().any(|(x, _)| *x == k)
}
fn set_delete(set: &mut Vec<(String, JsValue)>, v: &JsValue) {
    let k = stringify_compact(v);
    set.retain(|(x, _)| *x != k);
}

pub struct GatewayWsConnection {
    ws: WsSender,
    gateway: Arc<dyn GatewayBackend>,
    token: String,
    server_version: String,
    state: Mutex<State>,
    binding: JsValue,
}

fn is_record(v: &JsValue) -> bool {
    matches!(v, JsValue::Obj(_) | JsValue::Arr(_))
}
fn is_str(v: &JsValue) -> bool {
    matches!(v, JsValue::Str(_))
}

/// `(params as {sessionKey?})?.sessionKey` for any params value.
fn session_key_of(params: &JsValue) -> JsValue {
    get(params, "sessionKey").clone()
}

impl GatewayWsConnection {
    pub fn new(ws: WsSender, gateway: Arc<dyn GatewayBackend>, token: String, server_version: String) -> Arc<Self> {
        let generation = CONNECTION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let binding = obj(vec![("connectionId", s(&pilotdeck_gateway_client::random_uuid_v4())), ("generation", JsValue::Num(generation as f64))]);
        Arc::new(GatewayWsConnection { ws, gateway, token, server_version, state: Mutex::new(State::default()), binding })
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn send(&self, v: JsValue) {
        self.ws.send_text(&stringify_compact(&v));
    }

    pub fn send_notification(&self, name: &str, payload: JsValue) {
        if !self.st().authed {
            return;
        }
        self.send(obj(vec![("type", s("notification")), ("name", s(name)), ("payload", payload)]));
    }

    /// `handleClose`
    pub fn handle_close(&self) {
        let sessions: Vec<JsValue> = {
            let st = self.st();
            let mut all = st.in_flight.clone();
            for e in &st.interaction {
                if !all.iter().any(|(k, _)| *k == e.0) {
                    all.push(e.clone());
                }
            }
            all.into_iter().map(|(_, v)| v).collect()
        };
        for session_key in sessions {
            let disconnected = if self.gateway.has("disconnectInteraction") {
                match self.gateway.call("disconnectInteraction", Some(obj(vec![("sessionKey", session_key.clone()), ("binding", self.binding.clone())]))) {
                    Ok(d) => Some(d),
                    // TS: the rejection escapes handleClose (unhandled), so cleanup stops here.
                    Err(_) => return,
                }
            } else {
                None
            };
            let in_flight = set_has(&self.st().in_flight, &session_key);
            let preserve = disconnected.as_ref().is_some_and(|d| truthy(get(d, "preserveTurn")));
            if in_flight && !preserve {
                let _ = self.gateway.call("abortTurn", Some(obj(vec![("sessionKey", session_key.clone())])));
            }
        }
        let mut st = self.st();
        st.in_flight.clear();
        st.interaction.clear();
    }

    /// `handleMessage`: hello is handled inline; authed requests run on their own thread
    /// (TS starts each `handleRequest` without awaiting the previous one).
    pub fn handle_message(self: &Arc<Self>, message: String) {
        let frame = match pilotdeck_skills::v8json::parse(&message) {
            Ok(f) => f,
            Err(_) => {
                self.ws.close(4002, "invalid_json");
                return;
            }
        };
        if !self.st().authed {
            self.handle_hello(&frame);
            return;
        }
        if !(is_record(&frame) && get(&frame, "type") == &s("request") && is_str(get(&frame, "id")) && is_str(get(&frame, "method"))) {
            self.ws.close(4002, "invalid_frame");
            return;
        }
        // submit_turn registers its session synchronously (before any await in TS).
        if get(&frame, "method") == &s("submit_turn") {
            let sk = session_key_of(get(&frame, "params"));
            if truthy(&sk) {
                let mut st = self.st();
                set_add(&mut st.in_flight, &sk);
                set_add(&mut st.interaction, &sk);
            }
        }
        let me = self.clone();
        std::thread::spawn(move || me.handle_request(frame));
    }

    fn handle_hello(&self, frame: &JsValue) {
        let is_hello = is_record(frame) && get(frame, "type") == &s("hello") && is_str(get(frame, "protocolVersion"))
            && is_str(get(frame, "clientName")) && is_str(get(frame, "clientVersion")) && is_str(get(frame, "token"));
        if !is_hello {
            self.ws.close(4001, "hello_required");
            return;
        }
        if get(frame, "protocolVersion") != &s(PILOTDECK_GATEWAY_PROTOCOL_VERSION) {
            self.ws.close(4001, "protocol_mismatch");
            return;
        }
        if get(frame, "token") != &s(&self.token) {
            self.ws.close(4003, "auth_failed");
            return;
        }
        {
            let mut st = self.st();
            st.authed = true;
            st.client_name = Some(get(frame, "clientName").clone());
        }
        // TS: a describeServer rejection is an unhandled rejection (no frame is sent).
        if let Ok(info) = self.gateway.call("describeServer", None) {
            self.send(obj(vec![("type", s("hello_ok")), ("protocolVersion", s(PILOTDECK_GATEWAY_PROTOCOL_VERSION)),
                ("serverVersion", s(&self.server_version)), ("serverInfo", info), ("interactionBinding", self.binding.clone())]));
        }
    }

    fn handle_request(&self, frame: JsValue) {
        let id = get(&frame, "id").clone();
        let method = match get(&frame, "method") {
            JsValue::Str(m) => m.clone(),
            _ => String::new(),
        };
        let params = get(&frame, "params").clone();
        if method == "submit_turn" {
            let sk = session_key_of(&params);
            let mut p = js_spread(if matches!(params, JsValue::Undefined | JsValue::Null) { &JsValue::Undefined } else { &params });
            p.set("interactionBinding", self.binding.clone());
            let p = JsValue::Obj(p);
            let project_key = get(&p, "projectKey").clone();
            let mut seq = 0f64;
            let mut last_completed: Option<JsValue> = None;
            let mut last_error: Option<JsValue> = None;
            let r = if !self.gateway.has("submitTurn") {
                Err(GwError::Plain { message: "this.options.gateway.submitTurn is not a function".into() })
            } else { self.gateway.submit_turn(p, &mut |event| {
                let ty = get(&event, "type").clone();
                if ty == s("turn_completed") {
                    last_completed = Some(event.clone());
                }
                if ty == s("error") {
                    last_error = Some(event.clone());
                }
                if ty == s("permission_request") && truthy(&sk) {
                    self.send_notification("permission_changed", obj(vec![("type", s("requested")), ("sessionKey", sk.clone()),
                        ("projectKey", project_key.clone()), ("requestId", get(&event, "requestId").clone())]));
                }
                self.send(obj(vec![("type", s("event")), ("id", id.clone()), ("seq", JsValue::Num(seq)), ("final", JsValue::Bool(false)), ("event", event)]));
                seq += 1.0;
            }) };
            if truthy(&sk) {
                set_delete(&mut self.st().in_flight, &sk);
            }
            match r {
                Err(e) => self.send_error(&id, &method, e),
                Ok(()) => {
                    let terminal = last_error.or(last_completed).unwrap_or_else(|| obj(vec![("type", s("error")), ("code", s("result_unknown")),
                        ("message", s("Gateway stream ended without a terminal result.")), ("recoverable", JsValue::Bool(true))]));
                    self.send(obj(vec![("type", s("event")), ("id", id), ("seq", JsValue::Num(seq)), ("final", JsValue::Bool(true)), ("event", terminal)]));
                }
            }
            return;
        }
        match self.dispatch(&method, &params) {
            Ok(result) => {
                if method == "permission_decide" {
                    // TS reads `input.sessionKey` on the raw params after the gateway call.
                    if let JsValue::Undefined | JsValue::Null = params {
                        let what = if matches!(params, JsValue::Null) { "null" } else { "undefined" };
                        return self.send_error(&id, &method, GwError::Plain { message: format!("Cannot read properties of {what} (reading 'sessionKey')") });
                    }
                    let (sk, rid, pk) = (get(&params, "sessionKey"), get(&params, "requestId"), get(&params, "projectKey"));
                    if is_str(sk) && is_str(rid) {
                        let mut n = vec![("type", s("resolved")), ("sessionKey", sk.clone())];
                        if is_str(pk) {
                            n.push(("projectKey", pk.clone()));
                        }
                        n.push(("requestId", rid.clone()));
                        self.send_notification("permission_changed", obj(n));
                    }
                }
                self.send(obj(vec![("type", s("response")), ("id", id), ("ok", JsValue::Bool(true)), ("result", result)]));
            }
            Err(e) => self.send_error(&id, &method, e),
        }
    }

    fn send_error(&self, id: &JsValue, method: &str, e: GwError) {
        let error = match e {
            GwError::SkillValidation { code, message, validation } =>
                obj(vec![("code", code), ("message", s(&message)), ("validation", validation)]),
            GwError::SkillManager { code, message } => obj(vec![("code", code), ("message", s(&message))]),
            GwError::Structured { code, message, details, status } => {
                let sop = (method == "sop_status" || method == "sop_resume") && matches!(status, JsValue::Num(n) if n.fract() == 0.0 && (400.0..=599.0).contains(&n));
                let mut o = JsObject::new();
                o.set("code", code);
                o.set("message", s(&message));
                if !matches!(details, JsValue::Undefined) {
                    o.set("details", details.clone());
                }
                if sop {
                    let mut d = if matches!(details, JsValue::Obj(_) | JsValue::Arr(_)) { js_spread(&details) } else { JsObject::new() };
                    d.set("httpStatus", status);
                    o.set("details", JsValue::Obj(d));
                }
                JsValue::Obj(o)
            }
            GwError::Plain { message } => obj(vec![("code", s("gateway_request_failed")), ("message", s(&message))]),
        };
        self.send(obj(vec![("type", s("response")), ("id", id.clone()), ("ok", JsValue::Bool(false)), ("error", error)]));
    }

    fn required(&self, gm: &str, params: Option<JsValue>) -> GwResult {
        if !self.gateway.has(gm) {
            return Err(GwError::Plain { message: format!("this.options.gateway.{gm} is not a function") });
        }
        self.gateway.call(gm, params)
    }
    fn optional(&self, wire: &str, gm: &str, params: Option<JsValue>) -> GwResult {
        if !self.gateway.has(gm) {
            return Err(GwError::capability_unavailable(format!("{wire} is unavailable.")));
        }
        self.gateway.call(gm, params)
    }

    fn dispatch(&self, method: &str, params: &JsValue) -> GwResult {
        let p = || Some(params.clone());
        let ok = |r: GwResult| r.map(|_| obj(vec![("ok", JsValue::Bool(true))]));
        if let Some(gm) = OPTIONAL.iter().find(|(w, _)| *w == method).map(|(_, g)| *g) {
            let args = if NO_ARG.contains(&gm) { None } else { p() };
            return self.optional(method, gm, args);
        }
        if let Some(gm) = OPTIONAL_OK.iter().find(|(w, _)| *w == method).map(|(_, g)| *g) {
            return ok(self.optional(method, gm, p()));
        }
        if let Some(gm) = REQUIRED.iter().find(|(w, _)| *w == method).map(|(_, g)| *g) {
            let args = if NO_ARG.contains(&gm) { None } else { p() };
            return self.required(gm, args);
        }
        if let Some(gm) = CAPABILITY.iter().find(|(w, _)| *w == method).map(|(_, g)| *g) {
            if !self.gateway.has(gm) {
                return Err(GwError::capability_unavailable(format!("Gateway capability {method} is unavailable.")));
            }
            return self.gateway.call(gm, p());
        }
        if let Some(gm) = SKILL.iter().find(|(w, _)| *w == method).map(|(_, g)| *g) {
            if !self.gateway.has(gm) {
                return Err(GwError::SkillManager { code: s("not_configured"), message: "Skill management is not enabled on this gateway.".into() });
            }
            return self.gateway.call(gm, p());
        }
        let with_binding = |extra: bool| {
            let mut o = js_spread(if matches!(params, JsValue::Null) { &JsValue::Undefined } else { params });
            if extra {
                o.set("interactionBinding", self.binding.clone());
            }
            JsValue::Obj(o)
        };
        let fallback = |gm: &str, args: Option<JsValue>, f: JsValue| -> GwResult {
            if self.gateway.has(gm) { self.gateway.call(gm, args) } else { Ok(f) }
        };
        match method {
            "abort_turn" => ok(self.required("abortTurn", p())),
            "close_session" => ok(self.required("closeSession", p())),
            "close_project_sessions" => {
                if !self.gateway.has("closeProjectSessions") {
                    return Err(GwError::Plain { message: "Project session closure is unavailable.".into() });
                }
                self.gateway.call("closeProjectSessions", p())
            }
            "record_agent_status_message" => fallback("recordAgentStatusMessage", p(), obj(vec![("recorded", JsValue::Bool(false))])),
            "active_turn_snapshot" => {
                let sk = match session_key_of(params) {
                    JsValue::Undefined | JsValue::Null => s(""),
                    v => v,
                };
                fallback("getActiveTurnSnapshot", p(), obj(vec![("active", JsValue::Bool(false)), ("sessionKey", sk), ("events", JsValue::Arr(vec![]))]))
            }
            "reconnect_interaction" => {
                if self.gateway.has("reconnectInteraction") {
                    let pr = if matches!(params, JsValue::Undefined | JsValue::Null) { JsValue::Obj(JsObject::new()) } else { params.clone() };
                    let sk = get(&pr, "sessionKey").clone();
                    if truthy(&sk) {
                        set_add(&mut self.st().interaction, &sk);
                    }
                    let sk_arg = match sk {
                        JsValue::Undefined | JsValue::Null => s(""),
                        v => v,
                    };
                    return self.gateway.call("reconnectInteraction", Some(obj(vec![("sessionKey", sk_arg),
                        ("previousBinding", get(&pr, "previousBinding").clone()), ("nextBinding", self.binding.clone())])));
                }
                Ok(obj(vec![("outcome", s("stale_binding")), ("requests", JsValue::Arr(vec![]))]))
            }
            "elicitation_respond" => self.required("respondElicitation", Some(with_binding(true))),
            "permission_decide" => {
                let sk = get(params, "sessionKey");
                let client_sdk = self.st().client_name.as_ref() == Some(&s("sdk"));
                let sdk_control = client_sdk && (!is_str(sk) || !set_has(&self.st().interaction, sk));
                self.required("permissionDecide", Some(with_binding(!sdk_control)))
            }
            "reload_config" => fallback("reloadConfig", None, obj(vec![("reloaded", JsValue::Bool(false)), ("reason", s("unsupported"))])),
            "prepare_weixin_login" => fallback("prepareWeixinLogin", None,
                obj(vec![("requested", JsValue::Bool(false)), ("requestedAt", s(&iso_now())), ("reason", s("unsupported"))])),
            "reload_extensions" => fallback("reloadExtensions", p(), obj(vec![("reloaded", JsValue::Bool(false)), ("reason", s("unsupported"))])),
            "always_on_apply" => fallback("alwaysOnApply", p(), obj(vec![("sessionKey", s("")),
                ("error", obj(vec![("code", s("not_configured")), ("message", s("Always-On apply not available"))]))])),
            "always_on_abort" => fallback("alwaysOnAbort", p(), obj(vec![("aborted", JsValue::Bool(false)), ("sessionKey", s("")),
                ("error", obj(vec![("code", s("not_configured")), ("message", s("Always-On abort not available"))]))])),
            "always_on_rerun_plan" => fallback("alwaysOnRerunPlan", p(), obj(vec![("runId", s("")),
                ("error", obj(vec![("code", s("not_configured")), ("message", s("Always-On rerun not available"))]))])),
            _ => Err(GwError::Plain { message: format!("Unknown gateway method {method}.") }),
        }
    }
}

/// Gateway methods the TS dispatcher calls with no argument.
const NO_ARG: &[&str] = &["resolveSettings", "supportedAgents", "describeServer", "listProjects"];

/// `if (gateway.x) return gateway.x(params); return reject(CAPABILITY_UNAVAILABLE "<wire> is unavailable.")`
/// (sop_status / sop_resume throw synchronously instead of rejecting; the frame is identical).
const OPTIONAL: &[(&str, &str)] = &[
    ("upload_create", "uploadCreate"), ("upload_get", "uploadGet"), ("upload_part", "uploadPart"),
    ("upload_complete", "uploadComplete"), ("upload_cancel", "uploadCancel"), ("run_get", "runGet"),
    ("run_events", "runEvents"), ("run_reattach", "runReattach"), ("permission_list", "listPermissions"),
    ("memory_list", "memoryList"), ("memory_wipe", "memoryWipe"), ("snapshot_list", "snapshotList"),
    ("snapshot_get", "snapshotGet"), ("snapshot_restore", "snapshotRestore"), ("manager_sessions", "managerSessions"),
    ("manager_browsers", "managerBrowsers"), ("native_archive_manifest", "nativeArchiveManifest"),
    ("native_archive_entries", "nativeArchiveEntries"), ("native_archive_artifact", "nativeArchiveArtifact"),
    ("sop_status", "sopStatus"), ("sop_resume", "resumeSop"),
    ("export_session_transcript", "exportSessionTranscript"), ("restore_session_transcript", "restoreSessionTranscript"),
    ("rename_session", "renameSession"), ("tag_session", "tagSession"), ("mcp_server_status", "mcpServerStatus"),
    ("set_mcp_servers", "setMcpServers"), ("set_mcp_permission_mode_override", "setMcpPermissionModeOverride"),
    ("project_file_read", "projectFileRead"), ("set_permission_mode", "setPermissionMode"),
    ("apply_flag_settings", "applyFlagSettings"), ("update_settings", "updateSettings"),
    ("resolve_settings", "resolveSettings"), ("set_session_thinking", "setSessionThinking"),
    ("output_styles_list", "outputStylesList"), ("set_output_style", "setOutputStyle"),
    ("reload_output_styles", "reloadOutputStyles"), ("usage_snapshot", "usageSnapshot"),
    ("model_usage_snapshot", "modelUsageSnapshot"), ("rewind_files", "rewindFiles"),
    ("background_task_stop", "stopBackgroundTask"), ("background_tasks", "backgroundTasks"),
    ("hook_async_result", "submitAsyncHookResult"), ("seed_read_state", "seedReadState"),
    ("supported_agents", "supportedAgents"), ("user_dialog_list", "listUserDialogs"),
    ("user_dialog_claim", "claimUserDialog"), ("user_dialog_release", "releaseUserDialog"),
];
/// Optional methods whose result is replaced by `{ ok: true }`.
const OPTIONAL_OK: &[(&str, &str)] = &[
    ("delete_session", "deleteSession"), ("mcp_server_reconnect", "reconnectMcpServer"), ("mcp_server_toggle", "toggleMcpServer"),
];
/// Non-optional Gateway methods (a missing one is a TypeError in TS).
const REQUIRED: &[(&str, &str)] = &[
    ("steer_turn", "steerTurn"), ("cancel_steer", "cancelSteer"), ("list_sessions", "listSessions"),
    ("resume_session", "resumeSession"), ("new_session", "newSession"), ("describe_server", "describeServer"),
    ("cron_create", "cronCreate"), ("cron_list", "cronList"), ("cron_update", "cronUpdate"), ("cron_delete", "cronDelete"),
    ("cron_stop", "cronStop"), ("cron_run_now", "cronRunNow"), ("user_dialog_respond", "respondUserDialog"),
    ("grant_session_permission", "grantSessionPermission"), ("read_session_messages", "readSessionMessages"),
    ("read_subagent_messages", "readSubagentMessages"), ("fork_session", "forkSession"),
    ("replace_last_turn", "replaceLastTurn"), ("finalize_last_turn_replacement", "finalizeLastTurnReplacement"),
    ("list_projects", "listProjects"), ("describe_project", "describeProject"),
];
/// `requireCapability(...)`
const CAPABILITY: &[(&str, &str)] = &[
    ("project_files_list", "projectFilesList"), ("commands_list", "commandsList"), ("model_catalog_list", "modelCatalogList"),
    ("session_model_get", "sessionModelGet"), ("session_model_set", "sessionModelSet"), ("session_model_clear", "sessionModelClear"),
];
/// `requireSkillMethod(...)`
const SKILL: &[(&str, &str)] = &[
    ("skill_list", "skillsList"), ("skill_read", "skillRead"), ("skill_write", "skillWrite"), ("skill_create", "skillCreate"),
    ("skill_delete", "skillDelete"), ("skill_import", "skillImport"), ("skill_validate", "skillValidate"), ("skill_scan", "skillScan"),
];
