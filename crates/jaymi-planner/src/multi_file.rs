//! Multi-file Intelligence (Sprint C1.7).
//!
//! Reason across the project for coordinated coding changes:
//! rename symbol · move module · extract class · split file.
//!
//! The Planner determines **affected files** from Workspace Intelligence
//! (and never invents paths via an LLM). Changes become Create/Modify/Delete
//! ops → Execution Plans → Review Before Action. No direct LLM editing.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use jaymi_context::ContextBundle;
use jaymi_core::UserRequest;

use crate::code_generation::{CodeGeneration, GenerationOp};

/// Kind of multi-file coding change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiFileKind {
    /// Rename a symbol across observed references / related files.
    RenameSymbol,
    /// Move a module/file and update related observed files.
    MoveModule,
    /// Extract a class/type into a new module file.
    ExtractClass,
    /// Split an active file into multiple modules.
    SplitFile,
}

impl MultiFileKind {
    /// Stable id for diagnostics / AssembleHints-style notes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RenameSymbol => "rename_symbol",
            Self::MoveModule => "move_module",
            Self::ExtractClass => "extract_class",
            Self::SplitFile => "split_file",
        }
    }

    /// Human label for Execution Plan / markdown.
    pub fn label(self) -> &'static str {
        match self {
            Self::RenameSymbol => "Rename symbol",
            Self::MoveModule => "Move module",
            Self::ExtractClass => "Extract class",
            Self::SplitFile => "Split file",
        }
    }
}

/// One file the Planner observed as in-scope for a multi-file change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AffectedFile {
    /// Absolute or workspace path from WI (not LLM-invented).
    pub path: PathBuf,
    /// Role: definition / reference / import / source / destination / companion.
    pub role: String,
    /// Why this path was included.
    pub rationale: String,
}

/// Multi-file plan: affected files + change summary (Planner-owned).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct MultiFilePlan {
    /// Intent kind.
    pub kind: Option<MultiFileKind>,
    /// User / bound goal.
    pub goal: String,
    /// Focus symbol / type / module name when known.
    pub focus: Option<String>,
    /// Files determined from Workspace Intelligence.
    pub affected_files: Vec<AffectedFile>,
    /// Ordered change summary for Execution Plan steps.
    pub change_summary: Vec<String>,
    /// Observational risk notes.
    pub estimated_risk: Vec<String>,
}

impl MultiFilePlan {
    /// Conversation / Review Card prefix markdown.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Multi-file Intelligence\n");
        if let Some(kind) = self.kind {
            out.push_str(&format!("\n### Kind\n- {}\n", kind.label()));
        }
        out.push_str("\n### Goal\n- ");
        out.push_str(if self.goal.is_empty() {
            "(none)"
        } else {
            &self.goal
        });
        out.push('\n');
        if let Some(focus) = &self.focus {
            out.push_str("\n### Focus\n- ");
            out.push_str(focus);
            out.push('\n');
        }

        out.push_str("\n### Affected Files\n");
        if self.affected_files.is_empty() {
            out.push_str("- _(none observed yet — open a file or enrich Workspace Intelligence)_\n");
        } else {
            for file in &self.affected_files {
                out.push_str(&format!(
                    "- `{}` ({}) — {}\n",
                    file.path.display(),
                    file.role,
                    file.rationale
                ));
            }
        }

        out.push_str("\n### Change Summary\n");
        if self.change_summary.is_empty() {
            out.push_str("- _(none)_\n");
        } else {
            for step in &self.change_summary {
                out.push_str("- ");
                out.push_str(step);
                out.push('\n');
            }
        }

        out.push_str("\n### Estimated Risk\n");
        if self.estimated_risk.is_empty() {
            out.push_str("- Medium — multi-file edits require Review Before Action.\n");
        } else {
            for risk in &self.estimated_risk {
                out.push_str("- ");
                out.push_str(risk);
                out.push('\n');
            }
        }

        out.push_str(
            "\n### Review\n- No direct LLM editing — mutations go through Execution Plans only.\n",
        );
        out.push_str("- Review Before Action is required before tools run.\n");
        out
    }
}

