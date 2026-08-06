//! Plan revision — modify Execution Plans before approval without full replan.
//!
//! Content stays immutable: Modify creates a **child** plan that supersedes the
//! paused parent. Only affected steps / tool inputs are regenerated. Context
//! reassembly is skipped for ordinary modifications.

use std::path::{Path, PathBuf};

use jaymi_tools::{ToolInput, MANAGE_PATH_TOOL_ID, WRITE_FILE_TOOL_ID};

use crate::execution_plan::{ExecutionPlan, ExecutionPlanId, ExecutionStep};

/// How broadly a modification rewrites the paused plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModificationScope {
    /// Only some resources / step details change; tool identity stays.
    Partial,
    /// Tool or action kind changes (e.g. write → rename).
    Full,
}

/// Result of applying a user modify-note to a paused plan.
#[derive(Debug, Clone)]
pub struct PlanRevisionDraft {
    /// Updated tool input for the revised plan.
    pub tool_input: ToolInput,
    /// Tool id after revision (may change on full modifications).
    pub tool_id: String,
    /// Regenerated steps (unchanged steps copied; affected ones rebuilt).
    pub steps: Vec<ExecutionStep>,
    /// Resources the revised plan would touch.
    pub affected_resources: Vec<String>,
    /// Human-readable diff vs the parent plan.
    pub changes: Vec<String>,
    /// Partial vs full rewrite.
    pub scope: ModificationScope,
    /// Originating request text for the child plan.
    pub originating_request: String,
    /// True when ContextBundle must be rebuilt (capability / intent shift).
    pub requires_context_reassemble: bool,
}

