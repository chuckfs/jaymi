//! Jaymi default context policy — deterministic inclusion rules.

use std::sync::Arc;

use crate::budget::ProviderPriority;
use crate::relevance::{IntentTag, RequestKind};

use super::decision::{
    ContextPolicyCandidate, ContextPolicyDecision,
};
use super::sensitivity::Sensitivity;
use super::ContextPolicy;

/// Stable id for the default Jaymi context policy.
pub const DEFAULT_CONTEXT_POLICY_ID: &str = "jaymi_default_context";

/// Default deterministic context policy implementing Sprint A9 initial rules.
pub struct JaymiDefaultContextPolicy;

impl ContextPolicy for JaymiDefaultContextPolicy {
    fn id(&self) -> &'static str {
        DEFAULT_CONTEXT_POLICY_ID
    }

    fn evaluate(&self, candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
        // Global sensitivity gate — never assemble above the request max unless forced.
        if candidate.sensitivity > candidate.inputs.max_sensitivity
            && !sensitivity_required(candidate)
        {
            return ContextPolicyDecision::deny(format!(
                "Provider sensitivity '{}' exceeds request maximum '{}'",
                candidate.sensitivity.as_str(),
                candidate.inputs.max_sensitivity.as_str()
            ));
        }

        let mut decision = match candidate.provider_id {
            "conversation" => evaluate_conversation(candidate),
            "project" => evaluate_project(candidate),
            "workspace" => evaluate_workspace(candidate),
            "editor" => evaluate_editor(candidate),
            "search" => evaluate_search(candidate),
            "memory" => evaluate_memory(candidate),
            "diagnostics" => evaluate_diagnostics(candidate),
            "permission" => evaluate_permission(candidate),
            other => ContextPolicyDecision::allow(
                format!("No specific rule for '{other}'; retained for extensibility"),
                candidate.provider_priority,
            ),
        };

        if decision.participate && candidate.sensitivity >= Sensitivity::Sensitive {
            decision.requires_user_approval = true;
            if !decision.reason.contains("user approval") {
                decision.reason.push_str("; sensitive content requires user approval");
            }
        }

        decision
    }
}

/// Default policy set registered by the Context Policy Engine.
pub fn default_context_policies() -> Vec<Arc<dyn ContextPolicy>> {
    vec![Arc::new(JaymiDefaultContextPolicy)]
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
    let mut decision = ContextPolicyDecision::allow(
        "Active project is open",
        ProviderPriority::PROJECT,
    );
    // When a project is open it is always in scope for the request.
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

fn evaluate_editor(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    let session = candidate.inputs.session;
    let has_focus = session.current_file.path.is_some() || session.current_selection.path.is_some();
    if !has_focus {
        return ContextPolicyDecision::deny(
            "No current file or selection (open editors are not included by default)",
        );
    }
    let mut decision = ContextPolicyDecision::allow(
        "Current file and selection only",
        ProviderPriority::EDITOR,
    );
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
    decision
}

fn evaluate_search(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    let signals = candidate.inputs.signals;
    // Gate on canonical Intent facets / structured request — never free-text heuristics.
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
    // Memory provider itself filters to matching memories; policy only gates participation.
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
        || signals.coding_workspace();

    if !useful {
        return ContextPolicyDecision::deny(
            "Request does not benefit from memory matching",
        );
    }
    let mut decision = ContextPolicyDecision::allow(
        "Memories matching the current request only (provider filters relevance)",
        ProviderPriority::MEMORY,
    );
    if candidate.sensitivity >= Sensitivity::Private {
        // Private memory bodies are redacted in assembly; ids/summaries remain.
        decision.exclude_sensitive = true;
        decision.constraints.redact_memory_content = true;
    }
    decision
}

fn evaluate_diagnostics(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    let signals = candidate.inputs.signals;
    let session = candidate.inputs.session;
    let coding_capability = signals.active_capabilities.iter().any(|id| {
        id == "code" || id == "lsp" || id == "debug" || id == "debugging"
    }) || signals.coding_workspace();
    let debug_intent = signals.has_intent(IntentTag::Lsp)
        || signals.has_intent(IntentTag::Code)
        || matches!(signals.request_kind, RequestKind::Lsp);
    let has_diags = !session.diagnostics.diagnostics.is_empty();

    if !(coding_capability || debug_intent) {
        return ContextPolicyDecision::deny(
            "Diagnostics require coding capability or debug intent",
        );
    }
    if !has_diags && !debug_intent {
        return ContextPolicyDecision::deny("No diagnostics attached for this request");
    }
    ContextPolicyDecision::allow(
        if debug_intent {
            "Debug intent detected"
        } else {
            "Coding capability active"
        },
        ProviderPriority::DIAGNOSTICS,
    )
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
    // Private/Sensitive providers may still participate when their specific rule allows
    // and the request clearly involves that subsystem.
    match candidate.provider_id {
        "conversation" | "permission" => true,
        "editor" => {
            candidate.inputs.session.current_file.path.is_some()
                || candidate.inputs.session.current_selection.path.is_some()
        }
        "memory" => true, // gated by evaluate_memory usefulness
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::BudgetEstimate;
    use crate::policy::{ContextPolicyInputs, Sensitivity};
    use crate::relevance::{RelevanceScore, RelevanceSignals, RequestKind};
    use crate::ContextSessionInputs;
    use jaymi_core::UserRequest;

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
    }

    #[test]
    fn project_excluded_when_closed() {
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
            "project",
            &inputs,
            Sensitivity::Project,
        ));
        assert!(!decision.participate);
    }

    #[test]
    fn search_excluded_without_retrieval_need() {
        let request = UserRequest::new("hello there");
        let session = ContextSessionInputs::default();
        let signals = RelevanceSignals::derive(&request, &session);
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
        assert!(decision.reason.contains("does not require retrieval"));
    }

    #[test]
    fn editor_strips_open_files_constraint() {
        let request = UserRequest::new("look at this");
        let mut session = ContextSessionInputs::default();
        session.current_file.path = Some("/tmp/a.rs".into());
        let signals = RelevanceSignals::derive(&request, &session);
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
        let request = UserRequest::new("look at this");
        let mut session = ContextSessionInputs::default();
        session.current_file.path = Some("/tmp/a.rs".into());
        session.current_selection.path = Some("/tmp/a.rs".into());
        session.current_selection.text = Some("secret snippet".into());
        let signals = RelevanceSignals::derive(&request, &session);
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
        let request = UserRequest::new("remember this");
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
            "custom_sensitive",
            &inputs,
            Sensitivity::Sensitive,
        ));
        assert!(decision.participate);
        assert!(decision.requires_user_approval);
    }
}