/// Detect multi-file coding intents (not C1.4 plan / C1.5 generate).
pub fn detect_multi_file_request(request: &UserRequest) -> Option<MultiFileKind> {
    if request.coding_action.is_some()
        || request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
        || request.file.is_some()
        || request.directory.is_some()
        || request.lsp.is_some()
    {
        return None;
    }
    let lower = request.content.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    classify_multi_file(&lower)
}

/// True when C1.4 Coding Plan detect should yield to C1.7.
pub fn steals_from_coding_plan_detect(lower: &str) -> bool {
    classify_multi_file(lower.trim_end_matches(|c: char| c == '.' || c == '!')).is_some()
}

fn classify_multi_file(lower: &str) -> Option<MultiFileKind> {
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c == '!');
    if looks_like_rename_symbol(trimmed) {
        Some(MultiFileKind::RenameSymbol)
    } else if looks_like_move_module(trimmed) {
        Some(MultiFileKind::MoveModule)
    } else if looks_like_extract_class(trimmed) {
        Some(MultiFileKind::ExtractClass)
    } else if looks_like_split_file(trimmed) {
        Some(MultiFileKind::SplitFile)
    } else {
        None
    }
}

fn looks_like_rename_symbol(lower: &str) -> bool {
    lower == "rename symbol"
        || lower == "rename variable"
        || lower.starts_with("rename symbol ")
        || lower.starts_with("rename variable ")
        || lower.starts_with("rename this symbol")
        || lower.starts_with("rename this variable")
        || (lower.starts_with("rename ")
            && (lower.contains("symbol") || lower.contains("variable") || lower.contains("method")))
}

fn looks_like_move_module(lower: &str) -> bool {
    lower == "move module"
        || lower.starts_with("move module ")
        || lower.starts_with("move this module")
        || lower.starts_with("move file ")
        || (lower.starts_with("move ") && lower.contains("module"))
}

fn looks_like_extract_class(lower: &str) -> bool {
    lower == "extract class"
        || lower == "extract type"
        || lower == "extract struct"
        || lower.starts_with("extract class ")
        || lower.starts_with("extract type ")
        || lower.starts_with("extract struct ")
        || lower.starts_with("extract this class")
        || lower.starts_with("extract this type")
}

fn looks_like_split_file(lower: &str) -> bool {
    lower == "split file"
        || lower == "split this file"
        || lower.starts_with("split file ")
        || lower.starts_with("split this file")
        || (lower.starts_with("split ") && lower.contains("file"))
}

/// Build a multi-file plan from already-assembled Workspace Intelligence only.
pub fn plan_from_bundle(
    kind: MultiFileKind,
    goal: &str,
    bundle: &ContextBundle,
) -> MultiFilePlan {
    let root = bundle
        .active_project()
        .root_directory
        .as_ref()
        .map(PathBuf::from);
    let editor = bundle.editor_intelligence();
    let current = bundle.current_file();
    let open = bundle.open_files();
    let memory = bundle.workspace_memory();
    let project = bundle.project_intelligence();

    let focus = focus_name(kind, goal, editor, current);
    let mut plan = MultiFilePlan {
        kind: Some(kind),
        goal: goal.to_string(),
        focus: focus.clone(),
        affected_files: Vec::new(),
        change_summary: Vec::new(),
        estimated_risk: Vec::new(),
    };

    match kind {
        MultiFileKind::RenameSymbol => {
            fill_rename_symbol(&mut plan, focus.as_deref(), editor, current, open, memory);
        }
        MultiFileKind::MoveModule => {
            fill_move_module(&mut plan, root.as_deref(), current, open, memory, project);
        }
        MultiFileKind::ExtractClass => {
            fill_extract_class(&mut plan, focus.as_deref(), editor, current, open, project);
        }
        MultiFileKind::SplitFile => {
            fill_split_file(&mut plan, current, open, project);
        }
    }

    plan.estimated_risk.push(
        "Risk: **Medium–High** — multi-file changes need Review Before Action.".into(),
    );
    if plan.affected_files.is_empty() {
        plan.estimated_risk.push(
            "No concrete paths observed — open a file / enrich editor intelligence before approving."
                .into(),
        );
        plan.change_summary.push(
            "Insufficient Workspace Intelligence to schedule mutations — gather context first."
                .into(),
        );
    } else {
        plan.change_summary.push(
            "Constitutional gate: mutations run only after Review Approve (no LLM edits)."
                .into(),
        );
    }
    plan
}

