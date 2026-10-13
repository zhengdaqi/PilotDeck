//! Port of `src/cli/commands/gatewaySetup.ts` (`pilotdeck gateway setup [feishu|lark|weixin|wechat|wecom|…]`).
//!
//! readline semantics on a pipe: prompts go to stdout without newline; a line is consumed per question;
//! EOF while a question is pending (including a final line without "\n") ends the process with exit 0.

use crate::http;
use crate::yamlfmt;
use pilotdeck_session_search::js::{js_trim, utf16_len, utf16_slice};
use pilotdeck_skills::jsjson::{js_number_to_string, stringify_pretty, JsObject, JsValue};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FEISHU_TOKEN_URL: &str = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
const LARK_TOKEN_URL: &str = "https://open.larksuite.com/open-apis/auth/v3/tenant_access_token/internal";
const WECOM_DEFAULT_WS_URL: &str = "wss://openws.work.weixin.qq.com";
const WECOM_QR_GENERATE_URL: &str = "https://work.weixin.qq.com/ai/qc/generate";
const WECOM_QR_QUERY_URL: &str = "https://work.weixin.qq.com/ai/qc/query_result";
const WECOM_QR_CODE_PAGE: &str = "https://work.weixin.qq.com/ai/qc/gen?source=hermes&scode=";
const WEIXIN_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

/// Process ends (exit 0) because stdin closed while a question was pending.
pub struct Eof;

/// Uncaught error → `main().catch` prints the message, exit 1.
pub enum Stop {
    Eof,
    Error(String),
}

impl From<Eof> for Stop {
    fn from(_: Eof) -> Self {
        Stop::Eof
    }
}

pub struct Ctx {
    pub pilot_home: PathBuf,
    pub yaml_path: PathBuf,
    pub weixin_creds: PathBuf,
    pub home: PathBuf,
    input: Box<dyn BufRead>,
}

fn env_nonempty(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

impl Ctx {
    pub fn from_env() -> Ctx {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let pilot_home = env_nonempty("PILOT_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".pilotdeck"));
        let yaml_path = env_nonempty("PILOTDECK_CONFIG_PATH").map(PathBuf::from).unwrap_or_else(|| pilot_home.join("pilotdeck.yaml"));
        Ctx {
            weixin_creds: pilot_home.join("weixin-credentials.json"),
            pilot_home,
            yaml_path,
            home,
            input: Box::new(std::io::BufReader::new(std::io::stdin())),
        }
    }

    fn question(&mut self, prompt: &str) -> Result<String, Eof> {
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        let mut buf = Vec::new();
        match self.input.read_until(b'\n', &mut buf) {
            Ok(_) if buf.last() == Some(&b'\n') => {
                buf.pop();
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                Ok(String::from_utf8_lossy(&buf).into_owned())
            }
            _ => Err(Eof),
        }
    }
}

fn log(s: &str) {
    println!("{s}");
}

fn lower_trim(s: &str) -> String {
    js_trim(s).to_lowercase()
}

// ---- JS value helpers -------------------------------------------------------

fn get<'a>(v: Option<&'a JsValue>, k: &str) -> Option<&'a JsValue> {
    match v? {
        JsValue::Obj(o) => o.get(k),
        _ => None,
    }
}

fn truthy(v: Option<&JsValue>) -> bool {
    match v {
        None | Some(JsValue::Undefined) | Some(JsValue::Null) => false,
        Some(JsValue::Bool(b)) => *b,
        Some(JsValue::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(JsValue::Str(s)) => !s.is_empty(),
        _ => true,
    }
}

fn nullish(v: Option<&JsValue>) -> bool {
    matches!(v, None | Some(JsValue::Undefined) | Some(JsValue::Null))
}

/// `String(v)`
fn js_str(v: Option<&JsValue>) -> String {
    match v {
        None | Some(JsValue::Undefined) => "undefined".into(),
        Some(JsValue::Null) => "null".into(),
        Some(JsValue::Bool(b)) => b.to_string(),
        Some(JsValue::Num(n)) => js_number_to_string(*n),
        Some(JsValue::Str(s)) => s.clone(),
        Some(JsValue::Arr(a)) => a
            .iter()
            .map(|x| if nullish(Some(x)) { String::new() } else { js_str(Some(x)) })
            .collect::<Vec<_>>()
            .join(","),
        Some(JsValue::Obj(_)) => "[object Object]".into(),
    }
}

/// `a || b` over strings in templates.
fn or_str(v: Option<&JsValue>, fallback: &str) -> String {
    if truthy(v) { js_str(v) } else { fallback.to_string() }
}

fn coalesce<'a>(vals: &[Option<&'a JsValue>]) -> Option<&'a JsValue> {
    vals.iter().copied().find(|v| !nullish(*v)).flatten()
}

