//! PilotDeck web UI (Leptos CSR). Slice 1: workspace file-mention composer behaviour +
//! in-browser self-test that runs the shared TS-derived vectors through the same Rust code.
use leptos::prelude::*;
use pilotdeck_web_ui_core::{date_utils, ime, office_preview, vectors, workspace_file_mention as wfm};

const BATCH2: &str = include_str!("../../../crates/web-ui-core/tests/fixtures/batch2.json");
const FIXTURE: &str = include_str!("../../../crates/web-ui-core/tests/fixtures/workspace_file_mention.json");

#[component]
fn App() -> impl IntoView {
    let (text, set_text) = signal(String::new());
    let (status, set_status) = signal(String::new());
    let mention = move |rel: &'static str| {
        let cur = text.with(|t| t.encode_utf16().count() as i64);
        let r = wfm::insert_workspace_file_mention(&text.get(), rel, cur);
        set_status.set(if r.already_present { "already present".into() } else { String::new() });
        set_text.set(r.input);
    };
    let (n1, bad1) = vectors::run_workspace_file_mention(FIXTURE);
    // Batch 2 runs with the browser's own Date.parse, which is checked against the value TS recorded.
    let parse = |s: &str| js_sys::Date::parse(s);
    let (n2, bad2) = vectors::run_batch2(BATCH2, Some(&parse));
    let n = n1 + n2;
    let bad: Vec<String> = bad1.into_iter().chain(bad2).collect();
    let (sent, set_sent) = signal(Vec::<String>::new());
    let (service, set_service) = signal(String::from("builtin"));
    let since = js_sys::Date::now() - 3.0 * 3_600_000.0;
    view! {
        <textarea id="composer" prop:value=move || text.get() on:input=move |e| set_text.set(event_target_value(&e))></textarea>
        <button id="mention" on:click=move |_| mention("src/a.ts")>"@src/a.ts"</button>
        <span id="status">{move || status.get()}</span>
        <textarea id="ime-composer" on:keydown=move |e: leptos::ev::KeyboardEvent| {
            // Same rule as the React composer: Enter submits unless an IME composition is active.
            let ev = serde_json::json!({"key": e.key(), "keyCode": e.key_code(), "which": e.which(), "isComposing": e.is_composing()});
            if e.key() == "Enter" && !ime::is_ime_enter_event(&ev) {
                e.prevent_default();
                let v = event_target_value(&e);
                set_sent.update(|s| s.push(v));
            }
        }></textarea>
        <ul id="sent">{move || sent.get().into_iter().map(|m| view! { <li>{m}</li> }).collect::<Vec<_>>()}</ul>
        <span id="time-ago">{move || match date_utils::format_time_ago(since, js_sys::Date::now()) {
            t @ date_utils::TimeAgo::Key(..) => date_utils::english(&t).unwrap_or_default(),
            date_utils::TimeAgo::LocaleDate(ms) => String::from(js_sys::Date::new(&ms.into()).to_locale_date_string("default", &js_sys::Object::new())),
        }}</span>
        <input id="office-service" on:input=move |e| set_service.set(event_target_value(&e)) />
        <span id="office-normalized">{move || office_preview::normalize_office_preview_service(Some(&serde_json::Value::String(service.get())))}</span>
        <pre id="selftest">{format!("{{\"checked\":{n},\"mismatches\":{}}}", bad.len())}</pre>
        <pre id="selftest-detail">{bad.into_iter().take(5).collect::<Vec<_>>().join("\n")}</pre>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