/// Convert a multi-file plan into Planner-owned generation ops (never LLM edits).
pub fn code_generation_from_plan(plan: &MultiFilePlan) -> CodeGeneration {
    let kind = plan.kind.map(|k| k.as_str()).unwrap_or("multi_file");
    let mut operations = Vec::new();

    match plan.kind {
        Some(MultiFileKind::RenameSymbol) => {
            let symbol = plan.focus.as_deref().unwrap_or("symbol");
            for file in &plan.affected_files {
                operations.push(GenerationOp::modify_file(
                    file.path.clone(),
                    rename_stub(&file.path, symbol, &plan.goal),
                    format!("Rename `{}` in {} ({})", symbol, file.role, file.rationale),
                ));
            }
        }
        Some(MultiFileKind::MoveModule) => {
            let source = plan
                .affected_files
                .iter()
                .find(|f| f.role == "source")
                .or_else(|| plan.affected_files.first());
            let destination = plan
                .affected_files
                .iter()
                .find(|f| f.role == "destination");
            if let (Some(src), Some(dest)) = (source, destination) {
                operations.push(GenerationOp::create_file(
                    dest.path.clone(),
                    move_stub(&src.path, &dest.path, &plan.goal),
                    format!("Create moved module at {}", dest.path.display()),
                ));
                operations.push(GenerationOp::delete_file(
                    src.path.clone(),
                    format!("Remove old module path {}", src.path.display()),
                ));
            }
            for file in plan
                .affected_files
                .iter()
                .filter(|f| f.role == "import" || f.role == "reference")
            {
                operations.push(GenerationOp::modify_file(
                    file.path.clone(),
                    import_update_stub(&file.path, &plan.goal),
                    format!("Update imports/refs ({})", file.rationale),
                ));
            }
        }
        Some(MultiFileKind::ExtractClass) => {
            let name = plan.focus.as_deref().unwrap_or("Extracted");
            if let Some(dest) = plan.affected_files.iter().find(|f| f.role == "destination") {
                operations.push(GenerationOp::create_file(
                    dest.path.clone(),
                    extract_stub(name, &plan.goal),
                    format!("Extract `{name}` into new module"),
                ));
            }
            for file in plan
                .affected_files
                .iter()
                .filter(|f| f.role == "source" || f.role == "definition")
            {
                operations.push(GenerationOp::modify_file(
                    file.path.clone(),
                    extract_source_stub(name, &plan.goal),
                    format!("Leave re-export / call sites in {}", file.path.display()),
                ));
            }
        }
        Some(MultiFileKind::SplitFile) => {
            if let Some(companion) = plan.affected_files.iter().find(|f| f.role == "companion") {
                operations.push(GenerationOp::create_file(
                    companion.path.clone(),
                    split_companion_stub(&companion.path, &plan.goal),
                    String::from("Create companion module for split"),
                ));
            }
            for file in plan.affected_files.iter().filter(|f| f.role == "source") {
                operations.push(GenerationOp::modify_file(
                    file.path.clone(),
                    split_source_stub(&file.path, &plan.goal),
                    String::from("Trim source after split"),
                ));
            }
        }
        None => {}
    }

    CodeGeneration {
        goal: format!("Multi-file ({kind}): {}", plan.goal),
        operations,
        coding_plan_summary: plan.change_summary.clone(),
    }
}

