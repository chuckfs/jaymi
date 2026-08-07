//! Jaymi default context policy — deterministic inclusion + Context Selection.

use std::sync::Arc;

use crate::budget::ProviderPriority;
use crate::candidate::{score_candidate, CandidateItemDecision, ContextCandidate};
use crate::relevance::{IntentTag, RequestKind};

use super::decision::{ContextPolicyCandidate, ContextPolicyDecision, ContextPolicyInputs};
use super::selection::{assess_context_selection, ContextSelectionAssessment, ContextSelectionProfile};
use super::sensitivity::Sensitivity;
use super::ContextPolicy;

/// Stable id for the default Jaymi context policy.
pub const DEFAULT_CONTEXT_POLICY_ID: &str = "jaymi_default_context";

/// Default deterministic context policy implementing Sprint A9 + B2.8 selection.
pub struct JaymiDefaultContextPolicy;

impl ContextPolicy for JaymiDefaultContextPolicy {
    fn id(&self) -> &'static str {
        DEFAULT_CONTEXT_POLICY_ID
    }

    fn evaluate(&self, candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
        if candidate.sensitivity > candidate.inputs.max_sensitivity
            && !sensitivity_required(candidate)
        {
            return ContextPolicyDecision::deny(format!(
                "Provider sensitivity '{}' exceeds request maximum '{}'",
                candidate.sensitivity.as_str(),
                candidate.inputs.max_sensitivity.as_str()
            ));
        }

        let selection =
            assess_context_selection(candidate.inputs.request, candidate.inputs.signals);
        if !provider_allowed_by_selection(candidate.provider_id, &selection, candidate.inputs) {
            return ContextPolicyDecision::deny(format!(
                "Context selection profile '{}' omits provider '{}'",
                selection.profile.as_str(),
                candidate.provider_id
            ));
        }

        let mut decision = match candidate.provider_id {
            "conversation" => evaluate_conversation(candidate),
            "project" => evaluate_project(candidate),
            "workspace" => evaluate_workspace(candidate),
            "editor" => evaluate_editor(candidate, &selection),
            "search" => evaluate_search(candidate),
            "memory" => evaluate_memory(candidate),
            "diagnostics" => evaluate_diagnostics(candidate, &selection),
            "permission" => evaluate_permission(candidate),
            "runtime" => evaluate_runtime(candidate, &selection),
            "workspace_memory" => evaluate_workspace_memory(candidate, &selection),
            "git_status" => evaluate_git_status(candidate, &selection),
            "workspace_inventory" => evaluate_workspace_inventory(candidate, &selection),
            "file_summaries" => evaluate_file_summaries(candidate, &selection),
            other => ContextPolicyDecision::allow(
                format!(
                    "No specific rule for '{other}'; retained under selection '{}'",
                    selection.profile.as_str()
                ),
                candidate.provider_priority,
            ),
        };

        if decision.participate {
            decision.reason = format!(
                "{} · selection={} [{}]",
                decision.reason,
                selection.profile.as_str(),
                selection.matched_rules.join(",")
            );
        }

        if decision.participate && candidate.sensitivity >= Sensitivity::Sensitive {
            decision.requires_user_approval = true;
            if !decision.reason.contains("user approval") {
                decision.reason.push_str("; sensitive content requires user approval");
            }
        }

        decision
    }

    fn evaluate_candidate_item(
        &self,
        candidate: &ContextCandidate,
        provider_relevance: u8,
        now_unix: i64,
        inputs: &ContextPolicyInputs<'_>,
    ) -> CandidateItemDecision {
        let selection = assess_context_selection(inputs.request, inputs.signals);
        let mut scores = score_candidate(
            candidate,
            provider_relevance,
            now_unix,
            inputs.max_sensitivity,
        );
        if !scores.privacy_ok {
            return CandidateItemDecision::deny(
                format!(
                    "Candidate privacy '{}' exceeds request maximum '{}'",
                    candidate.sensitivity.as_str(),
                    inputs.max_sensitivity.as_str()
                ),
                scores,
            );
        }
        if !provider_allowed_by_selection(candidate.provider_id, &selection, inputs) {
            return CandidateItemDecision::deny(
                format!(
                    "Selection '{}' omits provider '{}'",
                    selection.profile.as_str(),
                    candidate.provider_id
                ),
                scores,
            );
        }
        if selection.profile.omits_kind(candidate.kind) && !candidate.required {
            return CandidateItemDecision::deny(
                format!(
                    "Selection '{}' omits candidate kind '{}'",
                    selection.profile.as_str(),
                    candidate.kind.as_str()
                ),
                scores,
            );
        }
        if selection.profile.prefers_kind(candidate.kind) {
            let relevance = scores.relevance.saturating_add(10).min(100);
            let importance = scores.importance.saturating_add(15).min(100);
            scores = crate::candidate::CandidateScores::combine(
                relevance,
                scores.recency,
                importance,
                scores.privacy_ok,
            );
        }
        if !candidate.required && scores.relevance < 20 && scores.importance < 40 {
            return CandidateItemDecision::deny(
                "Candidate relevance/importance below inclusion floor",
                scores,
            );
        }
        CandidateItemDecision::allow(
            format!(
                "Selected · profile={} · relevance={} recency={} importance={}",
                selection.profile.as_str(),
                scores.relevance,
                scores.recency,
                scores.importance
            ),
            scores,
        )
    }
}