fn is_plain_object(v: Option<&JsValue>) -> bool {
    matches!(v, Some(JsValue::Obj(_)))
}

fn mask_secret(value: &str) -> String {
    let n = utf16_len(value);
    if n <= 8 {
        return value.to_string();
    }
    format!("{}…{}", utf16_slice(value, 0, 4), utf16_slice(value, n - 4, n))
}

fn coerce_setup_list(v: Option<&JsValue>) -> Vec<String> {
    match v {
        Some(JsValue::Arr(a)) => a.iter().map(|x| js_trim(&js_str(Some(x))).to_string()).filter(|s| !s.is_empty()).collect(),
        Some(JsValue::Str(s)) => s.split(',').map(|x| js_trim(x).to_string()).filter(|s| !s.is_empty()).collect(),
        _ => vec![],
    }
}

fn normalize_policy(v: Option<&JsValue>, fallback: &str) -> String {
    let raw = lower_trim(&if nullish(v) { String::new() } else { js_str(v) });
    if matches!(raw.as_str(), "open" | "allowlist" | "disabled") { raw } else { fallback.into() }
}

// ---- YAML config ------------------------------------------------------------

fn load_yaml_config(ctx: &Ctx) -> Option<JsValue> {
    let raw = std::fs::read(&ctx.yaml_path).ok()?;
    match yamlfmt::parse(&String::from_utf8_lossy(&raw)) {
        Ok(JsValue::Null) | Err(_) => None,
        Ok(v) => Some(v),
    }
}

fn type_name(v: &JsValue) -> &'static str {
    match v {
        JsValue::Str(_) => "string",
        JsValue::Num(_) => "number",
        JsValue::Bool(_) => "boolean",
        _ => "object",
    }
}

/// `yamlConfig.adapters ??= {}; yamlConfig.adapters[name] = value` with JS strict-mode TypeErrors.
fn set_adapter(cfg: JsValue, name: &str, value: JsValue) -> Result<JsValue, String> {
    let cannot = |prop: &str, on: &JsValue| {
        format!("Cannot create property '{prop}' on {} '{}'", type_name(on), js_str(Some(on)))
    };
    let JsValue::Obj(mut root) = cfg else {
        if matches!(cfg, JsValue::Arr(_)) {
            // JS sets a non-index property on the array; YAML stringify ignores it.
            return Ok(cfg);
        }
        return Err(cannot("adapters", &cfg));
    };
    let adapters = root.get("adapters").cloned();
    let mut adapters = match adapters {
        Some(JsValue::Obj(o)) => o,
        v if !truthy(v.as_ref()) => JsObject::new(),
        Some(other) => {
            if matches!(other, JsValue::Arr(_)) {
                // Non-index property on an array: invisible after stringify.
                return Ok(JsValue::Obj(root));
            }
            return Err(cannot(name, &other));
        }
        None => JsObject::new(),
    };
    adapters.set(name, value);
    root.set("adapters", JsValue::Obj(adapters));
    Ok(JsValue::Obj(root))
}

fn save_yaml_config(ctx: &Ctx, cfg: &JsValue) -> Result<(), String> {
    if let Some(parent) = ctx.yaml_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| node_fs_error(&e, "mkdir", parent))?;
    }
    std::fs::write(&ctx.yaml_path, yamlfmt::stringify(cfg)).map_err(|e| node_fs_error(&e, "open", &ctx.yaml_path))
}

