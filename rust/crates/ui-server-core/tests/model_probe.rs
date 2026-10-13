//! Fixture: rust/parity/ui_server/gen_model_probe.mjs (real TS/JS under node).
use base64::Engine;
use pilotdeck_ui_server_core::model_probe::*;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::future::Future;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/model_probe.json")).unwrap()
}
fn dec(v: &Value) -> Option<&Value> {
    if v.get("$undef").is_some() { None } else { Some(v) }
}
fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}
fn js_str(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(v) => pilotdeck_ui_server_core::js_object_string(v),
    }
}
fn res<T: Into<Value>>(r: Result<T, String>) -> Value {
    match r {
        Ok(v) => json!({ "v": v.into() }),
        Err(e) => json!({ "throw": format!("TypeError: {e}") }),
    }
}
fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

#[test]
fn endpoints_and_shapes() {
    let fx = fixture();
    for c in fx["endpoints"].as_array().unwrap() {
        let i = &c["input"];
        let input = EndpointInput { protocol: s(i, "protocol").unwrap(), base_url: s(i, "baseUrl").unwrap(), model: s(i, "model"), google_method: s(i, "googleMethod") };
        assert_eq!(json!(build_provider_chat_endpoint_candidates(&input)), c["chat"], "{i}");
        assert_eq!(json!(build_provider_chat_endpoint(&input)), c["chatOne"], "{i}");
        assert_eq!(json!(build_provider_models_endpoint_candidates(input.protocol, input.base_url)), c["models"], "{i}");
        assert_eq!(json!(build_provider_models_endpoint(input.protocol, input.base_url)), c["modelsOne"], "{i}");
    }
    for c in fx["google"].as_array().unwrap() {
        assert_eq!(json!(normalize_google_probe_model(dec(&c["m"]).and_then(Value::as_str))), c["out"]);
    }
    for c in fx["shapes"].as_array().unwrap() {
        let (p, b) = (c["p"].as_str().unwrap(), dec(&c["b"]));
        assert_eq!(json!(is_expected_provider_response_shape(p, b)), c["chat"], "{c}");
        assert_eq!(json!(is_expected_provider_models_response_shape(p, b)), c["models"], "{c}");
    }
    for c in fx["network"].as_array().unwrap() {
        let e = &c["e"];
        let f = if e.get("str").is_some() {
            FetchFailure::Other(Some(e["str"].clone()))
        } else {
            FetchFailure::Error { message: e["message"].as_str().unwrap().into(), code: e.get("code").cloned(), cause: e.get("cause").cloned() }
        };
        let n = normalize_network_error(&f);
        assert_eq!((json!(n.code), json!(n.message)), (c["code"].clone(), c["message"].clone()), "{e}");
    }
}

#[test]
fn helpers() {
    let fx = fixture();
    let mut n = 0;
    for c in fx["helpers"].as_array().unwrap() {
        if let Some(p) = s(c, "p") {
            let b = dec(&c["b"]);
            assert_eq!(res(has_error_finish(b, p)), c["err"], "err {c}");
            assert_eq!(res(extract_probe_text(b, p)), c["text"], "text {c}");
            assert_eq!(res(is_incomplete(b, p)), c["inc"], "inc {c}");
        } else if let Some(t) = c.get("strip") {
            assert_eq!(json!(strip_thinking_value(dec(t))), c["out"], "{c}");
        } else if let Some(d) = c.get("d") {
            let ds = js_str(dec(d));
            let st = dec(&c["s"]).and_then(Value::as_u64).map(|x| x as u16);
            assert_eq!(json!(looks_like_image_unsupported(&ds)), c["img"], "{c}");
            assert_eq!(json!(classify_probe_error(&ds, st)), c["cls"], "{c}");
        } else if let Some(t) = s(c, "rt") {
            assert_eq!(response_detail(t, 418, "I'm a teapot"), c["detail"], "{c}");
        } else {
            assert_eq!(json!(is_valid_image_color_answer(dec(&c["ans"]), dec(&c["col"]))), c["ok"], "{c}");
        }
        n += 1;
    }
    assert_eq!(n, 2723);
}

