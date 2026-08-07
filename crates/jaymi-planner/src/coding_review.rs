//! Coding Review (Sprint C1.3).
//!
//! Planner-owned structured code review built **only** from already-assembled
//! Workspace Intelligence / ContextBundle.
//!
//! Constitutional constraints:
//! - No new context systems (ContextEngine remains sole ContextBundle factory)
//! - No provider bypasses
//! - No filesystem scans
//! - No tool execution
//! - No edits
//! - No Execution Plans
//! - Planner ownership unchanged (detect + scaffold + instruct Reasoning)

use jaymi_context::ContextBundle;
use jaymi_core::UserRequest;

/// What the user asked to review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewFocus {
    /// Active file.
    File,
    /// Current selection / function.
    Function,
    /// Working-tree / recent changes (GitSnapshot-derived).
    Changes,
}

impl ReviewFocus {
    /// Stable id for AssembleHints / diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Function => "function",
            Self::Changes => "changes",
        }
    }
}

/// Structured coding review returned to the conversation.
///
/// Observation scaffolds from Workspace Intelligence. Reasoning elaborates;
/// it must not invent tools, edits, execution, or Execution Plans.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodingReview {
    /// What is working well.
    pub strengths: Vec<String>,
    /// Gaps, smells, missing clarity.
    pub weaknesses: Vec<String>,
    /// Likely bugs / diagnostics / failure signals.
    pub potential_bugs: Vec<String>,
    /// Complexity observations.
    pub complexity: Vec<String>,
    /// Performance-related observations.
    pub performance: Vec<String>,
    /// Maintainability observations.
    pub maintainability: Vec<String>,
    /// Architecture / boundary observations.
    pub architecture: Vec<String>,
}

impl CodingReview {
    /// True when every section is empty.
    pub fn is_empty(&self) -> bool {
        self.strengths.is_empty()
            && self.weaknesses.is_empty()
            && self.potential_bugs.is_empty()
            && self.complexity.is_empty()
            && self.performance.is_empty()
            && self.maintainability.is_empty()
            && self.architecture.is_empty()
    }

    /// Conversation-visible markdown.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("## Coding Review\n");
        push_section(&mut out, "Strengths", &self.strengths);
        push_section(&mut out, "Weaknesses", &self.weaknesses);
        push_section(&mut out, "Potential Bugs", &self.potential_bugs);
        push_section(&mut out, "Complexity", &self.complexity);
        push_section(&mut out, "Performance", &self.performance);
        push_section(&mut out, "Maintainability", &self.maintainability);
        push_section(&mut out, "Architecture", &self.architecture);
        out
    }

    /// Prompt instruction for Reasoning.
    pub fn prompt_instruction(&self, focus: ReviewFocus) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Respond with Coding Review for focus=`{}`.",
            focus.as_str()
        ));
        lines.push(
            "Use only Workspace Intelligence already in this prompt (file, selection, editor, diagnostics, git, project, runtime, workspace memory, conversation)."
                .into(),
        );
        lines.push(
            "This is review only — do not modify files, execute tools or commands, invent unseen code, or produce an Execution Plan."
                .into(),
        );
        lines.push(
            "Fill these markdown headings exactly: Strengths · Weaknesses · Potential Bugs · Complexity · Performance · Maintainability · Architecture."
                .into(),
        );
        lines.push("Starter observations from Workspace Intelligence:".into());
        lines.push(self.to_markdown());
        lines.join("\n")
    }
}

fn push_section(out: &mut String, title: &str, items: &[String]) {
    out.push_str("\n### ");
    out.push_str(title);
    out.push('\n');
    if items.is_empty() {
        out.push_str("- _(not observed in Workspace Intelligence yet)_\n");
        return;
    }
    for item in items {
        out.push_str("- ");
        out.push_str(item);
        out.push('\n');
    }
}

/// Planner assessment: coding review mode is active for this conversational turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewAssessment {
    /// Review focus.
    pub focus: ReviewFocus,
    /// Rule ids that matched (diagnostics).
    pub matched_rules: Vec<&'static str>,
    /// WI scaffold (filled after Context assemble).
    pub scaffold: CodingReview,
}

impl ReviewAssessment {
    /// Stable AssembleHints label.
    pub fn hint_id(&self) -> String {
        format!("review:{}", self.focus.as_str())
    }
}

/// Extension key on [`jaymi_context::LlmContext`] for the prompt section.
pub const LLM_EXTENSION_KEY: &str = "coding_review";

