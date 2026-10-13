//! Port of `agent/stream/TurnTimeline.ts`.
use crate::json::*;
use crate::obj;
use std::collections::HashMap;

pub struct Timeline {
    turn_id: String,
    last_model_position: Option<JsValue>,
    previous_id: Option<String>,
    next_order: f64,
    revision: f64,
    positions: Vec<(String, JsValue)>,
    provisional: Vec<String>,
    offsets: HashMap<String, f64>,
}

fn utf16_len(s: &str) -> f64 {
    s.encode_utf16().count() as f64
}

impl Timeline {
    pub fn new(turn_id: &str) -> Self {
        Timeline {
            turn_id: turn_id.to_string(),
            last_model_position: None,
            previous_id: None,
            next_order: 0.0,
            revision: 0.0,
            positions: Vec::new(),
            provisional: Vec::new(),
            offsets: HashMap::new(),
        }
    }
    fn get(&self, id: &str) -> Option<&JsValue> {
        self.positions.iter().find(|(k, _)| k == id).map(|(_, v)| v)
    }
    fn put(&mut self, id: &str, v: JsValue) {
        if let Some(slot) = self.positions.iter_mut().find(|(k, _)| k == id) {
            slot.1 = v;
        } else {
            self.positions.push((id.to_string(), v));
        }
    }

    pub fn position(&mut self, id: &str, provisional: bool) -> JsValue {
        let mut position = match self.get(id) {
            Some(p) => p.clone(),
            None => {
                let order = self.next_order;
                self.next_order += 1.0;
                self.revision += 1.0;
                let p = obj! {"version" => 1.0, "turnId" => &self.turn_id, "id" => id,
                    "previousId" => self.previous_id.clone(), "order" => order, "revision" => self.revision};
                self.put(id, p.clone());
                if provisional {
                    if !self.provisional.iter().any(|x| x == id) {
                        self.provisional.push(id.to_string());
                    }
                } else {
                    self.previous_id = Some(id.to_string());
                }
                p
            }
        };
        if !provisional {
            if let Some(i) = self.provisional.iter().position(|x| x == id) {
                self.provisional.remove(i);
                let my_order = gn(&position, "order").unwrap_or(0.0);
                let prev = self
                    .positions
                    .iter()
                    .filter(|(k, v)| gn(v, "order").unwrap_or(0.0) < my_order && !self.provisional.iter().any(|p| p == k))
                    .max_by(|a, b| gn(&a.1, "order").unwrap_or(0.0).partial_cmp(&gn(&b.1, "order").unwrap_or(0.0)).unwrap())
                    .map(|(k, _)| k.clone());
                // `{ ...position, previousId: previous?.id }` keeps the key (possibly undefined).
                let mut next = JsObject::new();
                if let JsValue::Obj(o) = &position {
                    for (k, v) in o.iter() {
                        next.set(k.clone(), v.clone());
                    }
                }
                next.set("previousId", prev.into_js());
                position = JsValue::Obj(next);
                self.put(id, position.clone());
                let advance = match &self.previous_id {
                    None => true,
                    Some(p) => self.get(p).and_then(|v| gn(v, "order")).unwrap_or(0.0) < my_order,
                };
                if advance {
                    self.previous_id = Some(id.to_string());
                }
            }
        }
        position
    }

    pub fn update(&mut self, id: &str) -> JsValue {
        let mut next = self.position(id, false);
        self.revision += 1.0;
        set(&mut next, "revision", self.revision);
        self.put(id, next.clone());
        next
    }

