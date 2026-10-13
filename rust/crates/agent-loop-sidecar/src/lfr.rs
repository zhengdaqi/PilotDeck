//! Port of `LargeFileRepair` (src/agent/loop/LargeFileRepair.ts).
use crate::json::*;

pub enum Decision {
    Continue { prompt: String, purpose: String, strip: Option<&'static str> },
    Stop { reason: String },
}

const MAX_PRE_DRAFT_REPAIR_ATTEMPTS: u32 = 5;
const MAX_POST_DRAFT_REPAIR_ATTEMPTS: u32 = 5;
const MAX_TRUNCATION_RECOVERIES: u32 = 10;
pub const LARGE_FILE_OUTPUT_RETRY_TOKENS: f64 = 16_384.0;
const INTERNAL_RECOVERY_HINT: &str = "INTERNAL RECOVERY HINT - not a user request. Continue the original task.";

fn is_file_write(name: &str) -> bool {
    name == "write_file" || name == "edit_file"
}

#[derive(Default)]
pub struct LargeFileRepair {
    pre_draft_attempts: u32,
    post_draft_attempts: u32,
    truncation_recoveries: u32,
    wrote_file: bool,
    pending: bool,
    recent_file_paths: Vec<String>,
}

pub struct ToolContext<'a> {
    pub output_truncated: bool,
    pub repaired_tool_calls: bool,
    pub finish_reason: Option<&'a str>,
}

impl LargeFileRepair {
    pub fn has_pending_repair(&self) -> bool {
        self.pending
    }

    pub fn on_invalid_tool_input(&mut self) -> Option<Decision> {
        if !self.pending {
            return None;
        }
        if self.wrote_file {
            return Some(self.try_post_draft("large_file_invalid_input_after_write"));
        }
        Some(self.try_pre_draft("large_file_invalid_input", "error_pair"))
    }

    pub fn on_no_tool_calls(&mut self) -> Option<Decision> {
        if !self.pending || self.wrote_file {
            return None;
        }
        Some(self.try_pre_draft("large_file_no_tool_call", "assistant"))
    }

    pub fn analyze_tool_results(&mut self, results: &[JsValue], ctx: &ToolContext) -> Option<Decision> {
        self.record_writes(results);
        if self.wrote_file {
            if !has_post_draft_risk(results, ctx, self.pending) {
                if results.iter().all(|r| gs(r, "type") == Some("success")) || results.iter().any(is_successful_file_write) {
                    self.pending = false;
                }
                return None;
            }
            self.pending = true;
            return Some(self.try_post_draft("large_file_post_draft_repair"));
        }
        if has_pre_draft_risk(results, ctx) {
            self.pending = true;
            return Some(self.try_pre_draft("large_file_pre_draft_repair", "error_pair"));
        }
        None
    }

    pub fn recover_from_repaired_truncation(&mut self, tool_calls: &[JsValue]) -> Option<Decision> {
        if !tool_calls.iter().any(|c| is_file_write(gs(c, "name").unwrap_or(""))) {
            return None;
        }
        if self.truncation_recoveries >= MAX_TRUNCATION_RECOVERIES {
            return None;
        }
        self.truncation_recoveries += 1;
        self.pending = true;
        if self.wrote_file {
            return Some(Decision::Continue { purpose: "large_file_repaired_truncation_after_write".into(), strip: Some("assistant"),
                prompt: post_draft_prompt(&self.recent_file_paths, self.post_draft_attempts) });
        }
        Some(Decision::Continue { purpose: "large_file_repaired_truncation".into(), strip: Some("assistant"),
            prompt: pre_draft_prompt(self.pre_draft_attempts + 1) })
    }

    fn try_pre_draft(&mut self, purpose: &str, strip: &'static str) -> Decision {
        if self.pre_draft_attempts >= MAX_PRE_DRAFT_REPAIR_ATTEMPTS {
            return Decision::Stop { reason: format!("Large file repair failed before any workspace file was created after {} attempts.", self.pre_draft_attempts) };
        }
        self.pre_draft_attempts += 1;
        Decision::Continue { purpose: purpose.into(), strip: Some(strip), prompt: pre_draft_prompt(self.pre_draft_attempts) }
    }

    fn try_post_draft(&mut self, purpose: &str) -> Decision {
        if self.post_draft_attempts >= MAX_POST_DRAFT_REPAIR_ATTEMPTS {
            return Decision::Stop { reason: format!("Large file repair stopped after {} post-draft attempts. A workspace file already exists; report the current file path and remaining gap.", self.post_draft_attempts) };
        }
        self.post_draft_attempts += 1;
        Decision::Continue { purpose: purpose.into(), strip: None, prompt: post_draft_prompt(&self.recent_file_paths, self.post_draft_attempts) }
    }

    fn record_writes(&mut self, results: &[JsValue]) {
        for r in results {
            if gs(r, "type") != Some("success") || !is_file_write(gs(r, "toolName").unwrap_or("")) {
                continue;
            }
            self.wrote_file = true;
            let data = get(r, "data");
            if let (true, Some(fp)) = (is_obj(data), gs(data, "filePath")) {
                if !fp.is_empty() {
                    let mut next = vec![fp.to_string()];
                    next.extend(self.recent_file_paths.iter().filter(|p| p.as_str() != fp).cloned());
                    next.truncate(5);
                    self.recent_file_paths = next;
                }
            }
        }
    }
}