fn node_fs_error(e: &std::io::Error, syscall: &str, path: &Path) -> String {
    let (code, desc) = match e.kind() {
        std::io::ErrorKind::NotFound => ("ENOENT", "no such file or directory"),
        std::io::ErrorKind::PermissionDenied => ("EACCES", "permission denied"),
        _ if e.raw_os_error() == Some(20) => ("ENOTDIR", "not a directory"),
        _ if e.raw_os_error() == Some(21) => ("EISDIR", "illegal operation on a directory"),
        _ if e.raw_os_error() == Some(17) => ("EEXIST", "file already exists"),
        _ => ("EIO", "i/o error"),
    };
    format!("{code}: {desc}, {syscall} '{}'", path.display())
}

fn write_adapter(ctx: &Ctx, name: &str, value: JsValue) -> Result<(), Stop> {
    let cfg = load_yaml_config(ctx).unwrap_or(JsValue::Obj(JsObject::new()));
    let cfg = set_adapter(cfg, name, value).map_err(Stop::Error)?;
    save_yaml_config(ctx, &cfg).map_err(Stop::Error)
}

// ---- entry ------------------------------------------------------------------

pub fn run_gateway_setup(ctx: &mut Ctx, argv: &[String]) -> Result<(), Stop> {
    log("\n╔══════════════════════════════════════════════════╗");
    log("║  PilotDeck Gateway Setup                        ║");
    log("║  配置 IM 平台连接                                ║");
    log("╚══════════════════════════════════════════════════╝\n");
    let platform = argv.first().map(|p| p.to_lowercase());
    match platform.as_deref() {
        Some("feishu" | "lark") => setup_feishu(ctx),
        Some("weixin" | "wechat") => setup_weixin(ctx),
        Some("wecom" | "work-weixin" | "workwechat") => setup_wecom(ctx),
        _ => match select_platform(ctx)? {
            Some("feishu") => setup_feishu(ctx),
            Some("weixin") => setup_weixin(ctx),
            Some("wecom") => setup_wecom(ctx),
            Some(_) => {
                setup_feishu(ctx)?;
                log("");
                setup_weixin(ctx)?;
                log("");
                setup_wecom(ctx)
            }
            None => {
                log("未选择任何平台，退出。");
                Ok(())
            }
        },
    }
}

fn select_platform(ctx: &mut Ctx) -> Result<Option<&'static str>, Stop> {
    let cfg = load_yaml_config(ctx);
    let adapters = get(cfg.as_ref(), "adapters");
    let feishu = if truthy(get(get(adapters, "feishu"), "enabled")) { "✅ 已启用" } else { "未配置" };
    let weixin = if ctx.weixin_creds.exists() { "✅ 已有凭据" } else { "未配置" };
    let wecom = if truthy(get(get(adapters, "wecom"), "enabled")) { "✅ 已启用" } else { "未配置" };
    log("可配置的平台：");
    log(&format!("  1) 飞书 / Lark      [{feishu}]"));
    log(&format!("  2) 微信 (iLink)     [{weixin}]"));
    log(&format!("  3) 企业微信 WeCom   [{wecom}]"));
    log("  4) 全部配置");
    log("  q) 退出\n");
    let a = lower_trim(&ctx.question("请选择 [1/2/3/4/q]: ")?);
    Ok(match a.as_str() {
        "1" | "feishu" => Some("feishu"),
        "2" | "weixin" => Some("weixin"),
        "3" | "wecom" => Some("wecom"),
        "4" | "all" => Some("all"),
        _ => None,
    })
}

const RULE: &str = "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";