fn focus_name(
    kind: MultiFileKind,
    goal: &str,
    editor: &jaymi_context::EditorIntelligenceSection,
    current: &jaymi_context::CurrentFileSection,
) -> Option<String> {
    match kind {
        MultiFileKind::RenameSymbol => editor
            .symbol
            .as_ref()
            .map(|s| s.name.clone())
            .or_else(|| parse_rename_target(goal)),
        MultiFileKind::ExtractClass => editor
            .enclosing_type
            .as_ref()
            .map(|s| s.name.clone())
            .or_else(|| parse_extract_target(goal))
            .or_else(|| {
                current
                    .path
                    .as_ref()
                    .and_then(|p| Path::new(p).file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
            }),
        MultiFileKind::MoveModule | MultiFileKind::SplitFile => current
            .path
            .as_ref()
            .and_then(|p| Path::new(p).file_stem())
            .map(|s| s.to_string_lossy().into_owned()),
    }
}

fn parse_rename_target(goal: &str) -> Option<String> {
    let lower = goal.to_ascii_lowercase();
    for prefix in ["rename symbol ", "rename variable ", "rename method ", "rename "] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let token = rest
                .split_whitespace()
                .next()?
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            if !token.is_empty() && token != "this" && token != "the" {
                // Preserve original casing from goal when possible.
                let start = goal.to_ascii_lowercase().find(token)?;
                return Some(goal[start..start + token.len()].to_string());
            }
        }
    }
    None
}

fn parse_extract_target(goal: &str) -> Option<String> {
    let lower = goal.to_ascii_lowercase();
    for prefix in ["extract class ", "extract type ", "extract struct "] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let token = rest
                .split_whitespace()
                .next()?
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            if !token.is_empty() && token != "this" && token != "the" {
                let start = goal.to_ascii_lowercase().find(token)?;
                return Some(goal[start..start + token.len()].to_string());
            }
        }
    }
    None
}

fn fill_rename_symbol(
    plan: &mut MultiFilePlan,
    focus: Option<&str>,
    editor: &jaymi_context::EditorIntelligenceSection,
    current: &jaymi_context::CurrentFileSection,
    open: &jaymi_context::OpenFilesSection,
    memory: &jaymi_context::WorkspaceMemorySection,
) {
    let mut seen = BTreeSet::new();
    let push = |plan: &mut MultiFilePlan, seen: &mut BTreeSet<String>, path: &str, role: &str, why: &str| {
        if path.trim().is_empty() || !seen.insert(path.to_string()) {
            return;
        }
        plan.affected_files.push(AffectedFile {
            path: PathBuf::from(path),
            role: role.into(),
            rationale: why.into(),
        });
    };

    if let Some(path) = &current.path {
        push(
            plan,
            &mut seen,
            path,
            "definition",
            "Active file — likely definition / primary edit site",
        );
    }
    for reference in editor.references.iter().take(24) {
        push(
            plan,
            &mut seen,
            &reference.path,
            "reference",
            "Editor intelligence reference observation",
        );
    }
    for entry in open.files.iter().take(12) {
        push(
            plan,
            &mut seen,
            &entry.path,
            "open",
            "Open editor may hold additional references",
        );
    }
    for path in memory.recent_edits.iter().take(8) {
        push(
            plan,
            &mut seen,
            path,
            "recent_edit",
            "Recent edit may reference the symbol",
        );
    }

    let symbol = focus.unwrap_or("symbol");
    plan.change_summary.push(format!(
        "Rename `{}` across {} observed file(s) from Workspace Intelligence.",
        symbol,
        plan.affected_files.len()
    ));
    plan.change_summary.push(
        "Prefer LSP rename preview when available; this plan uses WI-observed paths only.".into(),
    );
}

