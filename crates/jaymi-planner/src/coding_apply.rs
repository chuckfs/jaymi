//! Apply Approved Coding Changes (Sprint C3.6).
//!
//! After Review Approve, the Planner alone executes the frozen generation
//! batch through existing tools (`write_file` / `manage_path`) → providers.
//!
//! This module builds per-operation results and aggregated
//! [`ExecutionSummary`] values. It does **not** invent a coding mutation
//! bypass, retry loop, or provider-owned approval.
//!
//! Pipeline (required):
//! ```text
//! Approved ExecutionPlan → Tool → Provider → Project
//! ```

use jaymi_tools::ToolOutput;

use crate::execution_plan::{
    EstimatedRisk, ExecutionPlan, ExecutionStatus, ExecutionSummary,
};

/// Outcome of one generation tool call inside an approved batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingOpResult {
    /// 1-based order in the batch (matches ExecutionPlan steps).
    pub order: usize,
    /// Tool id that ran (`write_file` / `manage_path`).
    pub tool_id: String,
    /// Resource path when known.
    pub path: Option<String>,
    /// Whether the tool reported success.
    pub success: bool,
    /// Tool / Planner message for this op.
    pub message: String,
    /// Files edited by this op (from tool metadata).
    pub files_edited: Vec<String>,
    /// Actions performed (from tool metadata).
    pub actions_performed: Vec<String>,
}

impl CodingOpResult {
    /// Build from a tool output.
    pub fn from_tool_output(
        order: usize,
        tool_id: impl Into<String>,
        path: Option<String>,
        output: &ToolOutput,
    ) -> Self {
        let tool_id = tool_id.into();
        let message = output
            .message
            .clone()
            .unwrap_or_else(|| {
                if output.success {
                    format!("completed {tool_id}")
                } else {
                    format!("tool '{tool_id}' reported failure")
                }
            });
        Self {
            order,
            tool_id,
            path,
            success: output.success,
            message,
            files_edited: output.metadata.files_edited.clone(),
            actions_performed: output.metadata.actions_performed.clone(),
        }
    }

    /// Build from an orchestrator / provider error (treated as hard stop).
    pub fn from_error(
        order: usize,
        tool_id: impl Into<String>,
        path: Option<String>,
        error: impl Into<String>,
    ) -> Self {
        let tool_id = tool_id.into();
        let message = error.into();
        Self {
            order,
            tool_id,
            path,
            success: false,
            message,
            files_edited: Vec::new(),
            actions_performed: Vec::new(),
        }
    }
}

/// Aggregate generation op results into an [`ExecutionSummary`].
///
/// - All succeeded → Completed, `partial = false`
/// - Some succeeded then stop → Failed, `partial = true`
/// - First op failed → Failed, `partial = false`
pub fn summary_from_op_results(
    plan: &ExecutionPlan,
    results: &[CodingOpResult],
    duration_ms: u64,
) -> ExecutionSummary {
    let succeeded = results.iter().filter(|r| r.success).count();
    let failed = results.iter().any(|r| !r.success);
    let partial = succeeded > 0 && failed;

    let mut actions = Vec::new();
    let mut files = Vec::new();
    let mut tools = Vec::new();
    let mut outputs = Vec::new();
    let mut errors = Vec::new();
    let mut resources = Vec::new();

    for result in results {
        if !tools.iter().any(|t| t == &result.tool_id) {
            tools.push(result.tool_id.clone());
        }
        if let Some(path) = &result.path {
            if !resources.iter().any(|r| r == path) {
                resources.push(path.clone());
            }
        }
        if result.actions_performed.is_empty() {
            let label = result
                .path
                .as_deref()
                .map(|p| format!("{} · {p}", result.tool_id))
                .unwrap_or_else(|| result.tool_id.clone());
            actions.push(format!(
                "{}. {} — {}",
                result.order,
                label,
                if result.success { "ok" } else { "failed" }
            ));
        } else {
            for action in &result.actions_performed {
                actions.push(format!("{}. {action}", result.order));
            }
        }
        for file in &result.files_edited {
            if !files.iter().any(|f| f == file) {
                files.push(file.clone());
            }
        }
        if result.path.is_some() && result.success && result.files_edited.is_empty() {
            if let Some(path) = &result.path {
                if !files.iter().any(|f| f == path) {
                    files.push(path.clone());
                }
            }
        }
        outputs.push(format!(
            "op {} ({}): {}",
            result.order,
            if result.success { "ok" } else { "failed" },
            result.message
        ));
        if !result.success {
            errors.push(format!(
                "op {} (`{}`): {}",
                result.order, result.tool_id, result.message
            ));
        }
    }

    if resources.is_empty() {
        resources = plan.affected_resources().to_vec();
    }

    let status = plan.status();
    let next = if partial {
        vec![
            "Review what already changed before retrying".into(),
            "Ask Jaymi to continue from the partial result".into(),
            "Cancel further mutation if the project is inconsistent".into(),
        ]
    } else if failed {
        vec![
            "Fix the reported error and propose a new plan".into(),
            "Ask Jaymi to explain the failure".into(),
        ]
    } else {
        match plan.estimated_risk() {
            EstimatedRisk::High => vec!["Verify the high-risk changes".into()],
            _ => vec!["Continue with the next coding task".into()],
        }
    };

    ExecutionSummary {
        plan_id: plan.id().clone(),
        status,
        goal: plan.originating_request().to_string(),
        actions_performed: actions,
        resources_changed: resources,
        files_edited: files,
        duration_ms,
        warnings: if partial {
            vec![format!(
                "Stopped after {} of {} operation(s) succeeded — remaining ops were not executed",
                succeeded,
                results.len()
            )]
        } else {
            Vec::new()
        },
        error: errors.first().cloned(),
        errors,
        next_suggested_actions: next,
        tools_executed: tools,
        outputs,
        partial,
        files_moved_to_trash: Vec::new(),
        files_permanently_deleted: Vec::new(),
        recovery_available: None,
        deletion_method: plan.deletion_method(),
    }
}