/// Detect whether this request asks for Coding Review (no tools / edits / plans).
pub fn detect_review_request(request: &UserRequest) -> Option<ReviewAssessment> {
    // Coding Actions do not yet include a Review button — free-text only for C1.3.
    if request.coding_action.is_some() {
        return None;
    }
    let content = request.content.trim();
    if content.is_empty() {
        return None;
    }
    let lower = content.to_ascii_lowercase();
    if !looks_like_review_request(&lower) {
        return None;
    }

    let (focus, rule) = if looks_like_changes_review(&lower) {
        (ReviewFocus::Changes, "review_changes")
    } else if looks_like_function_review(&lower) {
        (ReviewFocus::Function, "review_function")
    } else if looks_like_file_review(&lower) {
        (ReviewFocus::File, "review_file")
    } else {
        // Bare "review" / "code review" — prefer selection when present later in scaffold;
        // detect defaults to file (WI will note missing selection).
        (ReviewFocus::File, "review_generic")
    };

    Some(ReviewAssessment {
        focus,
        matched_rules: vec![rule],
        scaffold: CodingReview::default(),
    })
}

fn looks_like_review_request(lower: &str) -> bool {
    lower.contains("review this")
        || lower.contains("review my")
        || lower.contains("review the")
        || lower.starts_with("review ")
        || lower.contains("code review")
        || lower.contains("please review")
}

fn looks_like_changes_review(lower: &str) -> bool {
    lower.contains("my changes")
        || lower.contains("the changes")
        || lower.contains("these changes")
        || lower.contains("my diff")
        || lower.contains("the diff")
        || lower.contains("working tree")
        || lower.contains("unstaged")
        || lower.contains("staged changes")
        || (lower.contains("review") && lower.contains("changes"))
}

fn looks_like_function_review(lower: &str) -> bool {
    lower.contains("this function")
        || lower.contains("this method")
        || lower.contains("this selection")
        || lower.contains("selected code")
        || (lower.contains("review")
            && (lower.contains("function") || lower.contains("method") || lower.contains("selection")))
}

fn looks_like_file_review(lower: &str) -> bool {
    lower.contains("this file")
        || lower.contains("the file")
        || lower.contains("current file")
        || (lower.contains("review") && lower.contains("file"))
}

/// Attach review scaffold after Context assemble.
pub fn finalize_assessment(
    mut assessment: ReviewAssessment,
    bundle: &ContextBundle,
) -> ReviewAssessment {
    assessment.scaffold = scaffold_from_bundle(assessment.focus, bundle);
    assessment
}