fn setup_feishu(ctx: &mut Ctx) -> Result<(), Stop> {
    log(RULE);
    log("  飞书 / Lark 配置");
    log(&format!("{RULE}\n"));
    let cfg = load_yaml_config(ctx);
    let fs = get(get(cfg.as_ref(), "adapters"), "feishu");
    let current_app_id = or_str(get(fs, "appId"), "");
    let current_secret = or_str(get(fs, "appSecret"), "");
    let current_domain = or_str(get(fs, "domainName"), "feishu");
    let domain_answer = lower_trim(&ctx.question(&format!("使用哪个平台？ [feishu/lark] (当前: {current_domain}): "))?);
    let domain = if domain_answer == "lark" { "lark" } else { "feishu" };
    let console_url = if domain == "lark" { "https://open.larksuite.com" } else { "https://open.feishu.cn" };
    // attemptFeishuQRCreation: the bundled @larksuiteoapi/node-sdk has no AppTicketManager → always null.
    log(&format!("\n请在 {console_url} 创建企业自建应用："));
    log("  1. 创建应用 → 获取 App ID 和 App Secret");
    log("  2. 权限管理 → 添加以下权限：");
    log("     - im:message (收发消息)");
    log("     - im:message:send_as_bot (以机器人身份发消息)");
    log("     - im:resource (访问图片/文件)");
    log("     - im:chat (群聊元信息)");
    log("  3. 事件订阅 → 添加 im.message.receive_v1");
    log("  4. 版本管理 → 发布版本\n");
    let hint = if current_app_id.is_empty() { String::new() } else { format!(" (当前: {})", mask_secret(&current_app_id)) };
    let a = js_trim(&ctx.question(&format!("App ID{hint}: "))?).to_string();
    let app_id = if a.is_empty() { current_app_id } else { a };
    let shint = if current_secret.is_empty() { "" } else { " (回车保留当前值)" };
    let s = js_trim(&ctx.question(&format!("App Secret{shint}: "))?).to_string();
    let app_secret = if s.is_empty() { current_secret } else { s };
    if app_id.is_empty() || app_secret.is_empty() {
        log("\n⚠️  未提供完整凭据，跳过飞书配置。");
        return Ok(());
    }
    log("\n🔍 验证凭据...");
    let url = if domain == "lark" { LARK_TOKEN_URL } else { FEISHU_TOKEN_URL };
    match test_feishu_credentials(&app_id, &app_secret, url) {
        Ok(()) => log("✅ 凭据验证通过！"),
        Err(e) => {
            log(&format!("\n❌ 凭据验证失败: {e}"));
            let p = lower_trim(&ctx.question("是否仍然保存配置？ [y/N]: ")?);
            if p != "y" && p != "yes" {
                log("已取消。");
                return Ok(());
            }
        }
    }
    let value = JsObject::new()
        .with("enabled", JsValue::Bool(true))
        .with("appId", JsValue::Str(app_id))
        .with("appSecret", JsValue::Str(app_secret))
        .with("connectionMode", JsValue::Str("stream".into()))
        .with("domainName", JsValue::Str(domain.into()));
    write_adapter(ctx, "feishu", JsValue::Obj(value))?;
    log("\n✅ 飞书配置已写入 pilotdeck.yaml");
    log("   连接模式: stream (WebSocket, 推荐 — 无需公网 IP)");
    log("   重启 PilotDeck 服务后生效\n");
    Ok(())
}

pub fn test_feishu_credentials(app_id: &str, app_secret: &str, url: &str) -> Result<(), String> {
    let body = pilotdeck_skills::jsjson::stringify_compact(&JsValue::Obj(
        JsObject::new().with("app_id", JsValue::Str(app_id.into())).with("app_secret", JsValue::Str(app_secret.into())),
    ));
    let r = http::post_json(url, &[("Content-Type", "application/json; charset=utf-8")], &body)
        .map_err(|e| if e == "__timeout__" { http::FETCH_FAILED.to_string() } else { e })?;
    let json = http::json(&r)?;
    if matches!(json, JsValue::Null) {
        return Err("Cannot read properties of null (reading 'code')".into());
    }
    let code = get(Some(&json), "code");
    if matches!(code, Some(JsValue::Num(n)) if *n == 0.0) && truthy(get(Some(&json), "tenant_access_token")) {
        return Ok(());
    }
    let msg = get(Some(&json), "msg");
    Err(format!("code={} msg={}", js_str(code), if nullish(msg) { "unknown".into() } else { js_str(msg) }))
}

