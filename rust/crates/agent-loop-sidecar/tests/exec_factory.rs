//! Replays the factory prologue of the recorded TS `pure_text` execute (plan/todo read,
//! model metadata) against the native runtime and checks call order and payloads.
use pilotdeck_agent_loop_sidecar::json::*;
use pilotdeck_agent_loop_sidecar::rt::{Incoming, Rt};
use std::io::Write;
use std::sync::mpsc::{channel, Sender};

struct ChanWriter(Sender<String>, Vec<u8>);
impl Write for ChanWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.1.extend_from_slice(b);
        while let Some(i) = self.1.iter().position(|c| *c == b'\n') {
            let line: Vec<u8> = self.1.drain(..=i).collect();
            let _ = self.0.send(String::from_utf8_lossy(&line[..line.len() - 1]).to_string());
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn parse(s: &str) -> JsValue {
    pilotdeck_skills::v8json::parse(s).expect("json")
}

#[test]
fn factory_prologue_matches_ts_recording() {
    let fx = parse(include_str!("fixtures/pure_text_factory.json"));
    let (in_tx, in_rx) = channel();
    let (out_tx, out_rx) = channel();
    let handle = std::thread::spawn(move || Rt::new(in_rx, Box::new(ChanWriter(out_tx, Vec::new()))).serve());
    in_tx.send(Incoming::Line(to_json(get(&fx, "execute")))).unwrap();
    let accepted = parse(&out_rx.recv().unwrap());
    assert_eq!(gs(&accepted, "kind"), Some("response"));
    assert_eq!(gb(&accepted, "ok"), Some(true));
    let expected = ga(&fx, "expectedCalls").to_vec();
    let answers = [get(&fx, "planTodo").clone(), get(&fx, "metadata").clone()];
    for (exp, answer) in expected.iter().zip(answers) {
        let call = parse(&out_rx.recv().unwrap());
        assert_eq!(gs(&call, "method"), Some("module_call"));
        assert_eq!(get(&call, "module"), get(exp, "module"));
        assert_eq!(to_json(get(&call, "payload")), to_json(get(exp, "payload")));
        let rid = gs(&call, "requestId").unwrap();
        let erid = gs(exp, "requestId").unwrap();
        if erid.starts_with("plan-todo-") {
            assert!(rid.starts_with("plan-todo-") && rid.ends_with("-1"), "{rid}");
        } else {
            assert_eq!(rid, erid);
        }
        let mut answer = answer.clone();
        set(&mut answer, "inReplyTo", gs(&call, "messageId").unwrap());
        set(&mut answer, "requestId", rid);
        in_tx.send(Incoming::Line(to_json(&answer))).unwrap();
    }
    // The AgentLoop body is not ported yet: the operation must fail loudly, never fake success.
    let fin = parse(&out_rx.recv().unwrap());
    assert_eq!(gs(&fin, "outcome"), Some("failed"));
    assert_eq!(gb(&fin, "final"), Some(true));
    in_tx.send(Incoming::Eof).unwrap();
    assert_eq!(handle.join().unwrap(), 0);
}