/// Apply a free-text modification note to a paused plan + tool input.
///
/// Supported examples:
/// - "Skip README" / "except README" → drop README paths (partial)
/// - "Delete only temporary files" → constrain delete to temp patterns (partial)
/// - "Rename instead of overwrite" → write_file → manage_path rename (full)
pub fn apply_modification_note(
    plan: &ExecutionPlan,
    tool_id: &str,
    tool_input: &ToolInput,
    note: &str,
) -> PlanRevisionDraft {
    let note_trim = note.trim();
    let note_l = note_trim.to_lowercase();
    let mut input = tool_input.clone();
    let mut tool_id = tool_id.to_string();
    let mut changes = Vec::new();
    let mut scope = ModificationScope::Partial;
    let mut requires_context_reassemble = false;

    if note_trim.is_empty() {
        changes.push("No modification note provided — plan steps retained".into());
        return PlanRevisionDraft {
            tool_input: input,
            tool_id,
            steps: plan.steps().to_vec(),
            affected_resources: plan.affected_resources().to_vec(),
            changes,
            scope,
            originating_request: format!(
                "{} (revision of {})",
                plan.originating_request(),
                plan.id()
            ),
            requires_context_reassemble: false,
        };
    }

    // --- Partial: skip README ---
    if note_l.contains("skip readme")
        || note_l.contains("except readme")
        || note_l.contains("without readme")
    {
        let before_paths = input.paths.len();
        input.paths.retain(|path| !is_readme(path));
        if before_paths != input.paths.len() {
            changes.push(format!(
                "Skipped README ({} path(s) removed)",
                before_paths - input.paths.len()
            ));
        }
        if input.path.as_ref().is_some_and(|path| is_readme(path)) {
            changes.push("Cleared README as primary resource".into());
            input.path = None;
        }
        if changes.is_empty() {
            changes.push("Skip README requested (no README paths were present)".into());
        }
    }

    // --- Partial: delete only temporary files ---
    if note_l.contains("temporary")
        || note_l.contains("temp file")
        || note_l.contains("only temp")
        || note_l.contains(".tmp")
    {
        if input.command.as_deref() == Some("delete")
            || note_l.contains("delete")
            || plan.planner_intent().as_str().contains("manage")
        {
            let before = input.paths.len();
            if !input.paths.is_empty() {
                input.paths.retain(|path| is_temporary(path));
                changes.push(format!(
                    "Constrained delete to temporary files ({} → {} path(s))",
                    before,
                    input.paths.len()
                ));
            } else if let Some(path) = input.path.clone() {
                if is_temporary(&path) {
                    changes.push(format!(
                        "Delete limited to temporary file {}",
                        path.display()
                    ));
                } else {
                    let parent = path
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from("."));
                    let temp = parent.join("*.tmp");
                    changes.push(format!(
                        "Retargeted delete from {} to temporary pattern {}",
                        path.display(),
                        temp.display()
                    ));
                    input.path = Some(temp);
                }
            } else {
                changes.push("Delete constrained to temporary files only".into());
            }
            if input.command.is_none() {
                input.command = Some("delete".into());
            }
        }
    }

    // --- Full: rename instead of overwrite ---
    if note_l.contains("rename instead")
        || (note_l.contains("rename") && note_l.contains("overwrite"))
    {
        scope = ModificationScope::Full;
        if tool_id == WRITE_FILE_TOOL_ID || (input.path.is_some() && input.content.is_some()) {
            let from = input
                .path
                .clone()
                .unwrap_or_else(|| PathBuf::from("untitled"));
            let to = rename_destination_from_note(&note_l, &from);
            tool_id = MANAGE_PATH_TOOL_ID.to_string();
            input = ToolInput::manage_path("rename", from.clone(), Some(to.display().to_string()));
            changes.push(format!(
                "Changed write/overwrite into rename {} → {}",
                from.display(),
                to.display()
            ));
            // Same FileManagement capability — no context rebuild required.
            requires_context_reassemble = false;
        } else if input.command.as_deref() != Some("rename") {
            if let Some(from) = input.path.clone() {
                let to = rename_destination_from_note(&note_l, &from);
                tool_id = MANAGE_PATH_TOOL_ID.to_string();
                input = ToolInput::manage_path("rename", from.clone(), Some(to.display().to_string()));
                changes.push(format!(
                    "Changed action into rename {} → {}",
                    from.display(),
                    to.display()
                ));
            }
        }
    }

    // --- Partial: explicit path retarget "use PATH" / "to PATH" ---
    if let Some(new_path) = extract_path_directive(&note_l) {
        if let Some(old) = input.path.clone() {
            if old != new_path {
                changes.push(format!(
                    "Retargeted resource {} → {}",
                    old.display(),
                    new_path.display()
                ));
                input.path = Some(new_path);
            }
        } else {
            changes.push(format!("Set resource to {}", new_path.display()));
            input.path = Some(new_path);
        }
    }

    if changes.is_empty() {
        // Generic note — treat as partial guidance on the primary step.
        changes.push(format!("Applied modification guidance: {note_trim}"));
        scope = ModificationScope::Partial;
    }

    let steps = regenerate_affected_steps(plan, &tool_id, &input, &changes, scope);
    let affected_resources = resources_from_input(&input, plan);

    let originating_request = format!(
        "{} — modified: {note_trim}",
        plan.originating_request()
    );

    PlanRevisionDraft {
        tool_input: input,
        tool_id,
        steps,
        affected_resources,
        changes,
        scope,
        originating_request,
        requires_context_reassemble,
    }
}