fn setup_wecom(ctx: &mut Ctx) -> Result<(), Stop> {
    log(RULE);
    log("  企业微信 WeCom AI Bot 配置");
    log(&format!("{RULE}\n"));
    let cfg = load_yaml_config(ctx);
    let wc = get(get(cfg.as_ref(), "adapters"), "wecom");
    let extra = get(wc, "extra");
    let s_or_empty = |v: Option<&JsValue>| if nullish(v) { String::new() } else { js_str(v) };
    let current_bot_id = s_or_empty(coalesce(&[get(wc, "token"), get(extra, "bot_id"), get(extra, "botId")]));
    let current_secret = s_or_empty(coalesce(&[get(extra, "secret"), get(extra, "botSecret")]));
    let ws = coalesce(&[get(extra, "websocket_url"), get(extra, "websocketUrl")]);
    let current_ws = if nullish(ws) { WECOM_DEFAULT_WS_URL.to_string() } else { js_str(ws) };
    if !current_bot_id.is_empty() && !current_secret.is_empty() {
        log(&format!("已有企业微信配置 (Bot ID: {})", mask_secret(&current_bot_id)));
        let a = lower_trim(&ctx.question("重新配置？ [y/N]: ")?);
        if a != "y" && a != "yes" {
            let groups = if is_plain_object(get(extra, "groups")) { get(extra, "groups").cloned() } else { None };
            write_wecom(ctx, WeCom {
                bot_id: current_bot_id,
                secret: current_secret,
                websocket_url: current_ws,
                dm_policy: normalize_policy(get(extra, "dm_policy"), "open"),
                allow_from: coerce_setup_list(coalesce(&[get(extra, "allow_from"), get(extra, "allowFrom")])),
                group_policy: normalize_policy(get(extra, "group_policy"), "disabled"),
                group_allow_from: coerce_setup_list(coalesce(&[get(extra, "group_allow_from"), get(extra, "groupAllowFrom")])),
                groups,
            })?;
            log("企业微信已启用（使用现有凭据）。\n");
            return Ok(());
        }
    }
    log("企业微信 AI Bot 通过官方 WebSocket 网关连接，无需公网回调地址。");
    log("请确保你有企业微信组织管理员权限，或已在管理后台拿到 Bot ID 和 Secret。\n");
    let mut creds: Option<(String, String)> = None;
    let scan = lower_trim(&ctx.question("是否扫码自动创建/获取 Bot ID 和 Secret？ [Y/n]: ")?);
    if scan != "n" && scan != "no" {
        creds = qr_scan_for_wecom_bot_info(WECOM_QR_GENERATE_URL, WECOM_QR_QUERY_URL, Duration::from_millis(300_000), Duration::from_millis(3000), &mut |m| log(m));
        if creds.is_none() {
            log("\n扫码未完成或接口不可用，将使用手动输入模式。\n");
        }
    }
    let (mut bot_id, mut secret) = creds.unwrap_or_default();
    if bot_id.is_empty() || secret.is_empty() {
        log("手动配置步骤：");
        log("  1. 登录企业微信管理后台");
        log("  2. 进入应用管理，创建 AI Bot / 智能机器人");
        log("  3. 选择 API 模式");
        log("  4. 复制 Bot ID 和 Secret\n");
        let hint = if current_bot_id.is_empty() { String::new() } else { format!(" (当前: {})", mask_secret(&current_bot_id)) };
        let b = js_trim(&ctx.question(&format!("Bot ID{hint}: "))?).to_string();
        bot_id = if b.is_empty() { current_bot_id.clone() } else { b };
        let sh = if current_secret.is_empty() { "" } else { " (回车保留当前值)" };
        let s = js_trim(&ctx.question(&format!("Secret{sh}: "))?).to_string();
        secret = if s.is_empty() { current_secret.clone() } else { s };
    }
    if bot_id.is_empty() || secret.is_empty() {
        log("\n未提供完整 Bot ID / Secret，跳过企业微信配置。\n");
        return Ok(());
    }
    let w = js_trim(&ctx.question(&format!("WebSocket URL (默认: {WECOM_DEFAULT_WS_URL}): "))?).to_string();
    let websocket_url = if w.is_empty() { WECOM_DEFAULT_WS_URL.to_string() } else { w };
    let dm = js_trim(&ctx.question("私聊访问策略 [1=open 推荐, 2=allowlist, 3=disabled] (默认 1): ")?).to_string();
    let mut dm_policy = match dm.as_str() { "2" => "allowlist", "3" => "disabled", _ => "open" }.to_string();
    let mut allow_from = vec![];
    if dm_policy == "allowlist" {
        let a = js_trim(&ctx.question("允许私聊的企业微信用户 ID（逗号分隔，留空则暂时禁用私聊）: ")?).to_string();
        allow_from = coerce_setup_list(Some(&JsValue::Str(a)));
        if allow_from.is_empty() {
            dm_policy = "disabled".into();
        }
    }
    let g = js_trim(&ctx.question("群聊访问策略 [1=disabled 推荐, 2=open, 3=allowlist] (默认 1): ")?).to_string();
    let group_policy = match g.as_str() { "2" => "open", "3" => "allowlist", _ => "disabled" }.to_string();
    let mut group_allow_from = vec![];
    if group_policy == "allowlist" {
        let a = js_trim(&ctx.question("允许响应的群聊 chat_id（逗号分隔）: ")?).to_string();
        group_allow_from = coerce_setup_list(Some(&JsValue::Str(a)));
    }
    write_wecom(ctx, WeCom { bot_id, secret, websocket_url, dm_policy, allow_from, group_policy, group_allow_from, groups: None })?;
    log("\n企业微信配置已写入 pilotdeck.yaml");
    log("连接模式: AI Bot WebSocket (无需公网 IP)");
    log("重启 PilotDeck 服务后生效。\n");
    Ok(())
}

