//! Patch Generation from approved Coding Plans (Sprint C3.4).
//!
//! Produces **precise proposed file modifications** after a Coding Plan is
//! approved for generation — never automatic application.
//!
//! Pipeline:
//! Coding Plan (remembered) → user asks to generate → draft patches →
//! (optional) ReasoningEngine elaboration → validate vs current file state →
//! valid patches → GenerationOp / ExecutionPlan → Review Before Action.
//!
//! Constitutional constraints:
//! - Targeted patches (not blind whole-file rewrites)
//! - Never write to disk in this sprint's generation turn
//! - Model never calls filesystem providers directly
//! - Patch content elaboration stays behind ReasoningEngine
//! - Planner owns validation + conversion into execution architecture
//! - Review Before Action preserved; no automatic mutation
//! - On failure: project unchanged + explicit explanation

use std::path::{Path, PathBuf};

use crate::code_generation::{
    self, CodeGeneration, GenerationOp, GenerationOpKind,
};
use crate::coding_plan::{CodingPlan, CodingPlanOrigin};

/// Cap proposals copied from a Coding Plan into a patch batch.
pub const PATCH_PROPOSAL_CAP: usize = 16;
/// Max original/proposed snippet chars kept in diagnostics / markdown.
pub const SNIPPET_CHAR_CAP: usize = 480;

/// Patch change types supported in C3.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchChangeType {
    CreateFile,
    ModifyFile,
    DeleteFile,
    RenameMove,
}

impl PatchChangeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreateFile => "create_file",
            Self::ModifyFile => "modify_file",
            Self::DeleteFile => "delete_file",
            Self::RenameMove => "rename_move",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::CreateFile => "Create file",
            Self::ModifyFile => "Modify file",
            Self::DeleteFile => "Delete file",
            Self::RenameMove => "Rename / move",
        }
    }
}

/// Location hint inside a file (line range and/or symbol).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct PatchLocation {
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub symbol: Option<String>,
    pub note: Option<String>,
}

impl PatchLocation {
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        match (self.start_line, self.end_line) {
            (Some(a), Some(b)) if a == b => parts.push(format!("line {}", a.saturating_add(1))),
            (Some(a), Some(b)) => parts.push(format!(
                "lines {}–{}",
                a.saturating_add(1),
                b.saturating_add(1)
            )),
            (Some(a), None) => parts.push(format!("line {}", a.saturating_add(1))),
            _ => {}
        }
        if let Some(sym) = &self.symbol {
            parts.push(format!("symbol `{sym}`"));
        }
        if let Some(note) = &self.note {
            parts.push(note.clone());
        }
        if parts.is_empty() {
            "unspecified".into()
        } else {
            parts.join(", ")
        }
    }
}

/// One proposed patch (pre-validation).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PatchProposal {
    pub file: PathBuf,
    pub original_location: Option<PatchLocation>,
    pub change_type: PatchChangeType,
    /// Expected current content / anchor for modify (staleness detection).
    pub original_content: Option<String>,
    /// Proposed body (create/modify) — never applied in this sprint.
    pub proposed_content: Option<String>,
    pub rationale: String,
    /// Destination for rename/move.
    pub destination: Option<PathBuf>,
    /// Simple fingerprint of observed file bytes when drafted.
    pub observed_hash: Option<String>,
}

/// Validation outcome for a single proposal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchValidationStatus {
    Valid,
    Stale { reason: String },
    Invalid { reason: String },
    MissingFile { reason: String },
    UnexpectedExists { reason: String },
}

impl PatchValidationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Stale { .. } => "stale",
            Self::Invalid { .. } => "invalid",
            Self::MissingFile { .. } => "missing_file",
            Self::UnexpectedExists { .. } => "unexpected_exists",
        }
    }

    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Valid => None,
            Self::Stale { reason }
            | Self::Invalid { reason }
            | Self::MissingFile { reason }
            | Self::UnexpectedExists { reason } => Some(reason.as_str()),
        }
    }
}

/// Proposal after validation against current file state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ValidatedPatch {
    pub proposal: PatchProposal,
    pub status: PatchValidationStatus,
}

