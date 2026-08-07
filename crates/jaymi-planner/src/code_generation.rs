//! Code Generation from approved Coding Plans (Sprint C1.5).
//!
//! Generation produces typed file operations ([`GenerationOpKind::CreateFile`],
//! [`ModifyFile`](GenerationOpKind::ModifyFile), [`DeleteFile`](GenerationOpKind::DeleteFile)).
//! The Planner alone converts those operations into [`crate::ExecutionPlan`]s
//! that must pass Review Before Action before tools run.
//!
//! Constitutional constraints:
//! - No provider writes directly
//! - No LLM edits directly
//! - Planner owns all mutations (ops → Execution Plan → Review → Tool)
//! - Review Before Action is mandatory for generation batches

use std::path::{Path, PathBuf};

use jaymi_core::{DeletionMethod, UserRequest};
use jaymi_tools::{ToolInput, MANAGE_PATH_TOOL_ID, WRITE_FILE_TOOL_ID};

use crate::coding_plan::CodingPlan;

/// Kind of file mutation proposed by generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationOpKind {
    /// Create a new file (write_file when absent).
    CreateFile,
    /// Overwrite / update an existing file (write_file).
    ModifyFile,
    /// Delete a path (manage_path delete).
    DeleteFile,
}

impl GenerationOpKind {
    /// Stable id for diagnostics / Review Card steps.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreateFile => "create_file",
            Self::ModifyFile => "modify_file",
            Self::DeleteFile => "delete_file",
        }
    }

    /// Human label for plan steps.
    pub fn label(self) -> &'static str {
        match self {
            Self::CreateFile => "Create file",
            Self::ModifyFile => "Modify file",
            Self::DeleteFile => "Delete file",
        }
    }
}

/// One Planner-owned generation operation (never executed by providers/LLMs).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GenerationOp {
    /// Create / modify / delete.
    pub kind: GenerationOpKind,
    /// Absolute or workspace-relative path the Planner resolved.
    pub path: PathBuf,
    /// File body for create/modify (Planner-authored; not a live LLM edit).
    pub content: Option<String>,
    /// Why this op is in the batch (from Coding Plan / goal).
    pub rationale: String,
}

impl GenerationOp {
    /// Build a create-file op.
    pub fn create_file(
        path: impl Into<PathBuf>,
        content: impl Into<String>,
        rationale: impl Into<String>,
    ) -> Self {
        Self {
            kind: GenerationOpKind::CreateFile,
            path: path.into(),
            content: Some(content.into()),
            rationale: rationale.into(),
        }
    }

    /// Build a modify-file op.
    pub fn modify_file(
        path: impl Into<PathBuf>,
        content: impl Into<String>,
        rationale: impl Into<String>,
    ) -> Self {
        Self {
            kind: GenerationOpKind::ModifyFile,
            path: path.into(),
            content: Some(content.into()),
            rationale: rationale.into(),
        }
    }

    /// Build a delete-file op.
    pub fn delete_file(path: impl Into<PathBuf>, rationale: impl Into<String>) -> Self {
        Self {
            kind: GenerationOpKind::DeleteFile,
            path: path.into(),
            content: None,
            rationale: rationale.into(),
        }
    }

    /// Step description for an Execution Plan.
    pub fn step_description(&self) -> String {
        format!(
            "{} `{}` — {}",
            self.kind.label(),
            self.path.display(),
            self.rationale
        )
    }
}

/// Batch of generation operations derived from an approved Coding Plan (or explicit ops).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodeGeneration {
    /// User / Coding Plan goal.
    pub goal: String,
    /// Ordered Create / Modify / Delete operations.
    pub operations: Vec<GenerationOp>,
    /// Optional markdown summary carried from the Coding Plan.
    pub coding_plan_summary: Vec<String>,
}

impl CodeGeneration {
    /// True when there are no operations.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Conversation / diagnostics markdown (proposal only until reviewed).
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Code Generation\n");
        out.push_str("\n### Goal\n");
        out.push_str("- ");
        out.push_str(if self.goal.is_empty() {
            "(none)"
        } else {
            &self.goal
        });
        out.push('\n');

