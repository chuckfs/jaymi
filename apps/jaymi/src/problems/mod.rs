//! Built-in Problems panel providers.
//!
//! Architecture: Application boot → [`ProblemsRegistry`] → these providers →
//! `CodingState.problems`. Each provider interprets one slice of
//! [`ProblemsCollectContext`] built by `Application::build_problems_context`.
//! The Coding Workspace UI never talks to individual sources — it only
//! renders the registry's aggregated output.

use std::sync::Arc;

use jaymi_capabilities::{ProblemIssue, ProblemSeverity, ProblemsCollectContext, ProblemsProvider};

/// `rust-analyzer` (LSP) diagnostics, already mapped into issues by the caller.
struct LspProblemsProvider;

impl ProblemsProvider for LspProblemsProvider {
    fn id(&self) -> &str {
        "lsp"
    }

    fn label(&self) -> &str {
        "rust-analyzer"
    }

    fn collect(&self, ctx: &ProblemsCollectContext) -> Vec<ProblemIssue> {
        ctx.lsp_issues.clone()
    }
}

/// Planner turn blocked by policy / permissions / capability errors.
struct PlannerProblemsProvider;

impl ProblemsProvider for PlannerProblemsProvider {
    fn id(&self) -> &str {
        "planner"
    }

    fn label(&self) -> &str {
        "Planner"
    }

    fn collect(&self, ctx: &ProblemsCollectContext) -> Vec<ProblemIssue> {
        if !ctx.planner_blocked {
            return Vec::new();
        }
        let message = ctx
            .planner_summary
            .clone()
            .unwrap_or_else(|| "Planner turn blocked".to_string());
        let severity = if ctx.permission_denied {
            ProblemSeverity::Error
        } else {
            ProblemSeverity::Warning
        };
        vec![ProblemIssue::advisory(
            "planner",
            "Planner",
            severity,
            message,
        )]
    }
}

/// Project Explorer load failures and Git panel errors.
struct WorkspaceProblemsProvider;

impl ProblemsProvider for WorkspaceProblemsProvider {
    fn id(&self) -> &str {
        "workspace"
    }

    fn label(&self) -> &str {
        "Workspace"
    }

    fn collect(&self, ctx: &ProblemsCollectContext) -> Vec<ProblemIssue> {
        let mut issues = Vec::new();
        if let Some(message) = &ctx.workspace_error {
            issues.push(ProblemIssue::advisory(
                "workspace",
                "Workspace",
                ProblemSeverity::Warning,
                message.clone(),
            ));
        }
        if let Some(message) = &ctx.git_error {
            issues.push(ProblemIssue::advisory(
                "workspace",
                "Git",
                ProblemSeverity::Warning,
                message.clone(),
            ));
        }
        issues
    }
}

/// Denied permission / policy decisions from the last Planner turn.
struct PermissionsProblemsProvider;

impl ProblemsProvider for PermissionsProblemsProvider {
    fn id(&self) -> &str {
        "permissions"
    }

    fn label(&self) -> &str {
        "Permissions"
    }

    fn collect(&self, ctx: &ProblemsCollectContext) -> Vec<ProblemIssue> {
        if !ctx.permission_denied {
            return Vec::new();
        }
        let decision = ctx
            .permission_decision
            .clone()
            .unwrap_or_else(|| "Denied".to_string());
        vec![ProblemIssue::advisory(
            "permissions",
            "Permissions",
            ProblemSeverity::Warning,
            format!("Permission {decision}"),
        )]
    }
}

/// Search / indexing / content-understanding health.
struct SearchProblemsProvider;

impl ProblemsProvider for SearchProblemsProvider {
    fn id(&self) -> &str {
        "search"
    }

    fn label(&self) -> &str {
        "Search"
    }

    fn collect(&self, ctx: &ProblemsCollectContext) -> Vec<ProblemIssue> {
        let mut issues = Vec::new();
        if let Some(detail) = &ctx.search_unhealthy {
            issues.push(ProblemIssue::advisory(
                "search",
                "Search Engine",
                ProblemSeverity::Warning,
                format!("Search engine unhealthy: {detail}"),
            ));
        }
        if matches!(ctx.index_status.as_deref(), Some("Disabled") | Some("Error")) {
            let status = ctx.index_status.clone().unwrap_or_default();
            let detail = ctx.index_detail.clone().unwrap_or_default();
            issues.push(ProblemIssue::advisory(
                "search",
                "Index",
                ProblemSeverity::Warning,
                format!("Index {status}: {detail}"),
            ));
        }
        if let Some(failure) = &ctx.understanding_failure {
            issues.push(ProblemIssue::advisory(
                "search",
                "Understanding",
                ProblemSeverity::Warning,
                failure.clone(),
            ));
        }
        issues
    }
}

/// Memory subsystem health.
struct MemoryProblemsProvider;

impl ProblemsProvider for MemoryProblemsProvider {
    fn id(&self) -> &str {
        "memory"
    }

    fn label(&self) -> &str {
        "Memory"
    }

