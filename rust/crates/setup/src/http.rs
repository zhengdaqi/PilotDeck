//! Minimal `fetch()` equivalent: env proxy like `installGlobalProxy`, undici-style error text.

use std::time::Duration;

pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// `fetch failed` is what undici reports for any network-level failure.
pub const FETCH_FAILED: &str = "fetch failed";

fn proxy_for(url: &str) -> Option<ureq::Proxy> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let proxy = env("PILOTDECK_PROXY").or_else(|| env("https_proxy")).or_else(|| env("HTTPS_PROXY"))
        .or_else(|| env("http_proxy")).or_else(|| env("HTTP_PROXY"))?;
    let host = url.split("://").nth(1)?.split(['/', ':', '?']).next()?.to_ascii_lowercase();
    let mut no_proxy: Vec<String> = vec!["127.0.0.1".into(), "localhost".into()];
    if let Some(n) = env("NO_PROXY").or_else(|| env("no_proxy")) {
        no_proxy.extend(n.split([',', ' ']).filter(|s| !s.is_empty()).map(|s| s.trim().to_ascii_lowercase()));
    }
    for n in &no_proxy {
        if n == "*" {
            return None;
        }
        let n = n.trim_start_matches('*').trim_start_matches('.');
        let n = n.split(':').next().unwrap_or(n);
        if host == n || host.ends_with(&format!(".{n}")) {
            return None;
        }
    }
    ureq::Proxy::new(proxy).ok()
}

fn agent(url: &str, timeout: Option<Duration>) -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new();
    if let Some(p) = proxy_for(url) {
        b = b.proxy(p);
    }
    if let Some(t) = timeout {
        b = b.timeout(t);
    }
    b.build()
}

fn finish(r: Result<ureq::Response, ureq::Error>) -> Result<Response, String> {
    let resp = match r {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(t)) => {
            if t.kind() == ureq::ErrorKind::Io && t.to_string().contains("timed out") {
                return Err("__timeout__".into());
            }
            return Err(FETCH_FAILED.into());
        }
    };
    let status = resp.status();
    let body = resp.into_string().map_err(|_| FETCH_FAILED.to_string())?;
    Ok(Response { status, body })
}

pub fn get(url: &str, headers: &[(&str, &str)], timeout: Option<Duration>) -> Result<Response, String> {
    let mut req = agent(url, timeout).get(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    finish(req.call())
}

pub fn post_json(url: &str, headers: &[(&str, &str)], body: &str) -> Result<Response, String> {
    let mut req = agent(url, None).post(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    finish(req.send_string(body))
}

/// `await res.json()` → V8 SyntaxError message on invalid JSON.
pub fn json(r: &Response) -> Result<pilotdeck_skills::jsjson::JsValue, String> {
    pilotdeck_skills::v8json::parse(&r.body)
}