/// Fill a WI review scaffold (observation only).
pub fn scaffold_from_bundle(focus: ReviewFocus, bundle: &ContextBundle) -> CodingReview {
    let mut review = CodingReview::default();
    let file = bundle.current_file();
    let selection = bundle.current_selection();
    let editor = bundle.editor_intelligence();
    let diagnostics = bundle.diagnostics();
    let git = bundle.git_status();
    let project = bundle.project_intelligence();
    let memory = bundle.workspace_memory();
    let runtime = bundle.runtime_intelligence();
    let open = bundle.open_files();

    match focus {
        ReviewFocus::File => {
            if let Some(path) = &file.path {
                review.strengths.push(format!(
                    "Reviewing active file `{path}` from Workspace Intelligence."
                ));
                if let Some(lang) = &file.language {
                    review
                        .maintainability
                        .push(format!("Observed language: {lang}"));
                }
                if file.dirty {
                    review
                        .weaknesses
                        .push("Active file has unsaved edits — review may miss buffer-only state."
                            .into());
                }
            } else {
                review.weaknesses.push(
                    "No active file observed — open a file before reviewing.".into(),
                );
            }
            for summary in bundle.file_summaries().entries.iter().take(3) {
                review.maintainability.push(format!(
                    "file_summary `{}`: {}",
                    summary.path, summary.summary
                ));
            }
        }
        ReviewFocus::Function => {
            if let Some(path) = selection.path.as_ref().or(file.path.as_ref()) {
                review.strengths.push(format!(
                    "Reviewing selection / function context in `{path}`."
                ));
            } else {
                review.weaknesses.push(
                    "No file/selection path observed for function review.".into(),
                );
            }
            if let Some(text) = selection.text.as_ref().filter(|t| !t.trim().is_empty()) {
                let preview: String = text.chars().take(200).collect();
                review
                    .complexity
                    .push(format!("Selection preview ({chars} chars shown): {preview}", chars = preview.chars().count().min(200)));
                let lines = text.lines().count();
                if lines > 40 {
                    review.complexity.push(format!(
                        "Selection spans ~{lines} lines — high local complexity signal."
                    ));
                } else if lines > 0 {
                    review
                        .complexity
                        .push(format!("Selection spans ~{lines} lines."));
                }
            } else {
                review.weaknesses.push(
                    "No selection text in Workspace Intelligence — select a function to deepen review."
                        .into(),
                );
            }
            if let Some(func) = &editor.enclosing_function {
                review
                    .architecture
                    .push(format!("enclosing_function: {}", func.name));
            }
            if let Some(symbol) = &editor.symbol {
                review
                    .architecture
                    .push(format!("symbol: {}", symbol.name));
            }
            if let Some(ty) = &editor.enclosing_type {
                review
                    .architecture
                    .push(format!("enclosing_type: {}", ty.name));
            }
        }
        ReviewFocus::Changes => {
            review.strengths.push(
                "Reviewing working-tree changes from GitSnapshot-derived status (no git execution)."
                    .into(),
            );
            if !git.is_repository {
                review.weaknesses.push(
                    "No Git repository observed — cannot review changes from GitSnapshot.".into(),
                );
            } else {
                if let Some(branch) = &git.branch {
                    review
                        .architecture
                        .push(format!("git_branch: {branch}"));
                }
                if !git.summary.is_empty() {
                    review
                        .maintainability
                        .push(format!("git_summary: {}", git.summary));
                }
                let dirty =
                    git.modified_count + git.staged_count + git.untracked_count + git.conflict_count;
                if dirty == 0 {
                    review.weaknesses.push(
                        "Working tree appears clean — no dirty paths observed to review.".into(),
                    );
                } else {
                    review.complexity.push(format!(
                        "Dirty paths: modified={}, staged={}, untracked={}, conflict={}.",
                        git.modified_count,
                        git.staged_count,
                        git.untracked_count,
                        git.conflict_count
                    ));
                }
                for path in git.dirty_paths.iter().take(8) {
                    review
                        .maintainability
                        .push(format!("dirty_path: {path}"));
                }
                for path in git.staged_paths.iter().take(6) {
                    review.strengths.push(format!("staged_path: {path}"));
                }
                for path in git.untracked_paths.iter().take(4) {
                    review
                        .weaknesses
                        .push(format!("untracked_path: {path}"));
                }
                for path in git.conflict_paths.iter().take(4) {
                    review
                        .potential_bugs
                        .push(format!("conflict_path: {path}"));
                }
                for commit in git.recent_commits.iter().take(3) {
                    let short = if commit.short_sha.is_empty() {
                        commit.sha.as_str()
                    } else {
                        commit.short_sha.as_str()
                    };
                    review.architecture.push(format!(
                        "recent_commit: {short} — {}",
                        commit.subject
                    ));
                }
            }
        }
    }

    // Shared WI signals for all review foci.
    for entry in diagnostics.diagnostics.iter().take(10) {
        let path = entry.path.as_deref().unwrap_or("(workspace)");
        review.potential_bugs.push(format!(
            "[{}] {}: {}",
            entry.severity, path, entry.message
        ));
    }
    if diagnostics.diagnostics.is_empty() && matches!(focus, ReviewFocus::File | ReviewFocus::Function)
    {
        review.strengths.push(
            "No diagnostics observed on this assemble for the review focus.".into(),
        );
    }

    if let Some(hover) = &editor.hover {
        if !hover.contents.trim().is_empty() {
            let preview: String = hover.contents.chars().take(120).collect();
            review
                .maintainability
                .push(format!("hover: {preview}"));
        }
    }
    for reference in editor.references.iter().take(5) {
        review.architecture.push(format!(
            "reference: {}:{}",
            reference.path, reference.range.start_line
        ));
    }
    for entry in open.files.iter().take(6) {
        review
            .maintainability
            .push(format!("open_file: {}", entry.path));
    }

    if let Some(shape) = &project.layout_shape {
        review
            .architecture
            .push(format!("layout_shape: {shape}"));
    }
    for lang in project.languages.iter().take(4) {
        review
            .architecture
            .push(format!("project_language: {lang}"));
    }
    for dir in project.top_level_dirs.iter().take(6) {
        review
            .architecture
            .push(format!("top_level_dir: {dir}"));
    }

    if let Some(objective) = &memory.coding_objective {
        review
            .maintainability
            .push(format!("coding_objective: {objective}"));
    }
    for path in memory.recent_edits.iter().take(4) {
        review
            .complexity
            .push(format!("recent_edit: {path}"));
    }
    for failure in memory.recent_failures.iter().take(3) {
        review
            .potential_bugs
            .push(format!("recent_failure: {failure}"));
    }
    for failure in runtime.recent_failures.iter().take(3) {
        review
            .potential_bugs
            .push(format!("runtime_failure: {failure}"));
    }
    if let Some(command) = &runtime.last_command {
        review
            .performance
            .push(format!("last_terminal_command: {command}"));
    }
    for running in runtime.running.iter().take(2) {
        review
            .performance
            .push(format!("terminal_running: {running}"));
    }

    // Honest defaults when a section stayed empty.
    if review.performance.is_empty() {
        review.performance.push(
            "No runtime performance signals observed — review cannot claim hotspots without evidence."
                .into(),
        );
    }
    if review.complexity.is_empty() {
        review.complexity.push(
            "No strong complexity signals observed in Workspace Intelligence yet.".into(),
        );
    }

    review
        .weaknesses
        .push("Review is observational only — no edits, tools, or Execution Plans were produced.".into());

    review
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        BundleDiagnostic, ContextBundle, CurrentFileSection, CurrentSelectionSection,
        DiagnosticsSection, EditorIntelligenceSection, EditorSymbol, GitStatusSection,
    };

    #[test]
    fn detects_review_phrases() {
        let file = detect_review_request(&UserRequest::new("Review this file.")).unwrap();
        assert_eq!(file.focus, ReviewFocus::File);

        let func = detect_review_request(&UserRequest::new("Review this function.")).unwrap();
        assert_eq!(func.focus, ReviewFocus::Function);

        let changes = detect_review_request(&UserRequest::new("Review my changes.")).unwrap();
        assert_eq!(changes.focus, ReviewFocus::Changes);

        assert!(detect_review_request(&UserRequest::new("Explain this file.")).is_none());
        assert!(detect_review_request(&UserRequest::new("hello")).is_none());
    }

    #[test]
    fn scaffold_review_uses_bundle_only() {
        let bundle = ContextBundle::builder()
            .current_file(CurrentFileSection {
                path: Some("/proj/lib.rs".into()),
                dirty: false,
                language: Some("rust".into()),
            })
            .current_selection(CurrentSelectionSection {
                path: Some("/proj/lib.rs".into()),
                start_line: 10,
                start_column: 0,
                end_line: 40,
                end_column: 1,
                text: Some("fn heavy() {\n  // ...\n}\n".repeat(5)),
            })
            .editor_intelligence(EditorIntelligenceSection {
                enclosing_function: Some(EditorSymbol {
                    name: "heavy".into(),
                    ..EditorSymbol::default()
                }),
                ..EditorIntelligenceSection::default()
            })
            .diagnostics(DiagnosticsSection {
                diagnostics: vec![BundleDiagnostic {
                    severity: "warning".into(),
                    path: Some("/proj/lib.rs".into()),
                    message: "unused variable".into(),
                    line: Some(12),
                    column: Some(4),
                    source: None,
                }],
            })
            .git_status(GitStatusSection {
                is_repository: true,
                branch: Some("main".into()),
                summary: "1 modified".into(),
                modified_count: 1,
                dirty_paths: vec!["lib.rs".into()],
                ..GitStatusSection::default()
            })
            .build();

        let file = scaffold_from_bundle(ReviewFocus::File, &bundle);
        assert!(file.strengths.iter().any(|s| s.contains("lib.rs")));
        assert!(file.potential_bugs.iter().any(|b| b.contains("unused")));
        let md = file.to_markdown();
        assert!(md.contains("### Strengths"));
        assert!(md.contains("### Potential Bugs"));
        assert!(md.contains("### Architecture"));

        let function = scaffold_from_bundle(ReviewFocus::Function, &bundle);
        assert!(function.architecture.iter().any(|a| a.contains("heavy")));
        assert!(function.complexity.iter().any(|c| c.contains("lines")));

        let changes = scaffold_from_bundle(ReviewFocus::Changes, &bundle);
        assert!(changes.maintainability.iter().any(|m| m.contains("dirty_path")));
        assert!(changes
            .weaknesses
            .iter()
            .any(|w| w.contains("observational only")));
    }
}
