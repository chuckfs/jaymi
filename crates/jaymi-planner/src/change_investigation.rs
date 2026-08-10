//! Change Investigation (Sprint C3.2).
//!
//! After a Planner-owned [`crate::coding_edit::CodingEditObjective`] is
//! established, investigate what the requested mutation would touch — by
//! **reusing** C2 deep investigation / relationship / search helpers.
//!
//! Constitutional constraints:
//! - No second investigation system (calls into `deep_investigation` /
//!   `relationship_tracing` / WI — never duplicates soft-path ownership)
//! - Planner orchestrates tool mediation (`dispatch_tool_backed`)
//! - ContextEngine remains sole ContextBundle factory
//! - Read-only — no writes, mutation tools, patches, or Execution Plans
//! - Bounded by existing C2 caps
//! - Explicit unknowns when evidence is insufficient

use std::path::{Path, PathBuf};
use std::time::Instant;

use jaymi_context::ContextBundle;

use crate::coding_edit::{
    CodingEditKind, CodingEditObjective, CodingEditStatus, EditScope,
};
use crate::deep_investigation::{
    self, DeepInvestigationReport, InvestigationAssessment, InvestigationKind,
};
use crate::relationship_tracing::{TraceAssessment, TraceKind};

/// Cap conversation-facing rows (align with deep investigation).
pub const CONVERSATION_ROW_CAP: usize = deep_investigation::CONVERSATION_EVIDENCE_CAP;
/// Re-export C2 search hit limit for Planner orchestration.
pub const SEARCH_HIT_LIMIT: usize = deep_investigation::SEARCH_HIT_LIMIT;

/// How thoroughly investigation ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ChangeInvestigationMode {
    /// Full Planner-mediated C2 gather under budget.
    Full,
    /// Ambient WI / objective only (clarify / missing project).
    Light,
    /// Explicitly skipped (should be rare).
    Skipped,
}

impl ChangeInvestigationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Light => "light",
            Self::Skipped => "skipped",
        }
    }
}

impl Default for ChangeInvestigationMode {
    fn default() -> Self {
        Self::Light
    }
}

/// One evidence row (mirrors C2 investigation evidence shape).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChangeEvidence {
    pub path: PathBuf,
    pub title: String,
    pub matched: String,
    pub relevance: u32,
    pub line: Option<u32>,
    pub preview: String,
    pub source: String,
}

/// Bounded step for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChangeInvestigationStep {
    pub index: u32,
    pub action: String,
    pub detail: String,
    pub evidence_added: usize,
}

/// Planner-authored change investigation (C3.2) — read-only.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ChangeInvestigationReport {
    pub mode: ChangeInvestigationMode,
    pub goal: String,
    pub edit_kind: Option<CodingEditKind>,
    pub primary_file: Option<String>,
    pub primary_symbols: Vec<String>,
    pub affected_files: Vec<String>,
    pub affected_symbols: Vec<String>,
    pub related_dependencies: Vec<String>,
    pub implementation_patterns: Vec<String>,
    pub tests_possibly_affected: Vec<String>,
    pub configuration_possibly_affected: Vec<String>,
    pub likely_side_effects: Vec<String>,
    pub architectural_constraints: Vec<String>,
    pub related_functionality: Vec<String>,
    pub evidence: Vec<ChangeEvidence>,
    pub searches: Vec<String>,
    pub steps: Vec<ChangeInvestigationStep>,
    /// Explicit unknowns — what could not be established.
    pub unknowns: Vec<String>,
    pub truncated: bool,
    pub stop_reason: Option<String>,
    pub elapsed_ms: u64,
    pub files_inspected: usize,
    pub symbols_followed: usize,
    pub diagnostics: Vec<String>,
}