/// Planner-authored patch generation report (C3.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct PatchGenerationReport {
    pub goal: String,
    pub patches: Vec<ValidatedPatch>,
    pub failures: Vec<String>,
    pub reasoning_used: bool,
    /// True when at least one patch validated and was converted to generation ops.
    pub converted_to_execution: bool,
    pub diagnostics: Vec<String>,
}

impl PatchGenerationReport {
    pub fn valid_count(&self) -> usize {
        self.patches.iter().filter(|p| p.status.is_valid()).count()
    }

    pub fn invalid_count(&self) -> usize {
        self.patches.len().saturating_sub(self.valid_count())
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Patch Generation\n");
        out.push_str("\n### Goal\n- ");
        out.push_str(if self.goal.is_empty() {
            "(none)"
        } else {
            &self.goal
        });
        out.push('\n');

        out.push_str(&format!(
            "\n### Status\n- valid={} · invalid/stale={} · reasoning={} · converted_to_review={}\n",
            self.valid_count(),
            self.invalid_count(),
            self.reasoning_used,
            self.converted_to_execution
        ));

        out.push_str("\n### Proposed changes\n");
        if self.patches.is_empty() {
            out.push_str("- _(none)_\n");
        } else {
            for (i, patch) in self.patches.iter().enumerate() {
                let p = &patch.proposal;
                out.push_str(&format!(
                    "{}. **{}** `{}` — status=`{}`\n",
                    i + 1,
                    p.change_type.as_str(),
                    p.file.display(),
                    patch.status.as_str()
                ));
                if let Some(loc) = &p.original_location {
                    out.push_str(&format!("   - location: {}\n", loc.display()));
                }
                if let Some(dest) = &p.destination {
                    out.push_str(&format!("   - destination: `{}`\n", dest.display()));
                }
                out.push_str(&format!("   - rationale: {}\n", p.rationale));
                if let Some(orig) = &p.original_content {
                    out.push_str(&format!(
                        "   - original: {}\n",
                        truncate(orig, SNIPPET_CHAR_CAP)
                    ));
                }
                if let Some(prop) = &p.proposed_content {
                    out.push_str(&format!(
                        "   - proposed: {}\n",
                        truncate(prop, SNIPPET_CHAR_CAP)
                    ));
                }
                if let Some(reason) = patch.status.reason() {
                    out.push_str(&format!("   - validation: {reason}\n"));
                }
            }
        }

        if !self.failures.is_empty() {
            out.push_str("\n### Failures\n");
            for f in &self.failures {
                out.push_str(&format!("- {f}\n"));
            }
            out.push_str(
                "- Original project unchanged — no files were written.\n",
            );
        }

        out.push_str("\n### Notes\n");
        out.push_str("- Patch generation does not write to disk.\n");
        out.push_str(
            "- Valid patches convert to the existing ExecutionPlan path and still require Review Before Action.\n",
        );
        out.push_str(
            "- Reasoning elaborates proposals; the Planner validates and owns tool conversion.\n",
        );
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "patch_generation goal_chars={} patches={} valid={} invalid={} failures={} reasoning={} converted={}",
            self.goal.chars().count(),
            self.patches.len(),
            self.valid_count(),
            self.invalid_count(),
            self.failures.len(),
            self.reasoning_used,
            self.converted_to_execution,
        )
    }
}

/// Whether this remembered Coding Plan should use C3.4 patch generation.
pub fn should_use_patch_generation(plan: &CodingPlan) -> bool {
    matches!(plan.origin, CodingPlanOrigin::ChangeInvestigation)
        || matches!(plan.kind, Some(crate::coding_plan::CodingPlanKind::Mutation))
}

/// Prompt instruction for ReasoningEngine patch elaboration.
pub fn reasoning_prompt_instruction(plan: &CodingPlan, goal: &str) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Generate targeted file patches for Coding Plan goal: {goal}"
    ));
    lines.push(
        "Stay behind ReasoningEngine only — do not claim filesystem writes; Planner validates and reviews."
            .into(),
    );
    lines.push(
        "Prefer surgical edits: identify file, location, change_type, original excerpt, proposed content, rationale."
            .into(),
    );
    lines.push(
        "Supported change_type values: create_file, modify_file, delete_file, rename_move."
            .into(),
    );
    lines.push(
        "Emit markdown sections starting with `### Patch N` and bullet fields: file, change_type, location, original, proposed, destination, rationale."
            .into(),
    );
    lines.push("Coding Plan context:".into());
    lines.push(plan.to_markdown());
    lines.join("\n")
}