/// Default policy set registered by the Context Policy Engine.
pub fn default_context_policies() -> Vec<Arc<dyn ContextPolicy>> {
    vec![Arc::new(JaymiDefaultContextPolicy)]
}

/// Selection allowlist with one Planner override: capability ids ride on the
/// `workspace` provider, so that provider stays allowed when hints carry caps.
fn provider_allowed_by_selection(
    provider_id: &str,
    selection: &ContextSelectionAssessment,
    inputs: &ContextPolicyInputs<'_>,
) -> bool {
    if provider_id == "workspace" && !inputs.signals.active_capabilities.is_empty() {
        return true;
    }
    selection.profile.allows_provider(provider_id)
}

fn evaluate_conversation(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    let mut decision = ContextPolicyDecision::allow(
        "Recent user interaction",
        ProviderPriority::CONVERSATION,
    );
    decision.bypass_relevance = true;
    decision.can_truncate = true;
    let _ = candidate;
    decision
}

fn evaluate_project(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    if candidate.inputs.request.close_project
        || matches!(
            candidate.inputs.signals.intent,
            jaymi_core::IntentId::CloseProject
        )
    {
        return ContextPolicyDecision::deny("Close-project request; project context omitted");
    }
    if !candidate.inputs.project_open {
        return ContextPolicyDecision::deny("No project is open");
    }
    let mut decision =
        ContextPolicyDecision::allow("Active project is open", ProviderPriority::PROJECT);
    decision.bypass_relevance = true;
    decision
}

fn evaluate_workspace(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    if candidate.inputs.session.workspace_kind.is_none()
        && candidate.inputs.signals.active_capabilities.is_empty()
    {
        return ContextPolicyDecision::deny("No active workspace");
    }
    let mut decision = ContextPolicyDecision::allow(
        "Active workspace only (inactive workspaces never included)",
        ProviderPriority::CRITICAL,
    );
    decision.bypass_relevance = true;
    decision
}