impl ChangeInvestigationReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Change Investigation\n");
        out.push_str(&format!(
            "\n### Mode\n- {} (read-only — no files modified)\n",
            self.mode.as_str()
        ));
        if let Some(kind) = self.edit_kind {
            out.push_str(&format!("\n### Edit kind\n- {}\n", kind.label()));
        }
        out.push_str("\n### Objective\n- ");
        out.push_str(if self.goal.is_empty() {
            "(none)"
        } else {
            &self.goal
        });
        out.push('\n');

        out.push_str("\n### Primary target\n");
        if let Some(file) = &self.primary_file {
            out.push_str(&format!("- File: `{file}`\n"));
        }
        for sym in &self.primary_symbols {
            out.push_str(&format!("- Symbol: `{sym}`\n"));
        }
        if self.primary_file.is_none() && self.primary_symbols.is_empty() {
            out.push_str("- _(unresolved)_\n");
        }

        push_list(&mut out, "Affected files", &self.affected_files, true);
        push_list(&mut out, "Affected symbols", &self.affected_symbols, true);
        push_list(
            &mut out,
            "Related dependencies",
            &self.related_dependencies,
            false,
        );
        push_list(
            &mut out,
            "Implementation patterns",
            &self.implementation_patterns,
            false,
        );
        push_list(
            &mut out,
            "Tests that may be affected",
            &self.tests_possibly_affected,
            true,
        );
        push_list(
            &mut out,
            "Configuration that may be affected",
            &self.configuration_possibly_affected,
            true,
        );
        push_list(&mut out, "Likely side effects", &self.likely_side_effects, false);
        push_list(
            &mut out,
            "Architectural constraints",
            &self.architectural_constraints,
            false,
        );
        push_list(
            &mut out,
            "Existing related functionality",
            &self.related_functionality,
            false,
        );

        out.push_str("\n### Evidence\n");
        if self.evidence.is_empty() {
            out.push_str("- _(no solid evidence gathered yet)_\n");
        } else {
            for (i, hit) in self.evidence.iter().take(CONVERSATION_ROW_CAP).enumerate() {
                let loc = hit
                    .line
                    .map(|l| format!(":{}", l.saturating_add(1)))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "{}. `{}{}` — {} ({})\n",
                    i + 1,
                    hit.title,
                    loc,
                    hit.matched,
                    hit.source
                ));
                if !hit.preview.trim().is_empty() {
                    out.push_str(&format!("   {}\n", truncate(&hit.preview, 140)));
                }
            }
        }

        out.push_str("\n### What is unknown\n");
        if self.unknowns.is_empty() {
            out.push_str("- _(no explicit unknowns recorded)_\n");
        } else {
            for u in &self.unknowns {
                out.push_str(&format!("- {u}\n"));
            }
        }

        if self.truncated {
            out.push_str("\n_Investigation truncated under C2 budget._\n");
        }

        out.push_str("\n### Notes\n");
        out.push_str("- Read-only — reuses C2 investigation helpers; no mutation tools.\n");
        out.push_str("- Patches and Execution Plans are deferred to later C3 slices.\n");
        out
    }

    pub fn to_diagnostics_summary(&self) -> String {
        format!(
            "change_investigation mode={} kind={} files={} symbols={} evidence={} searches={} unknowns={} steps={} truncated={} stop={} ms={}",
            self.mode.as_str(),
            self.edit_kind.map(|k| k.as_str()).unwrap_or("unknown"),
            self.affected_files.len(),
            self.affected_symbols.len(),
            self.evidence.len(),
            self.searches.len(),
            self.unknowns.len(),
            self.steps.len(),
            self.truncated,
            self.stop_reason.as_deref().unwrap_or("—"),
            self.elapsed_ms,
        )
    }
}

fn push_list(out: &mut String, title: &str, items: &[String], backticks: bool) {
    out.push_str(&format!("\n### {title}\n"));
    if items.is_empty() {
        out.push_str("- _(none observed)_\n");
        return;
    }
    for item in items.iter().take(CONVERSATION_ROW_CAP) {
        if backticks {
            out.push_str(&format!("- `{item}`\n"));
        } else {
            out.push_str(&format!("- {item}\n"));
        }
    }
}

/// Whether a full C2-mediated gather should run.
pub fn should_run_full(objective: &CodingEditObjective) -> bool {
    matches!(objective.status, CodingEditStatus::Understood)
}

/// Build a C2 InvestigationAssessment from a coding-edit objective (no detect_*).
pub fn assessment_from_objective(objective: &CodingEditObjective) -> InvestigationAssessment {
    let kind = match objective.kind {
        Some(CodingEditKind::Fix) => InvestigationKind::ChangeImpact,
        Some(CodingEditKind::Remove) => InvestigationKind::ChangeImpact,
        Some(CodingEditKind::Rename) => InvestigationKind::ChangeImpact,
        Some(CodingEditKind::Refactor) | Some(CodingEditKind::Optimize) => {
            InvestigationKind::ChangeImpact
        }
        Some(CodingEditKind::Add) | Some(CodingEditKind::Create) => {
            InvestigationKind::FeaturePlacement
        }
        Some(CodingEditKind::Change) => InvestigationKind::ChangeImpact,
        Some(CodingEditKind::Generic) | None => InvestigationKind::General,
    };
    let subject = objective
        .target_symbols
        .first()
        .cloned()
        .or_else(|| named_phrase_from_goal(&objective.goal))
        .or_else(|| {
            objective
                .target_file
                .as_ref()
                .and_then(|p| Path::new(p).file_stem().map(|s| s.to_string_lossy().into_owned()))
        });
    InvestigationAssessment {
        kind,
        goal: objective.goal.clone(),
        subject,
        matched_rules: vec!["c32_from_coding_edit"],
    }
}