/// Draft proposals from a Coding Plan (Planner-owned; content may be stub-level).
pub fn draft_proposals_from_plan(
    plan: &CodingPlan,
    goal: &str,
    project_root: Option<&Path>,
) -> Vec<PatchProposal> {
    let root = project_root
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut out = Vec::new();

    for proposal in plan.files_to_create.iter().take(PATCH_PROPOSAL_CAP) {
        if let Some(rel) = code_generation::extract_path_candidate(proposal) {
            let path = code_generation::resolve_under_root(&root, &rel);
            out.push(PatchProposal {
                file: path.clone(),
                original_location: Some(PatchLocation {
                    note: Some("new file".into()),
                    ..Default::default()
                }),
                change_type: PatchChangeType::CreateFile,
                original_content: None,
                proposed_content: Some(targeted_create_body(&path, goal, proposal)),
                rationale: format!("Coding Plan create: {proposal}"),
                destination: None,
                observed_hash: None,
            });
        }
    }

    for proposal in plan.files_to_modify.iter().take(PATCH_PROPOSAL_CAP) {
        if let Some(rel) = code_generation::extract_path_candidate(proposal) {
            let path = code_generation::resolve_under_root(&root, &rel);
            if out.iter().any(|p| p.file == path) {
                continue;
            }
            out.push(PatchProposal {
                file: path.clone(),
                original_location: Some(PatchLocation {
                    note: Some("targeted modification".into()),
                    ..Default::default()
                }),
                change_type: PatchChangeType::ModifyFile,
                original_content: None, // filled after read
                proposed_content: Some(targeted_modify_body(&path, goal, proposal, None)),
                rationale: format!("Coding Plan modify: {proposal}"),
                destination: None,
                observed_hash: None,
            });
        }
    }

    for proposal in plan.files_to_remove.iter().take(PATCH_PROPOSAL_CAP) {
        if let Some(rel) = code_generation::extract_path_candidate(proposal) {
            let path = code_generation::resolve_under_root(&root, &rel);
            out.push(PatchProposal {
                file: path,
                original_location: None,
                change_type: PatchChangeType::DeleteFile,
                original_content: None,
                proposed_content: None,
                rationale: format!("Coding Plan remove: {proposal}"),
                destination: None,
                observed_hash: None,
            });
        }
    }

    // Rename/move hints in plan steps / summary.
    for proposal in plan
        .plan_steps
        .iter()
        .chain(plan.summary.iter())
        .chain(plan.files_to_modify.iter())
    {
        let lower = proposal.to_ascii_lowercase();
        if !(lower.contains("rename ") || lower.contains("move ")) {
            continue;
        }
        if let Some((src, dest)) = parse_rename_pair(proposal, &root) {
            if out.iter().any(|p| {
                p.change_type == PatchChangeType::RenameMove && p.file == src
            }) {
                continue;
            }
            out.push(PatchProposal {
                file: src,
                original_location: None,
                change_type: PatchChangeType::RenameMove,
                original_content: None,
                proposed_content: None,
                rationale: format!("Coding Plan rename/move: {proposal}"),
                destination: Some(dest),
                observed_hash: None,
            });
        }
    }

    out.truncate(PATCH_PROPOSAL_CAP);
    out
}

/// Merge Reasoning markdown patches onto drafts (Reasoning elaborates; Planner still validates).
pub fn merge_reasoning_patches(
    drafts: &[PatchProposal],
    reasoning_text: &str,
    project_root: Option<&Path>,
) -> Vec<PatchProposal> {
    let parsed = parse_patches_from_reasoning(reasoning_text, project_root);
    if parsed.is_empty() {
        return drafts.to_vec();
    }
    let mut merged = drafts.to_vec();
    for incoming in parsed {
        if let Some(existing) = merged.iter_mut().find(|p| {
            p.file == incoming.file && p.change_type == incoming.change_type
        }) {
            if incoming.proposed_content.is_some() {
                existing.proposed_content = incoming.proposed_content;
            }
            if incoming.original_content.is_some() {
                existing.original_content = incoming.original_content;
            }
            if incoming.original_location.is_some() {
                existing.original_location = incoming.original_location;
            }
            if !incoming.rationale.is_empty() {
                existing.rationale = incoming.rationale;
            }
            if incoming.destination.is_some() {
                existing.destination = incoming.destination;
            }
        } else {
            merged.push(incoming);
        }
    }
    merged.truncate(PATCH_PROPOSAL_CAP);
    merged
}

