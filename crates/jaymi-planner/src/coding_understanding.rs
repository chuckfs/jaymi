//! Coding Understanding (Sprint C1.1) + Project Understanding (Sprint C1.2).
//!
//! Planner-owned structured understanding of coding / project context,
//! built **only** from already-assembled Workspace Intelligence / ContextBundle.
//!
//! Constitutional constraints:
//! - No new context systems (ContextEngine remains sole ContextBundle factory)
//! - No provider bypasses
//! - No filesystem scans
//! - No tool execution
//! - No edits / planning / execution
//! - Planner ownership unchanged (detect + scaffold + instruct Reasoning)

use jaymi_context::ContextBundle;
use jaymi_core::{CodingAction, UserRequest};

/// What the user is asking to understand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnderstandingFocus {
    /// Current editor selection / enclosing function.
    Selection,
    /// Active file responsibilities.
    File,
    /// Project-level orientation (Sprint C1.2 deepens this focus).
    Project,
    /// Diagnostics / compiler error.
    Diagnostic,
}

impl UnderstandingFocus {
    /// Stable id for AssembleHints / diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::File => "file",
            Self::Project => "project",
            Self::Diagnostic => "diagnostic",
        }
    }
}

/// Angle within [`UnderstandingFocus::Project`] (Sprint C1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectUnderstandingAngle {
    /// Whole-project orientation ("Explain this project.").
    Overview,
    /// Architecture / layout shape ("What architecture does this use?").
    Architecture,
    /// Where a described feature should live.
    FeaturePlacement,
    /// Which modules / packages matter most.
    ImportantModules,
}

impl ProjectUnderstandingAngle {
    /// Stable id for hints / diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Architecture => "architecture",
            Self::FeaturePlacement => "feature_placement",
            Self::ImportantModules => "important_modules",
        }
    }
}

/// Structured coding / project understanding returned to the conversation.
///
/// Fields are observational scaffolds from Workspace Intelligence. Reasoning
/// elaborates them; it must not invent tools, edits, plans, or filesystem scans.
///
/// For [`UnderstandingFocus::Project`], markdown headings map to Project
/// Understanding sections (Overview · Architecture · Important Modules · …).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CodingUnderstanding {
    /// Overview / purpose of the focus.
    pub purpose: Vec<String>,
    /// Architecture signals / responsibilities.
    pub responsibilities: Vec<String>,
    /// Important modules, packages, symbols.
    pub key_symbols: Vec<String>,
    /// Relationships (layout, git, conversation, open files, deps).
    pub relationships: Vec<String>,
    /// Activity risks, dirty state, missing intelligence, failures.
    pub potential_issues: Vec<String>,
    /// Honest next steps — understanding only (no silent edits / plans / tools).
    pub suggested_next_actions: Vec<String>,
}

impl CodingUnderstanding {
    /// True when every section is empty.
    pub fn is_empty(&self) -> bool {
        self.purpose.is_empty()
            && self.responsibilities.is_empty()
            && self.key_symbols.is_empty()
            && self.relationships.is_empty()
            && self.potential_issues.is_empty()
            && self.suggested_next_actions.is_empty()
    }

    /// Conversation-visible markdown (structured understanding).
    pub fn to_markdown(&self) -> String {
        self.to_markdown_for(UnderstandingFocus::File)
    }

    /// Markdown with focus-aware title / headings.
    pub fn to_markdown_for(&self, focus: UnderstandingFocus) -> String {
        let mut out = String::new();
        if focus == UnderstandingFocus::Project {
            out.push_str("## Project Understanding\n");
            push_section(&mut out, "Overview", &self.purpose);
            push_section(&mut out, "Architecture", &self.responsibilities);
            push_section(&mut out, "Important Modules", &self.key_symbols);
            push_section(&mut out, "Relationships", &self.relationships);
            push_section(&mut out, "Activity & Risks", &self.potential_issues);
            push_section(&mut out, "Suggested Next Actions", &self.suggested_next_actions);
        } else {
            out.push_str("## Coding Understanding\n");
            push_section(&mut out, "Purpose", &self.purpose);
            push_section(&mut out, "Responsibilities", &self.responsibilities);
            push_section(&mut out, "Key Symbols", &self.key_symbols);
            push_section(&mut out, "Relationships", &self.relationships);
            push_section(&mut out, "Potential Issues", &self.potential_issues);
            push_section(&mut out, "Suggested Next Actions", &self.suggested_next_actions);
        }
        out
    }

    /// Prompt instruction body for Reasoning (WI scaffold + fill rules).
    pub fn prompt_instruction(&self, focus: UnderstandingFocus) -> String {
        self.prompt_instruction_with_angle(focus, None, None)
    }

