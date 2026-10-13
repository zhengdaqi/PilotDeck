//! Node.js `fs` error objects (`code`, message text) for errors that reach HTTP responses.
use std::io;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct NodeError {
    pub code: String,
    pub message: String,
    /// The `status` own property some service errors carry.
    pub status: Option<u16>,
}

impl NodeError {
    pub fn new(code: &str, message: impl Into<String>, status: Option<u16>) -> Self {
        NodeError { code: code.into(), message: message.into(), status }
    }

    /// `ENOENT: no such file or directory, lstat '/x'`.
    pub fn from_io(e: &io::Error, syscall: &str, path: &Path) -> Self {
        let (code, text) = match e.raw_os_error().map(errno_name) {
            Some(Some(pair)) => pair,
            _ => match e.kind() {
                io::ErrorKind::NotFound => ("ENOENT", "no such file or directory"),
                io::ErrorKind::PermissionDenied => ("EACCES", "permission denied"),
                io::ErrorKind::AlreadyExists => ("EEXIST", "file already exists"),
                _ => ("EIO", "i/o error"),
            },
        };
        NodeError { code: code.into(), message: format!("{code}: {text}, {syscall} '{}'", path.display()), status: None }
    }
}

/// libuv's names and messages for the errno values these routes can hit.
fn errno_name(n: i32) -> Option<(&'static str, &'static str)> {
    #[cfg(unix)]
    {
        Some(match n {
            libc_errno::ENOENT => ("ENOENT", "no such file or directory"),
            libc_errno::ENOTDIR => ("ENOTDIR", "not a directory"),
            libc_errno::EACCES => ("EACCES", "permission denied"),
            libc_errno::EEXIST => ("EEXIST", "file already exists"),
            libc_errno::ELOOP => ("ELOOP", "too many symbolic links encountered"),
            libc_errno::ENAMETOOLONG => ("ENAMETOOLONG", "name too long"),
            libc_errno::EISDIR => ("EISDIR", "illegal operation on a directory"),
            libc_errno::EXDEV => ("EXDEV", "cross-device link not permitted"),
            _ => return None,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = n;
        None
    }
}

#[cfg(unix)]
#[allow(dead_code)]
mod libc_errno {
    // Linux / macOS values (identical for these).
    pub const ENOENT: i32 = 2;
    pub const EACCES: i32 = 13;
    pub const EEXIST: i32 = 17;
    pub const EXDEV: i32 = 18;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    #[cfg(target_os = "linux")]
    pub const ENAMETOOLONG: i32 = 36;
    #[cfg(not(target_os = "linux"))]
    pub const ENAMETOOLONG: i32 = 63;
    #[cfg(target_os = "linux")]
    pub const ELOOP: i32 = 40;
    #[cfg(not(target_os = "linux"))]
    pub const ELOOP: i32 = 62;
}
