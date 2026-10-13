//! Port of ui/server/services/webUpdateCommand.js (`describeUpdateReason`, `createCommandClient`,
//! `runWebUpdateCommand`). Sans-IO: the host performs HTTP (with a streaming body), sleeps, reads the clock,
//! generates the update id, resolves `realpath(projectRoot)`, signs the local Web credentials
//! (`localHeaders`: SQLite + JWT stay host-side) and builds the offline update service.
//! Errors keep the JS shape: `failure(reason, message)` vs plain errors; V8 TypeError texts for property reads
//! on null/undefined are reproduced; a JSON SyntaxError carries no contract text (`message: None`).
use crate::js_number::{js_number_to_string, js_trim, number};
use crate::js_object::{field, truthy};
use crate::js_object_string;
use crate::model_probe::FetchFailure;
use crate::network_fetch::js_slice_units;
use serde_json::{json, Map, Value};
use std::future::Future;

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateError {
    /// `Object.assign(new Error(message), { reason })`
    Failure { reason: Value, message: String },
    /// Any other thrown error: `name` and `message` (`None` for a JSON SyntaxError, whose text is V8's).
    Plain { name: String, message: Option<String> },
}

fn failure(reason: impl Into<Value>, message: impl Into<String>) -> UpdateError {
    UpdateError::Failure { reason: reason.into(), message: message.into() }
}

fn type_error(m: String) -> UpdateError {
    UpdateError::Plain { name: "TypeError".into(), message: Some(m) }
}

/// `new Error(v).message` for a value picked out of JSON (`undefined` → "").
fn error_message(v: Option<&Value>) -> String {
    v.map(js_object_string).unwrap_or_default()
}

/// `obj.prop` where reading from null/undefined throws.
fn read<'a>(obj: Option<&'a Value>, prop: &str) -> Result<Option<&'a Value>, UpdateError> {
    match obj {
        None => Err(type_error(format!("Cannot read properties of undefined (reading '{prop}')"))),
        Some(Value::Null) => Err(type_error(format!("Cannot read properties of null (reading '{prop}')"))),
        o => Ok(field(o, prop)),
    }
}

const REASONS: [(&str, &str); 13] = [
    ("development", "Developer branches/worktrees and development runtimes do not support self-update."),
    ("localChanges", "Local changes are present. Update this workspace manually."),
    ("container", "Container deployments must update their image manually."),
    ("desktop", "Use the desktop application settings to update this installation."),
    ("notGit", "Self-update requires a standard Git deployment."),
    ("unofficial", "This checkout does not use the official repository."),
    ("shallow", "Shallow Git checkouts do not support self-update."),
    ("unknownVersion", "The current build version cannot be identified."),
    ("runtimeMismatch", "The running version does not match the checkout/build. Restart it before checking again."),
    ("ahead", "This deployment is ahead of the latest Release. Automatic downgrades are disabled."),
    ("diverged", "This deployment has diverged from the Release. Update it manually."),
    ("lockBusy", "Another update or an unconfirmed cleanup holds the update lock."),
    ("restartRequired", "An update is already prepared. Restart the service first."),
];

/// `describeUpdateReason(reason)`; the `reasons` literal inherits Object.prototype, whose members stringify.
pub fn describe_update_reason(reason: &Value) -> String {
    let key = js_object_string(reason);
    let text = match REASONS.iter().find(|(k, _)| *k == key) {
        Some((_, t)) => t.to_string(),
        None => match key.as_str() {
            "constructor" => "function Object() { [native code] }".into(),
            "__proto__" => "[object Object]".into(),
            "__defineGetter__" | "__defineSetter__" | "hasOwnProperty" | "__lookupGetter__" | "__lookupSetter__" | "isPrototypeOf" | "propertyIsEnumerable" | "toString" | "valueOf" | "toLocaleString" => {
                format!("function {key}() {{ [native code] }}")
            }
            _ => "The Release update is unavailable. See settings or update manually.".into(),
        },
    };
    format!("{key}: {text}")
}

pub trait BodyStream {
    /// Next body chunk; `Err(message)` when the stream errors.
    fn next_chunk(&mut self) -> impl Future<Output = Option<Result<Vec<u8>, String>>>;
    /// Early exit from `for await (… of response.body)` (`iterator.return()` → `reader.cancel()`): rejects with
    /// the stream's error when the body already failed (e.g. the reader pre-fetched an error), as in JS.
    fn cancel(&mut self) -> impl Future<Output = Result<(), String>>;
}

