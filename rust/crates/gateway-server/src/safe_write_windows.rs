//! Windows implementation of `safe_write` (see the module docs there). Not yet run on Windows (pending Windows
//! verification, ADR-0004 production gate). Compile-checked with `--target x86_64-pc-windows-gnu`.
use super::WriteErr;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const ERROR_ALREADY_EXISTS: i32 = 183;
const ERROR_FILE_EXISTS: i32 = 80;

/// Opens a directory without following reparse points, and holds it so it cannot be renamed or deleted.
fn open_dir_pinned(p: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(p)
}

fn attrs(f: &File) -> io::Result<u32> {
    Ok(f.metadata()?.file_attributes())
}

/// Resolves the reparse point at `p` to root-relative components plus `rest`, or `Boundary` if it leaves the root.
fn resolve_link(root: &Path, p: &Path, rest: &[String]) -> Result<Vec<String>, WriteErr> {
    let real = std::fs::canonicalize(p).map_err(|_| WriteErr::Boundary)?;
    let real_root = std::fs::canonicalize(root).map_err(|_| WriteErr::Boundary)?;
    let rel = real.strip_prefix(&real_root).map_err(|_| WriteErr::Boundary)?;
    let mut out: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    out.extend(rest.iter().cloned());
    Ok(out)
}

/// Writes `data` to `<root>/<rel components>`, creating parent directories. `root` must be canonical.
pub fn write_in_root(root: &str, rel: &[String], data: &[u8]) -> Result<(), WriteErr> {
    let root_path = PathBuf::from(root);
    let mut comps: Vec<String> = rel.to_vec();
    'restart: for _hop in 0..16 {
        let Some((file, dirs)) = comps.split_last() else { return Err(WriteErr::Boundary) };
        let (file, dirs) = (file.clone(), dirs.to_vec());
        // Handles of every directory on the way, held until the write is done.
        let mut pinned: Vec<File> = vec![open_dir_pinned(&root_path).map_err(|e| WriteErr::Io(e, "open", root.into()))?];
        let mut cur = root_path.clone();
        for (i, d) in dirs.iter().enumerate() {
            cur.push(d);
            let abs = cur.to_string_lossy().into_owned();
            let h = match open_dir_pinned(&cur) {
                Ok(h) => h,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    match std::fs::create_dir(&cur) {
                        Ok(()) => {}
                        Err(e) if matches!(e.raw_os_error(), Some(ERROR_ALREADY_EXISTS) | Some(ERROR_FILE_EXISTS)) => {}
                        Err(e) => return Err(WriteErr::Io(e, "mkdir", abs)),
                    }
                    open_dir_pinned(&cur).map_err(|e| WriteErr::Io(e, "mkdir", abs.clone()))?
                }
                Err(e) => return Err(WriteErr::Io(e, "mkdir", abs)),
            };
            let a = attrs(&h).map_err(|e| WriteErr::Io(e, "mkdir", abs.clone()))?;
            if a & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                drop(h);
                comps = resolve_link(&root_path, &cur, &comps[i + 1..])?;
                continue 'restart;
            }
            if a & FILE_ATTRIBUTE_DIRECTORY == 0 {
                return Err(WriteErr::Io(io::Error::from(io::ErrorKind::AlreadyExists), "mkdir", abs));
            }
            pinned.push(h);
        }
        cur.push(&file);
        let target = cur.to_string_lossy().into_owned();
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&cur)
            .map_err(|e| WriteErr::Io(e, "open", target.clone()))?;
        let a = attrs(&f).map_err(|e| WriteErr::Io(e, "open", target.clone()))?;
        if a & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            drop(f);
            comps = resolve_link(&root_path, &cur, &[])?;
            continue 'restart;
        }
        let r = f.set_len(0).and_then(|_| f.seek(io::SeekFrom::Start(0))).and_then(|_| f.write_all(data));
        drop(pinned);
        return r.map(|_| ()).map_err(|e| WriteErr::Io(e, "write", target));
    }
    Err(WriteErr::Io(io::Error::other("too many levels of symbolic links"), "open", format!("{root}\\{}", rel.join("\\"))))
}
