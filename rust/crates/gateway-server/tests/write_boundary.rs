//! SRC-DEFECT-007 fix (ADR-0004 intended difference): a write must not escape the project root through symlinks.
use pilotdeck_gateway_server::files::{FileErr, ProjectFileService};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn setup(tag: &str) -> (Tmp, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("pd-wb-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let root = base.join("root");
    let outside = base.join("outside");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    fs::write(root.join("src/a.ts"), "a").unwrap();
    symlink(&outside, root.join("link_out")).unwrap();
    symlink(outside.join("secret.txt"), root.join("secret_link.txt")).unwrap();
    symlink(outside.join("not-yet.txt"), root.join("dangling_out.txt")).unwrap();
    symlink(outside.join("no-dir"), root.join("dangling_dir")).unwrap();
    symlink(root.join("src"), root.join("link_in")).unwrap();
    (Tmp(base), root, outside)
}

fn boundary(r: Result<(), FileErr>) -> bool {
    matches!(r, Err(FileErr::Boundary(_)))
}

fn listing(p: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(p).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn writes_through_symlinks_out_of_root_are_refused() {
    let (_t, root, outside) = setup("refuse");
    let svc = ProjectFileService::new(root.to_str().unwrap());
    for rel in ["link_out/planted.txt", "secret_link.txt", "dangling_out.txt", "link_out/new/deep/x.txt", "dangling_dir/x.txt", "src/../link_out/p.txt"] {
        assert!(boundary(svc.write_file(rel, "pwn", false)), "{rel} must be refused");
    }
    assert_eq!(listing(&outside), vec!["secret.txt"], "nothing created outside the root (no files, no dirs)");
    assert_eq!(fs::read_to_string(outside.join("secret.txt")).unwrap(), "secret");
}

#[test]
fn writes_inside_root_still_work() {
    let (_t, root, _o) = setup("allow");
    let svc = ProjectFileService::new(root.to_str().unwrap());
    assert!(svc.write_file("new/dir/n.txt", "hello", false).is_ok());
    assert!(svc.write_file("link_in/b.ts", "via in-root link", false).is_ok());
    assert!(svc.write_file("src/a.ts", "overwrite", false).is_ok());
    assert_eq!(fs::read_to_string(root.join("new/dir/n.txt")).unwrap(), "hello");
    assert_eq!(fs::read_to_string(root.join("src/b.ts")).unwrap(), "via in-root link");
    assert_eq!(fs::read_to_string(root.join("src/a.ts")).unwrap(), "overwrite");
    assert!(boundary(svc.write_file("../evil.txt", "x", false)));
}

/// Negative control: the fixture really escapes when the boundary check is skipped (what TS does:
/// lexical check, then fs.writeFile follows the link), so the test above would catch a regression.
#[test]
fn negative_control_unchecked_write_escapes() {
    let (_t, root, outside) = setup("control");
    fs::write(root.join("link_out/planted.txt"), "x").unwrap();
    fs::write(root.join("secret_link.txt"), "y").unwrap();
    assert_eq!(listing(&outside), vec!["planted.txt", "secret.txt"]);
    assert_eq!(fs::read_to_string(outside.join("secret.txt")).unwrap(), "y");
}
