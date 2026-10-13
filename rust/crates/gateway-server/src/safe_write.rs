//! Race-free write inside a root directory (SRC-DEFECT-007, ADR-0004).
//!
//! Unix (`safe_write_unix.rs`):
//! - It walks from an fd for the root, one component at a time, with `openat(O_DIRECTORY|O_NOFOLLOW)`, and creates
//!   missing directories with `mkdirat`. The file is opened with `openat(O_NOFOLLOW|O_CREAT)`.
//! - A symlink found on the way is followed only after its target is resolved and checked to be inside the root.
//!   The walk then restarts from the root fd on the resolved relative path, so a symlink swapped in after the check
//!   is never followed.
//!
//! Windows (`safe_write_windows.rs`):
//! - Every directory on the way is opened with `FILE_FLAG_OPEN_REPARSE_POINT` (never follows a symlink or junction)
//!   and its handle is held without `FILE_SHARE_DELETE`. While a directory is held it cannot be renamed, deleted or
//!   replaced by a junction.
//! - Reparse points (symlinks, junctions) are resolved and checked against the root, and the walk restarts.
//! - The file is also opened with `FILE_FLAG_OPEN_REPARSE_POINT`. It is checked not to be a reparse point before it
//!   is truncated and written.
//!
//! Both cap the walk at 16 restarts, like ELOOP.
use std::io;

pub enum WriteErr {
    Boundary,
    /// (io error, syscall, absolute path) for Node-style messages.
    Io(io::Error, &'static str, String),
}

#[cfg(unix)]
#[path = "safe_write_unix.rs"]
mod imp;
#[cfg(windows)]
#[path = "safe_write_windows.rs"]
mod imp;

pub use imp::write_in_root;