fn fill_move_module(
    plan: &mut MultiFilePlan,
    root: Option<&Path>,
    current: &jaymi_context::CurrentFileSection,
    open: &jaymi_context::OpenFilesSection,
    memory: &jaymi_context::WorkspaceMemorySection,
    project: &jaymi_context::ProjectIntelligenceSection,
) {
    let Some(source) = active_source_path(current, open) else {
        plan.change_summary
            .push("Open the module file to move before scheduling a multi-file plan.".into());
        return;
    };
    plan.affected_files.push(AffectedFile {
        path: PathBuf::from(&source),
        role: "source".into(),
        rationale: "Active file is the module to move".into(),
    });

    let file_name = Path::new(&source)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "module.rs".into());
    let dest_dir = first_matching_dir(&project.top_level_dirs, &["src", "crates", "lib", "apps"])
        .unwrap_or("src");
    let destination = match root {
        Some(root) => root.join(dest_dir).join("moved").join(&file_name),
        None => PathBuf::from(dest_dir).join("moved").join(&file_name),
    };
    plan.affected_files.push(AffectedFile {
        path: destination,
        role: "destination".into(),
        rationale: format!("Proposed destination under observed `{dest_dir}/` layout"),
    });

    let mut seen: BTreeSet<String> = plan
        .affected_files
        .iter()
        .map(|f| f.path.display().to_string())
        .collect();
    for entry in open.files.iter().take(10) {
        if seen.insert(entry.path.clone()) {
            plan.affected_files.push(AffectedFile {
                path: PathBuf::from(&entry.path),
                role: "import".into(),
                rationale: "Open file may import the moved module".into(),
            });
        }
    }
    for path in memory.recent_edits.iter().take(6) {
        if seen.insert(path.clone()) {
            plan.affected_files.push(AffectedFile {
                path: PathBuf::from(path),
                role: "reference".into(),
                rationale: "Recent edit may import the moved module".into(),
            });
        }
    }

    plan.change_summary.push(format!(
        "Move `{}` to a new path under observed project layout.",
        Path::new(&source)
            .file_name()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    ));
    plan.change_summary.push(format!(
        "Update {} related file(s) observed via open tabs / recent edits.",
        plan.affected_files
            .iter()
            .filter(|f| f.role == "import" || f.role == "reference")
            .count()
    ));
}

fn fill_extract_class(
    plan: &mut MultiFilePlan,
    focus: Option<&str>,
    editor: &jaymi_context::EditorIntelligenceSection,
    current: &jaymi_context::CurrentFileSection,
    open: &jaymi_context::OpenFilesSection,
    project: &jaymi_context::ProjectIntelligenceSection,
) {
    let Some(source) = active_source_path(current, open) else {
        plan.change_summary
            .push("Open a source file containing the type to extract.".into());
        return;
    };
    plan.affected_files.push(AffectedFile {
        path: PathBuf::from(&source),
        role: "source".into(),
        rationale: editor
            .enclosing_type
            .as_ref()
            .map(|t| format!("Enclosing type `{}` observed in editor intelligence", t.name))
            .unwrap_or_else(|| "Active file hosts the type to extract".into()),
    });

    let name = focus.unwrap_or("Extracted");
    let parent = Path::new(&source).parent().unwrap_or(Path::new("."));
    let ext = Path::new(&source)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("rs");
    let dest = if project.languages.iter().any(|l| l.eq_ignore_ascii_case("rust")) {
        parent.join(format!("{}.{}", to_snake(name), ext))
    } else {
        parent.join(format!("{name}.{ext}"))
    };
    plan.affected_files.push(AffectedFile {
        path: dest,
        role: "destination".into(),
        rationale: format!("New module for extracted `{name}` beside the source"),
    });
    for entry in open.files.iter().take(6) {
        if entry.path != source {
            plan.affected_files.push(AffectedFile {
                path: PathBuf::from(&entry.path),
                role: "reference".into(),
                rationale: "Open file may need updated imports after extract".into(),
            });
        }
    }

    plan.change_summary.push(format!(
        "Extract `{name}` into a new file; leave a thin surface in the source."
    ));
}

fn fill_split_file(
    plan: &mut MultiFilePlan,
    current: &jaymi_context::CurrentFileSection,
    open: &jaymi_context::OpenFilesSection,
    _project: &jaymi_context::ProjectIntelligenceSection,
) {
    let Some(source) = active_source_path(current, open) else {
        plan.change_summary
            .push("Open the file to split before scheduling mutations.".into());
        return;
    };
    plan.affected_files.push(AffectedFile {
        path: PathBuf::from(&source),
        role: "source".into(),
        rationale: "Active file to split".into(),
    });
    let parent = Path::new(&source).parent().unwrap_or(Path::new("."));
    let stem = Path::new(&source)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "part".into());
    let ext = Path::new(&source)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("rs");
    let companion = parent.join(format!("{stem}_part.{ext}"));
    plan.affected_files.push(AffectedFile {
        path: companion,
        role: "companion".into(),
        rationale: "Companion module for extracted portion".into(),
    });
    for entry in open.files.iter().take(4) {
        if entry.path != source {
            plan.affected_files.push(AffectedFile {
                path: PathBuf::from(&entry.path),
                role: "reference".into(),
                rationale: "Open file may import the split module".into(),
            });
        }
    }
    plan.change_summary.push(format!(
        "Split `{}` into a companion module and trim the source.",
        Path::new(&source)
            .file_name()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    ));
}