/// Conversation prefix when a coding batch completes or stops.
pub fn coding_apply_headline(summary: &ExecutionSummary, op_count: usize) -> String {
    let succeeded = summary
        .outputs
        .iter()
        .filter(|line| line.contains("(ok)"))
        .count();
    if summary.partial {
        format!("Code generation stopped partially ({succeeded}/{op_count} op(s) succeeded).")
    } else if matches!(summary.status, ExecutionStatus::Failed) {
        format!("Code generation failed ({succeeded} of {op_count} op(s) completed).")
    } else {
        format!("Code generation completed ({op_count} op(s)).")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{
        EstimatedReversibility, ExecutionPlanParams, PlanLineage, PlanPermissionRequirement,
        ReviewRequirement,
    };
    use jaymi_capabilities::Capability;
    use jaymi_core::IntentId;

    fn sample_plan() -> ExecutionPlan {
        let mut plan = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: "Code generation: test".into(),
            planner_intent: IntentId::WriteFile,
            capability: Capability::FileManagement,
            proposed_tools: vec!["write_file".into()],
            steps: vec![],
            estimated_risk: EstimatedRisk::Medium,
            affected_resources: vec!["/tmp/a.rs".into(), "/tmp/b.rs".into()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "write".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::PartiallyReversible,
            expected_outputs: vec!["Applied ops".into()],
            deletion_method: None,
            action_preview: None,
            lineage: PlanLineage::root(),
        });
        plan.mark_ready().unwrap();
        plan.mark_awaiting_review().unwrap();
        plan.approve().unwrap();
        plan.mark_executing().unwrap();
        plan
    }

    #[test]
    fn all_success_is_complete_not_partial() {
        let mut plan = sample_plan();
        let _ = plan.mark_completed();
        let results = vec![
            CodingOpResult {
                order: 1,
                tool_id: "write_file".into(),
                path: Some("/tmp/a.rs".into()),
                success: true,
                message: "wrote a".into(),
                files_edited: vec!["/tmp/a.rs".into()],
                actions_performed: vec!["Wrote a".into()],
            },
            CodingOpResult {
                order: 2,
                tool_id: "write_file".into(),
                path: Some("/tmp/b.rs".into()),
                success: true,
                message: "wrote b".into(),
                files_edited: vec!["/tmp/b.rs".into()],
                actions_performed: vec!["Wrote b".into()],
            },
        ];
        let summary = summary_from_op_results(&plan, &results, 12);
        assert!(!summary.partial);
        assert_eq!(summary.status, ExecutionStatus::Completed);
        assert_eq!(summary.files_edited.len(), 2);
        assert!(summary.errors.is_empty());
    }

    #[test]
    fn mid_batch_failure_is_partial_and_lists_successes() {
        let mut plan = sample_plan();
        let _ = plan.mark_failed();
        let results = vec![
            CodingOpResult {
                order: 1,
                tool_id: "write_file".into(),
                path: Some("/tmp/a.rs".into()),
                success: true,
                message: "wrote a".into(),
                files_edited: vec!["/tmp/a.rs".into()],
                actions_performed: vec!["Wrote a".into()],
            },
            CodingOpResult::from_error(2, "write_file", Some("/tmp/missing/b.rs".into()), "no parent"),
        ];
        let summary = summary_from_op_results(&plan, &results, 9);
        assert!(summary.partial);
        assert_eq!(summary.status, ExecutionStatus::Failed);
        assert_eq!(summary.files_edited, vec!["/tmp/a.rs".to_string()]);
        assert!(!summary.errors.is_empty());
        assert!(summary.warnings.iter().any(|w| w.contains("Stopped after")));
        assert!(summary.render_conversation().contains("partial"));
    }
}
