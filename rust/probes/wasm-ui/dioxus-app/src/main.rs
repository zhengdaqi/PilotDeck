use dioxus::prelude::*;

fn app() -> Element {
    let mut by = use_signal(|| "project");
    rsx! {
        button { id: "toggle", onclick: move |_| { let n = probe_shared::toggle(by()); by.set(n) }, "group: {by}" }
        ul { id: "groups",
            for (l, n) in probe_shared::rows(by()) { li { "{l} ({n})" } }
        }
    }
}

fn main() {
    dioxus::launch(app);
}