fn active_source_path(
    current: &jaymi_context::CurrentFileSection,
    open: &jaymi_context::OpenFilesSection,
) -> Option<String> {
    if let Some(path) = &current.path {
        if !path.trim().is_empty() {
            return Some(path.clone());
        }
    }
    open.files
        .iter()
        .find(|entry| entry.active)
        .map(|entry| entry.path.clone())
        .or_else(|| open.files.first().map(|entry| entry.path.clone()))
}

fn first_matching_dir<'a>(dirs: &'a [String], candidates: &[&str]) -> Option<&'a str> {
    for candidate in candidates {
        if let Some(found) = dirs.iter().find(|d| d.eq_ignore_ascii_case(candidate)) {
            return Some(found.as_str());
        }
    }
    None
}

fn to_snake(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "extracted".into()
    } else {
        trimmed.to_string()
    }
}

fn rename_stub(path: &Path, symbol: &str, goal: &str) -> String {
    format!(
        "// Jaymi multi-file rename (C1.7) — Review required before write.\n\
         // File: {}\n\
         // Symbol: {symbol}\n\
         // Goal: {goal}\n\
         // NOTE: Prefer LSP workspace-edit rename when available; this stub is\n\
         // Planner-authored from Workspace Intelligence paths only.\n",
        path.display()
    )
}

fn move_stub(from: &Path, to: &Path, goal: &str) -> String {
    format!(
        "// Jaymi multi-file move (C1.7) — Review required before write.\n\
         // Moved from: {}\n\
         // Moved to: {}\n\
         // Goal: {goal}\n",
        from.display(),
        to.display()
    )
}

fn import_update_stub(path: &Path, goal: &str) -> String {
    format!(
        "// Jaymi multi-file import update (C1.7) — Review required.\n\
         // File: {}\n\
         // Goal: {goal}\n\
         // TODO: update imports to the moved module path.\n",
        path.display()
    )
}

fn extract_stub(name: &str, goal: &str) -> String {
    format!(
        "// Jaymi multi-file extract (C1.7) — Review required before write.\n\
         // Extracted: {name}\n\
         // Goal: {goal}\n\n\
         pub struct {name} {{\n    // TODO: move fields / methods here\n}}\n"
    )
}

fn extract_source_stub(name: &str, goal: &str) -> String {
    format!(
        "// Jaymi multi-file extract source update (C1.7) — Review required.\n\
         // Re-export / call sites for `{name}` after extract.\n\
         // Goal: {goal}\n\
         // TODO: replace inlined type with module import.\n"
    )
}

fn split_companion_stub(path: &Path, goal: &str) -> String {
    format!(
        "// Jaymi multi-file split companion (C1.7) — Review required.\n\
         // Companion: {}\n\
         // Goal: {goal}\n\n\
         // TODO: move the extracted portion here.\n",
        path.display()
    )
}