/// Copy unchanged steps; rebuild only steps whose tool/resource were affected.
fn regenerate_affected_steps(
    plan: &ExecutionPlan,
    tool_id: &str,
    input: &ToolInput,
    changes: &[String],
    scope: ModificationScope,
) -> Vec<ExecutionStep> {
    let primary_resource = input
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .or_else(|| {
            input
                .paths
                .first()
                .map(|path| path.display().to_string())
        })
        .or_else(|| plan.affected_resources().first().cloned())
        .unwrap_or_else(|| "unspecified".into());

    let description = match scope {
        ModificationScope::Full => {
            if input.command.as_deref() == Some("rename") {
                format!(
                    "Rename path (revised): {}",
                    changes.first().cloned().unwrap_or_else(|| "rename".into())
                )
            } else {
                format!(
                    "Revised action: {}",
                    changes.first().cloned().unwrap_or_else(|| "full modify".into())
                )
            }
        }
        ModificationScope::Partial => {
            let base = plan
                .steps()
                .first()
                .map(|step| step.description.clone())
                .unwrap_or_else(|| "Execute tool".into());
            if changes.is_empty() {
                base
            } else {
                format!("{base} [{}]", changes.join("; "))
            }
        }
    };

    if plan.steps().len() <= 1 || matches!(scope, ModificationScope::Full) {
        // Single-step plans (today's default): regenerate the one step.
        return vec![ExecutionStep {
            order: 1,
            description,
            tool_id: Some(tool_id.to_string()),
            resource: Some(primary_resource),
        }];
    }

    // Multi-step: keep steps whose resource/tool were not mentioned in changes;
    // regenerate the primary (order 1) step only.
    let mut steps = Vec::new();
    for step in plan.steps() {
        let affected = step.order == 1
            || step.tool_id.as_deref() == Some(tool_id)
            || step
                .resource
                .as_ref()
                .is_some_and(|resource| resource_mentioned(resource, changes));
        if affected {
            steps.push(ExecutionStep {
                order: step.order,
                description: if step.order == 1 {
                    description.clone()
                } else {
                    format!("{} [revised]", step.description)
                },
                tool_id: Some(tool_id.to_string()),
                resource: Some(primary_resource.clone()),
            });
        } else {
            steps.push(step.clone());
        }
    }
    steps
}

fn resources_from_input(input: &ToolInput, plan: &ExecutionPlan) -> Vec<String> {
    let mut resources = Vec::new();
    if let Some(path) = &input.path {
        resources.push(path.display().to_string());
    }
    for path in &input.paths {
        let label = path.display().to_string();
        if !resources.contains(&label) {
            resources.push(label);
        }
    }
    if let Some(dest) = input.content.as_ref().filter(|_| {
        input.command.as_deref() == Some("rename")
    }) {
        if !resources.contains(dest) {
            resources.push(dest.clone());
        }
    }
    if resources.is_empty() {
        resources = plan.affected_resources().to_vec();
    }
    resources
}

fn is_readme(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            let lower = name.to_lowercase();
            lower == "readme" || lower.starts_with("readme.")
        })
        .unwrap_or(false)
}

fn is_temporary(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext = ext.to_lowercase();
            matches!(ext.as_str(), "tmp" | "temp" | "swp" | "bak")
        })
        .unwrap_or(false)
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                let lower = name.to_lowercase();
                lower.starts_with('.') && lower.contains("tmp")
                    || lower.ends_with('~')
                    || lower.contains("temp")
            })
            .unwrap_or(false)
}

fn rename_destination_from_note(note_l: &str, from: &Path) -> PathBuf {
    if let Some(path) = extract_path_directive(note_l) {
        return path;
    }
    let parent = from.parent().unwrap_or_else(|| Path::new("."));
    let stem = from
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = from
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    parent.join(format!("{stem}.renamed{ext}"))
}

fn extract_path_directive(note_l: &str) -> Option<PathBuf> {
    for marker in [" to ", " use ", " path ", "to ", "use ", "path "] {
        if let Some(idx) = note_l.find(marker) {
            // Prefer whole-word markers; skip mid-token matches for short forms.
            if marker.ends_with(' ')
                && !marker.starts_with(' ')
                && idx > 0
                && note_l
                    .as_bytes()
                    .get(idx.wrapping_sub(1))
                    .is_some_and(|b| b.is_ascii_alphanumeric())
            {
                continue;
            }
            let rest = note_l[idx + marker.len()..].trim();
            let token = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == ',');
            if token.starts_with('/') || token.starts_with('.') || token.contains('/') {
                return Some(PathBuf::from(token));
            }
        }
    }
    None
}

fn resource_mentioned(resource: &str, changes: &[String]) -> bool {
    changes.iter().any(|change| change.contains(resource))
}

