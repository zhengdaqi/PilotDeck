//! Port of `src/gateway/server/authToken.ts` (`ensureGatewayAuthToken`).
use base64::Engine;
use pilotdeck_session_search::paths;
use std::io::Read;

pub fn resolve_gateway_token_path() -> String {
    let home = paths::resolve_pilot_home_from_env();
    let base = if home.is_empty() { "~/.pilotdeck".to_string() } else { home };
    paths::resolve(&[&base, "server-token"])
}

pub fn read_gateway_auth_token(path: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let t = text.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}');
    if t.is_empty() { None } else { Some(t.to_string()) }
}

/// Returns (token, tokenPath).
pub fn ensure_gateway_auth_token() -> std::io::Result<(String, String)> {
    let path = resolve_gateway_token_path();
    if let Some(t) = read_gateway_auth_token(&path) {
        return Ok((t, path));
    }
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    let dir = std::path::Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path)?;
        f.write_all(format!("{token}\n").as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&path, format!("{token}\n"))?;
    }
    Ok((token, path))
}