fn evaluate_editor(
    candidate: &ContextPolicyCandidate<'_>,
    selection: &ContextSelectionAssessment,
) -> ContextPolicyDecision {
    let session = candidate.inputs.session;
    let has_focus = session.current_file.path.is_some()
        || session.current_selection.path.is_some()
        || session
            .editor_snapshot
            .as_ref()
            .is_some_and(|snap| snap.has_editor_state());
    let coding_profile = matches!(
        selection.profile,
        ContextSelectionProfile::DebugCompile
            | ContextSelectionProfile::CodingGeneral
            | ContextSelectionProfile::FileEdit
    );
    if !has_focus && !coding_profile {
        return ContextPolicyDecision::deny(
            "No current file or selection (open editors are not included by default)",
        );
    }
    let mut decision =
        ContextPolicyDecision::allow("Current file and selection", ProviderPriority::EDITOR);
    // Open editors stay omitted by default (privacy / budget); only current file + selection.
    decision.constraints.exclude_open_files = true;
    let has_selection_text = session
        .current_selection
        .text
        .as_ref()
        .map(|text| !text.trim().is_empty())
        .unwrap_or(false);
    if has_selection_text {
        decision.requires_user_approval = true;
        decision.reason.push_str("; selection text requires user approval");
    }
    if matches!(selection.profile, ContextSelectionProfile::DebugCompile) {
        decision.bypass_relevance = true;
    }
    decision
}

fn evaluate_search(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    let signals = candidate.inputs.signals;
    let needs_retrieval = matches!(
        signals.intent,
        jaymi_core::IntentId::SearchKnowledge
            | jaymi_core::IntentId::SearchProjectKnowledge
            | jaymi_core::IntentId::DiscoverInventory
            | jaymi_core::IntentId::ListDirectory
            | jaymi_core::IntentId::ListProjectTree
            | jaymi_core::IntentId::ReadFile
            | jaymi_core::IntentId::IndexRoots
    ) || matches!(
        signals.request_kind,
        RequestKind::Search | RequestKind::Discover | RequestKind::FileRead | RequestKind::Index
    ) || signals.has_intent(IntentTag::Search)
        || signals.has_intent(IntentTag::Discover)
        || signals.has_intent(IntentTag::Read)
        || !candidate.inputs.session.search_hits.is_empty();

    if !needs_retrieval {
        return ContextPolicyDecision::deny("Request does not require retrieval");
    }
    ContextPolicyDecision::allow(
        "Intent requires retrieval / search context",
        ProviderPriority::SEARCH,
    )
}

fn evaluate_memory(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    let signals = candidate.inputs.signals;
    let useful = matches!(
        signals.request_kind,
        RequestKind::Chat
            | RequestKind::ProjectSession
            | RequestKind::Search
            | RequestKind::FileRead
            | RequestKind::FileWrite
            | RequestKind::Lsp
            | RequestKind::Git
            | RequestKind::Terminal
    ) || signals.has_intent(IntentTag::Chat)
        || signals.has_intent(IntentTag::Code)
        || signals.has_intent(IntentTag::Project)
        || signals.coding_workspace()
        || matches!(
            signals.complexity_id(),
            Some("greeting") | Some("small_talk")
        );

    if !useful {
        return ContextPolicyDecision::deny("Request does not benefit from memory matching");
    }
    let mut decision = ContextPolicyDecision::allow(
        "Memories matching the current request only (provider filters relevance)",
        ProviderPriority::MEMORY,
    );
    if candidate.sensitivity >= Sensitivity::Private {
        decision.exclude_sensitive = true;
        decision.constraints.redact_memory_content = true;
    }
    if matches!(
        signals.complexity_id(),
        Some("greeting") | Some("small_talk")
    ) {
        decision.bypass_relevance = true;
    }
    decision
}

fn evaluate_diagnostics(
    candidate: &ContextPolicyCandidate<'_>,
    selection: &ContextSelectionAssessment,
) -> ContextPolicyDecision {
    let signals = candidate.inputs.signals;
    let session = candidate.inputs.session;
    let coding_capability = signals.active_capabilities.iter().any(|id| {
        id == "code" || id == "lsp" || id == "debug" || id == "debugging"
    }) || signals.coding_workspace();
    let debug_intent = signals.has_intent(IntentTag::Lsp)
        || signals.has_intent(IntentTag::Code)
        || matches!(signals.request_kind, RequestKind::Lsp)
        || matches!(
            selection.profile,
            ContextSelectionProfile::DebugCompile | ContextSelectionProfile::CodingGeneral
        );
    let has_diags = !session.diagnostics.diagnostics.is_empty()
        || session
            .editor_snapshot
            .as_ref()
            .is_some_and(|snap| !snap.diagnostics.is_empty());

    if !(coding_capability || debug_intent) {
        return ContextPolicyDecision::deny(
            "Diagnostics require coding capability or debug intent",
        );
    }
    if !has_diags && !debug_intent {
        return ContextPolicyDecision::deny("No diagnostics attached for this request");
    }
    let mut decision = ContextPolicyDecision::allow(
        if matches!(selection.profile, ContextSelectionProfile::DebugCompile) {
            "Debug/compile selection"
        } else if debug_intent {
            "Debug intent detected"
        } else {
            "Coding capability active"
        },
        ProviderPriority::DIAGNOSTICS,
    );
    if matches!(selection.profile, ContextSelectionProfile::DebugCompile) {
        decision.bypass_relevance = true;
    }
    decision
}