pub struct FetchInit<'a> {
    pub method: Option<&'a str>,
    pub headers: Option<&'a Map<String, Value>>,
    pub body: Option<String>,
    pub timeout_ms: u64,
    /// Always `"error"` (redirects are refused).
    pub redirect: &'static str,
}

pub trait UpdateHost {
    type Body: BodyStream;
    fn fetch(&mut self, url: &str, init: FetchInit<'_>) -> impl Future<Output = Result<(u16, Self::Body), FetchFailure>>;
    fn now_ms(&mut self) -> f64;
    fn sleep(&mut self, ms: u64) -> impl Future<Output = ()>;
    fn random_uuid(&mut self) -> String;
    /// `realpathSync(projectRoot)`; the error is rethrown as a plain Error.
    fn realpath_project_root(&mut self) -> Result<String, String>;
    /// `localHeaders(env)`.
    fn headers(&mut self) -> impl Future<Output = Result<Map<String, Value>, UpdateError>>;
    /// `offline()` was called (createWebUpdateService).
    fn offline_created(&mut self);
    fn offline_check(&mut self) -> impl Future<Output = Result<Value, UpdateError>>;
    fn offline_apply(&mut self, target: &Value, output: &mut dyn FnMut(Value)) -> impl Future<Output = Result<(), UpdateError>>;
}

/// WHATWG UTF-8 decoder in streaming mode (incomplete trailing sequences wait for the next chunk).
#[derive(Default)]
pub struct StreamDecoder {
    pending: Vec<u8>,
}

impl StreamDecoder {
    pub fn decode(&mut self, chunk: &[u8]) -> String {
        self.pending.extend_from_slice(chunk);
        let mut out = String::new();
        let mut rest: &[u8] = &self.pending;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    rest = &[];
                    break;
                }
                Err(e) => {
                    let (ok, tail) = rest.split_at(e.valid_up_to());
                    out.push_str(std::str::from_utf8(ok).unwrap());
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{FFFD}');
                            rest = &tail[n..];
                        }
                        None => {
                            rest = tail;
                            break;
                        }
                    }
                }
            }
        }
        self.pending = rest.to_vec();
        out
    }
}