struct WeCom {
    bot_id: String,
    secret: String,
    websocket_url: String,
    dm_policy: String,
    allow_from: Vec<String>,
    group_policy: String,
    group_allow_from: Vec<String>,
    groups: Option<JsValue>,
}

fn strs(v: &[String]) -> JsValue {
    JsValue::Arr(v.iter().map(|s| JsValue::Str(s.clone())).collect())
}

fn write_wecom(ctx: &Ctx, c: WeCom) -> Result<(), Stop> {
    let mut extra = JsObject::new()
        .with("secret", JsValue::Str(c.secret))
        .with("websocket_url", JsValue::Str(if c.websocket_url.is_empty() { WECOM_DEFAULT_WS_URL.into() } else { c.websocket_url }))
        .with("dm_policy", JsValue::Str(c.dm_policy))
        .with("group_policy", JsValue::Str(c.group_policy));
    if !c.allow_from.is_empty() {
        extra.set("allow_from", strs(&c.allow_from));
    }
    if !c.group_allow_from.is_empty() {
        extra.set("group_allow_from", strs(&c.group_allow_from));
    }
    if let Some(g) = c.groups {
        extra.set("groups", g);
    }
    let v = JsObject::new()
        .with("enabled", JsValue::Bool(true))
        .with("token", JsValue::Str(c.bot_id))
        .with("extra", JsValue::Obj(extra));
    write_adapter(ctx, "wecom", JsValue::Obj(v))
}

fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn fetch_json(url: &str) -> Result<JsValue, String> {
    let r = http::get(url, &[("User-Agent", "PilotDeck/1.0")], None).map_err(|e| if e == "__timeout__" { http::FETCH_FAILED.into() } else { e })?;
    if !r.ok() {
        return Err(format!("HTTP {}", r.status));
    }
    let v = http::json(&r)?;
    Ok(if is_plain_object(Some(&v)) { v } else { JsValue::Obj(JsObject::new()) })
}

