//! Differential test against the TS oracle fixture (gen_tui_core_fixture.mjs).
use pilotdeck_tui_core::{reducer, render, sidebar, summary, truncate, width};
use serde_json::{json, Value};

fn fx() -> Value {
    serde_json::from_str(include_str!("fixtures/tui_core.json")).unwrap()
}

#[test]
fn graphemes_match_intl_segmenter() {
    for c in fx()["graphemes"].as_array().unwrap() {
        let got: Vec<&str> = width::graphemes(c["input"].as_str().unwrap());
        let want: Vec<&str> = c["segments"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(got, want, "{:?}", c["input"]);
    }
}

#[test]
fn reducer_steps() {
    for c in fx()["reducer"].as_array().unwrap() {
        let mut st = reducer::ReducerState::default();
        for (e, want) in c["events"].as_array().unwrap().iter().zip(c["steps"].as_array().unwrap()) {
            st = reducer::apply(&st, e);
            assert_eq!(&serde_json::to_value(&st).unwrap(), want, "event {e}");
        }
    }
}

#[test]
fn render_state() {
    for c in fx()["render"].as_array().unwrap() {
        let mut st = render::TuiRenderState::default();
        for e in c["events"].as_array().unwrap() {
            render::apply_tui_event(&mut st, e);
        }
        assert_eq!(serde_json::to_value(&st).unwrap(), c["state"]);
    }
}

#[test]
fn tool_summary() {
    for c in fx()["summary"].as_array().unwrap() {
        let a = c["args"].as_array().unwrap();
        let got = summary::format_tool_summary(a[0].as_str().unwrap(), a[1].as_str(), a[2].as_i64().unwrap(), a[3].as_i64().unwrap(), a[4].as_bool().unwrap(), a[5].as_str().unwrap());
        assert_eq!(got, c["result"].as_str().unwrap(), "{a:?}");
    }
}

#[test]
fn grouping() {
    for c in fx()["group"].as_array().unwrap() {
        let msgs: Vec<reducer::TuiMessage> = serde_json::from_value(c["messages"].clone()).unwrap();
        assert_eq!(serde_json::to_value(summary::group_consecutive_tools(&msgs)).unwrap(), c["result"]);
    }
}

#[test]
fn sidebar_helpers() {
    for c in fx()["sidebar"].as_array().unwrap() {
        let sessions = c["sessions"].as_array().unwrap().clone();
        let groups = sidebar::group_sessions(&sessions, c["by"].as_str().unwrap());
        assert_eq!(serde_json::to_value(&groups).unwrap(), c["groups"]);
        let col = sidebar::compute_smart_collapse(&groups, c["active"].as_str().unwrap());
        let mut want: Vec<String> = serde_json::from_value(c["collapsed"].clone()).unwrap();
        want.sort();
        assert_eq!(col.iter().cloned().collect::<Vec<_>>(), want);
        assert_eq!(serde_json::to_value(sidebar::flatten_sidebar_rows(&groups, &col)).unwrap(), c["rows"]);
        let icons: Vec<&str> = sessions.iter().map(sidebar::session_status_icon).collect();
        assert_eq!(json!(icons), c["icons"]);
        let titles: Vec<String> = sessions.iter().zip(c["maxLens"].as_array().unwrap()).map(|(s, m)| sidebar::session_display_title(s, m.as_u64().unwrap() as usize)).collect();
        assert_eq!(json!(titles), c["titles"]);
    }
}

#[test]
fn truncation() {
    for c in fx()["truncate"].as_array().unwrap() {
        let (i, w) = (c["input"].as_str().unwrap(), c["width"].as_i64().unwrap());
        assert_eq!(truncate::truncate_for_display(i, w), c["result"].as_str().unwrap(), "{i:?} @ {w}");
        assert_eq!(truncate::count_visual_lines(i, w) as u64, c["count"].as_u64().unwrap(), "{i:?} @ {w}");
    }
}

#[test]
fn relative_time_and_session_key() {
    let f = fx();
    for c in f["relative"].as_array().unwrap() {
        assert_eq!(sidebar::relative_time(c["now"].as_f64().unwrap(), c["ts"].as_f64().unwrap()), c["result"].as_str().unwrap());
    }
    for c in f["sessionKey"].as_array().unwrap() {
        assert_eq!(pilotdeck_tui_core::default_tui_session_key(c["projectKey"].as_str().unwrap()), c["result"].as_str().unwrap());
    }
}