        out.push_str("\n### Operations\n");
        if self.operations.is_empty() {
            out.push_str("- _(none)_\n");
        } else {
            for (index, op) in self.operations.iter().enumerate() {
                out.push_str(&format!(
                    "- {}. **{}** `{}` — {}\n",
                    index + 1,
                    op.kind.as_str(),
                    op.path.display(),
                    op.rationale
                ));
            }
        }

        out.push_str(
            "\n### Review\n- Review Before Action is required — tools run only after Approve.\n",
        );
        out.push_str(
            "- Providers and Reasoning never write files; the Planner owns mutations.\n",
        );
        out
    }
}

/// Tool binding for one generation op (Planner → ToolInput only).
#[derive(Debug, Clone)]
pub struct GenerationToolCall {
    /// Registered tool id (`write_file` / `manage_path`).
    pub tool_id: String,
    /// Frozen tool input.
    pub input: ToolInput,
    /// Resource path for gates / plan.
    pub path: PathBuf,
    /// Permission action for the gate.
    pub permission_action: jaymi_permissions::PermissionAction,
    /// Step label.
    pub action_label: String,
}

/// Detect “generate / implement the (coding) plan” — not C1.4 planning.
pub fn detect_generation_request(request: &UserRequest) -> bool {
    if request.coding_action.is_some()
        || request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
        || request.file.is_some()
        || request.directory.is_some()
    {
        return false;
    }
    let lower = request.content.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    looks_like_apply_generation(&lower)
}

fn looks_like_apply_generation(lower: &str) -> bool {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '!');
    matches!(
        trimmed,
        "generate the code"
            | "generate code"
            | "generate it"
            | "generate this"
            | "implement the plan"
            | "implement the coding plan"
            | "apply the plan"
            | "apply the coding plan"
            | "execute the coding plan"
            | "run the coding plan"
    ) || trimmed.starts_with("generate the code ")
        || trimmed.starts_with("generate code for")
        || trimmed.starts_with("implement the coding plan")
        || trimmed.starts_with("apply the coding plan")
}

/// True when a phrase should stay on C1.5 generation, not C1.4 Coding Plan detect.
pub fn steals_from_coding_plan_detect(lower: &str) -> bool {
    looks_like_apply_generation(lower.trim_end_matches(|c: char| c == '.' || c == '!'))
}

/// Materialize Planner-owned ops from a Coding Plan (deterministic stubs; no FS / LLM writes).
pub fn generation_from_coding_plan(
    plan: &CodingPlan,
    goal: impl Into<String>,
    project_root: Option<&Path>,
) -> CodeGeneration {
    let goal = goal.into();
    let root = project_root
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut operations = Vec::new();

    for (index, proposal) in plan.files_to_create.iter().enumerate() {
        if let Some(rel) = extract_path_candidate(proposal) {
            let path = resolve_under_root(&root, &rel);
            let content = stub_content_for_create(&path, &goal, index);
            operations.push(GenerationOp::create_file(
                path,
                content,
                format!("Coding Plan create proposal: {proposal}"),
            ));
        }
    }

    for (index, proposal) in plan.files_to_modify.iter().enumerate() {
        if let Some(rel) = extract_path_candidate(proposal) {
            let path = resolve_under_root(&root, &rel);
            // Skip duplicate paths already queued as creates.
            if operations.iter().any(|op| op.path == path) {
                continue;
            }
            let content = stub_content_for_modify(&path, &goal, index);
            operations.push(GenerationOp::modify_file(
                path,
                content,
                format!("Coding Plan modify proposal: {proposal}"),
            ));
        }
    }

    // Optional explicit deletes in plan text (rare).
    for proposal in plan
        .plan_steps
        .iter()
        .chain(plan.summary.iter())
        .chain(plan.files_to_modify.iter())
    {
        let lower = proposal.to_ascii_lowercase();
        if lower.contains("delete ") || lower.contains("remove file") {
            if let Some(rel) = extract_path_candidate(proposal) {
                let path = resolve_under_root(&root, &rel);
                if !operations
                    .iter()
                    .any(|op| op.path == path && op.kind == GenerationOpKind::DeleteFile)
                {
                    operations.push(GenerationOp::delete_file(
                        path,
                        format!("Coding Plan delete proposal: {proposal}"),
                    ));
                }
            }
        }
    }

    // If the plan named no concrete paths, still produce a minimal create under root.
    if operations.is_empty() {
        let fallback = root.join(fallback_create_rel(&goal));
        operations.push(GenerationOp::create_file(
            fallback,
            stub_content_for_create(Path::new("generated.md"), &goal, 0),
            String::from(
                "Coding Plan had no concrete paths — Planner proposed a minimal create.",
            ),
        ));
    }

    CodeGeneration {
        goal,
        operations,
        coding_plan_summary: plan.summary.clone(),
    }
}

