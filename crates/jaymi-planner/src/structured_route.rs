//! Production conversational routing ownership (Sprint C5.16).
//!
//! One authoritative answer to a single question:
//!
//! > Does this request belong to a Planner-owned structured path that must
//! > **not** fall through to ordinary LLM streaming?
//!
//! Ownership only. This module never executes tools, never mutates the
//! filesystem, never assembles a [`jaymi_context::ContextBundle`], and never
//! calls the Reasoning Engine. Every function here is a pure predicate over the
//! request plus already-resolved Planner state.
//!
//! ## Why this exists
//!
//! Before C5.16 the live streaming entry point
//! ([`crate::Planner::start_conversation_stream`]) and the blocking observer
//! entry point ([`crate::Planner::handle_conversational_with_observer`]) each
//! carried their own hand-maintained detector list. They drifted: the streaming
//! path — the only one the real UI uses — gated a small subset, so most already
//! implemented C2/C3/C4/C5 capabilities were unreachable from typed chat and
//! were silently answered as LLM prose instead.
//!
//! Both entry points now derive routing from [`detect_structured_route`], so the
//! lists cannot drift again. Detector precedence and the existing
//! `steals_from_*` mutual-exclusion semantics are preserved verbatim — this
//! module reorders nothing and loosens nothing.
//!
//! Constitutional constraints:
//! - Planner owns routing; the LLM is never the routing authority
//! - Detection is deterministic and offline (no provider calls)
//! - Ownership never implies approval — Review Before Action still gates mutation

use jaymi_core::UserRequest;

use crate::decision::Intent;
use crate::verification_diagnostics::VerificationEvidence;

/// A Planner-owned structured path that must not fall through to LLM prose.
///
/// Variants are named for the module that owns the path, not for the phrasing
/// that reaches it. Dispatch lives in
/// [`crate::Planner::dispatch_structured_route`] — this enum carries no behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructuredRoute {
    /// C4.6 — cancel an active bounded verification loop.
    VerificationLoopCancel,
    /// C0.1 — typed Coding Action with a deterministic Planner reply.
    CodingActionDeterministic,
    /// C1.8 — find dead code.
    DeadCode,
    /// C2.3 — relationship tracing.
    RelationshipTracing,
    /// C2.4 — deep investigation.
    DeepInvestigation,
    /// C2.5 — codebase Q&A.
    CodebaseQa,
    /// C2.2 — symbol navigation.
    SymbolNavigation,
    /// C2.1 — intelligent code search (editor-references soft path).
    CodeSearch,
    /// C1.5 / C3.4 — consume an existing Coding Plan into patch/ops generation.
    CodeGeneration,
    /// C1.7 — multi-file rename / move / extract / split.
    MultiFile,
    /// C4.1 — test execution.
    TestExecution,
    /// C4.2 — build awareness.
    BuildAwareness,
    /// C4.4 — evidence-gated corrective planning.
    FixFromVerification,
    /// C5.8 / C5.14 — agent pause / resume / cancel / status.
    AgentControl,
    /// C5.9 — goal failure / recovery.
    AgentFailureRecovery,
    /// C5.10 — coding goal completion.
    CodingGoalCompletion,
    /// C5.11 — long-running session continuity.
    AgentSessionContinuity,
    /// C5.12 — agentic coding experience (begin / status).
    AgenticCodingExperience,
    /// C5.1 — persistent multi-step Coding Goal.
    CodingGoal,
    /// C5.2 — goal decomposition into tasks.
    GoalDecomposition,
    /// C5.3 — agent lifecycle state machine.
    AgentLifecycle,
    /// C5.4 — multi-step execution start / continue / advance.
    MultiStepExecution,
    /// C5.5 — agent context maintain / show / compact.
    AgentContextMaintenance,
    /// C5.6 — approval / autonomy policy status.
    ApprovalAutonomyPolicy,
    /// C5.7 — goal progress UX.
    AgentProgress,
    /// C3.1 — coding edit intent (mutation classification → Coding Plan).
    CodingEdit,
}

