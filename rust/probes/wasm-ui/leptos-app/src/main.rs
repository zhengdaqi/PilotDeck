use leptos::prelude::*;

#[component]
fn App() -> impl IntoView {
    let (by, set_by) = signal("project");
    view! {
        <button id="toggle" on:click=move |_| set_by.update(|b| *b = probe_shared::toggle(b))>"group: " {move || by.get()}</button>
        <ul id="groups">
            {move || probe_shared::rows(by.get()).into_iter().map(|(l, n)| view! { <li>{format!("{l} ({n})")}</li> }).collect::<Vec<_>>()}
        </ul>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