/// Parse `### Patch N` sections from Reasoning output.
pub fn parse_patches_from_reasoning(
    text: &str,
    project_root: Option<&Path>,
) -> Vec<PatchProposal> {
    let root = project_root
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut out = Vec::new();
    let mut current: Option<PatchProposal> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("### Patch") || line.starts_with("## Patch ") {
            if let Some(done) = current.take() {
                if !done.file.as_os_str().is_empty() {
                    out.push(done);
                }
            }
            current = Some(PatchProposal {
                file: PathBuf::new(),
                original_location: None,
                change_type: PatchChangeType::ModifyFile,
                original_content: None,
                proposed_content: None,
                rationale: String::new(),
                destination: None,
                observed_hash: None,
            });
            continue;
        }
        let Some(patch) = current.as_mut() else {
            continue;
        };
        let lower = line.to_ascii_lowercase();
        if let Some(v) = bullet_value(line, &["file:", "- file:"]) {
            if let Some(rel) = code_generation::extract_path_candidate(v)
                .or_else(|| code_generation::extract_path_candidate(&format!("`{v}`")))
            {
                patch.file = code_generation::resolve_under_root(&root, &rel);
            }
        } else if let Some(v) = bullet_value(line, &["change_type:", "- change_type:", "type:"]) {
            patch.change_type = parse_change_type(v);
        } else if let Some(v) = bullet_value(line, &["location:", "- location:"]) {
            patch.original_location = Some(PatchLocation {
                note: Some(v.to_string()),
                ..Default::default()
            });
        } else if let Some(v) = bullet_value(line, &["original:", "- original:"]) {
            patch.original_content = Some(v.to_string());
        } else if let Some(v) = bullet_value(line, &["proposed:", "- proposed:"]) {
            patch.proposed_content = Some(v.to_string());
        } else if let Some(v) = bullet_value(line, &["destination:", "- destination:"]) {
            if let Some(rel) = code_generation::extract_path_candidate(v)
                .or_else(|| code_generation::extract_path_candidate(&format!("`{v}`")))
            {
                patch.destination = Some(code_generation::resolve_under_root(&root, &rel));
            }
        } else if let Some(v) = bullet_value(line, &["rationale:", "- rationale:"]) {
            patch.rationale = v.to_string();
        } else if lower.starts_with("- ") && patch.proposed_content.is_none() {
            // ignore
        }
    }
    if let Some(done) = current.take() {
        if !done.file.as_os_str().is_empty() {
            out.push(done);
        }
    }
    out
}

