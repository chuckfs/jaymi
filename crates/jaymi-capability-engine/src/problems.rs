//! Problems aggregation — registry of [`ProblemsProvider`]s for the Coding Problems panel.
//!
//! Architecture:
//! Application → [`ProblemsRegistry`] → [`ProblemsProvider`]s → [`ProblemIssue`]s → CodingState
//!
//! Future providers register on the same registry; the Coding Workspace only renders
//! aggregated issues and never talks to individual sources.

use std::sync::{Arc, RwLock};

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};

const NAME: &str = "problems-registry";
const DEPENDENCIES: &[&str] = &[];

/// Normalized severity for a problem issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProblemSeverity {
    /// Blocking error.
    Error,
    /// Non-blocking warning.
    Warning,
    /// Informational note.
    Info,
    /// Hint / suggestion.
    Hint,
}

impl ProblemSeverity {
    /// Stable lowercase label for UI / Monaco.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Hint => "hint",
        }
    }

    /// Parse common severity labels.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" | "err" | "1" => Self::Error,
            "warning" | "warn" | "2" => Self::Warning,
            "hint" | "4" => Self::Hint,
            _ => Self::Info,
        }
    }
}

impl std::fmt::Display for ProblemSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One aggregated problem shown in the Coding Problems panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemIssue {
    /// Stable id within a collect pass (`{source}:{index}` or richer).
    pub id: String,
    /// Severity.
    pub severity: ProblemSeverity,
    /// Source id (`lsp`, `planner`, `workspace`, `permissions`, `search`, `memory`, …).
    pub source: String,
    /// Human-readable source label (e.g. `rust-analyzer`).
    pub source_label: String,
    /// Related file path, when any.
    pub path: Option<String>,
    /// Zero-based start line, when known.
    pub line: Option<u32>,
    /// Zero-based start column, when known.
    pub column: Option<u32>,
    /// Zero-based end line, when known.
    pub end_line: Option<u32>,
    /// Zero-based end column, when known.
    pub end_column: Option<u32>,
    /// Human-readable message.
    pub message: String,
}

impl ProblemIssue {
    /// Builder for a path-less advisory issue.
    pub fn advisory(
        source: impl Into<String>,
        source_label: impl Into<String>,
        severity: ProblemSeverity,
        message: impl Into<String>,
    ) -> Self {
        let source = source.into();
        let source_label = source_label.into();
        let message = message.into();
        Self {
            id: format!("{source}:{}", message.chars().take(48).collect::<String>()),
            severity,
            source,
            source_label,
            path: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            message,
        }
    }

    /// Whether this issue can jump to Monaco (needs a file path).
    pub fn can_jump(&self) -> bool {
        self.path
            .as_deref()
            .map(|path| !path.trim().is_empty())
            .unwrap_or(false)
    }
}

/// Snapshot of inputs every [`ProblemsProvider`] may read.
///
/// Built by Application before [`ProblemsRegistry::collect_all`]. Providers must
/// not reach into UI or shell out — they only interpret this context.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProblemsCollectContext {
    /// Active project root, when any.
    pub project_root: Option<String>,
    /// LSP / rust-analyzer diagnostics already mapped into issues.
    pub lsp_issues: Vec<ProblemIssue>,
    /// Explorer / workspace error message, when any.
    pub workspace_error: Option<String>,
    /// Git panel last error, when any.
    pub git_error: Option<String>,
    /// Last planner turn was blocked.
    pub planner_blocked: bool,
    /// Last planner summary.
    pub planner_summary: Option<String>,
    /// Last permission / policy decision label (e.g. Denied).
    pub permission_decision: Option<String>,
    /// True when the last planner turn was permission-denied.
    pub permission_denied: bool,
    /// Index / discovery subsystem status label (`Operational`, `Disabled`, …).
    pub index_status: Option<String>,
    /// Index / discovery detail string.
    pub index_detail: Option<String>,
    /// Search engine health detail when unhealthy.
    pub search_unhealthy: Option<String>,
    /// Understanding / parse failure summary.
    pub understanding_failure: Option<String>,
    /// Memory subsystem status label.
    pub memory_status: Option<String>,
    /// Memory subsystem detail.
    pub memory_detail: Option<String>,
    /// True when memory reports unhealthy.
    pub memory_unhealthy: bool,
}

/// Trait implemented by every Problems source.
///
/// Register instances on [`ProblemsRegistry`]. Future plugins/providers should
/// call [`ProblemsRegistry::register`] at boot — the panel never hardcodes sources.
pub trait ProblemsProvider: Send + Sync {
    /// Stable source id (`lsp`, `planner`, …).
    fn id(&self) -> &str;

    /// Human-readable label for the Source column.
    fn label(&self) -> &str;

    /// Collect current issues from [`ProblemsCollectContext`].
    fn collect(&self, ctx: &ProblemsCollectContext) -> Vec<ProblemIssue>;
}

/// Registry of [`ProblemsProvider`]s.
#[derive(Default)]
pub struct ProblemsRegistry {
    initialized: bool,
    providers: RwLock<Vec<Arc<dyn ProblemsProvider>>>,
}

impl std::fmt::Debug for ProblemsRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProblemsRegistry")
            .field("initialized", &self.initialized)
            .field("provider_count", &self.len())
            .finish()
    }
}