fn evaluate_runtime(
    _candidate: &ContextPolicyCandidate<'_>,
    selection: &ContextSelectionAssessment,
) -> ContextPolicyDecision {
    let mut decision = ContextPolicyDecision::allow(
        "Terminal / runtime intelligence for selection profile",
        ProviderPriority::RUNTIME,
    );
    if matches!(
        selection.profile,
        ContextSelectionProfile::DebugCompile | ContextSelectionProfile::Terminal
    ) {
        decision.bypass_relevance = true;
    }
    decision
}

fn evaluate_workspace_memory(
    _candidate: &ContextPolicyCandidate<'_>,
    selection: &ContextSelectionAssessment,
) -> ContextPolicyDecision {
    let mut decision = ContextPolicyDecision::allow(
        "Workspace activity memory (distinct from Conversation Memory)",
        ProviderPriority::WORKSPACE_MEMORY,
    );
    if matches!(
        selection.profile,
        ContextSelectionProfile::DebugCompile
            | ContextSelectionProfile::CodingGeneral
            | ContextSelectionProfile::Terminal
            | ContextSelectionProfile::FileEdit
    ) {
        decision.bypass_relevance = true;
    }
    decision
}

fn evaluate_git_status(
    _candidate: &ContextPolicyCandidate<'_>,
    selection: &ContextSelectionAssessment,
) -> ContextPolicyDecision {
    let mut decision = ContextPolicyDecision::allow(
        "Git status for selection profile",
        ProviderPriority::GIT_STATUS,
    );
    if matches!(
        selection.profile,
        ContextSelectionProfile::ProjectOverview | ContextSelectionProfile::Git
    ) {
        decision.bypass_relevance = true;
    }
    decision
}

fn evaluate_workspace_inventory(
    _candidate: &ContextPolicyCandidate<'_>,
    selection: &ContextSelectionAssessment,
) -> ContextPolicyDecision {
    let mut decision = ContextPolicyDecision::allow(
        "Filesystem / inventory for selection profile",
        ProviderPriority::WORKSPACE_INVENTORY,
    );
    if matches!(
        selection.profile,
        ContextSelectionProfile::ProjectOverview | ContextSelectionProfile::Search
    ) {
        decision.bypass_relevance = true;
    }
    decision
}

fn evaluate_file_summaries(
    _candidate: &ContextPolicyCandidate<'_>,
    selection: &ContextSelectionAssessment,
) -> ContextPolicyDecision {
    let mut decision = ContextPolicyDecision::allow(
        "File summaries for selection profile",
        ProviderPriority::FILE_SUMMARIES,
    );
    if matches!(
        selection.profile,
        ContextSelectionProfile::ProjectOverview | ContextSelectionProfile::FileEdit
    ) {
        decision.bypass_relevance = true;
    }
    decision
}

fn evaluate_permission(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    let mut decision = ContextPolicyDecision::allow(
        "Permission summary always included",
        ProviderPriority::PERMISSION,
    );
    decision.bypass_relevance = true;
    decision.constraints.permission_summary_only = true;
    let _ = candidate;
    decision
}