impl StructuredRoute {
    /// Stable id for diagnostics / logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerificationLoopCancel => "verification_loop_cancel",
            Self::CodingActionDeterministic => "coding_action_deterministic",
            Self::DeadCode => "dead_code",
            Self::RelationshipTracing => "relationship_tracing",
            Self::DeepInvestigation => "deep_investigation",
            Self::CodebaseQa => "codebase_qa",
            Self::SymbolNavigation => "symbol_navigation",
            Self::CodeSearch => "code_search",
            Self::CodeGeneration => "code_generation",
            Self::MultiFile => "multi_file",
            Self::TestExecution => "test_execution",
            Self::BuildAwareness => "build_awareness",
            Self::FixFromVerification => "fix_from_verification",
            Self::AgentControl => "agent_control",
            Self::AgentFailureRecovery => "agent_failure_recovery",
            Self::CodingGoalCompletion => "coding_goal_completion",
            Self::AgentSessionContinuity => "agent_session_continuity",
            Self::AgenticCodingExperience => "agentic_coding_experience",
            Self::CodingGoal => "coding_goal",
            Self::GoalDecomposition => "goal_decomposition",
            Self::AgentLifecycle => "agent_lifecycle",
            Self::MultiStepExecution => "multi_step_execution",
            Self::AgentContextMaintenance => "agent_context_maintenance",
            Self::ApprovalAutonomyPolicy => "approval_autonomy_policy",
            Self::AgentProgress => "agent_progress",
            Self::CodingEdit => "coding_edit",
        }
    }
}

/// Planner state consulted by routing (resolved by the caller; never fetched here).
///
/// Keeping this a plain snapshot keeps the module pure: routing cannot reach
/// into locks, stores, or providers on its own.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoutingState<'a> {
    /// Most recent verification evidence (C4.4 deixis gate).
    pub verification_evidence: Option<&'a VerificationEvidence>,
    /// True when a bounded verification loop is currently active (C4.6).
    pub verification_loop_active: bool,
    /// True when a remembered Coding Plan is available to generate from.
    pub has_actionable_coding_plan: bool,
}