/// Bind each op to a ToolInput (Planner → tools only).
pub fn tool_calls_for_generation(
    generation: &CodeGeneration,
    deletion_method: DeletionMethod,
) -> Result<Vec<GenerationToolCall>, String> {
    let mut calls = Vec::with_capacity(generation.operations.len());
    for op in &generation.operations {
        calls.push(tool_call_for_op(op, deletion_method)?);
    }
    if calls.is_empty() {
        return Err("code generation has no operations".into());
    }
    Ok(calls)
}

fn tool_call_for_op(
    op: &GenerationOp,
    deletion_method: DeletionMethod,
) -> Result<GenerationToolCall, String> {
    match op.kind {
        GenerationOpKind::CreateFile | GenerationOpKind::ModifyFile => {
            let content = op
                .content
                .clone()
                .ok_or_else(|| format!("{} requires content", op.kind.as_str()))?;
            Ok(GenerationToolCall {
                tool_id: WRITE_FILE_TOOL_ID.to_string(),
                input: ToolInput::write_file(op.path.clone(), content),
                path: op.path.clone(),
                permission_action: jaymi_permissions::PermissionAction::Write,
                action_label: op.step_description(),
            })
        }
        GenerationOpKind::DeleteFile => Ok(GenerationToolCall {
            tool_id: MANAGE_PATH_TOOL_ID.to_string(),
            input: ToolInput::manage_delete(op.path.clone(), deletion_method),
            path: op.path.clone(),
            permission_action: jaymi_permissions::PermissionAction::Delete,
            action_label: op.step_description(),
        }),
    }
}

fn extract_path_candidate(text: &str) -> Option<String> {
    // Prefer `backtick` paths.
    if let Some(start) = text.find('`') {
        let rest = &text[start + 1..];
        if let Some(end) = rest.find('`') {
            let inner = rest[..end].trim();
            if looks_like_path(inner) {
                return Some(normalize_rel(inner));
            }
        }
    }
    // Fallback: first token with a slash or extension.
    for token in text.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            matches!(c, ',' | '.' | ';' | ':' | ')' | '(' | '"' | '\'' | '*')
        });
        if looks_like_path(cleaned) {
            return Some(normalize_rel(cleaned));
        }
    }
    None
}

fn looks_like_path(s: &str) -> bool {
    if s.is_empty() || s.contains(' ') {
        return false;
    }
    if s == "…" || s.contains("…") {
        return false;
    }
    s.contains('/')
        || s.contains('\\')
        || s.ends_with(".rs")
        || s.ends_with(".ts")
        || s.ends_with(".js")
        || s.ends_with(".py")
        || s.ends_with(".md")
        || s.ends_with(".toml")
        || s.ends_with(".json")
}

fn normalize_rel(s: &str) -> String {
    let s = s.trim().trim_start_matches("./");
    // Drop trailing path markers like `module` prose after slash-ellipsis forms.
    if s.ends_with('/') {
        format!("{s}generated_module.rs")
    } else {
        s.to_string()
    }
}

fn resolve_under_root(root: &Path, rel: &str) -> PathBuf {
    let candidate = PathBuf::from(rel);
    if candidate.is_absolute() {
        candidate
    } else {
        root.join(candidate)
    }
}

fn fallback_create_rel(goal: &str) -> String {
    let slug = goal
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let slug = slug.trim_matches('_');
    let slug = if slug.is_empty() {
        "generated"
    } else {
        slug
    };
    format!("{}.md", slug.chars().take(40).collect::<String>())
}

