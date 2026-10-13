//! Node `path.posix` helpers: resolve, relative, basename, isAbsolute.
pub use pilotdeck_session_search::paths::resolve;

pub fn is_absolute(p: &str) -> bool {
    p.starts_with('/')
}

pub fn relative(from: &str, to: &str) -> String {
    let f = resolve(&[from]);
    let t = resolve(&[to]);
    if f == t {
        return String::new();
    }
    let fs_: Vec<&str> = f.split('/').filter(|x| !x.is_empty()).collect();
    let ts: Vec<&str> = t.split('/').filter(|x| !x.is_empty()).collect();
    let mut i = 0;
    while i < fs_.len() && i < ts.len() && fs_[i] == ts[i] {
        i += 1;
    }
    let mut out: Vec<&str> = std::iter::repeat("..").take(fs_.len() - i).collect();
    out.extend(&ts[i..]);
    out.join("/")
}

/// `path.posix.basename(p)`: trailing slashes ignored; "" for "/" or "".
pub fn basename(p: &str) -> String {
    let t = p.trim_end_matches('/');
    match t.rfind('/') {
        Some(i) => t[i + 1..].to_string(),
        None => t.to_string(),
    }
}