async fn read_all<B: BodyStream>(body: &mut B) -> Result<String, UpdateError> {
    let mut bytes = Vec::new();
    while let Some(c) = body.next_chunk().await {
        bytes.extend(c.map_err(|m| UpdateError::Plain { name: "Error".into(), message: Some(m) })?);
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(text.strip_prefix('\u{FEFF}').map(str::to_string).unwrap_or(text))
}

/// `response.json()`
async fn response_json<B: BodyStream>(body: &mut B) -> Result<Value, UpdateError> {
    let text = read_all(body).await?;
    serde_json::from_str(&text).map_err(|_| UpdateError::Plain { name: "SyntaxError".into(), message: None })
}

fn plain_from_fetch(f: FetchFailure) -> UpdateError {
    match f {
        FetchFailure::Error { message, .. } => UpdateError::Plain { name: "Error".into(), message: Some(message) },
        FetchFailure::Other(v) => UpdateError::Plain { name: "Error".into(), message: v.as_ref().map(js_object_string) },
    }
}

pub enum CommandClient {
    Online { base: String, auth: Map<String, Value> },
    Offline,
}

/// `createCommandClient({ env })`: `server_port` is `env.SERVER_PORT`.
pub async fn create_command_client<H: UpdateHost>(h: &mut H, server_port: Option<&str>) -> Result<CommandClient, UpdateError> {
    let port = match server_port.filter(|p| !p.is_empty()) {
        Some(p) => number(p),
        None => 3001.0,
    };
    if !(port.is_finite() && port.fract() == 0.0) || !(1.0..=65535.0).contains(&port) {
        return Err(failure("invalidPort", "SERVER_PORT must identify the local Web service."));
    }
    let base = format!("http://127.0.0.1:{}", js_number_to_string(port));
    match h.fetch(&format!("{base}/health"), FetchInit { method: None, headers: None, body: None, timeout_ms: 3000, redirect: "error" }).await {
        Ok((status, _)) => {
            if !(200..300).contains(&status) {
                return Err(failure("runtimeUnavailable", "The configured Web port is not healthy. Check SERVER_PORT."));
            }
        }
        Err(e) => {
            let refused = matches!(&e, FetchFailure::Error { cause, .. } if field(cause.as_ref(), "code") == Some(&json!("ECONNREFUSED")));
            if !refused {
                return Err(plain_from_fetch(e));
            }
            h.offline_created();
            return Ok(CommandClient::Offline);
        }
    }
    let auth = h.headers().await?;
    let client = CommandClient::Online { base, auth };
    let context = response_json(&mut request(h, &client, "context", None, 120_000).await?).await?;
    let root = h.realpath_project_root().map_err(|m| UpdateError::Plain { name: "Error".into(), message: Some(m) })?;
    if read(Some(&context), "projectRoot")? != Some(&json!(root)) {
        return Err(failure("differentInstallation", "SERVER_PORT points to another installation. Use the matching Web service."));
    }
    Ok(client)
}

async fn request<H: UpdateHost>(h: &mut H, client: &CommandClient, route: &str, body: Option<Value>, timeout_ms: u64) -> Result<H::Body, UpdateError> {
    let CommandClient::Online { base, auth } = client else { unreachable!("offline clients make no requests") };
    let init = FetchInit { method: Some(if body.is_none() { "GET" } else { "POST" }), headers: Some(auth), body: body.map(|b| b.to_string()), timeout_ms, redirect: "error" };
    let (status, mut stream) = h.fetch(&format!("{base}/api/update/{route}"), init).await.map_err(plain_from_fetch)?;
    if !(200..300).contains(&status) {
        let detail = response_json(&mut stream).await.unwrap_or_else(|_| json!({}));
        let d = Some(&detail);
        let reason = read(d, "reason")?.filter(|r| truthy(Some(r))).cloned().unwrap_or(json!("runtimeUnavailable"));
        let message = [field(d, "message"), field(d, "error")]
            .into_iter()
            .find(|v| truthy(*v))
            .flatten()
            .map(js_object_string)
            .unwrap_or_else(|| format!("Web update request failed ({status}). Use settings or check SERVER_PORT."));
        return Err(UpdateError::Failure { reason, message });
    }
    Ok(stream)
}

pub async fn client_check<H: UpdateHost>(h: &mut H, client: &CommandClient) -> Result<Value, UpdateError> {
    match client {
        CommandClient::Offline => h.offline_check().await,
        CommandClient::Online { .. } => response_json(&mut request(h, client, "check", Some(json!({})), 120_000).await?).await,
    }
}

const INTERRUPTED: &str = "Update connection interrupted; recovering the server task status...";

/// Reads the NDJSON progress stream. `Ok(true)` = completed; `Ok(false)` = stream ended; `Err(Plain)` = interrupted.
async fn follow_stream<B: BodyStream>(stream: &mut B, output: &mut dyn FnMut(Value)) -> Result<bool, UpdateError> {
    let mut pending = String::new();
    let mut decoder = StreamDecoder::default();
    while let Some(chunk) = stream.next_chunk().await {
        let chunk = chunk.map_err(|m| UpdateError::Plain { name: "Error".into(), message: Some(m) })?;
        pending.push_str(&decoder.decode(&chunk));
        while let Some(end) = pending.find('\n') {
            let line = pending[..end].to_string();
            pending = pending[end + 1..].to_string();
            if js_trim(&line).is_empty() {
                continue;
            }
            let event: Value = serde_json::from_str(&line).map_err(|_| UpdateError::Plain { name: "SyntaxError".into(), message: None })?;
            let e = Some(&event);
            let message = read(e, "message")?;
            if truthy(message) {
                output(message.unwrap().clone());
            }
            if field(e, "status") == Some(&json!("error")) {
                let reason = field(e, "reason").filter(|r| truthy(Some(r))).cloned().unwrap_or(json!("applyFailed"));
                return Err(UpdateError::Failure { reason, message: error_message(field(e, "message")) });
            }
            if field(e, "stage") == Some(&json!("complete")) && field(e, "status") == Some(&json!("success")) {
                // `return` inside for-await awaits iterator.return(); a rejection there escapes the loop.
                // (When the loop body throws instead, JS keeps the original error, so no cancel check above.)
                stream.cancel().await.map_err(|m| UpdateError::Plain { name: "Error".into(), message: Some(m) })?;
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub async fn client_apply<H: UpdateHost>(h: &mut H, client: &CommandClient, target: &Value, output: &mut dyn FnMut(Value)) -> Result<(), UpdateError> {
    if let CommandClient::Offline = client {
        return h.offline_apply(target, output).await;
    }
    let update_id = h.random_uuid();
    let streamed = match request(h, client, "apply", Some(json!({ "target": target, "updateId": update_id })), 3_600_000).await {
        Ok(mut stream) => follow_stream(&mut stream, output).await,
        Err(e) => Err(e),
    };
    match streamed {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e @ UpdateError::Failure { .. }) => return Err(e),
        Err(UpdateError::Plain { .. }) => output(json!(INTERRUPTED)),
    }
    let deadline = h.now_ms() + 3_600_000.0;
    while h.now_ms() < deadline {
        let status = response_json(&mut request(h, client, "status", None, 120_000).await?).await?;
        let last = read(Some(&status), "lastUpdateResult")?;
        if field(last, "updateId") == Some(&json!(update_id)) {
            if truthy(field(last, "success")) {
                return Ok(());
            }
            let reason = field(last, "reason").filter(|r| truthy(Some(r))).cloned().unwrap_or(json!("applyFailed"));
            return Err(UpdateError::Failure { reason, message: "The server reported that the update failed.".into() });
        }
        if !truthy(field(Some(&status), "updateInProgress")) || field(Some(&status), "currentUpdateId") != Some(&json!(update_id)) {
            return Err(failure("updateUnconfirmed", "Update outcome is unconfirmed. Check settings before retrying or restarting."));
        }
        h.sleep(1000).await;
    }
    Err(failure("updateUnconfirmed", "Update is still unconfirmed. Check settings before restarting."))
}

pub async fn client_restart<H: UpdateHost>(h: &mut H, client: &CommandClient) -> Result<Value, UpdateError> {
    let result = response_json(&mut request(h, client, "restart", Some(json!({})), 120_000).await?).await?;
    if read(Some(&result), "status")? != Some(&json!("accepted")) {
        return Err(failure("restartUnconfirmed", "The runtime did not accept the restart request. Restart manually."));
    }
    Ok(result)
}

fn error_text(e: &UpdateError) -> String {
    match e {
        UpdateError::Failure { message, .. } => message.clone(),
        UpdateError::Plain { message, .. } => message.clone().unwrap_or_default(),
    }
}

/// `runWebUpdateCommand({ checkOnly, restart, client, output })` → exit code.
pub async fn run_web_update_command<H: UpdateHost>(h: &mut H, client: &CommandClient, check_only: bool, restart: bool, output: &mut dyn FnMut(Value)) -> Result<i32, UpdateError> {
    let status = client_check(h, client).await?;
    let s = Some(&status);
    if read(s, "reason")? == Some(&json!("upToDate")) {
        output(json!("Already at the latest Release."));
        return Ok(2);
    }
    if !truthy(field(s, "canUpdate")) {
        let reason = field(s, "reason").filter(|r| truthy(Some(r))).cloned().unwrap_or(json!("checkFailed"));
        let message = describe_update_reason(&reason);
        return Err(UpdateError::Failure { reason, message });
    }
    let latest = field(s, "latest");
    let tag = read(latest, "tagName")?;
    let sha = read(latest, "sourceSha")?;
    let short = match sha {
        Some(Value::String(x)) => js_slice_units(x, 8),
        None => return Err(type_error("Cannot read properties of undefined (reading 'slice')".into())),
        Some(_) => return Err(type_error("status.latest.sourceSha.slice is not a function".into())),
    };
    output(json!(format!("Release available: {} ({short})", tag.map(js_object_string).unwrap_or_else(|| "undefined".into()))));
    if check_only {
        return Ok(0);
    }
    if restart && matches!(client, CommandClient::Offline) {
        return Err(failure("restartUnavailable", "No running Web service was found. Run update without --restart, then start PilotDeck manually."));
    }
    let target = latest.cloned().unwrap_or(Value::Null);
    client_apply(h, client, &target, output).await?;
    if restart {
        if let Err(e) = client_restart(h, client).await {
            return Err(failure("restartFailed", format!("Update prepared, but restart was not confirmed: {} Restart manually; do not repeat the update.", error_text(&e))));
        }
        output(json!("Update prepared; the running Web service accepted the restart request."));
    } else {
        output(json!("Update prepared. Restart PilotDeck manually to load the new version."));
    }
    Ok(0)
}
