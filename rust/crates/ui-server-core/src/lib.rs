//! Pure service layer of `ui/server` (the Node web backend), ported service by service.
pub mod js_object;
pub mod js_number;
pub mod frontmatter;
pub mod js_yaml;
pub mod model_references;
pub mod command_parser;
pub mod node_path;
pub mod shell_quote;
pub mod settings_files;

/// Node's fs error message: `<CODE>: <description>, <syscall> '<path>'`.
pub fn node_fs_message(e: &std::io::Error, syscall: &str, path: &str) -> String {
    let (code, desc) = match e.raw_os_error() {
        Some(2) => ("ENOENT", "no such file or directory"),
        Some(13) => ("EACCES", "permission denied"),
        Some(20) => ("ENOTDIR", "not a directory"),
        Some(21) => ("EISDIR", "illegal operation on a directory"),
        Some(17) => ("EEXIST", "file already exists"),
        _ => return e.to_string(),
    };
    format!("{code}: {desc}, {syscall} '{path}'")
}
