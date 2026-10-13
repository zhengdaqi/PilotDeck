//! Success paths of gateway setup network flows against a local mock HTTP server.
use pilotdeck_setup::gateway_setup::{login_with_qr, qr_scan_for_wecom_bot_info, test_feishu_credentials};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Serves `routes` (path prefix → queue of (status, body)); records request lines + bodies.
fn mock(routes: Vec<(&'static str, Vec<(u16, &'static str)>)>) -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log = Arc::new(Mutex::new(Vec::new()));
    let lg = log.clone();
    let routes = Arc::new(Mutex::new(routes));
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h.trim().is_empty() { break; }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") { len = v.trim().parse().unwrap(); }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let path = line.split(' ').nth(1).unwrap_or("").to_string();
            lg.lock().unwrap().push(format!("{} {}", line.trim(), String::from_utf8_lossy(&body)));
            let mut rs = routes.lock().unwrap();
            let (st, b) = rs.iter_mut().find(|(p, _)| path.starts_with(p))
                .map(|(_, q)| if q.len() > 1 { q.remove(0) } else { q[0] }).unwrap_or((404, "{}"));
            let mut s = s;
            let _ = write!(s, "HTTP/1.1 {st} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len());
        }
    });
    (base, log)
}

#[test]
fn feishu_token_ok_and_error() {
    let (b, log) = mock(vec![("/ok", vec![(200, r#"{"code":0,"tenant_access_token":"t"}"#)]), ("/bad", vec![(200, r#"{"code":10003,"msg":"invalid"}"#)]), ("/null", vec![(200, "null")])]);
    assert_eq!(test_feishu_credentials("cli_a", "s\"x", &format!("{b}/ok")), Ok(()));
    assert!(log.lock().unwrap()[0].ends_with(r#"{"app_id":"cli_a","app_secret":"s\"x"}"#));
    assert_eq!(test_feishu_credentials("a", "b", &format!("{b}/bad")), Err("code=10003 msg=invalid".into()));
    assert_eq!(test_feishu_credentials("a", "b", &format!("{b}/null")), Err("Cannot read properties of null (reading 'code')".into()));
}

#[test]
fn wecom_qr_success_after_wait() {
    let (b, log) = mock(vec![
        ("/gen", vec![(200, r#"{"data":{"scode":"a b","auth_url":"https://auth/x"}}"#)]),
        ("/q", vec![(200, r#"{"data":{"status":"wait"}}"#), (200, r#"{"data":{"status":"SUCCESS","bot_info":{"botid":" B1 ","secret":"S1"}}}"#)]),
    ]);
    let mut out = Vec::new();
    let r = qr_scan_for_wecom_bot_info(&format!("{b}/gen"), &format!("{b}/q"), Duration::from_secs(10), Duration::from_millis(10), &mut |m| out.push(m.to_string()));
    assert_eq!(r, Some(("B1".into(), "S1".into())));
    assert!(out.contains(&"https://auth/x".to_string()));
    assert!(log.lock().unwrap().iter().any(|l| l.starts_with("GET /q?scode=a%20b ")));
}

#[test]
fn wecom_qr_incomplete_creds() {
    let (b, _) = mock(vec![("/gen", vec![(200, r#"{"data":{"scode":"s","auth_url":"u"}}"#)]), ("/q", vec![(200, r#"{"data":{"status":"success","bot_info":{"botid":"B"}}}"#)])]);
    let mut out = Vec::new();
    assert_eq!(qr_scan_for_wecom_bot_info(&format!("{b}/gen"), &format!("{b}/q"), Duration::from_secs(5), Duration::from_millis(10), &mut |m| out.push(m.to_string())), None);
    assert_eq!(out.last().unwrap(), "扫码成功但未返回完整 Bot 凭据。");
}

#[test]
fn weixin_login_scan_expire_confirm() {
    let (b, _) = mock(vec![
        ("/ilink/bot/get_bot_qrcode", vec![(200, r#"{"qrcode":"q1","qrcode_img_content":"img1"}"#), (200, r#"{"qrcode":"q2","qrcode_img_content":"img2"}"#)]),
        ("/ilink/bot/get_qrcode_status", vec![(200, r#"{"status":"scaned"}"#), (200, r#"{"status":"expired"}"#), (200, r#"{"status":"confirmed","ilink_bot_id":"acc","bot_token":"tok"}"#)]),
    ]);
    let (mut qrs, mut st) = (Vec::new(), Vec::new());
    let c = login_with_qr(&b, Duration::from_secs(10), Duration::from_millis(10), &mut |q| qrs.push(q.to_string()), &mut |s| st.push(s.to_string())).unwrap();
    assert_eq!((c.account_id.as_str(), c.bot_token.as_str(), c.base_url.as_str()), ("acc", "tok", b.as_str()));
    assert_eq!(qrs, ["img1", "img2"]);
    assert_eq!(st, ["waiting", "scanned", "refreshing", "waiting"]);
}

#[test]
fn weixin_login_missing_creds_and_http_error() {
    let (b, _) = mock(vec![("/ilink/bot/get_bot_qrcode", vec![(200, r#"{"qrcode":"q"}"#)]), ("/ilink/bot/get_qrcode_status", vec![(200, r#"{"status":"confirmed","bot_token":"t"}"#)])]);
    let e = login_with_qr(&b, Duration::from_secs(5), Duration::from_millis(10), &mut |_| {}, &mut |_| {}).err().unwrap();
    assert_eq!(e, "Login failed: server did not return required credentials");
    let (b2, _) = mock(vec![("/x", vec![(200, "{}")])]);
    let e = login_with_qr(&b2, Duration::from_secs(5), Duration::from_millis(10), &mut |_| {}, &mut |_| {}).err().unwrap();
    assert_eq!(e, "Failed to fetch QR code: 404");
}