fn sensitivity_required(candidate: &ContextPolicyCandidate<'_>) -> bool {
    match candidate.provider_id {
        "conversation" | "permission" => true,
        "editor" => {
            candidate.inputs.session.current_file.path.is_some()
                || candidate.inputs.session.current_selection.path.is_some()
        }
        "memory" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::BudgetEstimate;
    use crate::policy::{ContextPolicyInputs, Sensitivity};
    use crate::relevance::{RelevanceScore, RelevanceSignals, RequestKind};
    use crate::AssembleHints;
    use crate::ContextSessionInputs;
    use jaymi_core::{IntentId, UserRequest};

    fn candidate<'a>(
        id: &'static str,
        inputs: &'a ContextPolicyInputs<'a>,
        sensitivity: Sensitivity,
    ) -> ContextPolicyCandidate<'a> {
        ContextPolicyCandidate {
            provider_id: id,
            provider_priority: ProviderPriority::MEMORY,
            relevance: RelevanceScore::HIGH,
            sensitivity,
            estimate: BudgetEstimate::flexible(crate::budget::BudgetUnits::default()),
            inputs,
        }
    }

    #[test]
    fn conversation_always_included() {
        let request = UserRequest::new("hello");
        let session = ContextSessionInputs::default();
        let signals = RelevanceSignals::derive(&request, &session);
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: false,
            max_sensitivity: Sensitivity::Sensitive,
        };
        let decision = JaymiDefaultContextPolicy.evaluate(&candidate(
            "conversation",
            &inputs,
            Sensitivity::Private,
        ));
        assert!(decision.participate);
        assert!(decision.bypass_relevance);
        assert!(decision.reason.contains("Recent user interaction"));
        assert!(decision.reason.contains("selection=greeting"));
    }

    #[test]
    fn hello_includes_memory_omits_diagnostics() {
        let request = UserRequest::new("hello");
        let session = ContextSessionInputs::default();
        let hints = AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity("greeting");
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: false,
            max_sensitivity: Sensitivity::Sensitive,
        };
        assert!(JaymiDefaultContextPolicy
            .evaluate(&candidate("memory", &inputs, Sensitivity::Private))
            .participate);
        assert!(!JaymiDefaultContextPolicy
            .evaluate(&candidate("diagnostics", &inputs, Sensitivity::Project))
            .participate);
        assert!(!JaymiDefaultContextPolicy
            .evaluate(&candidate("runtime", &inputs, Sensitivity::Project))
            .participate);
    }

    #[test]
    fn compile_question_selects_debug_feeds() {
        let request = UserRequest::new("why won't this compile?");
        let mut session = ContextSessionInputs::default();
        session.current_file.path = Some("/tmp/main.rs".into());
        session.workspace_kind = Some("coding".into());
        let hints =
            AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity("coding_question");
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: true,
            max_sensitivity: Sensitivity::Sensitive,
        };
        for id in ["conversation", "diagnostics", "editor", "runtime"] {
            assert!(
                JaymiDefaultContextPolicy
                    .evaluate(&candidate(id, &inputs, Sensitivity::Private))
                    .participate,
                "{id} should participate"
            );
        }
        for id in ["git_status", "workspace_inventory", "search"] {
            assert!(
                !JaymiDefaultContextPolicy
                    .evaluate(&candidate(id, &inputs, Sensitivity::Project))
                    .participate,
                "{id} should be omitted"
            );
        }
    }

    #[test]
    fn summarize_project_selects_overview_feeds() {
        let request = UserRequest::new("summarize this project");
        let session = ContextSessionInputs {
            workspace_kind: Some("coding".into()),
            ..ContextSessionInputs::default()
        };
        let hints =
            AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity("project_question");
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: true,
            max_sensitivity: Sensitivity::Sensitive,
        };
        for id in ["project", "workspace_inventory", "git_status", "conversation"] {
            assert!(
                JaymiDefaultContextPolicy
                    .evaluate(&candidate(id, &inputs, Sensitivity::Project))
                    .participate,
                "{id} should participate"
            );
        }
        assert!(!JaymiDefaultContextPolicy
            .evaluate(&candidate("diagnostics", &inputs, Sensitivity::Project))
            .participate);
        assert!(!JaymiDefaultContextPolicy
            .evaluate(&candidate("runtime", &inputs, Sensitivity::Project))
            .participate);
    }

    #[test]
    fn project_excluded_when_closed() {
        let request = UserRequest::new("summarize this project");
        let session = ContextSessionInputs::default();
        let hints =
            AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity("project_question");
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: false,
            max_sensitivity: Sensitivity::Sensitive,
        };
        let decision = JaymiDefaultContextPolicy.evaluate(&candidate(
            "project",
            &inputs,
            Sensitivity::Project,
        ));
        assert!(!decision.participate);
    }

    #[test]
    fn search_excluded_without_retrieval_need() {
        let request = UserRequest::new("what time is it");
        let session = ContextSessionInputs::default();
        let hints =
            AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity("general_question");
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        assert_eq!(signals.request_kind, RequestKind::Chat);
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: false,
            max_sensitivity: Sensitivity::Sensitive,
        };
        let decision = JaymiDefaultContextPolicy.evaluate(&candidate(
            "search",
            &inputs,
            Sensitivity::Project,
        ));
        assert!(!decision.participate);
    }

    #[test]
    fn editor_strips_open_files_constraint() {
        let request = UserRequest::new("look at this function");
        let mut session = ContextSessionInputs::default();
        session.current_file.path = Some("/tmp/a.rs".into());
        let hints =
            AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity("coding_question");
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: true,
            max_sensitivity: Sensitivity::Sensitive,
        };
        let decision = JaymiDefaultContextPolicy.evaluate(&candidate(
            "editor",
            &inputs,
            Sensitivity::Private,
        ));
        assert!(decision.participate);
        assert!(decision.constraints.exclude_open_files);
        assert!(!decision.requires_user_approval);
    }

    #[test]
    fn editor_selection_text_requires_approval() {
        let request = UserRequest::new("look at this function");
        let mut session = ContextSessionInputs::default();
        session.current_file.path = Some("/tmp/a.rs".into());
        session.current_selection.path = Some("/tmp/a.rs".into());
        session.current_selection.text = Some("secret snippet".into());
        let hints =
            AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity("coding_question");
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: true,
            max_sensitivity: Sensitivity::Sensitive,
        };
        let decision = JaymiDefaultContextPolicy.evaluate(&candidate(
            "editor",
            &inputs,
            Sensitivity::Private,
        ));
        assert!(decision.participate);
        assert!(decision.requires_user_approval);
        assert!(decision.reason.contains("user approval"));
    }

    #[test]
    fn memory_marks_exclude_sensitive_with_redact_constraint() {
        let request = UserRequest::new("remember this preference");
        let session = ContextSessionInputs::default();
        let hints =
            AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity("general_question");
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: false,
            max_sensitivity: Sensitivity::Sensitive,
        };
        let decision = JaymiDefaultContextPolicy.evaluate(&candidate(
            "memory",
            &inputs,
            Sensitivity::Private,
        ));
        assert!(decision.participate);
        assert!(decision.exclude_sensitive);
        assert!(decision.constraints.redact_memory_content);
    }

    #[test]
    fn sensitive_provider_requires_approval() {
        let request = UserRequest::new("general question about life");
        let session = ContextSessionInputs::default();
        let hints =
            AssembleHints::new(IntentId::Unknown, Vec::new()).with_complexity("general_question");
        let signals = RelevanceSignals::derive_with(&request, &session, Some(&hints));
        let inputs = ContextPolicyInputs {
            request: &request,
            session: &session,
            signals: &signals,
            project_open: false,
            max_sensitivity: Sensitivity::Sensitive,
        };
        let decision = JaymiDefaultContextPolicy.evaluate(&candidate(
            "custom_sensitive",
            &inputs,
            Sensitivity::Sensitive,
        ));
        assert!(decision.participate);
        assert!(decision.requires_user_approval);
    }
}