/// Validate one proposal against observed file bytes (None = missing).
/// `destination_current` is used for rename/move destination collision checks.
pub fn validate_proposal(
    mut proposal: PatchProposal,
    current: Option<&str>,
    destination_current: Option<&str>,
) -> ValidatedPatch {
    // Attach hash when we observed content.
    if let Some(body) = current {
        proposal.observed_hash = Some(simple_hash(body));
        if proposal.original_content.is_none()
            && matches!(proposal.change_type, PatchChangeType::ModifyFile)
        {
            // Capture a bounded anchor for review display.
            proposal.original_content = Some(truncate(body, SNIPPET_CHAR_CAP));
        }
    }

    let status = match proposal.change_type {
        PatchChangeType::CreateFile => match current {
            Some(_) => PatchValidationStatus::UnexpectedExists {
                reason: format!(
                    "Refusing create — `{}` already exists (would not overwrite silently).",
                    proposal.file.display()
                ),
            },
            None => {
                if proposal
                    .proposed_content
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
                {
                    PatchValidationStatus::Invalid {
                        reason: "Create patch missing proposed content.".into(),
                    }
                } else {
                    PatchValidationStatus::Valid
                }
            }
        },
        PatchChangeType::ModifyFile => match current {
            None => PatchValidationStatus::MissingFile {
                reason: format!(
                    "Cannot modify missing file `{}`.",
                    proposal.file.display()
                ),
            },
            Some(body) => {
                if proposal
                    .proposed_content
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
                {
                    PatchValidationStatus::Invalid {
                        reason: "Modify patch missing proposed content.".into(),
                    }
                } else if let Some(orig) = &proposal.original_content {
                    if !orig.trim().is_empty() && !body.contains(orig.trim()) {
                        // If original is our truncated whole-file snapshot, accept hash path.
                        let truncated_whole = truncate(body, SNIPPET_CHAR_CAP);
                        if orig.trim() == truncated_whole.trim() {
                            PatchValidationStatus::Valid
                        } else {
                            PatchValidationStatus::Stale {
                                reason: format!(
                                    "Original context not found in `{}` — file may have changed.",
                                    proposal.file.display()
                                ),
                            }
                        }
                    } else {
                        PatchValidationStatus::Valid
                    }
                } else {
                    PatchValidationStatus::Valid
                }
            }
        },
        PatchChangeType::DeleteFile => match current {
            None => PatchValidationStatus::MissingFile {
                reason: format!(
                    "Cannot delete missing file `{}`.",
                    proposal.file.display()
                ),
            },
            Some(_) => PatchValidationStatus::Valid,
        },
        PatchChangeType::RenameMove => {
            let dest = proposal.destination.clone();
            match (current, dest) {
                (None, _) => PatchValidationStatus::MissingFile {
                    reason: format!(
                        "Cannot rename missing file `{}`.",
                        proposal.file.display()
                    ),
                },
                (_, None) => PatchValidationStatus::Invalid {
                    reason: "Rename/move patch missing destination.".into(),
                },
                (Some(_), Some(dest)) => {
                    if destination_current.is_some() {
                        PatchValidationStatus::UnexpectedExists {
                            reason: format!(
                                "Rename destination `{}` already exists.",
                                dest.display()
                            ),
                        }
                    } else {
                        PatchValidationStatus::Valid
                    }
                }
            }
        }
    };

    ValidatedPatch { proposal, status }
}

/// Convert only **valid** patches into GenerationOps for the existing review path.
pub fn generation_from_validated(
    goal: impl Into<String>,
    validated: &[ValidatedPatch],
) -> Result<CodeGeneration, String> {
    let goal = goal.into();
    let mut operations = Vec::new();
    for patch in validated.iter().filter(|p| p.status.is_valid()) {
        let p = &patch.proposal;
        match p.change_type {
            PatchChangeType::CreateFile => {
                let content = p
                    .proposed_content
                    .clone()
                    .ok_or_else(|| "valid create missing content".to_string())?;
                operations.push(GenerationOp::create_file(
                    p.file.clone(),
                    content,
                    p.rationale.clone(),
                ));
            }
            PatchChangeType::ModifyFile => {
                let content = p
                    .proposed_content
                    .clone()
                    .ok_or_else(|| "valid modify missing content".to_string())?;
                operations.push(GenerationOp::modify_file(
                    p.file.clone(),
                    content,
                    p.rationale.clone(),
                ));
            }
            PatchChangeType::DeleteFile => {
                operations.push(GenerationOp::delete_file(
                    p.file.clone(),
                    p.rationale.clone(),
                ));
            }
            PatchChangeType::RenameMove => {
                let dest = p
                    .destination
                    .clone()
                    .ok_or_else(|| "valid rename missing destination".to_string())?;
                operations.push(GenerationOp::rename_move(
                    p.file.clone(),
                    dest,
                    p.rationale.clone(),
                ));
            }
        }
    }
    if operations.is_empty() {
        return Err("no validated patches to convert".into());
    }
    Ok(CodeGeneration {
        goal,
        operations,
        coding_plan_summary: vec![
            "Converted from C3.4 Patch Generation (Review Before Action still required).".into(),
        ],
    })
}

/// Apply a targeted modify: if original anchor is present, replace it; else append a marker block.
pub fn apply_targeted_modify(current: &str, original: Option<&str>, proposed: &str) -> String {
    if let Some(orig) = original {
        let orig = orig.trim();
        if !orig.is_empty() && current.contains(orig) {
            return current.replacen(orig, proposed, 1);
        }
    }
    // Do not blindly rewrite the whole file — append a clearly marked patch block.
    let mut out = current.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n/* jaymi-patch-proposal begin */\n");
    out.push_str(proposed);
    if !proposed.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("/* jaymi-patch-proposal end */\n");
    out
}