/// `qrScanForWeComBotInfo` (URLs injectable for tests).
pub fn qr_scan_for_wecom_bot_info(
    generate_url: &str,
    query_url: &str,
    timeout: Duration,
    poll: Duration,
    log: &mut dyn FnMut(&str),
) -> Option<(String, String)> {
    log("正在连接企业微信生成扫码链接...");
    let generated = match fetch_json(&format!("{generate_url}?source=hermes")) {
        Ok(v) => v,
        Err(e) => {
            log(&format!("企业微信扫码失败: {e}"));
            return None;
        }
    };
    let data = get(Some(&generated), "data").filter(|v| is_plain_object(Some(v)));
    let field = |v: Option<&JsValue>, k: &str| {
        let x = get(v, k);
        js_trim(&if nullish(x) { String::new() } else { js_str(x) }).to_string()
    };
    let scode = field(data, "scode");
    let auth_url = field(data, "auth_url");
    if scode.is_empty() || auth_url.is_empty() {
        log("企业微信扫码接口返回格式异常。");
        return None;
    }
    let page = format!("{WECOM_QR_CODE_PAGE}{}", encode_uri_component(&scode));
    log("\n请用企业微信手机端扫描/打开以下链接：");
    log(&auth_url);
    log(&format!("\n如果无法直接扫码，请打开：\n{page}\n"));
    log("等待扫码结果...");
    let deadline = Instant::now() + timeout;
    let q = format!("{query_url}?scode={}", encode_uri_component(&scode));
    while Instant::now() < deadline {
        let result = fetch_json(&q).ok();
        let rd = get(result.as_ref(), "data").filter(|v| is_plain_object(Some(v)));
        if field(rd, "status").to_lowercase() == "success" {
            let bi = get(rd, "bot_info").filter(|v| is_plain_object(Some(v)));
            let id_v = coalesce(&[get(bi, "botid"), get(bi, "bot_id")]);
            let bot_id = js_trim(&if nullish(id_v) { String::new() } else { js_str(id_v) }).to_string();
            let secret = field(bi, "secret");
            if !bot_id.is_empty() && !secret.is_empty() {
                return Some((bot_id, secret));
            }
            log("扫码成功但未返回完整 Bot 凭据。");
            return None;
        }
        std::thread::sleep(poll);
    }
    log("企业微信扫码超时。");
    None
}

// ---- Weixin (iLink) ---------------------------------------------------------

fn load_weixin_credentials(ctx: &Ctx) -> Option<JsValue> {
    let raw = std::fs::read(&ctx.weixin_creds).ok()?;
    let v = pilotdeck_skills::v8json::parse(&String::from_utf8_lossy(&raw)).ok()?;
    if !is_plain_object(Some(&v)) {
        return None;
    }
    let o = Some(&v);
    (truthy(get(o, "baseUrl")) && truthy(get(o, "botToken")) && truthy(get(o, "accountId"))).then_some(v)
}

fn setup_weixin(ctx: &mut Ctx) -> Result<(), Stop> {
    log(RULE);
    log("  微信 iLink 配置");
    log(&format!("{RULE}\n"));
    if let Some(c) = load_weixin_credentials(ctx) {
        log(&format!("已有凭据 (accountId: {})", js_str(get(Some(&c), "accountId"))));
        let a = lower_trim(&ctx.question("重新登录？ [y/N]: ")?);
        if a != "y" && a != "yes" {
            write_adapter(ctx, "weixin", JsValue::Obj(JsObject::new().with("enabled", JsValue::Bool(true))))?;
            log("✅ 微信已启用（使用现有凭据）\n");
            return Ok(());
        }
    }
    log("⚠️  注意：");
    log("  - 使用 iLink Bot API，非企业微信");
    log("  - 建议使用小号登录，有封号风险");
    log("  - 群聊功能默认禁用\n");
    let p = lower_trim(&ctx.question("继续扫码登录？ [Y/n]: ")?);
    if p == "n" || p == "no" {
        log("已取消。");
        return Ok(());
    }
    log("\n╔══════════════════════════════════════════════╗");
    log("║  请用微信扫描二维码                          ║");
    log("╚══════════════════════════════════════════════╝\n");
    let result = login_with_qr(WEIXIN_BASE_URL, Duration::from_millis(480_000), Duration::from_secs(1), &mut |url| {
        log("扫码登录链接：");
        log(&format!("  {url}\n"));
        log("如果终端无法显示二维码，请在浏览器中打开上述链接。\n");
    }, &mut |status| {
        let label = match status {
            "waiting" => "⏳ 等待扫码...".to_string(),
            "scanned" => "📱 已扫码，等待确认...".to_string(),
            "expired" => "⏰ 二维码已过期，正在刷新...".to_string(),
            "refreshing" => "🔄 刷新中...".to_string(),
            other => format!("状态: {other}"),
        };
        log(&label);
    })
    .and_then(|r| {
        save_weixin_credentials(ctx, &r)?;
        write_adapter(ctx, "weixin", JsValue::Obj(JsObject::new().with("enabled", JsValue::Bool(true)))).map_err(|e| match e {
            Stop::Error(m) => m,
            Stop::Eof => String::new(),
        })?;
        Ok(r)
    });
    match result {
        Ok(r) => {
            log("\n✅ 微信登录成功！");
            log(&format!("   账号 ID: {}", r.account_id));
            log(&format!("   凭据已保存到: {}", ctx.weixin_creds.display()));
            log("   重启 PilotDeck 服务后生效\n");
        }
        Err(e) => {
            log(&format!("\n❌ 微信登录失败: {e}"));
            log("   请检查网络连接后重试。\n");
        }
    }
    Ok(())
}