/// Optional relationship seed when a primary symbol is known.
pub fn trace_assessment_from_objective(objective: &CodingEditObjective) -> Option<TraceAssessment> {
    let seed = objective.target_symbols.first().cloned()?;
    Some(TraceAssessment {
        kind: TraceKind::General,
        goal: objective.goal.clone(),
        seed_hint: Some(seed),
        matched_rules: vec!["c32_relationship_seed"],
    })
}

/// Light report from objective + ambient WI (no tools).
pub fn light_from_objective(
    objective: &CodingEditObjective,
    bundle: &ContextBundle,
) -> ChangeInvestigationReport {
    let started = Instant::now();
    let mut report = base_from_objective(objective, ChangeInvestigationMode::Light);
    enrich_from_workspace(&mut report, objective, bundle);
    report.steps.push(ChangeInvestigationStep {
        index: 1,
        action: "light_ambient".into(),
        detail: "NeedsClarification or insufficient target — ambient WI only".into(),
        evidence_added: report.evidence.len(),
    });
    for a in &objective.ambiguities {
        report.unknowns.push(a.clone());
    }
    if let Some(q) = &objective.clarification {
        report.unknowns.push(format!("Clarification pending: {q}"));
    }
    if report.affected_files.is_empty() && report.primary_file.is_none() {
        report
            .unknowns
            .push("Affected files unknown — no bound target and no ambient references.".into());
    }
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    report.diagnostics.push("mode=light".into());
    report
}

/// Fold a completed deep investigation into the change report, then enrich.
pub fn fold_deep_report(
    objective: &CodingEditObjective,
    bundle: &ContextBundle,
    deep: &DeepInvestigationReport,
    relationship_files: &[String],
    relationship_symbols: &[String],
) -> ChangeInvestigationReport {
    let started = Instant::now();
    let mut report = base_from_objective(objective, ChangeInvestigationMode::Full);
    report.searches = deep.searches.clone();
    report.truncated = deep.truncated;
    report.stop_reason = deep
        .stop_reason
        .as_ref()
        .map(|s| s.as_str().to_string());
    report.files_inspected = deep.files_inspected.len();
    report.symbols_followed = deep.symbols_followed.len();

    for step in &deep.steps {
        report.steps.push(ChangeInvestigationStep {
            index: step.index,
            action: step.action.clone(),
            detail: step.detail.clone(),
            evidence_added: step.evidence_added,
        });
    }

    for path in &deep.files_inspected {
        push_unique(&mut report.affected_files, path.clone());
    }
    for sym in &deep.symbols_followed {
        push_unique(&mut report.affected_symbols, sym.clone());
    }
    for path in relationship_files {
        push_unique(&mut report.affected_files, path.clone());
    }
    for sym in relationship_symbols {
        push_unique(&mut report.affected_symbols, sym.clone());
    }

    for hit in &deep.evidence {
        if report.evidence.len() >= CONVERSATION_ROW_CAP {
            report.truncated = true;
            break;
        }
        report.evidence.push(ChangeEvidence {
            path: hit.path.clone(),
            title: hit.title.clone(),
            matched: hit.matched.clone(),
            relevance: hit.relevance,
            line: hit.line,
            preview: hit.preview.clone(),
            source: hit.why.clone(),
        });
        classify_path_into_buckets(&mut report, &hit.path.display().to_string());
    }

    for gap in &deep.gaps {
        report.unknowns.push(gap.clone());
    }

    enrich_from_workspace(&mut report, objective, bundle);
    synthesize_side_effects(&mut report, objective);

    if report.evidence.is_empty() {
        report
            .unknowns
            .push("Insufficient project evidence to map the change impact.".into());
    }

    report.elapsed_ms = started
        .elapsed()
        .as_millis()
        .max(deep.elapsed_ms as u128) as u64;
    report.diagnostics.push(format!(
        "folded_deep kind={} steps={} hits={}",
        deep.kind.map(|k| k.as_str()).unwrap_or("unknown"),
        deep.steps.len(),
        deep.evidence.len()
    ));
    report
}