fn targeted_create_body(path: &Path, goal: &str, proposal: &str) -> String {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("generated");
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => format!(
            "// Jaymi patch proposal (C3.4) — create `{name}`\n\
             // Goal: {goal}\n\
             // Plan: {proposal}\n\
             // Review Before Action required; not written yet.\n\n\
             #![allow(dead_code)]\n\n\
             // TODO: implement per Coding Plan\n"
        ),
        Some("md") => format!("# {name}\n\nGoal: {goal}\n\n{proposal}\n"),
        _ => format!("# Jaymi create proposal for {name}\n# Goal: {goal}\n# {proposal}\n"),
    }
}

fn targeted_modify_body(
    path: &Path,
    goal: &str,
    proposal: &str,
    original: Option<&str>,
) -> String {
    let name = path.display();
    if let Some(orig) = original {
        // Surgical replacement candidate — Planner applies against current on convert.
        return format!(
            "{orig}\n// jaymi: updated for goal `{goal}` ({proposal})\n"
        );
    }
    format!(
        "// Targeted modification proposal for `{name}`\n\
         // Goal: {goal}\n\
         // Plan note: {proposal}\n\
         // (anchor filled after read; not a whole-file rewrite)\n"
    )
}

fn parse_rename_pair(text: &str, root: &Path) -> Option<(PathBuf, PathBuf)> {
    // Best-effort: two backtick paths, or "rename A to B".
    let mut paths = Vec::new();
    let mut rest = text;
    while let Some(rel) = code_generation::extract_path_candidate(rest) {
        paths.push(code_generation::resolve_under_root(root, &rel));
        if let Some(idx) = rest.find('`') {
            rest = &rest[idx + 1..];
            if let Some(end) = rest.find('`') {
                rest = &rest[end + 1..];
            } else {
                break;
            }
        } else {
            break;
        }
        if paths.len() == 2 {
            break;
        }
    }
    if paths.len() >= 2 {
        return Some((paths[0].clone(), paths[1].clone()));
    }
    None
}

fn parse_change_type(raw: &str) -> PatchChangeType {
    let t = raw.trim().to_ascii_lowercase();
    if t.contains("create") {
        PatchChangeType::CreateFile
    } else if t.contains("delete") || t.contains("remove") {
        PatchChangeType::DeleteFile
    } else if t.contains("rename") || t.contains("move") {
        PatchChangeType::RenameMove
    } else {
        PatchChangeType::ModifyFile
    }
}

fn bullet_value<'a>(line: &'a str, keys: &[&str]) -> Option<&'a str> {
    let lower = line.to_ascii_lowercase();
    for key in keys {
        let k = key.to_ascii_lowercase();
        if lower.starts_with(&k) {
            return Some(line[key.len()..].trim());
        }
        // allow "- file: x" when key is "file:"
        let prefixed = format!("- {k}");
        if lower.starts_with(&prefixed) {
            return Some(line[prefixed.len()..].trim().trim_start_matches(':').trim());
        }
    }
    None
}

fn simple_hash(text: &str) -> String {
    content_fingerprint(text)
}