#[test]
fn png_and_requests() {
    let fx = fixture();
    let png = &fx["png"];
    let probe = create_image_capability_probe(0);
    assert_eq!(json!(probe.color), png["color"]);
    assert_eq!(json!(probe.prompt), png["prompt"]);
    let b64 = base64::engine::general_purpose::STANDARD;
    let bytes = b64.decode(&probe.data).unwrap();
    assert_eq!(b64.encode(&bytes[..33]), png["header"].as_str().unwrap());
    assert_eq!(b64.encode(&bytes[bytes.len() - 12..]), png["iend"].as_str().unwrap());
    let idat_len = u32::from_be_bytes(bytes[33..37].try_into().unwrap()) as usize;
    assert_eq!(&bytes[37..41], b"IDAT");
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&bytes[41..41 + idat_len]).unwrap();
    assert_eq!(b64.encode(raw), png["raw"].as_str().unwrap());
    // chunk CRC over type+data
    let crc = u32::from_be_bytes(bytes[41 + idat_len..45 + idat_len].try_into().unwrap());
    let mut h = 0xffff_ffffu32;
    for &b in &bytes[37..41 + idat_len] {
        h ^= b as u32;
        for _ in 0..8 { h = (h >> 1) ^ (0xedb8_8320 & (h & 1).wrapping_neg()); }
    }
    assert_eq!(crc, !h);
    let placeholder = ImageProbe { data: "<png>".into(), ..probe };
    for c in fx["requests"].as_array().unwrap() {
        let i = &c["input"];
        let r = request_for(s(i, "protocol").unwrap(), s(i, "apiKey"), dec(&i["model"]).and_then(Value::as_str), i["image"].as_bool().unwrap(), i.get("maxTokens").and_then(dec), i["imageProbe"].is_string().then_some(&placeholder));
        let got = match r {
            Ok(r) => json!({ "v": { "headers": r.headers, "body": r.body } }),
            Err(e) => json!({ "throw": format!("TypeError: {e}") }),
        };
        assert_eq!(got, c["out"], "{i}");
    }
}

#[test]
fn probes() {
    let fx = fixture();
    let probe = create_image_capability_probe(0);
    for c in fx["probes"].as_array().unwrap() {
        let i = &c["input"];
        let input = ProbeInput {
            protocol: s(i, "protocol").unwrap(),
            base_url: s(i, "baseUrl").unwrap(),
            endpoint_url: s(i, "endpointUrl"),
            api_key: s(i, "apiKey"),
            model: dec(&i["model"]).and_then(Value::as_str),
            image: i["image"].as_bool().unwrap(),
            max_tokens: i.get("maxTokens").cloned(),
        };
        let script = c["script"].as_array().unwrap().clone();
        let log = RefCell::new(Vec::<String>::new());
        let mut k = 0;
        let data = probe.data.clone();
        let out = block_on(probe_model_connection(&input, Some(probe.clone()), |call: FetchCall| {
            log.borrow_mut().push(format!("fetch {} {} {}", call.url, Value::Object(call.headers), call.body.replace(&data, "<png>")));
            let step = script.get(k).cloned();
            k += 1;
            async move {
                match step {
                    None => Ok(FetchResponse { status: 404, status_text: "Not Found".into(), text: String::new() }),
                    Some(st) if st.get("throw").is_some() => {
                        let e = &st["throw"];
                        Err(FetchFailure::Error { message: e["message"].as_str().unwrap_or("").into(), code: e.get("code").cloned(), cause: e.get("cause").cloned() })
                    }
                    Some(st) => {
                        let status = st["status"].as_u64().unwrap() as u16;
                        let text = match status { 200 => "OK", 400 => "Bad Request", 401 => "Unauthorized", 404 => "Not Found", 405 => "Method Not Allowed", 429 => "Too Many Requests", _ => "Internal Server Error" };
                        Ok(FetchResponse { status, status_text: text.into(), text: st["text"].as_str().unwrap().into() })
                    }
                }
            }
        }));
        assert_eq!(json!(log.into_inner()), c["log"], "log {i} {}", json!(script));
        assert_eq!(json!({ "v": out }), c["result"], "{i} {}", json!(script));
    }
}