    /// Prompt instruction with optional project angle / feature hint (C1.2).
    pub fn prompt_instruction_with_angle(
        &self,
        focus: UnderstandingFocus,
        angle: Option<ProjectUnderstandingAngle>,
        feature_hint: Option<&str>,
    ) -> String {
        let mut lines = Vec::new();
        if focus == UnderstandingFocus::Project {
            let angle_label = angle.map(|a| a.as_str()).unwrap_or("overview");
            lines.push(format!(
                "Respond with Project Understanding (angle=`{angle_label}`)."
            ));
            lines.push(
                "Use only Workspace Intelligence already in this prompt: project intelligence (ProjectSnapshot), workspace state (WorkspaceSnapshot-derived), git status (GitSnapshot), workspace memory, conversation metadata, open files — plus any environmental resolution."
                    .into(),
            );
            lines.push(
                "Do not call tools, scan the filesystem, invent modules, modify files, execute commands, or produce an Execution Plan."
                    .into(),
            );
            lines.push(
                "Fill these markdown headings exactly: Overview · Architecture · Important Modules · Relationships · Activity & Risks · Suggested Next Actions."
                    .into(),
            );
            if let Some(feature) = feature_hint.filter(|s| !s.trim().is_empty()) {
                lines.push(format!(
                    "Feature placement question — propose honest candidate homes for: `{feature}` (from observed layout / modules only)."
                ));
            }
        } else {
            lines.push(format!(
                "Respond with Coding Understanding for focus=`{}`.",
                focus.as_str()
            ));
            lines.push(
                "Use only Workspace Intelligence already in this prompt (file, selection, editor, project, runtime, diagnostics, git, memory, environmental resolution)."
                    .into(),
            );
            lines.push(
                "Do not call tools, scan the filesystem, invent file contents, or propose applied edits."
                    .into(),
            );
            lines.push(
                "Fill these sections as markdown headings exactly: Purpose · Responsibilities · Key Symbols · Relationships · Potential Issues · Suggested Next Actions."
                    .into(),
            );
        }
        lines.push("Starter observations from Workspace Intelligence:".into());
        lines.push(self.to_markdown_for(focus));
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

/// Planner assessment: understanding mode is active for this conversational turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnderstandingAssessment {
    /// Focus of the understanding request.
    pub focus: UnderstandingFocus,
    /// Project angle when [`Self::focus`] is [`UnderstandingFocus::Project`].
    pub project_angle: Option<ProjectUnderstandingAngle>,
    /// Feature phrase extracted for placement questions (C1.2).
    pub feature_hint: Option<String>,
    /// Rule ids that matched (diagnostics).
    pub matched_rules: Vec<&'static str>,
    /// WI scaffold (filled after Context assemble).
    pub scaffold: CodingUnderstanding,
}

impl UnderstandingAssessment {
    /// Stable AssembleHints label.
    pub fn hint_id(&self) -> String {
        match (self.focus, self.project_angle) {
            (UnderstandingFocus::Project, Some(angle)) => {
                format!("understanding:project:{}", angle.as_str())
            }
            _ => format!("understanding:{}", self.focus.as_str()),
        }
    }
}

fn assessment(
    focus: UnderstandingFocus,
    rules: Vec<&'static str>,
) -> UnderstandingAssessment {
    UnderstandingAssessment {
        focus,
        project_angle: None,
        feature_hint: None,
        matched_rules: rules,
        scaffold: CodingUnderstanding::default(),
    }
}

fn project_assessment(
    angle: ProjectUnderstandingAngle,
    rules: Vec<&'static str>,
    feature_hint: Option<String>,
) -> UnderstandingAssessment {
    UnderstandingAssessment {
        focus: UnderstandingFocus::Project,
        project_angle: Some(angle),
        feature_hint,
        matched_rules: rules,
        scaffold: CodingUnderstanding::default(),
    }
}

/// Detect whether this request asks for Coding / Project Understanding (no tools / edits).
pub fn detect_understanding_request(request: &UserRequest) -> Option<UnderstandingAssessment> {
    if let Some(action) = request.coding_action {
        let (focus, rule) = match action {
            CodingAction::ExplainSelection => {
                (UnderstandingFocus::Selection, "coding_action_explain_selection")
            }
            CodingAction::ExplainFile => (UnderstandingFocus::File, "coding_action_explain_file"),
            CodingAction::EditSelection
            | CodingAction::RefactorSelection
            | CodingAction::SearchWorkspace
            | CodingAction::RunProject
            | CodingAction::OpenCodingActions => return None,
        };
        return Some(assessment(focus, vec![rule]));
    }

    let content = request.content.trim();
    if content.is_empty() {
        return None;
    }
    let lower = content.to_ascii_lowercase();
    // Coding Review owns "review …" phrasing (Sprint C1.3).
    if lower.contains("review") {
        return None;
    }

    // Diagnostic / compiler error understanding.
    if looks_like_diagnostic_question(&lower) {
        return Some(assessment(
            UnderstandingFocus::Diagnostic,
            vec!["diagnostic_question"],
        ));
    }

    // Project understanding (Sprint C1.2) — before selection so
    // "explain this project" is not misclassified as selection.
    if let Some(project) = detect_project_understanding(&lower, content) {
        return Some(project);
    }

    // Selection / function.
    if looks_like_selection_question(&lower) {
        return Some(assessment(
            UnderstandingFocus::Selection,
            vec!["selection_question"],
        ));
    }

    // File responsibilities.
    if looks_like_file_question(&lower) {
        return Some(assessment(
            UnderstandingFocus::File,
            vec!["file_question"],
        ));
    }

    None
}

fn detect_project_understanding(
    lower: &str,
    original: &str,
) -> Option<UnderstandingAssessment> {
    if looks_like_feature_placement_question(lower) {
        return Some(project_assessment(
            ProjectUnderstandingAngle::FeaturePlacement,
            vec!["project_feature_placement"],
            extract_feature_hint(original),
        ));
    }
    if looks_like_important_modules_question(lower) {
        return Some(project_assessment(
            ProjectUnderstandingAngle::ImportantModules,
            vec!["project_important_modules"],
            None,
        ));
    }
    if looks_like_architecture_question(lower) {
        return Some(project_assessment(
            ProjectUnderstandingAngle::Architecture,
            vec!["project_architecture"],
            None,
        ));
    }
    if looks_like_project_question(lower) {
        return Some(project_assessment(
            ProjectUnderstandingAngle::Overview,
            vec!["project_overview"],
            None,
        ));
    }
    None
}

fn looks_like_diagnostic_question(lower: &str) -> bool {
    (lower.contains("error")
        || lower.contains("diagnostic")
        || lower.contains("compile")
        || lower.contains("compiler"))
        && (lower.contains("why")
            || lower.contains("explain")
            || lower.contains("what")
            || lower.contains("happening")
            || lower.contains("mean"))
}

fn looks_like_project_question(lower: &str) -> bool {
    lower.contains("explain this project")
        || lower.contains("explain the project")
        || lower.contains("explain our project")
        || lower.contains("project overview")
        || lower.contains("overview of this project")
        || lower.contains("overview of the project")
        || ((lower.contains("project") || lower.contains("codebase") || lower.contains("repository"))
            && (lower.contains("how does")
                || lower.contains("how do")
                || lower.contains("what is this project")
                || lower.contains("how this project")
                || lower.contains("overview")
                || lower.contains("architecture")
                || lower.contains("explain")))
}

fn looks_like_architecture_question(lower: &str) -> bool {
    lower.contains("what architecture")
        || lower.contains("which architecture")
        || lower.contains("architecture does this use")
        || lower.contains("architecture of this")
        || lower.contains("architectural")
        || (lower.contains("architecture")
            && (lower.contains("project") || lower.contains("codebase") || lower.contains("use")))
}

fn looks_like_feature_placement_question(lower: &str) -> bool {
    lower.contains("where should this feature")
        || lower.contains("where should the feature")
        || lower.contains("where would this feature")
        || lower.contains("where does this feature belong")
        || lower.contains("where should i put")
        || lower.contains("where should we put")
        || (lower.contains("where should") && lower.contains("live"))
        || (lower.contains("where") && lower.contains("feature") && lower.contains("belong"))
}

fn looks_like_important_modules_question(lower: &str) -> bool {
    lower.contains("modules are most important")
        || lower.contains("most important modules")
        || lower.contains("what modules matter")
        || lower.contains("which modules")
        || lower.contains("important modules")
        || lower.contains("key modules")
        || lower.contains("core modules")
        || (lower.contains("modules")
            && (lower.contains("important") || lower.contains("main") || lower.contains("primary")))
}

fn extract_feature_hint(original: &str) -> Option<String> {
    let lower = original.to_ascii_lowercase();
    for marker in ["feature ", "put ", "add "] {
        if let Some(idx) = lower.find(marker) {
            let after = original[idx + marker.len()..].trim();
            let cleaned = after
                .trim_start_matches(['"', '\'', '`', ':'])
                .trim_end_matches(['?', '.', '!', '"', '\'', '`'])
                .trim();
            let cleaned = cleaned
                .trim_end_matches(" live")
                .trim_end_matches(" belong")
                .trim_end_matches(" go")
                .trim_end_matches('?')
                .trim();
            if cleaned.len() >= 2 && cleaned.len() <= 120 {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

fn looks_like_selection_question(lower: &str) -> bool {
    // Do not steal project / file explain phrasing.
    if lower.contains("project") || lower.contains("codebase") || lower.contains("repository") {
        return false;
    }
    if lower.contains("this file") || lower.contains("current file") {
        return false;
    }
    lower.contains("this function")
        || lower.contains("this method")
        || lower.contains("this selection")
        || lower.contains("selected code")
        || lower.contains("explain this")
        || lower.contains("what does this do")
        || (lower.contains("explain") && (lower.contains("function") || lower.contains("method")))
}

fn looks_like_file_question(lower: &str) -> bool {
    (lower.contains("this file") || lower.contains("current file") || lower.contains("the file"))
        && (lower.contains("responsible")
            || lower.contains("explain")
            || lower.contains("what is")
            || lower.contains("purpose")
            || lower.contains("for"))
        || lower.starts_with("explain the current file")
        || lower.starts_with("explain the file")
}

/// Fill a WI scaffold from an already-assembled ContextBundle (observation only).
pub fn scaffold_from_bundle(
    focus: UnderstandingFocus,
    bundle: &ContextBundle,
) -> CodingUnderstanding {
    scaffold_from_bundle_with_angle(focus, None, None, bundle)
}

/// Fill a WI scaffold with optional project angle / feature hint (C1.2).
pub fn scaffold_from_bundle_with_angle(
    focus: UnderstandingFocus,
    angle: Option<ProjectUnderstandingAngle>,
    feature_hint: Option<&str>,
    bundle: &ContextBundle,
) -> CodingUnderstanding {
    let mut understanding = CodingUnderstanding::default();

    let file = bundle.current_file();
    let selection = bundle.current_selection();
    let editor = bundle.editor_intelligence();
    let project = bundle.project_intelligence();
    let active_project = bundle.active_project();
    let diagnostics = bundle.diagnostics();
    let runtime = bundle.runtime_intelligence();
    let git = bundle.git_status();
    let memory = bundle.workspace_memory();
    let open = bundle.open_files();
    let conversation = bundle.conversation();
    let workspace = bundle.active_workspace();
    let inventory = bundle.workspace_inventory();

    match focus {
        UnderstandingFocus::Selection => {
            if let Some(path) = selection.path.as_ref().or(file.path.as_ref()) {
                understanding
                    .purpose
                    .push(format!("Understand the current selection in `{path}`."));
            } else {
                understanding.purpose.push(
                    "Understand the current selection (no path observed in Workspace Intelligence yet)."
                        .into(),
                );
            }
            if let Some(text) = selection.text.as_ref().filter(|t| !t.trim().is_empty()) {
                let preview: String = text.chars().take(160).collect();
                understanding
                    .responsibilities
                    .push(format!("Selection preview: {preview}"));
            }
            if let Some(func) = &editor.enclosing_function {
                understanding
                    .key_symbols
                    .push(format!("enclosing_function: {}", func.name));
            }
            if let Some(symbol) = &editor.symbol {
                understanding
                    .key_symbols
                    .push(format!("symbol: {}", symbol.name));
            }
            if let Some(ty) = &editor.enclosing_type {
                understanding
                    .key_symbols
                    .push(format!("enclosing_type: {}", ty.name));
            }
        }
        UnderstandingFocus::File => {
            if let Some(path) = &file.path {
                understanding
                    .purpose
                    .push(format!("Understand what `{path}` is responsible for."));
                if let Some(lang) = &file.language {
                    understanding
                        .responsibilities
                        .push(format!("Observed language: {lang}"));
                }
                if file.dirty {
                    understanding
                        .potential_issues
                        .push("Active file has unsaved edits.".into());
                }
            } else {
                understanding.purpose.push(
                    "Understand the active file (none observed in Workspace Intelligence yet)."
                        .into(),
                );
            }
            if let Some(func) = &editor.enclosing_function {
                understanding
                    .key_symbols
                    .push(format!("enclosing_function: {}", func.name));
            }
            if let Some(symbol) = &editor.symbol {
                understanding
                    .key_symbols
                    .push(format!("symbol: {}", symbol.name));
            }
            for summary in bundle.file_summaries().entries.iter().take(3) {
                understanding
                    .responsibilities
                    .push(format!("summary `{}`: {}", summary.path, summary.summary));
            }
        }
        UnderstandingFocus::Project => {
            fill_project_understanding(
                &mut understanding,
                angle.unwrap_or(ProjectUnderstandingAngle::Overview),
                feature_hint,
                active_project,
                project,
                workspace,
                inventory,
                git,
                memory,
                conversation,
                open,
            );
        }
        UnderstandingFocus::Diagnostic => {
            understanding
                .purpose
                .push("Understand why the current compiler / diagnostic issue is happening.".into());
            for entry in diagnostics.diagnostics.iter().take(8) {
                let path = entry.path.as_deref().unwrap_or("(workspace)");
                understanding.potential_issues.push(format!(
                    "[{}] {}: {}",
                    entry.severity, path, entry.message
                ));
            }
            if diagnostics.diagnostics.is_empty() {
                understanding.potential_issues.push(
                    "No diagnostics observed in Workspace Intelligence for this request.".into(),
                );
            }
            if let Some(path) = &file.path {
                understanding
                    .relationships
                    .push(format!("active_file: {path}"));
            }
        }
    }

    // Shared relationships / issues from WI (non-project foci; project fills its own).
    if focus != UnderstandingFocus::Project {
        for entry in open.files.iter().take(8) {
            understanding
                .relationships
                .push(format!("open_file: {}", entry.path));
        }
        for reference in editor.references.iter().take(6) {
            understanding.relationships.push(format!(
                "reference: {}:{}:{}",
                reference.path, reference.range.start_line, reference.range.start_column
            ));
        }
        if let Some(hover) = &editor.hover {
            if !hover.contents.trim().is_empty() {
                let preview: String = hover.contents.chars().take(160).collect();
                understanding
                    .key_symbols
                    .push(format!("hover: {preview}"));
            }
        }
        if let Some(branch) = &git.branch {
            understanding
                .relationships
                .push(format!("git_branch: {branch}"));
        }
        let dirty = git.modified_count + git.staged_count + git.untracked_count + git.conflict_count;
        if dirty > 0 {
            understanding.potential_issues.push(format!(
                "Working tree has {dirty} dirty path(s) (modified/staged/untracked/conflict)."
            ));
        }
        if let Some(command) = &runtime.last_command {
            understanding
                .relationships
                .push(format!("last_terminal_command: {command}"));
        }
        for running in runtime.running.iter().take(3) {
            understanding
                .relationships
                .push(format!("terminal_running: {running}"));
        }
        for failure in runtime.recent_failures.iter().take(3) {
            understanding
                .potential_issues
                .push(format!("runtime_failure: {failure}"));
        }
        if let Some(objective) = &memory.coding_objective {
            understanding
                .responsibilities
                .push(format!("coding_objective: {objective}"));
        }
        for failure in memory.recent_failures.iter().take(3) {
            understanding
                .potential_issues
                .push(format!("recent_failure: {failure}"));
        }
    }

    understanding
        .suggested_next_actions
        .extend(default_next_actions(focus, angle, feature_hint));
    understanding
}

fn fill_project_understanding(
    understanding: &mut CodingUnderstanding,
    angle: ProjectUnderstandingAngle,
    feature_hint: Option<&str>,
    active_project: &jaymi_context::ActiveProjectSection,
    project: &jaymi_context::ProjectIntelligenceSection,
    workspace: &jaymi_context::ActiveWorkspaceSection,
    inventory: &jaymi_context::WorkspaceInventorySection,
    git: &jaymi_context::GitStatusSection,
    memory: &jaymi_context::WorkspaceMemorySection,
    conversation: &jaymi_context::ConversationSection,
    open: &jaymi_context::OpenFilesSection,
) {
    // --- Overview (purpose) ---
    match angle {
        ProjectUnderstandingAngle::Overview => {
            if let Some(name) = &active_project.name {
                understanding
                    .purpose
                    .push(format!("Explain project `{name}` from Workspace Intelligence."));
            } else {
                understanding
                    .purpose
                    .push("Explain the open project from Workspace Intelligence.".into());
            }
        }
        ProjectUnderstandingAngle::Architecture => {
            understanding.purpose.push(
                "Describe the observed architecture / layout shape (no invented structure).".into(),
            );
        }
        ProjectUnderstandingAngle::FeaturePlacement => {
            if let Some(feature) = feature_hint.filter(|s| !s.is_empty()) {
                understanding.purpose.push(format!(
                    "Suggest honest candidate homes for feature `{feature}` from observed modules / layout."
                ));
            } else {
                understanding.purpose.push(
                    "Suggest where a new feature might live from observed modules / layout.".into(),
                );
            }
        }
        ProjectUnderstandingAngle::ImportantModules => {
            understanding.purpose.push(
                "Identify the most important observed modules / packages / top-level dirs.".into(),
            );
        }
    }
    if let Some(root) = &active_project.root_directory {
        understanding
            .purpose
            .push(format!("project_root: `{root}`"));
    }
    if let Some(id) = &active_project.project_id {
        understanding.purpose.push(format!("project_id: {id}"));
    }
    if let Some(kind) = &workspace.kind_id {
        understanding
            .purpose
            .push(format!("workspace_kind: {kind} (WorkspaceSnapshot-derived)"));
    }

    // --- Architecture (responsibilities) ---
    if let Some(shape) = &project.layout_shape {
        understanding
            .responsibilities
            .push(format!("layout_shape: {shape}"));
    }
    for lang in project.languages.iter().take(8) {
        understanding
            .responsibilities
            .push(format!("language: {lang}"));
    }
    for framework in project.frameworks.iter().take(8) {
        understanding
            .responsibilities
            .push(format!("framework: {framework}"));
    }
    if let Some(pm) = &project.package_manager {
        understanding
            .responsibilities
            .push(format!("package_manager: {pm}"));
    }
    if let Some(build) = &project.build_system {
        understanding
            .responsibilities
            .push(format!("build_system: {build}"));
    }
    if let Some(cargo) = &project.cargo_package {
        understanding
            .responsibilities
            .push(format!("cargo_package: {cargo}"));
    }
    if let Some(npm) = &project.npm_package {
        understanding
            .responsibilities
            .push(format!("npm_package: {npm}"));
    }
    if !project.dependency_summary.workspace_members.is_empty() {
        understanding.responsibilities.push(format!(
            "workspace_members: {}",
            project.dependency_summary.workspace_members.join(", ")
        ));
    }
    if project.dependency_summary.direct_count > 0 {
        understanding.responsibilities.push(format!(
            "direct_dependencies_observed: {}",
            project.dependency_summary.direct_count
        ));
    }
    if let Some(lock) = project.dependency_summary.lockfile_count {
        understanding
            .responsibilities
            .push(format!("lockfile_entries_observed: {lock}"));
    }

    // --- Important Modules (key_symbols) ---
    for dir in project.top_level_dirs.iter().take(12) {
        understanding
            .key_symbols
            .push(format!("top_level_dir: {dir}"));
    }
    for member in project.dependency_summary.workspace_members.iter().take(8) {
        understanding
            .key_symbols
            .push(format!("workspace_member: {member}"));
    }
    for dep in project.dependency_summary.top_level.iter().take(10) {
        understanding
            .key_symbols
            .push(format!("dependency: {dep}"));
    }
    for path in memory.recently_opened.iter().take(6) {
        understanding
            .key_symbols
            .push(format!("recently_opened: {path}"));
    }
    for path in memory.recent_edits.iter().take(6) {
        understanding
            .key_symbols
            .push(format!("recent_edit: {path}"));
    }
    if matches!(
        angle,
        ProjectUnderstandingAngle::FeaturePlacement | ProjectUnderstandingAngle::ImportantModules
    ) {
        for path in inventory.sample_paths.iter().take(8) {
            understanding
                .key_symbols
                .push(format!("inventory_sample: {path}"));
        }
    }

    // --- Relationships ---
    if let Some(branch) = project
        .repository_branch
        .as_ref()
        .or(git.branch.as_ref())
    {
        understanding
            .relationships
            .push(format!("git_branch: {branch}"));
    }
    if git.is_repository {
        if !git.summary.is_empty() {
            understanding
                .relationships
                .push(format!("git_summary: {}", git.summary));
        }
        for commit in git.recent_commits.iter().take(4) {
            let short = if commit.short_sha.is_empty() {
                commit.sha.as_str()
            } else {
                commit.short_sha.as_str()
            };
            understanding.relationships.push(format!(
                "recent_commit: {short} — {}",
                commit.subject
            ));
        }
    }
    for entry in open.files.iter().take(8) {
        understanding
            .relationships
            .push(format!("open_file: {}", entry.path));
    }
    if let Some(title) = &conversation.title {
        understanding
            .relationships
            .push(format!("conversation_title: {title}"));
    }
    if let Some(count) = conversation.message_count {
        understanding
            .relationships
            .push(format!("conversation_messages: {count}"));
    }
    if let Some(cid) = &conversation.id {
        understanding
            .relationships
            .push(format!("conversation_id: {cid}"));
    }
    if let Some(pid) = &conversation.project_id {
        understanding
            .relationships
            .push(format!("conversation_project_id: {pid}"));
    }
    if let Some(root) = &inventory.root {
        understanding
            .relationships
            .push(format!("inventory_root: {root}"));
    }
    if inventory.file_count > 0 || inventory.directory_count > 0 {
        understanding.relationships.push(format!(
            "inventory: {} files / {} dirs (status={})",
            inventory.file_count, inventory.directory_count, inventory.status
        ));
    }
    if let Some(objective) = &memory.coding_objective {
        understanding
            .relationships
            .push(format!("coding_objective: {objective}"));
    }
    for build in memory.recent_builds.iter().take(3) {
        understanding
            .relationships
            .push(format!("recent_build: {build}"));
    }

    // --- Activity & Risks ---
    let dirty = git.modified_count + git.staged_count + git.untracked_count + git.conflict_count;
    if dirty > 0 {
        understanding.potential_issues.push(format!(
            "Working tree has {dirty} dirty path(s) (modified={}, staged={}, untracked={}, conflict={}).",
            git.modified_count, git.staged_count, git.untracked_count, git.conflict_count
        ));
    }
    for path in git.dirty_paths.iter().take(5) {
        understanding
            .potential_issues
            .push(format!("dirty_path: {path}"));
    }
    for path in git.conflict_paths.iter().take(3) {
        understanding
            .potential_issues
            .push(format!("conflict_path: {path}"));
    }
    for failure in memory.recent_failures.iter().take(4) {
        understanding
            .potential_issues
            .push(format!("recent_failure: {failure}"));
    }
    if active_project.root_directory.is_none() && active_project.name.is_none() {
        understanding.potential_issues.push(
            "No active project identity observed — open a project for richer understanding.".into(),
        );
    }
    if project.languages.is_empty()
        && project.top_level_dirs.is_empty()
        && project.layout_shape.is_none()
    {
        understanding.potential_issues.push(
            "ProjectSnapshot intelligence is thin or missing in this ContextBundle.".into(),
        );
    }
    if matches!(angle, ProjectUnderstandingAngle::FeaturePlacement) {
        if let Some(feature) = feature_hint.filter(|s| !s.is_empty()) {
            for dir in project.top_level_dirs.iter().take(6) {
                understanding.suggested_next_actions.push(format!(
                    "Candidate home for `{feature}`: top-level `{dir}/` (observed layout only — confirm before editing)."
                ));
            }
            for member in project.dependency_summary.workspace_members.iter().take(4) {
                understanding.suggested_next_actions.push(format!(
                    "Candidate home for `{feature}`: workspace member `{member}` (confirm ownership first)."
                ));
            }
            if project.top_level_dirs.is_empty()
                && project.dependency_summary.workspace_members.is_empty()
            {
                understanding.suggested_next_actions.push(format!(
                    "Not enough layout intelligence yet to place `{feature}` — ask which subsystem owns it."
                ));
            }
        }
    }
}

fn default_next_actions(
    focus: UnderstandingFocus,
    angle: Option<ProjectUnderstandingAngle>,
    feature_hint: Option<&str>,
) -> Vec<String> {
    match focus {
        UnderstandingFocus::Selection => vec![
            "Ask a follow-up about a specific line or symbol.".into(),
            "Use Edit only after you confirm the intended change.".into(),
        ],
        UnderstandingFocus::File => vec![
            "Ask which responsibility to change before editing.".into(),
            "Open a related file if Relationships suggest a missing dependency.".into(),
        ],
        UnderstandingFocus::Project => match angle.unwrap_or(ProjectUnderstandingAngle::Overview) {
            ProjectUnderstandingAngle::Overview => vec![
                "Ask about architecture, important modules, or where a feature should live.".into(),
                "Stay in understanding mode — no tools, plans, or edits yet.".into(),
            ],
            ProjectUnderstandingAngle::Architecture => vec![
                "Ask which layer owns a concrete concern before proposing changes.".into(),
                "Do not produce an Execution Plan from architecture orientation alone.".into(),
            ],
            ProjectUnderstandingAngle::FeaturePlacement => {
                let mut actions = vec![
                    "Confirm the candidate module with the user before any edits.".into(),
                    "Understanding only — no file creation or planning yet.".into(),
                ];
                if feature_hint.is_none() {
                    actions.insert(
                        0,
                        "Name the feature (e.g. “auth refresh”) so placement can use layout hints."
                            .into(),
                    );
                }
                actions
            }
            ProjectUnderstandingAngle::ImportantModules => vec![
                "Ask for a walkthrough of one module before editing it.".into(),
                "Use Search later only after the module boundary is agreed.".into(),
            ],
        },
        UnderstandingFocus::Diagnostic => vec![
            "Confirm the failing command or file before proposing a fix.".into(),
            "Do not apply edits until the root cause is agreed.".into(),
        ],
    }
}

/// Attach understanding assessment after Context assemble.
pub fn finalize_assessment(
    mut assessment: UnderstandingAssessment,
    bundle: &ContextBundle,
) -> UnderstandingAssessment {
    assessment.scaffold = scaffold_from_bundle_with_angle(
        assessment.focus,
        assessment.project_angle,
        assessment.feature_hint.as_deref(),
        bundle,
    );
    assessment
}

/// Extension key on [`jaymi_context::LlmContext`] for the prompt section.
pub const LLM_EXTENSION_KEY: &str = "coding_understanding";

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_context::{
        ActiveProjectSection, ActiveWorkspaceSection, BundleDiagnostic, ContextBundle,
        ConversationSection, CurrentFileSection, CurrentSelectionSection, DiagnosticsSection,
        GitStatusSection, ProjectIntelligenceSection, WorkspaceInventorySection,
        WorkspaceMemorySection,
    };

    #[test]
    fn detects_explain_coding_actions() {
        let request = UserRequest::coding_action(CodingAction::ExplainFile);
        let assessment = detect_understanding_request(&request).expect("file");
        assert_eq!(assessment.focus, UnderstandingFocus::File);

        let request = UserRequest::coding_action(CodingAction::ExplainSelection);
        let assessment = detect_understanding_request(&request).expect("selection");
        assert_eq!(assessment.focus, UnderstandingFocus::Selection);

        assert!(detect_understanding_request(&UserRequest::coding_action(
            CodingAction::EditSelection
        ))
        .is_none());
    }

    #[test]
    fn detects_free_text_understanding() {
        assert_eq!(
            detect_understanding_request(&UserRequest::new("Explain this function."))
                .unwrap()
                .focus,
            UnderstandingFocus::Selection
        );
        assert_eq!(
            detect_understanding_request(&UserRequest::new(
                "What is this file responsible for?"
            ))
            .unwrap()
            .focus,
            UnderstandingFocus::File
        );
        assert_eq!(
            detect_understanding_request(&UserRequest::new("How does this project work?"))
                .unwrap()
                .focus,
            UnderstandingFocus::Project
        );
        assert_eq!(
            detect_understanding_request(&UserRequest::new(
                "Why is this compiler error happening?"
            ))
            .unwrap()
            .focus,
            UnderstandingFocus::Diagnostic
        );
        assert!(detect_understanding_request(&UserRequest::new("hello")).is_none());
    }

    #[test]
    fn detects_project_understanding_angles() {
        let overview =
            detect_understanding_request(&UserRequest::new("Explain this project.")).unwrap();
        assert_eq!(overview.focus, UnderstandingFocus::Project);
        assert_eq!(
            overview.project_angle,
            Some(ProjectUnderstandingAngle::Overview)
        );

        let architecture =
            detect_understanding_request(&UserRequest::new("What architecture does this use?"))
                .unwrap();
        assert_eq!(
            architecture.project_angle,
            Some(ProjectUnderstandingAngle::Architecture)
        );

        let modules = detect_understanding_request(&UserRequest::new(
            "What modules are most important?",
        ))
        .unwrap();
        assert_eq!(
            modules.project_angle,
            Some(ProjectUnderstandingAngle::ImportantModules)
        );

        let placement = detect_understanding_request(&UserRequest::new(
            "Where should this feature live: auth refresh?",
        ))
        .unwrap();
        assert_eq!(
            placement.project_angle,
            Some(ProjectUnderstandingAngle::FeaturePlacement)
        );
        assert!(
            placement
                .feature_hint
                .as_deref()
                .is_some_and(|h| h.to_ascii_lowercase().contains("auth")),
            "{:?}",
            placement.feature_hint
        );
    }

    #[test]
    fn scaffold_uses_bundle_only() {
        let bundle = ContextBundle::builder()
            .current_file(CurrentFileSection {
                path: Some("/proj/main.rs".into()),
                dirty: true,
                language: Some("rust".into()),
            })
            .current_selection(CurrentSelectionSection {
                path: Some("/proj/main.rs".into()),
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 10,
                text: Some("fn hello()".into()),
            })
            .active_project(ActiveProjectSection {
                project_id: Some("p1".into()),
                name: Some("Demo".into()),
                root_directory: Some("/proj".into()),
                detail: None,
            })
            .diagnostics(DiagnosticsSection {
                diagnostics: vec![BundleDiagnostic {
                    severity: "error".into(),
                    path: Some("/proj/main.rs".into()),
                    message: "cannot find value `x`".into(),
                    line: Some(1),
                    column: Some(0),
                    source: None,
                }],
            })
            .build();

        let file = scaffold_from_bundle(UnderstandingFocus::File, &bundle);
        assert!(file.purpose.iter().any(|p| p.contains("main.rs")));
        assert!(file.potential_issues.iter().any(|p| p.contains("unsaved")));
        assert!(!file.suggested_next_actions.is_empty());

        let selection = scaffold_from_bundle(UnderstandingFocus::Selection, &bundle);
        assert!(selection
            .responsibilities
            .iter()
            .any(|r| r.contains("fn hello")));

        let diagnostic = scaffold_from_bundle(UnderstandingFocus::Diagnostic, &bundle);
        assert!(diagnostic
            .potential_issues
            .iter()
            .any(|p| p.contains("cannot find value")));

        let markdown = file.to_markdown_for(UnderstandingFocus::File);
        assert!(markdown.contains("### Purpose"));
        assert!(markdown.contains("### Suggested Next Actions"));
    }

    #[test]
    fn project_scaffold_uses_wi_snapshots() {
        let bundle = ContextBundle::builder()
            .active_project(ActiveProjectSection {
                project_id: Some("jaymi".into()),
                name: Some("Jaymi".into()),
                root_directory: Some("/Users/charlie/jaymi".into()),
                detail: None,
            })
            .active_workspace(ActiveWorkspaceSection {
                kind_id: Some("coding".into()),
            })
            .project_intelligence(ProjectIntelligenceSection {
                languages: vec!["rust".into()],
                frameworks: vec!["egui".into()],
                package_manager: Some("cargo".into()),
                build_system: Some("cargo".into()),
                dependency_summary: jaymi_context::DependencyGraphSummary {
                    top_level: vec!["serde".into(), "tokio".into()],
                    direct_count: 2,
                    lockfile_count: Some(40),
                    workspace_members: vec!["jaymi".into(), "jaymi-planner".into()],
                },
                cargo_package: Some("jaymi".into()),
                npm_package: None,
                repository_branch: Some("main".into()),
                layout_shape: Some("cargo-workspace".into()),
                top_level_dirs: vec!["apps".into(), "crates".into(), "docs".into()],
            })
            .git_status(GitStatusSection {
                is_repository: true,
                branch: Some("main".into()),
                summary: "2 modified".into(),
                modified_count: 2,
                staged_count: 0,
                untracked_count: 0,
                conflict_count: 0,
                dirty_paths: vec!["ROADMAP.md".into()],
                ..GitStatusSection::default()
            })
            .workspace_memory(WorkspaceMemorySection {
                coding_objective: Some("Ship project understanding".into()),
                recent_edits: vec!["crates/jaymi-planner/src/coding_understanding.rs".into()],
                recently_opened: vec!["docs/coding-understanding.md".into()],
                recent_builds: vec!["cargo test -p jaymi-planner".into()],
                recent_failures: vec![],
            })
            .conversation(ConversationSection {
                id: Some("c1".into()),
                title: Some("Pair programming".into()),
                status: Some("active".into()),
                project_id: Some("jaymi".into()),
                message_count: Some(4),
            })
            .workspace_inventory(WorkspaceInventorySection {
                root: Some("/Users/charlie/jaymi".into()),
                file_count: 100,
                directory_count: 20,
                status: "ready".into(),
                sample_paths: vec!["crates/jaymi-planner".into()],
            })
            .build();

        let overview = scaffold_from_bundle_with_angle(
            UnderstandingFocus::Project,
            Some(ProjectUnderstandingAngle::Overview),
            None,
            &bundle,
        );
        assert!(overview.purpose.iter().any(|p| p.contains("Jaymi")));
        assert!(overview
            .responsibilities
            .iter()
            .any(|r| r.contains("cargo-workspace")));
        assert!(overview.key_symbols.iter().any(|k| k.contains("crates")));
        assert!(overview
            .relationships
            .iter()
            .any(|r| r.contains("conversation_title")));
        assert!(overview.potential_issues.iter().any(|p| p.contains("dirty")));
        let md = overview.to_markdown_for(UnderstandingFocus::Project);
        assert!(md.contains("## Project Understanding"));
        assert!(md.contains("### Overview"));
        assert!(md.contains("### Architecture"));
        assert!(md.contains("### Important Modules"));

        let placement = scaffold_from_bundle_with_angle(
            UnderstandingFocus::Project,
            Some(ProjectUnderstandingAngle::FeaturePlacement),
            Some("auth refresh"),
            &bundle,
        );
        assert!(placement
            .suggested_next_actions
            .iter()
            .any(|a| a.contains("auth refresh") && a.contains("Candidate home")));
    }
}
