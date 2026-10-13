use yew::prelude::*;

#[function_component(App)]
fn app() -> Html {
    let by = use_state(|| "project");
    let onclick = {
        let by = by.clone();
        Callback::from(move |_| by.set(probe_shared::toggle(*by)))
    };
    html! {
        <>
            <button id="toggle" {onclick}>{format!("group: {}", *by)}</button>
            <ul id="groups">
                { for probe_shared::rows(*by).into_iter().map(|(l, n)| html! { <li>{format!("{l} ({n})")}</li> }) }
            </ul>
        </>
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
