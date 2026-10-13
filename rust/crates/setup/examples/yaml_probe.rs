fn main() {
    for line in std::io::stdin().lines() {
        let v: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let js = pilotdeck_gateway_client_free::conv(&v);
        println!("{}", serde_json::to_string(&pilotdeck_setup::yamlfmt::stringify(&js)).unwrap());
    }
}
mod pilotdeck_gateway_client_free {
    use pilotdeck_skills::jsjson::{JsObject, JsValue};
    pub fn conv(v: &serde_json::Value) -> JsValue {
        match v {
            serde_json::Value::Null => JsValue::Null,
            serde_json::Value::Bool(b) => JsValue::Bool(*b),
            serde_json::Value::Number(n) => JsValue::Num(n.as_f64().unwrap()),
            serde_json::Value::String(s) => JsValue::Str(s.clone()),
            serde_json::Value::Array(a) => JsValue::Arr(a.iter().map(conv).collect()),
            serde_json::Value::Object(m) => {
                let mut o = JsObject::new();
                for (k, v) in m { o.set(k.clone(), conv(v)); }
                JsValue::Obj(o)
            }
        }
    }
}