fn split_source_stub(path: &Path, goal: &str) -> String {
    format!(
        "// Jaymi multi-file split source (C1.7) — Review required.\n\
         // Source: {}\n\
         // Goal: {goal}\n\
         // TODO: trim moved portion; `mod` / import the companion.\n",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        ActiveProjectSection, ContextBundle, CurrentFileSection, EditorIntelligenceSection,
        EditorReference, EditorSymbol, OpenFileEntry, OpenFilesSection, ProjectIntelligenceSection,
        WorkspaceMemorySection,
    };

    #[test]
    fn detects_multi_file_examples() {
        assert_eq!(
            detect_multi_file_request(&UserRequest::new("Rename symbol")),
            Some(MultiFileKind::RenameSymbol)
        );
        assert_eq!(
            detect_multi_file_request(&UserRequest::new("Rename variable.")),
            Some(MultiFileKind::RenameSymbol)
        );
        assert_eq!(
            detect_multi_file_request(&UserRequest::new("Move module")),
            Some(MultiFileKind::MoveModule)
        );
        assert_eq!(
            detect_multi_file_request(&UserRequest::new("Extract class")),
            Some(MultiFileKind::ExtractClass)
        );
        assert_eq!(
            detect_multi_file_request(&UserRequest::new("Split file.")),
            Some(MultiFileKind::SplitFile)
        );
        assert!(detect_multi_file_request(&UserRequest::new("Build Pong.")).is_none());
        assert!(detect_multi_file_request(&UserRequest::new("Generate the code.")).is_none());
    }

    #[test]
    fn rename_symbol_uses_wi_references_only() {
        let bundle = ContextBundle::builder()
            .active_project(ActiveProjectSection {
                project_id: Some("p".into()),
                name: Some("Demo".into()),
                root_directory: Some("/proj".into()),
                detail: None,
            })
            .current_file(CurrentFileSection {
                path: Some("/proj/src/lib.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            })
            .editor_intelligence(EditorIntelligenceSection {
                symbol: Some(EditorSymbol {
                    name: "foo".into(),
                    kind: Some("function".into()),
                    ..Default::default()
                }),
                references: vec![
                    EditorReference {
                        path: "/proj/src/lib.rs".into(),
                        ..Default::default()
                    },
                    EditorReference {
                        path: "/proj/src/main.rs".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            })
            .open_files(OpenFilesSection {
                files: vec![OpenFileEntry {
                    path: "/proj/src/util.rs".into(),
                    dirty: false,
                    active: false,
                }],
            })
            .workspace_memory(WorkspaceMemorySection {
                recent_edits: vec!["/proj/src/extra.rs".into()],
                ..Default::default()
            })
            .build();

        let plan = plan_from_bundle(MultiFileKind::RenameSymbol, "Rename symbol foo", &bundle);
        assert_eq!(plan.focus.as_deref(), Some("foo"));
        assert!(plan
            .affected_files
            .iter()
            .any(|f| f.path.ends_with("main.rs")));
        assert!(plan
            .affected_files
            .iter()
            .any(|f| f.path.ends_with("util.rs")));
        let gen = code_generation_from_plan(&plan);
        assert!(!gen.operations.is_empty());
        assert!(gen
            .operations
            .iter()
            .all(|op| op.kind == crate::code_generation::GenerationOpKind::ModifyFile));
        assert!(plan.to_markdown().contains("## Multi-file Intelligence"));
        assert!(plan.to_markdown().contains("Affected Files"));
    }

    #[test]
    fn extract_and_split_propose_create_and_modify() {
        let bundle = ContextBundle::builder()
            .current_file(CurrentFileSection {
                path: Some("/proj/src/big.rs".into()),
                dirty: true,
                language: Some("rust".into()),
            })
            .project_intelligence(ProjectIntelligenceSection {
                languages: vec!["rust".into()],
                top_level_dirs: vec!["src".into()],
                ..Default::default()
            })
            .editor_intelligence(EditorIntelligenceSection {
                enclosing_type: Some(EditorSymbol {
                    name: "Widget".into(),
                    kind: Some("struct".into()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .build();

        let extract = plan_from_bundle(MultiFileKind::ExtractClass, "Extract class", &bundle);
        let gen = code_generation_from_plan(&extract);
        assert!(gen
            .operations
            .iter()
            .any(|op| op.kind == crate::code_generation::GenerationOpKind::CreateFile));
        assert!(gen
            .operations
            .iter()
            .any(|op| op.kind == crate::code_generation::GenerationOpKind::ModifyFile));

        let split = plan_from_bundle(MultiFileKind::SplitFile, "Split file", &bundle);
        let gen = code_generation_from_plan(&split);
        assert!(gen
            .operations
            .iter()
            .any(|op| op.kind == crate::code_generation::GenerationOpKind::CreateFile));
    }
}
