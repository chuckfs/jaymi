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

        match candidate.provider_id {
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
        }
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
        || is_close_project_request(&candidate.inputs.request.content)
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
        && candidate.inputs.session.active_capabilities.capability_ids.is_empty()
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
    decision
}

fn evaluate_search(candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
    let signals = candidate.inputs.signals;
    let request = candidate.inputs.request;
    let needs_retrieval = matches!(
        signals.request_kind,
        RequestKind::Search | RequestKind::Discover | RequestKind::FileRead | RequestKind::Index
    ) || signals.has_intent(IntentTag::Search)
        || signals.has_intent(IntentTag::Discover)
        || signals.has_intent(IntentTag::Read)
        || request.search.is_some()
        || request.project_knowledge.is_some()
        || request.file.is_some()
        || references_files_or_symbols(&request.content)
        || !candidate.inputs.session.search_hits.is_empty();

    if !needs_retrieval {
        return ContextPolicyDecision::deny("Request does not require retrieval");
    }
    ContextPolicyDecision::allow(
        "Request requires retrieval / references files or symbols",
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
        decision.exclude_sensitive = true;
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
        || matches!(signals.request_kind, RequestKind::Lsp)
        || debug_request(&candidate.inputs.request.content);
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

fn references_files_or_symbols(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    const CUES: &[&str] = &[
        "file",
        "files",
        "path",
        "symbol",
        "function",
        "class",
        "struct",
        "trait",
        "module",
        "import",
        "find ",
        "search ",
        "where is",
        "locate",
        ".rs",
        ".ts",
        ".js",
        ".py",
        ".go",
        ".md",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
}

fn debug_request(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    const CUES: &[&str] = &[
        "debug",
        "debugger",
        "stack trace",
        "backtrace",
        "error:",
        "panic",
        "fix bug",
        "diagnose",
        "diagnostic",
        "lint",
        "compiler error",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
}

fn is_close_project_request(content: &str) -> bool {
    matches!(
        content.trim().to_ascii_lowercase().trim_end_matches('.'),
        "close project"
            | "close the project"
            | "close active project"
            | "leave project"
            | "leave the project"
    )
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
    }
}