/// Ordered ownership decision shared by every production entry point.
///
/// Order mirrors the historical dispatch chain exactly — narrower and
/// safety-relevant paths first — so `steals_from_*` yields keep working as
/// written. `intent` is the already-resolved [`Intent`]; it is used only to
/// keep typed Coding Actions on their deterministic reply.
pub fn detect_structured_route(
    request: &UserRequest,
    intent: &Intent,
    state: RoutingState<'_>,
) -> Option<StructuredRoute> {
    // C4.6 — an explicit stop only owns the turn while a loop is actually running.
    if state.verification_loop_active
        && crate::coding_verification_loop::detect_loop_cancel(&request.content)
    {
        return Some(StructuredRoute::VerificationLoopCancel);
    }

    // C0.1 — typed toolbar actions with a deterministic Planner reply.
    if let Some(action) = request.coding_action {
        if matches!(intent, Intent::Unknown)
            && crate::coding_actions::deterministic_coding_action_response(action).is_some()
        {
            return Some(StructuredRoute::CodingActionDeterministic);
        }
    }

    let lower = request.content.trim().to_ascii_lowercase();
    if crate::pair_programming::looks_like_dead_code_request(&lower) {
        return Some(StructuredRoute::DeadCode);
    }

    // C2 reading modes (narrowest first).
    if crate::relationship_tracing::detect_relationship_trace_request(request).is_some() {
        return Some(StructuredRoute::RelationshipTracing);
    }
    if crate::deep_investigation::detect_deep_investigation_request(request).is_some() {
        return Some(StructuredRoute::DeepInvestigation);
    }
    if crate::codebase_qa::detect_codebase_qa_request(request).is_some() {
        return Some(StructuredRoute::CodebaseQa);
    }
    if crate::symbol_navigation::detect_symbol_navigation_request(request).is_some()
        && request.lsp.is_none()
    {
        return Some(StructuredRoute::SymbolNavigation);
    }
    // Mirrors `Planner::try_code_search_soft_path`'s own guard so ownership and
    // dispatch agree without assembling context here.
    if let Some(assessment) = crate::code_search::detect_code_search_request(request) {
        if assessment.prefer_editor_references && request.search.is_none() {
            return Some(StructuredRoute::CodeSearch);
        }
    }

    // C1.5 / C3.4 — consume an existing Coding Plan.
    if crate::code_generation::detect_generation_request(request) {
        return Some(StructuredRoute::CodeGeneration);
    }
    if crate::multi_file::detect_multi_file_request(request).is_some() {
        return Some(StructuredRoute::MultiFile);
    }

    // C4 verification.
    if crate::test_execution::detect_test_execution_request(request).is_some() {
        return Some(StructuredRoute::TestExecution);
    }
    if crate::build_awareness::detect_build_awareness_request(request).is_some() {
        return Some(StructuredRoute::BuildAwareness);
    }
    if crate::fix_from_verification::detect_fix_from_verification_request(
        request,
        state.verification_evidence,
    )
    .is_some()
    {
        return Some(StructuredRoute::FixFromVerification);
    }

    // C5 goal / control / recovery.
    if crate::agent_pause_resume_cancel::detect_control_request(request).is_some() {
        return Some(StructuredRoute::AgentControl);
    }
    if crate::agent_failure_recovery::detect_recovery_request(request).is_some() {
        return Some(StructuredRoute::AgentFailureRecovery);
    }
    if crate::coding_goal_completion::detect_completion_request(request).is_some() {
        return Some(StructuredRoute::CodingGoalCompletion);
    }
    if crate::agent_session_continuity::detect_continuity_request(request).is_some() {
        return Some(StructuredRoute::AgentSessionContinuity);
    }
    if crate::agentic_coding_experience::detect_agentic_experience_request(request).is_some() {
        return Some(StructuredRoute::AgenticCodingExperience);
    }
    if crate::coding_goal::detect_coding_goal_request(request).is_some() {
        return Some(StructuredRoute::CodingGoal);
    }
    if crate::goal_decomposition::detect_decomposition_request(request).is_some() {
        return Some(StructuredRoute::GoalDecomposition);
    }
    if crate::agent_state_machine::detect_agent_lifecycle_request(request).is_some() {
        return Some(StructuredRoute::AgentLifecycle);
    }
    if crate::multi_step_execution::detect_multi_step_execution_request(request).is_some() {
        return Some(StructuredRoute::MultiStepExecution);
    }
    if crate::agent_context_maintenance::detect_agent_context_request(request).is_some() {
        return Some(StructuredRoute::AgentContextMaintenance);
    }
    if crate::approval_autonomy_policy::detect_autonomy_policy_request(request).is_some() {
        return Some(StructuredRoute::ApprovalAutonomyPolicy);
    }
    if crate::agent_progress_ux::detect_progress_request(request).is_some() {
        return Some(StructuredRoute::AgentProgress);
    }

    // C3.1 — broad mutation phrasing runs last so every narrower path wins.
    if crate::coding_edit::detect_coding_edit_request(request).is_some() {
        return Some(StructuredRoute::CodingEdit);
    }

    // C5.16 — state-aware continuation, strictly last: a bare affirmative only
    // owns the turn when pending Planner state makes the intent unambiguous.
    continuation_route(&lower, request, state)
}

/// Resolve a bare affirmative against pending Planner state.
///
/// Returns `None` — ordinary conversation — whenever nothing is pending. A bare
/// "yes" never becomes a coding command on its own, and can never approve:
/// approval is an explicit Review Card gesture, and by existing design a new
/// user request already invalidates any parked plan
/// (`Planner::invalidate_paused`). The most an affirmative can do is re-enter
/// generation, which lands on a fresh Review Card with nothing written.
fn continuation_route(
    lower: &str,
    request: &UserRequest,
    state: RoutingState<'_>,
) -> Option<StructuredRoute> {
    // Structured payloads are never a conversational continuation.
    if request.coding_action.is_some()
        || request.write_file.is_some()
        || request.manage_path.is_some()
        || request.terminal.is_some()
        || request.file.is_some()
        || request.directory.is_some()
        || request.lsp.is_some()
        || request.search.is_some()
        || request.project_knowledge.is_some()
    {
        return None;
    }
    if !looks_like_bare_affirmative(lower) {
        return None;
    }
    if state.has_actionable_coding_plan {
        return Some(StructuredRoute::CodeGeneration);
    }
    None
}