fn base_from_objective(
    objective: &CodingEditObjective,
    mode: ChangeInvestigationMode,
) -> ChangeInvestigationReport {
    ChangeInvestigationReport {
        mode,
        goal: objective.goal.clone(),
        edit_kind: objective.kind,
        primary_file: objective.target_file.clone(),
        primary_symbols: objective.target_symbols.clone(),
        affected_files: objective.relevant_files.clone(),
        affected_symbols: objective.target_symbols.clone(),
        architectural_constraints: vec![
            "Read-only investigation — no file modifications in this turn.".into(),
            "Mutations require Review Before Action in a later C3 slice.".into(),
        ],
        diagnostics: vec![format!(
            "from_coding_edit status={}",
            objective.status.as_str()
        )],
        ..ChangeInvestigationReport::default()
    }
}

fn enrich_from_workspace(
    report: &mut ChangeInvestigationReport,
    objective: &CodingEditObjective,
    bundle: &ContextBundle,
) {
    let project = bundle.project_intelligence();
    let editor = bundle.editor_intelligence();

    if let Some(pm) = &project.package_manager {
        report
            .related_dependencies
            .push(format!("package_manager={pm}"));
    }
    for dep in project.dependency_summary.top_level.iter().take(8) {
        push_unique(&mut report.related_dependencies, dep.clone());
    }
    for member in project.dependency_summary.workspace_members.iter().take(6) {
        push_unique(
            &mut report.related_functionality,
            format!("workspace_member={member}"),
        );
    }
    for dir in project.top_level_dirs.iter().take(8) {
        push_unique(
            &mut report.implementation_patterns,
            format!("top_level_dir={dir}"),
        );
    }

    for reference in editor.references.iter().take(12) {
        push_unique(&mut report.affected_files, reference.path.clone());
        classify_path_into_buckets(report, &reference.path);
    }

    if let Some(path) = &objective.target_file {
        classify_path_into_buckets(report, path);
        push_unique(&mut report.affected_files, path.clone());
    }

    let roots: Vec<String> = report
        .affected_files
        .iter()
        .filter_map(|p| path_subsystem(p))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if roots.len() > 1 {
        report.likely_side_effects.push(format!(
            "Change appears to cross subsystem boundaries: {}",
            roots.join(", ")
        ));
        report.architectural_constraints.push(
            "Coordinate shared contracts when editing across observed subsystems.".into(),
        );
    }

    match objective.expected_scope {
        Some(EditScope::MultiFile) => {
            report
                .implementation_patterns
                .push("Expected scope: multi_file (from edit intent)".into());
        }
        Some(EditScope::Project) => {
            report
                .implementation_patterns
                .push("Expected scope: project/feature-level".into());
        }
        Some(EditScope::Selection) => {
            report
                .implementation_patterns
                .push("Expected scope: selection-local".into());
        }
        _ => {}
    }

    for note in &objective.investigation_notes {
        push_unique(&mut report.related_functionality, note.clone());
    }
    for ctx in &objective.relevant_context {
        if ctx.starts_with("named_target_phrase:") {
            push_unique(&mut report.related_functionality, ctx.clone());
        }
    }
}

fn synthesize_side_effects(report: &mut ChangeInvestigationReport, objective: &CodingEditObjective) {
    if !report.tests_possibly_affected.is_empty() {
        report
            .likely_side_effects
            .push("Existing tests may need updates or new coverage.".into());
    }
    if !report.configuration_possibly_affected.is_empty() {
        report
            .likely_side_effects
            .push("Configuration / manifests may need corresponding updates.".into());
    }
    if matches!(
        objective.kind,
        Some(CodingEditKind::Remove) | Some(CodingEditKind::Rename)
    ) {
        report
            .likely_side_effects
            .push("Call sites and imports across affected files may break.".into());
    }
    if matches!(objective.kind, Some(CodingEditKind::Add) | Some(CodingEditKind::Create)) {
        report.likely_side_effects.push(
            "New surface may require capability / provider / UI wiring where those systems exist."
                .into(),
        );
    }
}