fn stub_content_for_create(path: &Path, goal: &str, index: usize) -> String {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("generated");
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => format!(
            "// Generated by Jaymi Planner (C1.5) — review required before write.\n\
             // Goal: {goal}\n\
             // File: {name} (create #{})\n\n\
             pub fn placeholder() {{\n    // TODO: implement\n}}\n",
            index + 1
        ),
        Some("ts") | Some("js") => format!(
            "// Generated by Jaymi Planner (C1.5) — review required before write.\n\
             // Goal: {goal}\n\
             export function placeholder() {{\n  // TODO: implement\n}}\n"
        ),
        Some("py") => format!(
            "# Generated by Jaymi Planner (C1.5) — review required before write.\n\
             # Goal: {goal}\n\n\
             def placeholder():\n    \"\"\"TODO: implement\"\"\"\n    pass\n"
        ),
        Some("toml") => format!(
            "# Generated by Jaymi Planner (C1.5) — review required before write.\n\
             # Goal: {goal}\n\n\
             [package]\nname = \"generated\"\nversion = \"0.1.0\"\n"
        ),
        _ => format!(
            "# Generated by Jaymi Planner (C1.5)\n\n\
             Goal: {goal}\n\n\
             This file was proposed from an approved Coding Plan. \
             Review Before Action gates the write — providers and LLMs do not edit directly.\n"
        ),
    }
}

fn stub_content_for_modify(path: &Path, goal: &str, index: usize) -> String {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    format!(
        "// Jaymi Planner generation modify (C1.5) — op #{}\n\
         // Target: {name}\n\
         // Goal: {goal}\n\
         // NOTE: Full merge with prior contents requires a later read+merge sprint;\n\
         // this Planner-authored body is the proposed write after Review Approve.\n",
        index + 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_plan::CodingPlan;

    #[test]
    fn detects_generate_phrases() {
        assert!(detect_generation_request(&UserRequest::new(
            "Generate the code."
        )));
        assert!(detect_generation_request(&UserRequest::new(
            "Implement the coding plan"
        )));
        assert!(detect_generation_request(&UserRequest::new(
            "Apply the plan."
        )));
        assert!(!detect_generation_request(&UserRequest::new("Build Pong.")));
        assert!(!detect_generation_request(&UserRequest::new(
            "Write tests."
        )));
        assert!(!detect_generation_request(&UserRequest::write_file(
            "/tmp/a.rs",
            "x"
        )));
    }

    #[test]
    fn materializes_create_and_modify_ops() {
        let plan = CodingPlan {
            files_to_create: vec!["`src/pong.rs` entrypoint (proposal only).".into()],
            files_to_modify: vec!["Possibly `/proj/main.rs` if wiring.".into()],
            summary: vec!["Goal: Build Pong.".into()],
            ..Default::default()
        };
        let gen = generation_from_coding_plan(&plan, "Build Pong.", Some(Path::new("/proj")));
        assert!(gen.operations.iter().any(|op| {
            op.kind == GenerationOpKind::CreateFile && op.path.ends_with("src/pong.rs")
        }));
        assert!(gen.operations.iter().any(|op| {
            op.kind == GenerationOpKind::ModifyFile && op.path.ends_with("main.rs")
        }));
        let calls = tool_calls_for_generation(&gen, DeletionMethod::Trash).unwrap();
        assert_eq!(calls.len(), gen.operations.len());
        assert!(calls.iter().any(|c| c.tool_id == WRITE_FILE_TOOL_ID));
    }

    #[test]
    fn delete_op_binds_manage_path() {
        let op = GenerationOp::delete_file("/proj/old.rs", "remove stale module");
        let call = tool_call_for_op(&op, DeletionMethod::Trash).unwrap();
        assert_eq!(call.tool_id, MANAGE_PATH_TOOL_ID);
        assert_eq!(call.input.command.as_deref(), Some("delete"));
    }

    #[test]
    fn markdown_lists_operations() {
        let gen = CodeGeneration {
            goal: "Build Pong.".into(),
            operations: vec![GenerationOp::create_file(
                "/proj/src/pong.rs",
                "// stub\n",
                "sample",
            )],
            coding_plan_summary: vec![],
        };
        let md = gen.to_markdown();
        assert!(md.contains("## Code Generation"));
        assert!(md.contains("create_file"));
        assert!(md.contains("Review Before Action"));
    }
}
