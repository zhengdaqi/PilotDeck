//! Execute handling for the native runtime (`Rt`): port of
//! `AgentLoopSidecarServer.handleExecute` (agentLoopSidecarServer.ts).
use crate::agent::Agent;
use crate::json::*;
use crate::obj;
use crate::now_ms;
use crate::rt::{earliest_deadline, next_id, Active, LoopErr, Rt};

fn serialize_error(e: &LoopErr) -> JsValue {
    obj! {"code" => e.code.clone(), "message" => e.message.clone()}
}

fn seed_state(a: &Agent) -> JsValue {
    obj! {"allowedReadFiles" => a.allowed_read_files.iter().map(|x| JsValue::Str(x.clone())).collect::<Vec<_>>(),
        "readFileState" => a.read_file_state.clone(), "writeSnapshots" => a.write_snapshots.clone()}
}

fn model_state(a: &Agent) -> JsValue {
    let calibration: Vec<JsValue> = a.calibration.iter().map(|(_, v)| v.clone()).collect();
    let caps: Vec<JsValue> = a.caps.iter().filter_map(|(key, c)| {
        if c.max_context_tokens.is_none() && c.session_max_output_tokens.is_none() && c.hard_max_output_tokens.is_none() {
            return None;
        }
        let mut parts = key.splitn(2, '\u{0}');
        let (p, m) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        if p.is_empty() || m.is_empty() {
            return None;
        }
        Some(obj! {"provider" => p, "model" => m, "maxContextTokens" => c.max_context_tokens,
            "sessionMaxOutputTokens" => c.session_max_output_tokens, "hardMaxOutputTokens" => c.hard_max_output_tokens})
    }).collect();
    obj! {"tokenCalibration" => if calibration.is_empty() { JsValue::Undefined } else { JsValue::Arr(calibration) },
        "tokenCaps" => if caps.is_empty() { JsValue::Undefined } else { JsValue::Arr(caps) }}
}

struct Ids { run: String, op: String, req: String, stream: String }

fn final_event(rt: &mut Rt, ids: &Ids, seq: f64, prefix: &str, event_type: &str, outcome: &str, code: Option<String>,
               error: JsValue, payload: JsValue) {
    rt.flush_events();
    rt.send(&obj! {"kind" => "event", "messageId" => next_id(prefix), "eventType" => event_type, "streamId" => &ids.stream,
        "sequence" => seq, "runId" => &ids.run, "operationId" => &ids.op, "requestId" => &ids.req, "final" => true,
        "outcome" => outcome, "code" => code, "error" => error, "payload" => payload});
}

fn result_unknown(rt: &mut Rt, ids: &Ids, seq: f64, error: JsValue) {
    rt.op_final(&ids.op, "result_unknown");
    let code = gs(&error, "code").map(String::from);
    final_event(rt, ids, seq, "unknown", "agent.execute.unknown", "result_unknown", code, error.clone(), obj! {"error" => error});
}

fn deadline_unknown(rt: &mut Rt, ids: &Ids, seq: f64) {
    result_unknown(rt, ids, seq, obj! {"code" => "DEADLINE_EXCEEDED", "message" => "Module execution deadline exceeded."});
}

pub fn handle_execute(rt: &mut Rt, request: JsValue) {
    let run_id = gs(&request, "runId").unwrap_or("").to_string();
    let op = gs(&request, "operationId").unwrap_or("").to_string();
    let request_id = gs(&request, "requestId").unwrap_or("").to_string();
    rt.op_accept(&run_id, &op, &request_id);
    let deadline_at = earliest_deadline(&[gs(&request, "operationDeadline"), gs(&request, "attemptDeadline")]);
    if let Some(d) = deadline_at {
        if d <= now_ms() {
            rt.op_final(&op, "failed");
            rt.send(&obj! {"kind" => "response", "messageId" => next_id("deadline"), "inReplyTo" => gs(&request, "messageId"),
                "requestId" => &request_id, "ok" => false, "final" => true, "outcome" => "failed", "code" => "DEADLINE_EXCEEDED"});
            return;
        }
    }
    rt.active = Some(Active { run_id: run_id.clone(), operation_id: op.clone(), request_id: request_id.clone(),
        reason: None, deadline_exceeded: false, deadline_at });
    let stream_id = next_id("stream");
    rt.send(&obj! {"kind" => "response", "messageId" => next_id("accepted"), "inReplyTo" => gs(&request, "messageId"),
        "requestId" => &request_id, "ok" => true, "streamId" => &stream_id, "cursor" => 0.0});
    let ids = Ids { run: run_id, op: op.clone(), req: request_id, stream: stream_id.clone() };
    let deadline_exceeded = |rt: &Rt| rt.active.as_ref().map_or(false, |a| a.deadline_exceeded);
    match crate::factory::create(rt, &request, &stream_id) {
        Err(e) => {
            // The factory publishes no stream events; sequence starts at 0.
            fail(rt, &ids, 0.0, &e, deadline_exceeded(rt));
        }
        Ok(mut agent) => {
            let outcome = crate::looprun::Run::new(&mut agent, rt).run();
            let seq = agent.sequence;
            rt.flush_events();
            match outcome {
                Err(e) => fail(rt, &ids, seq, &e, deadline_exceeded(rt)),
                Ok(_) if deadline_exceeded(rt) => deadline_unknown(rt, &ids, seq),
                Ok(res) => {
                    let ty = gs(&res.result, "type").unwrap_or("").to_string();
                    let outcome = match ty.as_str() { "aborted" => "cancelled", "success" => "completed", _ => "failed" };
                    let module_failure = rt.module_failure(&op);
                    let first_error = arr(get(&res.result, "errors")).and_then(|e| e.first().cloned());
                    let terminal_error = match ty.as_str() {
                        "max_turns" => Some(first_error.unwrap_or_else(|| obj! {"code" => "agent_max_turns_reached",
                            "message" => "AgentLoop maximum turn limit reached."})),
                        "error" => first_error.or_else(|| module_failure.clone()),
                        _ => None,
                    };
                    let terminal_module_failure = if ty == "error" { module_failure } else { None };
                    rt.op_final(&op, outcome);
                    let code = terminal_error.as_ref().and_then(|e| gs(e, "code")).map(String::from);
                    let payload = obj! {"result" => res.result, "messages" => JsValue::Arr(res.messages),
                        "moduleFailure" => terminal_module_failure, "seedState" => seed_state(&agent),
                        "modelState" => model_state(&agent)};
                    final_event(rt, &ids, seq, "final", &format!("agent.execute.{outcome}"), outcome, code,
                        terminal_error.unwrap_or(JsValue::Undefined), payload);
                }
            }
        }
    }
    rt.active = None;
    rt.clear_module_failure(&op);
}

fn fail(rt: &mut Rt, ids: &Ids, seq: f64, e: &LoopErr, deadline_exceeded: bool) {
    if deadline_exceeded {
        return deadline_unknown(rt, ids, seq);
    }
    let serialized = serialize_error(e);
    if e.code.as_deref() == Some("RESULT_UNKNOWN") {
        return result_unknown(rt, ids, seq, serialized);
    }
    let outcome = if rt.aborted() { "cancelled" } else { "failed" };
    rt.op_final(&ids.op, outcome);
    final_event(rt, ids, seq, "failed", &format!("agent.execute.{outcome}"), outcome, e.code.clone(),
        serialized.clone(), obj! {"error" => serialized});
}