pub struct WeixinCreds {
    pub base_url: String,
    pub bot_token: String,
    pub account_id: String,
}

fn save_weixin_credentials(ctx: &Ctx, c: &WeixinCreds) -> Result<(), String> {
    // TS mkdirs ~/.pilotdeck (not PILOT_HOME) before writing PILOT_HOME/weixin-credentials.json.
    let d = ctx.home.join(".pilotdeck");
    std::fs::create_dir_all(&d).map_err(|e| node_fs_error(&e, "mkdir", &d))?;
    let v = JsObject::new()
        .with("baseUrl", JsValue::Str(c.base_url.clone()))
        .with("botToken", JsValue::Str(c.bot_token.clone()))
        .with("accountId", JsValue::Str(c.account_id.clone()));
    std::fs::write(&ctx.weixin_creds, stringify_pretty(&JsValue::Obj(v))).map_err(|e| node_fs_error(&e, "open", &ctx.weixin_creds))
}

/// Port of `weixin-ilink@0.1.0` `loginWithQR` (base URL injectable for tests).
pub fn login_with_qr(
    base_url: &str,
    login_timeout: Duration,
    poll_sleep: Duration,
    on_qr: &mut dyn FnMut(&str),
    on_status: &mut dyn FnMut(&str),
) -> Result<WeixinCreds, String> {
    let base = if base_url.ends_with('/') { base_url.to_string() } else { format!("{base_url}/") };
    let fetch_qr = || -> Result<JsValue, String> {
        let r = http::get(&format!("{base}ilink/bot/get_bot_qrcode?bot_type=3"), &[], None)
            .map_err(|e| if e == "__timeout__" { http::FETCH_FAILED.into() } else { e })?;
        if !r.ok() {
            return Err(format!("Failed to fetch QR code: {}", r.status));
        }
        http::json(&r)
    };
    let poll = |qrcode: &str| -> Result<JsValue, String> {
        let url = format!("{base}ilink/bot/get_qrcode_status?qrcode={}", encode_uri_component(qrcode));
        match http::get(&url, &[("iLink-App-ClientVersion", "1")], Some(Duration::from_millis(35_000))) {
            Err(e) if e == "__timeout__" => Ok(JsValue::Obj(JsObject::new().with("status", JsValue::Str("wait".into())))),
            Err(e) => Err(e),
            Ok(r) if !r.ok() => Err(format!("QR status poll failed: {}", r.status)),
            Ok(r) => http::json(&r),
        }
    };
    let mut qr = fetch_qr()?;
    let mut refresh = 1;
    on_qr(&js_str(get(Some(&qr), "qrcode_img_content")));
    on_status("waiting");
    let deadline = Instant::now() + login_timeout;
    while Instant::now() < deadline {
        let st = poll(&js_str(get(Some(&qr), "qrcode")))?;
        match js_str(get(Some(&st), "status")).as_str() {
            "scaned" => on_status("scanned"),
            "expired" => {
                refresh += 1;
                if refresh > 3 {
                    return Err("QR code expired too many times".into());
                }
                on_status("refreshing");
                qr = fetch_qr()?;
                on_qr(&js_str(get(Some(&qr), "qrcode_img_content")));
                on_status("waiting");
            }
            "confirmed" => {
                let s = Some(&st);
                if !truthy(get(s, "ilink_bot_id")) || !truthy(get(s, "bot_token")) {
                    return Err("Login failed: server did not return required credentials".into());
                }
                return Ok(WeixinCreds {
                    bot_token: js_str(get(s, "bot_token")),
                    account_id: js_str(get(s, "ilink_bot_id")),
                    base_url: if truthy(get(s, "baseurl")) { js_str(get(s, "baseurl")) } else { base_url.to_string() },
                });
            }
            _ => {}
        }
        std::thread::sleep(poll_sleep);
    }
    Err("Login timed out".into())
}