    /// `timeline.message(message)`: annotate durable blocks in place.
    pub fn message(&mut self, message: &mut JsValue) {
        let queue_item = gs(get(message, "metadata"), "queueItemId").map(|s| s.to_string());
        let JsValue::Obj(m) = message else { return };
        let Some(JsValue::Arr(content)) = m.get_mut("content") else { return };
        for block in content.iter_mut() {
            let id = match gs(block, "type") {
                Some("text") | Some("thinking") => gs(block, "blockId").map(|s| s.to_string()),
                Some("tool_call") => Some(format!("tool:{}", js_str(get(block, "id")))),
                Some("tool_result") | Some("tool_result_reference") => {
                    Some(format!("result:{}", js_str(get(block, "toolCallId"))))
                }
                _ => None,
            };
            let has = matches!(get(block, "timeline"), JsValue::Undefined | JsValue::Null) == false;
            if let Some(id) = id {
                if !has {
                    let p = self.update(&id);
                    set(block, "timeline", p);
                }
            } else if let Some(q) = &queue_item {
                if !has {
                    let p = self.position(&format!("steer:{q}"), false);
                    set(block, "timeline", p);
                }
            }
        }
    }

    /// `timeline.event(event)`; message-bearing events must already be annotated
    /// by the caller (which owns the shared message object).
    pub fn event(&mut self, mut event: JsValue) -> JsValue {
        let t = gs(&event, "type").unwrap_or("").to_string();
        if t == "model_request_started" {
            for id in std::mem::take(&mut self.provisional) {
                self.positions.retain(|(k, _)| *k != id);
            }
        }
        if t == "model_event" {
            let inner = get(&event, "event").clone();
            match gs(&inner, "type") {
                Some("tool_call_start") => {
                    self.position(&format!("tool:{}", js_str(get(&inner, "id"))), true);
                }
                Some("tool_call_end") => {
                    self.position(&format!("tool:{}", js_str(get(get(&inner, "toolCall"), "id"))), true);
                }
                _ => {}
            }
            let it = gs(&inner, "type").unwrap_or("").to_string();
            if let Some(block_id) = gs(&event, "blockId").map(|s| s.to_string()) {
                if it == "text_delta" || it == "thinking_delta" {
                    let offset = *self.offsets.get(&block_id).unwrap_or(&0.0);
                    let text = gs(&inner, "text").unwrap_or("");
                    self.offsets.insert(block_id.clone(), offset + utf16_len(text));
                    let pos = self.update(&block_id);
                    self.last_model_position = Some(pos.clone());
                    let tl = spread(&pos, obj! {"offset" => offset});
                    set(&mut event, "timeline", tl);
                    return event;
                }
            }
            if let Some(last) = &self.last_model_position {
                if it == "message_end" || it == "tool_call_start" || it == "error" {
                    let through = gn(last, "order").unwrap_or(0.0);
                    self.revision += 1.0;
                    let b = obj! {"turnId" => &self.turn_id, "through" => through, "revision" => self.revision};
                    set(&mut event, "streamBoundary", b);
                    return event;
                }
            }
        }
        if t == "assistant_message" || t == "tool_results_projected" || t == "steer_applied" {
            if let JsValue::Obj(o) = &mut event {
                if let Some(m) = o.get_mut("message") {
                    self.message(m);
                }
            }
        }
        if t == "tool_calls_detected" {
            if let JsValue::Obj(o) = &mut event {
                if let Some(JsValue::Arr(calls)) = o.get_mut("calls") {
                    for call in calls.iter_mut() {
                        let id = format!("tool:{}", js_str(get(call, "id")));
                        let p = self.position(&id, false);
                        set(call, "timeline", p);
                    }
                }
            }
        }
        if t == "compact_started" || t == "compact_completed" {
            let id = format!("compact:{}", js_str(get(&event, "compactionId")));
            let p = self.update(&id);
            set(&mut event, "timeline", p);
            return event;
        }
        if t == "tool_result" {
            let id = format!("result:{}", js_str(get(get(&event, "result"), "toolCallId")));
            let p = self.position(&id, false);
            set(&mut event, "timeline", p);
            return event;
        }
        event
    }
}

/// Template-literal interpolation `${value}`.
pub fn js_str(v: &JsValue) -> String {
    crate::rt::js_string(v)
}