/// Bare affirmatives that carry no content of their own.
///
/// Deliberately exact-match only. Anything with a real object ("yes, refactor
/// the parser") is left to the ordinary detectors above, which already own it.
pub fn looks_like_bare_affirmative(lower: &str) -> bool {
    let trimmed = lower
        .trim()
        .trim_end_matches(|c: char| c == '.' || c == '!' || c == '?')
        .trim();
    matches!(
        trimmed,
        "go ahead"
            | "go ahead then"
            | "go for it"
            | "do it"
            | "do that"
            | "do so"
            | "please do"
            | "yes please"
            | "yes"
            | "yep"
            | "yeah"
            | "yup"
            | "ok"
            | "okay"
            | "ok go"
            | "sure"
            | "proceed"
            | "continue"
            | "carry on"
            | "keep going"
            | "sounds good"
            | "looks good"
            | "let's do it"
            | "lets do it"
            | "let's go"
            | "lets go"
            | "let's build it"
            | "lets build it"
            | "make it so"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> UserRequest {
        UserRequest::new(text)
    }

    #[test]
    fn bare_affirmatives_are_exact_match_only() {
        for yes in [
            "Go ahead.",
            "go ahead",
            "Do it.",
            "Proceed.",
            "Yes",
            "Okay!",
            "Let's do it.",
            "Make it so.",
        ] {
            assert!(
                looks_like_bare_affirmative(&yes.to_ascii_lowercase()),
                "{yes} should be a bare affirmative"
            );
        }
        for no in [
            "yes, refactor the parser",
            "go ahead and delete everything in src",
            "do it the way rust does it",
            "okay so how does this work",
        ] {
            assert!(
                !looks_like_bare_affirmative(&no.to_ascii_lowercase()),
                "{no} carries its own object — detectors own it"
            );
        }
    }

    #[test]
    fn continuation_requires_pending_state() {
        let idle = RoutingState::default();
        assert_eq!(
            detect_structured_route(&plain("Go ahead."), &Intent::Unknown, idle),
            None,
            "no pending state ⇒ ordinary conversation"
        );

        let with_plan = RoutingState {
            has_actionable_coding_plan: true,
            ..RoutingState::default()
        };
        assert_eq!(
            detect_structured_route(&plain("Go ahead."), &Intent::Unknown, with_plan),
            Some(StructuredRoute::CodeGeneration)
        );

        // Generation lands on a Review Card — it never approves anything.
        assert_eq!(
            detect_structured_route(&plain("Yes."), &Intent::Unknown, with_plan),
            Some(StructuredRoute::CodeGeneration)
        );
    }

    #[test]
    fn conversational_code_questions_are_never_owned() {
        let with_plan = RoutingState {
            has_actionable_coding_plan: true,
            ..RoutingState::default()
        };
        for phrase in [
            "Show me code for Pong.",
            "How would I implement Pong physics?",
            "Give me an example of Pong collision detection.",
            "Explain how I could build this.",
        ] {
            assert_eq!(
                detect_structured_route(&plain(phrase), &Intent::Unknown, with_plan),
                None,
                "{phrase} must stay ordinary conversation"
            );
        }
    }

    #[test]
    fn loop_cancel_only_owns_while_a_loop_runs() {
        let idle = RoutingState::default();
        let running = RoutingState {
            verification_loop_active: true,
            ..RoutingState::default()
        };
        let stop = plain("Stop the verification workflow.");
        assert_eq!(
            detect_structured_route(&stop, &Intent::Unknown, running),
            Some(StructuredRoute::VerificationLoopCancel)
        );
        assert_ne!(
            detect_structured_route(&stop, &Intent::Unknown, idle),
            Some(StructuredRoute::VerificationLoopCancel)
        );
    }

    #[test]
    fn precedence_keeps_generation_ahead_of_coding_edit() {
        // "Generate the changes." is both apply-generation and mutation-ish;
        // C1.5 must win so an existing plan is consumed rather than re-planned.
        assert_eq!(
            detect_structured_route(
                &plain("Generate the changes."),
                &Intent::Unknown,
                RoutingState::default()
            ),
            Some(StructuredRoute::CodeGeneration)
        );
    }
}
