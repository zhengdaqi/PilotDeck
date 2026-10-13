//! Race-free write inside a root directory (SRC-DEFECT-007, ADR-0004).
//!
//! It walks from an fd for the root, one path component at a time, with `openat(O_DIRECTORY|O_NOFOLLOW)`, and
//! creates missing directories with `mkdirat`. The file is opened with `openat(O_NOFOLLOW|O_CREAT)`.
//! A symlink found on the way (a directory or the final file) is followed only after its target is resolved and
//! checked to be inside the root. The walk then restarts from the root fd on the resolved relative path, so a
//! symlink swapped in after the check can never be followed. Depth is capped at 16 hops, like ELOOP.
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

pub enum WriteErr {
    Boundary,
    /// (io error, syscall, absolute path) for Node-style messages.
    Io(io::Error, &'static str, String),
}

fn cstr(s: &str) -> io::Result<CString> {
    CString::new(s).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))
}

fn openat(dir: &OwnedFd, name: &str, flags: i32, mode: u32) -> io::Result<OwnedFd> {
    let c = cstr(name)?;
    let fd = unsafe { libc::openat(dir.as_raw_fd(), c.as_ptr(), flags | libc::O_CLOEXEC, mode as libc::c_uint) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

fn is_symlink_at(dir: &OwnedFd, name: &str) -> bool {
    let Ok(c) = cstr(name) else { return false };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::fstatat(dir.as_raw_fd(), c.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };
    r == 0 && (st.st_mode & libc::S_IFMT) == libc::S_IFLNK
}

fn is_dir_at(dir: &OwnedFd, name: &str) -> Option<bool> {
    let c = cstr(name).ok()?;
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::fstatat(dir.as_raw_fd(), c.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };
    (r == 0).then_some((st.st_mode & libc::S_IFMT) == libc::S_IFDIR)
}

fn mkdirat(dir: &OwnedFd, name: &str) -> io::Result<()> {
    let c = cstr(name)?;
    if unsafe { libc::mkdirat(dir.as_raw_fd(), c.as_ptr(), 0o777) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Resolves the symlink at `<root>/<prefix>/<name>` to a root-relative path, or `Boundary` if it leaves the root
/// (or dangles in a way that cannot be checked).
fn resolve_link(root: &str, prefix: &[&str], name: &str, rest: &[&str]) -> Result<Vec<String>, WriteErr> {
    let mut p = Path::new(root).to_path_buf();
    for c in prefix {
        p.push(c);
    }
    p.push(name);
    let real = std::fs::canonicalize(&p).map_err(|_| WriteErr::Boundary)?;
    let rel = real.strip_prefix(root).map_err(|_| WriteErr::Boundary)?;
    let mut out: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    out.extend(rest.iter().map(|s| s.to_string()));
    Ok(out)
}

/// Writes `data` to `<root>/<rel components>`, creating parent directories. `root` must be canonical.
pub fn write_in_root(root: &str, rel: &[String], data: &[u8]) -> Result<(), WriteErr> {
    let mut comps: Vec<String> = rel.to_vec();
    'restart: for _hop in 0..16 {
        let root_fd = {
            let c = cstr(root).map_err(|e| WriteErr::Io(e, "open", root.into()))?;
            let fd = unsafe { libc::open(c.as_ptr(), libc::O_DIRECTORY | libc::O_RDONLY | libc::O_CLOEXEC) };
            if fd < 0 {
                return Err(WriteErr::Io(io::Error::last_os_error(), "open", root.into()));
            }
            unsafe { OwnedFd::from_raw_fd(fd) }
        };
        let Some((file, dirs)) = comps.split_last() else { return Err(WriteErr::Boundary) };
        let refs: Vec<&str> = comps.iter().map(String::as_str).collect();
        let mut cur = root_fd;
        let mut abs = root.to_string();
        for (i, d) in dirs.iter().enumerate() {
            abs = format!("{abs}/{d}");
            if is_symlink_at(&cur, d) {
                comps = resolve_link(root, &refs[..i], d, &refs[i + 1..])?;
                continue 'restart;
            }
            match is_dir_at(&cur, d) {
                Some(true) => {}
                // Node's recursive mkdir reports EEXIST at the first path component that is not a directory.
                Some(false) => return Err(WriteErr::Io(io::Error::from_raw_os_error(libc::EEXIST), "mkdir", abs)),
                None => match mkdirat(&cur, d) {
                    Ok(()) => {}
                    Err(e) if e.raw_os_error() == Some(libc::EEXIST) => {}
                    Err(e) => return Err(WriteErr::Io(e, "mkdir", abs)),
                },
            }
            cur = match openat(&cur, d, libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_RDONLY, 0) {
                Ok(fd) => fd,
                // Swapped for a symlink between the check and the open: go back through the checked path.
                Err(e) if e.raw_os_error() == Some(libc::ELOOP) || e.raw_os_error() == Some(libc::ENOTDIR) => continue 'restart,
                Err(e) => return Err(WriteErr::Io(e, "mkdir", abs)),
            };
        }
        let target = format!("{abs}/{file}");
        let fd = match openat(&cur, file, libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW, 0o666) {
            Ok(fd) => fd,
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
                comps = resolve_link(root, &refs[..dirs.len()], file, &[])?;
                continue 'restart;
            }
            Err(e) => return Err(WriteErr::Io(e, "open", target)),
        };
        let mut f = std::fs::File::from(fd);
        return io::Write::write_all(&mut f, data).map_err(|e| WriteErr::Io(e, "write", target));
    }
    Err(WriteErr::Io(io::Error::from_raw_os_error(libc::ELOOP), "open", format!("{root}/{}", rel.join("/"))))
}
