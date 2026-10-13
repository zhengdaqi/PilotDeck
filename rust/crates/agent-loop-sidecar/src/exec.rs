//! Execute handling for the native runtime (`Rt`). WORK IN PROGRESS: the AgentLoop
//! `runOrdered` port is not finished, so this only builds the factory state and then
//! fails the operation loudly. The shipped binary (`main.rs`) still uses the
//! control-plane-only `Server` and never reaches this code.
use crate::json::*;
use crate::obj;
use crate::rt::{next_id, Active, Rt};

pub fn handle_execute(rt: &mut Rt, request: JsValue) {
    let run_id = gs(&request, "runId").unwrap_or("").to_string();
    let op = gs(&request, "operationId").unwrap_or("").to_string();
    let request_id = gs(&request, "requestId").unwrap_or("").to_string();
    let stream_id = next_id("stream");
    rt.op_accept(&run_id, &op, &request_id);
    rt.active = Some(Active { run_id: run_id.clone(), operation_id: op.clone(), request_id: request_id.clone(),
        reason: None, deadline_exceeded: false, deadline_at: None });
    rt.send(&obj! {"kind" => "response", "messageId" => next_id("accepted"), "inReplyTo" => gs(&request, "messageId"),
        "requestId" => &request_id, "ok" => true, "streamId" => &stream_id, "cursor" => 0.0});
    let message = match crate::factory::create(rt, &request, &stream_id) {
        Ok(_) => "AgentLoop execution is not migrated to Rust yet.".to_string(),
        Err(e) => e.message,
    };
    rt.flush_events();
    rt.op_final(&op, "failed");
    rt.send(&obj! {"kind" => "event", "messageId" => next_id("event"), "eventType" => "failed", "streamId" => &stream_id,
        "sequence" => 0.0, "runId" => &run_id, "operationId" => &op, "requestId" => &request_id, "final" => true,
        "outcome" => "failed", "error" => obj! {"message" => message}});
    rt.active = None;
    rt.clear_module_failure(&op);
}