/// Stable non-crypto fingerprint for staleness / recovery diagnostics (C3.4 / C3.7).
pub fn content_fingerprint(text: &str) -> String {
    // Stable non-crypto fingerprint for staleness diagnostics.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn truncate(text: &str, max_chars: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max_chars {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Public snippet helper for Planner validation display.
pub fn snippet_preview(text: &str) -> String {
    truncate(text, SNIPPET_CHAR_CAP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_plan::{CodingPlan, CodingPlanKind, CodingPlanOrigin};

    #[test]
    fn drafts_create_modify_delete_from_plan() {
        let plan = CodingPlan {
            origin: CodingPlanOrigin::ChangeInvestigation,
            kind: Some(CodingPlanKind::Mutation),
            objective: Some("Add helper.".into()),
            files_to_create: vec!["`src/helper.rs` new module".into()],
            files_to_modify: vec!["`src/lib.rs` (from investigation)".into()],
            files_to_remove: vec!["`src/old.rs` (only if whole-file removal)".into()],
            ..Default::default()
        };
        let drafts = draft_proposals_from_plan(&plan, "Add helper.", Some(Path::new("/proj")));
        assert!(drafts
            .iter()
            .any(|p| p.change_type == PatchChangeType::CreateFile
                && p.file.ends_with("src/helper.rs")));
        assert!(drafts
            .iter()
            .any(|p| p.change_type == PatchChangeType::ModifyFile
                && p.file.ends_with("src/lib.rs")));
        assert!(drafts
            .iter()
            .any(|p| p.change_type == PatchChangeType::DeleteFile
                && p.file.ends_with("src/old.rs")));
    }

    #[test]
    fn validate_create_modify_stale_and_invalid() {
        let create = PatchProposal {
            file: PathBuf::from("/proj/new.rs"),
            original_location: None,
            change_type: PatchChangeType::CreateFile,
            original_content: None,
            proposed_content: Some("fn x() {}".into()),
            rationale: "add".into(),
            destination: None,
            observed_hash: None,
        };
        assert!(validate_proposal(create.clone(), None, None).status.is_valid());
        assert!(matches!(
            validate_proposal(create, Some("exists"), None).status,
            PatchValidationStatus::UnexpectedExists { .. }
        ));

        let modify = PatchProposal {
            file: PathBuf::from("/proj/a.rs"),
            original_location: None,
            change_type: PatchChangeType::ModifyFile,
            original_content: Some("fn old()".into()),
            proposed_content: Some("fn new()".into()),
            rationale: "retarget".into(),
            destination: None,
            observed_hash: None,
        };
        assert!(validate_proposal(modify.clone(), Some("fn old()\n"), None)
            .status
            .is_valid());
        assert!(matches!(
            validate_proposal(modify, Some("fn other()\n"), None).status,
            PatchValidationStatus::Stale { .. }
        ));

        let invalid = PatchProposal {
            file: PathBuf::from("/proj/a.rs"),
            original_location: None,
            change_type: PatchChangeType::ModifyFile,
            original_content: None,
            proposed_content: Some("".into()),
            rationale: "empty".into(),
            destination: None,
            observed_hash: None,
        };
        assert!(matches!(
            validate_proposal(invalid, Some("body"), None).status,
            PatchValidationStatus::Invalid { .. }
        ));
    }

    #[test]
    fn generation_from_validated_skips_invalid() {
        let patches = vec![
            ValidatedPatch {
                proposal: PatchProposal {
                    file: PathBuf::from("/proj/new.rs"),
                    original_location: None,
                    change_type: PatchChangeType::CreateFile,
                    original_content: None,
                    proposed_content: Some("ok".into()),
                    rationale: "create".into(),
                    destination: None,
                    observed_hash: None,
                },
                status: PatchValidationStatus::Valid,
            },
            ValidatedPatch {
                proposal: PatchProposal {
                    file: PathBuf::from("/proj/stale.rs"),
                    original_location: None,
                    change_type: PatchChangeType::ModifyFile,
                    original_content: Some("gone".into()),
                    proposed_content: Some("x".into()),
                    rationale: "stale".into(),
                    destination: None,
                    observed_hash: None,
                },
                status: PatchValidationStatus::Stale {
                    reason: "mismatch".into(),
                },
            },
        ];
        let gen = generation_from_validated("goal", &patches).unwrap();
        assert_eq!(gen.operations.len(), 1);
        assert_eq!(gen.operations[0].kind, GenerationOpKind::CreateFile);
    }

    #[test]
    fn parse_reasoning_patch_section() {
        let text = r#"
### Patch 1
- file: `src/a.rs`
- change_type: modify_file
- location: line 10
- original: fn a()
- proposed: fn a_v2()
- rationale: rename helper
"#;
        let parsed = parse_patches_from_reasoning(text, Some(Path::new("/proj")));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].change_type, PatchChangeType::ModifyFile);
        assert!(parsed[0].file.ends_with("src/a.rs"));
        assert_eq!(parsed[0].proposed_content.as_deref(), Some("fn a_v2()"));
    }

    #[test]
    fn targeted_modify_preserves_surrounding() {
        let current = "alpha\nfn old()\nomega\n";
        let out = apply_targeted_modify(current, Some("fn old()"), "fn new()");
        assert!(out.contains("alpha"));
        assert!(out.contains("fn new()"));
        assert!(out.contains("omega"));
        assert!(!out.contains("fn old()"));
    }
}