fn classify_path_into_buckets(report: &mut ChangeInvestigationReport, path: &str) {
    let lower = path.to_ascii_lowercase();
    if lower.contains("test")
        || lower.contains("/tests/")
        || lower.ends_with("_test.rs")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with("_spec.rs")
    {
        push_unique(&mut report.tests_possibly_affected, path.to_string());
    }
    if lower.ends_with("cargo.toml")
        || lower.ends_with("package.json")
        || lower.ends_with("pyproject.toml")
        || lower.contains("/config/")
        || lower.contains(".env")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with("settings.json")
    {
        push_unique(
            &mut report.configuration_possibly_affected,
            path.to_string(),
        );
    }
    if lower.contains("/docs/") || lower.ends_with(".md") {
        push_unique(
            &mut report.related_functionality,
            format!("docs_candidate={path}"),
        );
    }
}

fn path_subsystem(path: &str) -> Option<String> {
    let p = Path::new(path);
    let components: Vec<_> = p
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
        .collect();
    for window in components.windows(2) {
        if window[0] == "crates" || window[0] == "apps" || window[0] == "packages" {
            return Some(format!("{}/{}", window[0], window[1]));
        }
    }
    if let Some(idx) = components.iter().position(|c| c == "src") {
        if let Some(next) = components.get(idx + 1) {
            return Some(format!("src/{next}"));
        }
    }
    None
}

fn named_phrase_from_goal(goal: &str) -> Option<String> {
    let lower = goal.to_ascii_lowercase();
    for marker in [
        "add ",
        "create ",
        "change ",
        "fix ",
        "update ",
        "remove ",
        "to ",
    ] {
        if let Some(idx) = lower.find(marker) {
            let after = goal[idx + marker.len()..]
                .trim()
                .trim_end_matches(|c: char| c == '.' || c == '?' || c == '!');
            let phrase: String = after
                .split_whitespace()
                .take(5)
                .collect::<Vec<_>>()
                .join(" ");
            if phrase.len() >= 3
                && !phrase.eq_ignore_ascii_case("this")
                && !phrase.eq_ignore_ascii_case("it")
            {
                return Some(phrase);
            }
        }
    }
    None
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if value.trim().is_empty() {
        return;
    }
    if !list.iter().any(|x| x == &value) {
        list.push(value);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_edit::CodingEditObjective;

    #[test]
    fn assessment_maps_edit_kinds() {
        let mut obj = CodingEditObjective {
            status: CodingEditStatus::Understood,
            kind: Some(CodingEditKind::Add),
            goal: "Add image generation to Settings.".into(),
            target_symbols: vec!["Settings".into()],
            ..CodingEditObjective::default()
        };
        let a = assessment_from_objective(&obj);
        assert_eq!(a.kind, InvestigationKind::FeaturePlacement);
        assert_eq!(a.subject.as_deref(), Some("Settings"));

        obj.kind = Some(CodingEditKind::Fix);
        assert_eq!(
            assessment_from_objective(&obj).kind,
            InvestigationKind::ChangeImpact
        );
    }

    #[test]
    fn light_records_unknowns_from_ambiguities() {
        let obj = CodingEditObjective {
            status: CodingEditStatus::NeedsClarification,
            kind: Some(CodingEditKind::Fix),
            goal: "Fix this.".into(),
            ambiguities: vec!["No target".into()],
            clarification: Some("Which file?".into()),
            ..CodingEditObjective::default()
        };
        let bundle = ContextBundle::builder().build();
        let report = light_from_objective(&obj, &bundle);
        assert_eq!(report.mode, ChangeInvestigationMode::Light);
        assert!(!report.unknowns.is_empty());
        assert!(report.to_markdown().contains("Change Investigation"));
        assert!(report
            .to_diagnostics_summary()
            .contains("change_investigation"));
    }

    #[test]
    fn classifies_test_and_config_paths() {
        let mut report = ChangeInvestigationReport::default();
        classify_path_into_buckets(&mut report, "/proj/crates/foo/src/lib.rs");
        classify_path_into_buckets(&mut report, "/proj/crates/foo/tests/smoke.rs");
        classify_path_into_buckets(&mut report, "/proj/Cargo.toml");
        assert!(report
            .tests_possibly_affected
            .iter()
            .any(|p| p.contains("tests/smoke")));
        assert!(report
            .configuration_possibly_affected
            .iter()
            .any(|p| p.contains("Cargo.toml")));
    }
}