fn pre_draft_prompt(attempt: u32) -> String {
    let last = attempt >= MAX_PRE_DRAFT_REPAIR_ATTEMPTS;
    let max_lines = if attempt <= 2 { 80 } else if attempt <= 4 { 40 } else { 20 };
    let lines = [
        INTERNAL_RECOVERY_HINT.to_string(),
        String::new(),
        format!("The previous file write/edit attempt appears incomplete or invalid, likely because the model output was truncated. Recovery attempt {attempt}/{MAX_PRE_DRAFT_REPAIR_ATTEMPTS}."),
        format!("Use one small direct file tool call. Keep write_file content under {max_lines} lines."),
        String::new(),
        "Recovery guidance:".into(),
        "1. Continue the original task; do not treat this hint as a new user request.".into(),
        "2. Provide the required tool parameters such as file_path and content.".into(),
        "3. Create a minimal structurally valid skeleton first:".into(),
        "   - HTML: doctype, head, minimal style, body with one section, closing tags.".into(),
        "   - Code: imports, one function/class stub, exports if needed.".into(),
        "   - Prose/report: title, first paragraph, and a clear continuation marker.".into(),
        "4. After a file exists, extend it incrementally with focused edit_file calls.".into(),
        "5. Do not attempt to generate the complete artifact in one oversized call.".into(),
        String::new(),
        "Prefer write_file for the initial skeleton. Avoid shell heredocs or echo for this recovery step.".into(),
        if last { "Final recovery attempt: write the smallest viable skeleton, even if it only establishes the file structure.".into() } else { String::new() },
    ];
    // `.filter(Boolean)` drops the empty separator lines too.
    lines.into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

fn post_draft_prompt(paths: &[String], attempt: u32) -> String {
    let file_text = if !paths.is_empty() {
        format!("Recent file path context for orientation only: {}.", paths.join(", "))
    } else {
        "A workspace draft already exists.".to_string()
    };
    let last = attempt >= MAX_POST_DRAFT_REPAIR_ATTEMPTS;
    let lines = [
        INTERNAL_RECOVERY_HINT.to_string(),
        String::new(),
        file_text,
        String::new(),
        format!("Post-draft recovery attempt {attempt}/{MAX_POST_DRAFT_REPAIR_ATTEMPTS}: continue the original task from the current workspace state."),
        String::new(),
        "Recovery guidance:".into(),
        "1. If current content matters, inspect the file first with read_file.".into(),
        "2. Add or replace one small section at a time with edit_file; keep new content under 60 lines.".into(),
        "3. Do not rewrite the whole file or regenerate from scratch.".into(),
        "4. If you just wrote a script or generator, run it and verify the produced output before asking for clarification.".into(),
        "5. Ask the user only for a real blocker that cannot be resolved from the workspace or the original task.".into(),
        String::new(),
        "Use a focused edit_file replacement or insertion point such as a continuation marker when available.".into(),
        if last { "Final recovery attempt: make one focused edit, run/verify if applicable, or explain the concrete blocker.".into() } else { String::new() },
    ];
    lines.into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

fn read_issue_codes(r: &JsValue) -> Vec<String> {
    if gs(r, "type") != Some("error") {
        return Vec::new();
    }
    ga(get(get(r, "error"), "details"), "issues").iter().filter(|i| is_obj(i))
        .map(|i| gs(i, "code").unwrap_or("").to_string()).collect()
}

fn has_pre_draft_risk(results: &[JsValue], ctx: &ToolContext) -> bool {
    results.iter().any(|r| {
        if gs(r, "type") != Some("error") {
            return false;
        }
        let name = gs(r, "toolName").unwrap_or("");
        if !is_file_write(name) {
            return false;
        }
        let required = read_issue_codes(r).iter().any(|c| c == "required");
        if name == "write_file" && required {
            return true;
        }
        if ctx.output_truncated && required {
            return true;
        }
        looks_like_large_file_error(&crate::rt::js_string(get(get(r, "error"), "message")))
    })
}

fn has_post_draft_risk(results: &[JsValue], ctx: &ToolContext, pending: bool) -> bool {
    let explicit = ctx.output_truncated || ctx.repaired_tool_calls || ctx.finish_reason == Some("length");
    results.iter().any(|r| {
        if gs(r, "type") != Some("error") || !is_file_write(gs(r, "toolName").unwrap_or("")) {
            return false;
        }
        let code = gs(get(r, "error"), "code");
        if matches!(code, Some("permission_denied" | "permission_required" | "permission_cancelled")) {
            return false;
        }
        pending || explicit || code == Some("result_too_large")
            || looks_like_large_file_error(&crate::rt::js_string(get(get(r, "error"), "message")))
    })
}

fn is_successful_file_write(r: &JsValue) -> bool {
    gs(r, "type") == Some("success") && is_file_write(gs(r, "toolName").unwrap_or(""))
}

/// `/(?:output token|truncated|too large|large file|large artifact|max_output|missing required parameter `content`|required parameter `content` is missing)/iu`
pub fn looks_like_large_file_error(message: &str) -> bool {
    let m = message.to_lowercase();
    ["output token", "truncated", "too large", "large file", "large artifact", "max_output",
        "missing required parameter `content`", "required parameter `content` is missing"].iter().any(|p| m.contains(p))
}