    fn collect(&self, ctx: &ProblemsCollectContext) -> Vec<ProblemIssue> {
        if !ctx.memory_unhealthy {
            return Vec::new();
        }
        let detail = ctx
            .memory_detail
            .clone()
            .unwrap_or_else(|| "Memory subsystem unhealthy".to_string());
        vec![ProblemIssue::advisory(
            "memory",
            "Memory",
            ProblemSeverity::Warning,
            detail,
        )]
    }
}

/// Built-in Problems providers registered on `Application` boot.
///
/// Order here is registration order (stable tie-break within the same
/// severity after `ProblemsRegistry::collect_all` sorts). Plugins register
/// additional providers on the same registry — the panel never hardcodes
/// sources.
pub fn builtin_problem_providers() -> Vec<Arc<dyn ProblemsProvider>> {
    vec![
        Arc::new(LspProblemsProvider),
        Arc::new(PlannerProblemsProvider),
        Arc::new(WorkspaceProblemsProvider),
        Arc::new(PermissionsProblemsProvider),
        Arc::new(SearchProblemsProvider),
        Arc::new(MemoryProblemsProvider),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_providers_have_expected_ids() {
        let providers = builtin_problem_providers();
        let ids: Vec<&str> = providers.iter().map(|provider| provider.id()).collect();
        assert_eq!(
            ids,
            vec!["lsp", "planner", "workspace", "permissions", "search", "memory"]
        );
    }

    #[test]
    fn lsp_provider_passes_issues_through() {
        let provider = LspProblemsProvider;
        let issue = ProblemIssue {
            id: "lsp:0".into(),
            severity: ProblemSeverity::Error,
            source: "lsp".into(),
            source_label: "rust-analyzer".into(),
            path: Some("/tmp/a.rs".into()),
            line: Some(1),
            column: Some(0),
            end_line: Some(1),
            end_column: Some(2),
            message: "bad".into(),
        };
        let ctx = ProblemsCollectContext {
            lsp_issues: vec![issue.clone()],
            ..ProblemsCollectContext::default()
        };
        assert_eq!(provider.collect(&ctx), vec![issue]);
    }

    #[test]
    fn planner_provider_emits_only_when_blocked() {
        let provider = PlannerProblemsProvider;
        assert!(provider.collect(&ProblemsCollectContext::default()).is_empty());

        let blocked = ProblemsCollectContext {
            planner_blocked: true,
            planner_summary: Some("blocked by policy".into()),
            ..ProblemsCollectContext::default()
        };
        let issues = provider.collect(&blocked);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, ProblemSeverity::Warning);
        assert!(issues[0].message.contains("blocked by policy"));

        let denied = ProblemsCollectContext {
            planner_blocked: true,
            permission_denied: true,
            ..ProblemsCollectContext::default()
        };
        assert_eq!(provider.collect(&denied)[0].severity, ProblemSeverity::Error);
    }

    #[test]
    fn workspace_provider_emits_workspace_and_git_errors() {
        let provider = WorkspaceProblemsProvider;
        let ctx = ProblemsCollectContext {
            workspace_error: Some("explorer failed".into()),
            git_error: Some("git failed".into()),
            ..ProblemsCollectContext::default()
        };
        let issues = provider.collect(&ctx);
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().any(|issue| issue.message.contains("explorer failed")));
        assert!(issues.iter().any(|issue| issue.message.contains("git failed")));
    }

    #[test]
    fn permissions_provider_emits_only_when_denied() {
        let provider = PermissionsProblemsProvider;
        assert!(provider.collect(&ProblemsCollectContext::default()).is_empty());
        let ctx = ProblemsCollectContext {
            permission_denied: true,
            permission_decision: Some("Denied".into()),
            ..ProblemsCollectContext::default()
        };
        let issues = provider.collect(&ctx);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("Denied"));
    }

    #[test]
    fn search_provider_covers_search_index_and_understanding() {
        let provider = SearchProblemsProvider;
        let ctx = ProblemsCollectContext {
            search_unhealthy: Some("no embeddings".into()),
            index_status: Some("Disabled".into()),
            index_detail: Some("indexing_enabled=false".into()),
            understanding_failure: Some("parse failed for a.rs".into()),
            ..ProblemsCollectContext::default()
        };
        let issues = provider.collect(&ctx);
        assert_eq!(issues.len(), 3);
        assert!(issues.iter().any(|issue| issue.message.contains("no embeddings")));
        assert!(issues.iter().any(|issue| issue.message.contains("Disabled")));
        assert!(issues.iter().any(|issue| issue.message.contains("parse failed")));
    }

    #[test]
    fn memory_provider_emits_only_when_unhealthy() {
        let provider = MemoryProblemsProvider;
        assert!(provider.collect(&ProblemsCollectContext::default()).is_empty());
        let ctx = ProblemsCollectContext {
            memory_unhealthy: true,
            memory_detail: Some("store unreachable".into()),
            ..ProblemsCollectContext::default()
        };
        let issues = provider.collect(&ctx);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("store unreachable"));
    }
}