/// Compact history row for plan lineage diagnostics / UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanHistoryEntry {
    /// Plan id.
    pub plan_id: ExecutionPlanId,
    /// Parent plan when this is a revision.
    pub parent_plan_id: Option<ExecutionPlanId>,
    /// 1-based revision number.
    pub revision: u32,
    /// Status snapshot when recorded.
    pub status: String,
    /// Originating request.
    pub originating_request: String,
    /// Diff vs parent, when any.
    pub changes: Vec<String>,
    /// User modify note, when any.
    pub modification_note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{
        EstimatedReversibility, EstimatedRisk, ExecutionPlanParams, PlanPermissionRequirement,
        ReviewRequirement,
    };
    use jaymi_capabilities::Capability;
    use jaymi_core::IntentId;

    fn write_plan(path: &str) -> ExecutionPlan {
        ExecutionPlan::create(ExecutionPlanParams {
            originating_request: format!("Write {path}"),
            planner_intent: IntentId::WriteFile,
            capability: Capability::FileManagement,
            proposed_tools: vec![WRITE_FILE_TOOL_ID.into()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Write file".into(),
                tool_id: Some(WRITE_FILE_TOOL_ID.into()),
                resource: Some(path.into()),
            }],
            estimated_risk: EstimatedRisk::Medium,
            affected_resources: vec![path.into()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "write".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::PartiallyReversible,
            expected_outputs: vec!["written file".into()],
            lineage: Default::default(),
        })
    }

    #[test]
    fn skip_readme_is_partial() {
        let plan = write_plan("/tmp/README.md");
        let input = ToolInput {
            path: Some(PathBuf::from("/tmp/README.md")),
            paths: vec![
                PathBuf::from("/tmp/README.md"),
                PathBuf::from("/tmp/notes.txt"),
            ],
            content: Some("x".into()),
            ..Default::default()
        };
        let draft = apply_modification_note(&plan, WRITE_FILE_TOOL_ID, &input, "Skip README");
        assert_eq!(draft.scope, ModificationScope::Partial);
        assert!(!draft.tool_input.paths.iter().any(|p| is_readme(p)));
        assert!(draft.changes.iter().any(|c| c.contains("README")));
        assert!(!draft.requires_context_reassemble);
    }

    #[test]
    fn rename_instead_of_overwrite_is_full() {
        let plan = write_plan("/tmp/a.txt");
        let input = ToolInput::write_file("/tmp/a.txt", "body");
        let draft = apply_modification_note(
            &plan,
            WRITE_FILE_TOOL_ID,
            &input,
            "Rename instead of overwrite",
        );
        assert_eq!(draft.scope, ModificationScope::Full);
        assert_eq!(draft.tool_id, MANAGE_PATH_TOOL_ID);
        assert_eq!(draft.tool_input.command.as_deref(), Some("rename"));
        assert!(draft.changes.iter().any(|c| c.contains("rename")));
    }

    #[test]
    fn temporary_delete_is_partial() {
        let plan = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: "Delete /tmp/work".into(),
            planner_intent: IntentId::ManagePath,
            capability: Capability::FileManagement,
            proposed_tools: vec![MANAGE_PATH_TOOL_ID.into()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Delete path".into(),
                tool_id: Some(MANAGE_PATH_TOOL_ID.into()),
                resource: Some("/tmp/work".into()),
            }],
            estimated_risk: EstimatedRisk::High,
            affected_resources: vec!["/tmp/work".into()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "delete".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::Irreversible,
            expected_outputs: vec!["deleted".into()],
            lineage: Default::default(),
        });
        let input = ToolInput::manage_path("delete", "/tmp/work", None::<String>);
        let draft = apply_modification_note(
            &plan,
            MANAGE_PATH_TOOL_ID,
            &input,
            "Delete only temporary files",
        );
        assert_eq!(draft.scope, ModificationScope::Partial);
        assert!(draft
            .tool_input
            .path
            .as_ref()
            .is_some_and(|p| p.display().to_string().contains(".tmp")));
    }
}
