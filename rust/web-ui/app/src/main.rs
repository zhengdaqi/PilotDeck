//! PilotDeck web UI (Leptos CSR). Slice 1: workspace file-mention composer behaviour +
//! in-browser self-test that runs the shared TS-derived vectors through the same Rust code.
use leptos::prelude::*;
use pilotdeck_web_ui_core::{vectors, workspace_file_mention as wfm};

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
    let (n, bad) = vectors::run_workspace_file_mention(FIXTURE);
    view! {
        <textarea id="composer" prop:value=move || text.get() on:input=move |e| set_text.set(event_target_value(&e))></textarea>
        <button id="mention" on:click=move |_| mention("src/a.ts")>"@src/a.ts"</button>
        <span id="status">{move || status.get()}</span>
        <pre id="selftest">{format!("{{\"checked\":{n},\"mismatches\":{}}}", bad.len())}</pre>
        <pre id="selftest-detail">{bad.into_iter().take(5).collect::<Vec<_>>().join("\n")}</pre>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