impl ProblemsRegistry {
    /// Create an empty, uninitialized registry.
    pub fn new() -> Self {
        Self {
            initialized: false,
            providers: RwLock::new(Vec::new()),
        }
    }

    /// Register a problems provider.
    ///
    /// Fails when `id` is already registered.
    pub fn register(&self, provider: Arc<dyn ProblemsProvider>) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let mut guard = self
            .providers
            .write()
            .map_err(|_| JaymiError::new("problems registry lock poisoned"))?;
        if guard.iter().any(|existing| existing.id() == provider.id()) {
            return Err(JaymiError::new(format!(
                "problems provider already registered: {}",
                provider.id()
            )));
        }
        guard.push(provider);
        Ok(())
    }

    /// Register many providers; stops on the first failure.
    pub fn register_all(
        &self,
        providers: impl IntoIterator<Item = Arc<dyn ProblemsProvider>>,
    ) -> JaymiResult<()> {
        for provider in providers {
            self.register(provider)?;
        }
        Ok(())
    }

    /// Registered provider ids (stable sort).
    pub fn list_ids(&self) -> JaymiResult<Vec<String>> {
        self.ensure_initialized()?;
        let guard = self
            .providers
            .read()
            .map_err(|_| JaymiError::new("problems registry lock poisoned"))?;
        let mut ids: Vec<_> = guard
            .iter()
            .map(|provider| provider.id().to_string())
            .collect();
        ids.sort();
        Ok(ids)
    }

    /// Collect and merge issues from every registered provider.
    ///
    /// Order: provider registration order, then severity (error → hint), then path/message.
    pub fn collect_all(&self, ctx: &ProblemsCollectContext) -> JaymiResult<Vec<ProblemIssue>> {
        self.ensure_initialized()?;
        let guard = self
            .providers
            .read()
            .map_err(|_| JaymiError::new("problems registry lock poisoned"))?;
        let mut issues = Vec::new();
        for provider in guard.iter() {
            let mut batch = provider.collect(ctx);
            for (index, issue) in batch.iter_mut().enumerate() {
                if issue.id.trim().is_empty() {
                    issue.id = format!("{}:{index}", provider.id());
                }
                if issue.source.trim().is_empty() {
                    issue.source = provider.id().to_string();
                }
                if issue.source_label.trim().is_empty() {
                    issue.source_label = provider.label().to_string();
                }
            }
            issues.extend(batch);
        }
        issues.sort_by(|left, right| {
            severity_rank(left.severity)
                .cmp(&severity_rank(right.severity))
                .then(left.path.cmp(&right.path))
                .then(left.line.cmp(&right.line))
                .then(left.message.cmp(&right.message))
                .then(left.id.cmp(&right.id))
        });
        Ok(issues)
    }

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.providers.read().map(|guard| guard.len()).unwrap_or(0)
    }

    /// Whether no providers are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if !self.initialized {
            return Err(JaymiError::new("problems registry is not initialized"));
        }
        Ok(())
    }
}

fn severity_rank(severity: ProblemSeverity) -> u8 {
    match severity {
        ProblemSeverity::Error => 0,
        ProblemSeverity::Warning => 1,
        ProblemSeverity::Info => 2,
        ProblemSeverity::Hint => 3,
    }
}

impl Lifecycle for ProblemsRegistry {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![("providers".into(), self.len().to_string())])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        if let Ok(mut guard) = self.providers.write() {
            guard.clear();
        }
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubProvider {
        id: &'static str,
        issues: Vec<ProblemIssue>,
    }

    impl ProblemsProvider for StubProvider {
        fn id(&self) -> &str {
            self.id
        }

        fn label(&self) -> &str {
            self.id
        }

        fn collect(&self, _ctx: &ProblemsCollectContext) -> Vec<ProblemIssue> {
            self.issues.clone()
        }
    }

    #[test]
    fn registry_collects_and_sorts_by_severity() {
        let mut registry = ProblemsRegistry::new();
        registry.initialize().unwrap();
        registry
            .register(Arc::new(StubProvider {
                id: "memory",
                issues: vec![ProblemIssue::advisory(
                    "memory",
                    "Memory",
                    ProblemSeverity::Warning,
                    "memory pressure",
                )],
            }))
            .unwrap();
        registry
            .register(Arc::new(StubProvider {
                id: "lsp",
                issues: vec![ProblemIssue {
                    id: "lsp:0".into(),
                    severity: ProblemSeverity::Error,
                    source: "lsp".into(),
                    source_label: "rust-analyzer".into(),
                    path: Some("/tmp/a.rs".into()),
                    line: Some(3),
                    column: Some(0),
                    end_line: Some(3),
                    end_column: Some(1),
                    message: "bad ident".into(),
                }],
            }))
            .unwrap();

        let issues = registry
            .collect_all(&ProblemsCollectContext::default())
            .unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].source, "lsp");
        assert_eq!(issues[0].severity, ProblemSeverity::Error);
        assert_eq!(issues[1].source, "memory");
        assert!(registry
            .register(Arc::new(StubProvider {
                id: "lsp",
                issues: Vec::new(),
            }))
            .is_err());
    }
}
