//! Context Validation Suite (A10.9)
//!
//! Comprehensive guarantees for the Context system:
//! deterministic assembly & ordering, inclusion / exclusion, budget,
//! sensitivity, approval, cache invalidation, bundle immutability,
//! provider independence, and Context Policy determinism.
//!
//! Run with: `cargo test -p jaymi-context --test validation_suite`

#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_context::{
    ActiveWorkspaceSection, AssembleHints, BudgetEstimate, BudgetUnits, BundleDiagnostic,
    BundleSearchHit, ContextBudgetConfig, ContextBundle, ContextContribution, ContextEngine,
    ContextPolicy, ContextPolicyCandidate, ContextPolicyDecision, ContextProvider,
    ContextSessionInputs, ContextSource, ContextSources, CurrentFileSection,
    CurrentSelectionSection, DiagnosticsSection, ProviderInspectOutcome, ProviderPriority,
    ProviderRequest, RelevanceScore, SearchResultsSection, Sensitivity,
};
use jaymi_core::{IntentId, JaymiResult, Lifecycle, SearchRequest, UserRequest};
use jaymi_database::Database;
use jaymi_knowledge::SqliteKnowledgeStore;
use jaymi_memory_engine::{
    AppendMessageRequest, CreateConversationRequest, InMemoryMemoryStore, MemoryEngine,
    MemoryEngineApi, MessageRole,
};
use jaymi_project_engine::{InMemoryProjectStore, ProjectEngine, ProjectEngineApi};
use jaymi_search::{SearchEngine, SearchEngineApi};

fn temp_dir(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "jaymi-context-validation-{label}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn bound_engine() -> ContextEngine {
    let mut memory = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
    memory.initialize().unwrap();
    let memory = Arc::new(memory) as Arc<dyn MemoryEngineApi>;

    let mut projects = ProjectEngine::with_store(Arc::new(InMemoryProjectStore::new()));
    projects.initialize().unwrap();
    let projects = Arc::new(projects) as Arc<dyn ProjectEngineApi>;

    let data = temp_dir("bound");
    let mut db = Database::with_data_dir(&data);
    db.initialize().unwrap();
    let db = Arc::new(db);
    let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&db));
    knowledge.initialize().unwrap();
    let knowledge = Arc::new(knowledge);
    let mut search = SearchEngine::new(Arc::clone(&knowledge), None);
    search.initialize().unwrap();
    let search = Arc::new(search) as Arc<dyn SearchEngineApi>;

    let mut engine = ContextEngine::new();
    engine.initialize().unwrap();
    engine
        .bind_sources(ContextSources {
            memory,
            projects,
            search,
        })
        .unwrap();
    engine
}

fn fingerprint_bundle(bundle: &ContextBundle) -> String {
    format!(
        "ws={:?}|file={:?}|sel={:?}|open={}|search={}|mem={}|diag={}|perm={}|caps={:?}|src={:?}|pol={:?}|budget={:?}",
        bundle.workspace_kind(),
        bundle.current_file().path,
        bundle.current_selection().text,
        bundle.open_files().files.len(),
        bundle.search_results().hits.len(),
        bundle.memory().len(),
        bundle.diagnostics().diagnostics.len(),
        bundle.permissions().entries.len(),
        bundle.active_capabilities().capability_ids,
        bundle.sources(),
        bundle.policy().map(|p| &p.decisions),
        bundle.budget().map(|b| (b.used_characters, b.max_characters, b.truncated_providers.clone(), b.skipped_budget.clone())),
    )
}

/// Stable workspace contribution for custom-provider tests.
struct WorkspaceMark {
    id: &'static str,
    priority: u8,
    relevance: u8,
    sensitivity: Sensitivity,
    kind: &'static str,
    characters: usize,
    can_truncate: bool,
    contribute_calls: Arc<AtomicUsize>,
}

impl ContextProvider for WorkspaceMark {
    fn id(&self) -> &'static str {
        self.id
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::new(self.priority)
    }

    fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    fn relevance(&self, _request: &ProviderRequest<'_>) -> RelevanceScore {
        RelevanceScore::new(self.relevance)
    }

    fn estimate_size(&self, _request: &ProviderRequest<'_>) -> BudgetEstimate {
        BudgetEstimate {
            units: BudgetUnits::from_characters(self.characters, 4),
            can_truncate: self.can_truncate,
            can_summarize: self.can_truncate,
        }
    }

    fn contribute(
        &self,
        _request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        self.contribute_calls.fetch_add(1, AtomicOrdering::SeqCst);
        Ok(Some(ContextContribution {
            sources: vec![ContextSource::ActiveWorkspace],
            active_workspace: Some(ActiveWorkspaceSection {
                kind_id: Some(self.kind.into()),
            }),
            ..ContextContribution::default()
        }))
    }
}

struct SearchBlob {
    hits: usize,
    preview_chars: usize,
    contribute_calls: Arc<AtomicUsize>,
}

impl ContextProvider for SearchBlob {
    fn id(&self) -> &'static str {
        "search_blob"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::SEARCH
    }

    fn sensitivity(&self) -> Sensitivity {
        Sensitivity::Project
    }

    fn relevance(&self, _request: &ProviderRequest<'_>) -> RelevanceScore {
        RelevanceScore::HIGH
    }

    fn estimate_size(&self, _request: &ProviderRequest<'_>) -> BudgetEstimate {
        BudgetEstimate::flexible(BudgetUnits::from_characters(
            self.hits.saturating_mul(self.preview_chars.saturating_add(32)),
            4,
        ))
    }

    fn contribute(
        &self,
        _request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        self.contribute_calls.fetch_add(1, AtomicOrdering::SeqCst);
        Ok(Some(ContextContribution {
            sources: vec![ContextSource::SearchResults],
            search_results: Some(SearchResultsSection {
                hint: None,
                hits: (0..self.hits)
                    .map(|i| BundleSearchHit {
                        item_id: format!("hit-{i}"),
                        title: format!("title-{i}"),
                        path: None,
                        score: Some(1),
                        match_reason: None,
                        preview: Some("x".repeat(self.preview_chars)),
                        line: None,
                        column: None,
                    })
                    .collect(),
            }),
            ..ContextContribution::default()
        }))
    }
}

struct DecliningProvider;

impl ContextProvider for DecliningProvider {
    fn id(&self) -> &'static str {
        "declining"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::CRITICAL
    }

    fn relevance(&self, _request: &ProviderRequest<'_>) -> RelevanceScore {
        RelevanceScore::HIGH
    }

    fn estimate_size(&self, _request: &ProviderRequest<'_>) -> BudgetEstimate {
        BudgetEstimate::metadata(BudgetUnits::from_characters(8, 4))
    }

    fn contribute(
        &self,
        _request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        Ok(None)
    }
}

struct CountingLowRelevance {
    contribute_calls: Arc<AtomicUsize>,
}

impl ContextProvider for CountingLowRelevance {
    fn id(&self) -> &'static str {
        "low_relevance"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::CRITICAL
    }

    fn relevance(&self, _request: &ProviderRequest<'_>) -> RelevanceScore {
        RelevanceScore::LOW
    }

    fn estimate_size(&self, _request: &ProviderRequest<'_>) -> BudgetEstimate {
        BudgetEstimate::metadata(BudgetUnits::from_characters(16, 4))
    }

    fn contribute(
        &self,
        _request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        self.contribute_calls.fetch_add(1, AtomicOrdering::SeqCst);
        Ok(Some(ContextContribution {
            sources: vec![ContextSource::Diagnostics],
            diagnostics: Some(DiagnosticsSection {
                diagnostics: vec![BundleDiagnostic {
                    path: None,
                    severity: "info".into(),
                    message: "should not appear".into(),
                    line: None,
                    column: None,
                    source: None,
                }],
            }),
            ..ContextContribution::default()
        }))
    }
}

// ── Deterministic assembly ─────────────────────────────────────────────────

#[test]
fn deterministic_assembly_same_inputs_same_fingerprint() {
    let engine = bound_engine();
    engine.set_session_workspace(Some("coding".into()));
    let request = UserRequest::new("deterministic assembly probe");

    let first = engine.assemble(&request).unwrap();
    engine.invalidate_cache("validation");
    let second = engine.assemble(&request).unwrap();

    assert_eq!(fingerprint_bundle(&first), fingerprint_bundle(&second));
    assert_eq!(
        first.policy().unwrap().decisions,
        second.policy().unwrap().decisions
    );
    assert_eq!(
        first.active_capabilities().capability_ids,
        second.active_capabilities().capability_ids
    );
}

#[test]
fn deterministic_assembly_with_planner_hints() {
    let engine = bound_engine();
    let request = UserRequest::new("hinted assembly");
    let hints = AssembleHints::new(IntentId::SearchKnowledge, vec!["search".into()]);

    let first = engine.assemble_with(&request, Some(&hints)).unwrap();
    engine.invalidate_cache("validation");
    let second = engine.assemble_with(&request, Some(&hints)).unwrap();

    assert_eq!(fingerprint_bundle(&first), fingerprint_bundle(&second));
    assert_eq!(
        first.active_capabilities().capability_ids,
        vec!["search".to_string()]
    );
}

// ── Deterministic ordering ─────────────────────────────────────────────────

#[test]
fn deterministic_ordering_priority_not_registration() {
    let high_calls = Arc::new(AtomicUsize::new(0));
    let low_calls = Arc::new(AtomicUsize::new(0));

    let mut engine = ContextEngine::new();
    engine.initialize().unwrap();
    engine.set_budget_config(ContextBudgetConfig {
        max_characters: 120,
        max_tokens: None,
        chars_per_token: 4,
        reserved_characters: 20,
    });
    // Low priority registered first — allocation must still prefer high priority.
    engine
        .bind_providers(vec![
            Arc::new(SearchBlob {
                hits: 20,
                preview_chars: 40,
                contribute_calls: Arc::clone(&low_calls),
            }),
            Arc::new(WorkspaceMark {
                id: "high",
                priority: ProviderPriority::CRITICAL.value(),
                relevance: 90,
                sensitivity: Sensitivity::Workspace,
                kind: "coding",
                characters: 40,
                can_truncate: true,
                contribute_calls: Arc::clone(&high_calls),
            }),
        ])
        .unwrap();

    let bundle = engine.assemble(&UserRequest::new("order")).unwrap();
    assert_eq!(bundle.workspace_kind(), Some("coding"));
    assert!(high_calls.load(AtomicOrdering::SeqCst) >= 1);

    let inspection = engine.inspect_last().unwrap();
    let high = inspection
        .providers
        .iter()
        .find(|p| p.id == "high")
        .expect("high provider");
    let blob = inspection
        .providers
        .iter()
        .find(|p| p.id == "search_blob")
        .expect("blob provider");
    assert!(
        high.allocation_order.is_some()
            && blob.allocation_order.is_some()
            && high.allocation_order.unwrap() < blob.allocation_order.unwrap(),
        "critical provider must allocate before search blob; high={:?} blob={:?}",
        high.allocation_order,
        blob.allocation_order
    );
    assert_eq!(
        inspection.contributor_order.first().map(String::as_str),
        Some("high")
    );
}

#[test]
fn deterministic_ordering_evaluation_stable_across_assembles() {
    let engine = bound_engine();
    let request = UserRequest::new("eval order");
    let first = engine.assemble(&request).unwrap();
    let first_ids: Vec<_> = engine
        .inspect_last()
        .unwrap()
        .providers
        .iter()
        .map(|p| (p.evaluation_order, p.id.clone()))
        .collect();

    engine.invalidate_cache("validation");
    let second = engine.assemble(&request).unwrap();
    let second_ids: Vec<_> = engine
        .inspect_last()
        .unwrap()
        .providers
        .iter()
        .map(|p| (p.evaluation_order, p.id.clone()))
        .collect();

    assert_eq!(first_ids, second_ids);
    assert_eq!(first.assemble_generation() + 1, second.assemble_generation());
}

// ── Provider inclusion ─────────────────────────────────────────────────────

#[test]
fn provider_inclusion_core_providers_for_chat() {
    let engine = bound_engine();
    let bundle = engine.assemble(&UserRequest::new("hello inclusion")).unwrap();
    let inspection = engine.inspect_last().unwrap();

    assert!(bundle.sources().contains(&ContextSource::UserRequest));
    assert!(
        inspection
            .contributed()
            .iter()
            .any(|p| p.id == "conversation" || p.id == "permission" || p.id == "workspace"),
        "at least one core provider should contribute; got {:?}",
        inspection
            .contributed()
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn provider_inclusion_search_with_structured_request() {
    let engine = bound_engine();
    let bundle = engine
        .assemble(&UserRequest::search(SearchRequest::free_text("symbol Foo")))
        .unwrap();
    let policy = bundle.policy().unwrap();
    assert!(
        policy
            .decisions
            .iter()
            .any(|d| d.provider_id == "search" && d.included),
        "search must be included for structured search"
    );
    assert!(bundle.sources().contains(&ContextSource::SearchResults));
}

#[test]
fn provider_inclusion_via_assemble_hints() {
    let engine = bound_engine();
    let hints = AssembleHints::new(
        IntentId::Unknown,
        vec!["code".into(), "search".into()],
    );
    let bundle = engine
        .assemble_with(&UserRequest::new("build feature"), Some(&hints))
        .unwrap();
    assert_eq!(
        bundle.active_capabilities().capability_ids,
        vec!["code".to_string(), "search".to_string()]
    );
    assert!(bundle.sources().contains(&ContextSource::ActiveCapabilities));
}

// ── Provider exclusion ─────────────────────────────────────────────────────

#[test]
fn provider_exclusion_by_relevance_threshold() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut engine = ContextEngine::new();
    engine.initialize().unwrap();
    engine.set_relevance_threshold(40);
    engine
        .bind_providers(vec![
            Arc::new(CountingLowRelevance {
                contribute_calls: Arc::clone(&calls),
            }),
            Arc::new(WorkspaceMark {
                id: "keeper",
                priority: 50,
                relevance: 90,
                sensitivity: Sensitivity::Workspace,
                kind: "coding",
                characters: 24,
                can_truncate: true,
                contribute_calls: Arc::new(AtomicUsize::new(0)),
            }),
        ])
        .unwrap();

    let bundle = engine.assemble(&UserRequest::new("exclude low")).unwrap();
    assert_eq!(calls.load(AtomicOrdering::SeqCst), 0);
    assert_eq!(bundle.workspace_kind(), Some("coding"));
    assert!(bundle.diagnostics().diagnostics.is_empty());

    let inspection = engine.inspect_last().unwrap();
    assert!(inspection.providers.iter().any(|p| {
        p.id == "low_relevance"
            && matches!(p.outcome, ProviderInspectOutcome::SkippedRelevance { .. })
    }));
}

#[test]
fn provider_exclusion_search_on_plain_chat() {
    let engine = bound_engine();
    let _ = engine.assemble(&UserRequest::new("hello there")).unwrap();
    let inspection = engine.inspect_last().unwrap();
    let search = inspection
        .providers
        .iter()
        .find(|p| p.id == "search")
        .expect("search row");
    assert!(
        matches!(
            search.outcome,
            ProviderInspectOutcome::SkippedPolicy { .. }
                | ProviderInspectOutcome::SkippedRelevance { .. }
                | ProviderInspectOutcome::Declined
        ),
        "search omitted for plain chat: {:?}",
        search.outcome
    );
}

#[test]
fn provider_exclusion_by_custom_policy() {
    struct DenySearch;
    impl ContextPolicy for DenySearch {
        fn id(&self) -> &'static str {
            "deny_search"
        }
        fn evaluate(&self, candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
            if candidate.provider_id == "search" {
                ContextPolicyDecision::deny("validation denies search")
            } else {
                ContextPolicyDecision::allow("ok", candidate.provider_priority)
            }
        }
    }

    let engine = bound_engine();
    engine.set_context_policies(vec![Arc::new(DenySearch)]);
    let bundle = engine
        .assemble(&UserRequest::search(SearchRequest::free_text("still denied")))
        .unwrap();
    let policy = bundle.policy().unwrap();
    assert!(policy
        .decisions
        .iter()
        .any(|d| d.provider_id == "search" && !d.included));
}

// ── Budget enforcement ─────────────────────────────────────────────────────

#[test]
fn budget_enforcement_never_exceeds_configured_cap() {
    let engine = bound_engine();
    engine.set_max_characters(2_500);
    let bundle = engine
        .assemble(&UserRequest::new("budget cap validation"))
        .unwrap();
    let report = bundle.budget().expect("budget");
    assert_eq!(report.max_characters, 2_500);
    assert!(report.used_characters <= 2_500);
}

#[test]
fn budget_enforcement_fits_or_skips_oversized_provider() {
    let mut engine = ContextEngine::new();
    engine.initialize().unwrap();
    engine.set_budget_config(ContextBudgetConfig {
        max_characters: 400,
        max_tokens: None,
        chars_per_token: 4,
        reserved_characters: 50,
    });
    engine
        .bind_providers(vec![
            Arc::new(WorkspaceMark {
                id: "tiny",
                priority: ProviderPriority::CRITICAL.value(),
                relevance: 90,
                sensitivity: Sensitivity::Workspace,
                kind: "coding",
                characters: 32,
                can_truncate: true,
                contribute_calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(SearchBlob {
                hits: 30,
                preview_chars: 80,
                contribute_calls: Arc::new(AtomicUsize::new(0)),
            }),
        ])
        .unwrap();

    let bundle = engine.assemble(&UserRequest::new("fit me")).unwrap();
    let report = bundle.budget().unwrap();
    assert!(report.used_characters <= 350);
    assert_eq!(bundle.workspace_kind(), Some("coding"));
    assert!(
        report.truncated_providers.iter().any(|id| id == "search_blob")
            || report.skipped_budget.iter().any(|id| id == "search_blob")
            || bundle.search_results().hits.len() < 30,
        "oversized blob must be fitted or skipped; report={report:?}"
    );
}

#[test]
fn budget_enforcement_policy_forbids_truncation() {
    struct FixedBlob;
    impl ContextProvider for FixedBlob {
        fn id(&self) -> &'static str {
            "fixed_blob"
        }
        fn priority(&self) -> ProviderPriority {
            ProviderPriority::new(1)
        }
        fn relevance(&self, _: &ProviderRequest<'_>) -> RelevanceScore {
            RelevanceScore::HIGH
        }
        fn estimate_size(&self, _: &ProviderRequest<'_>) -> BudgetEstimate {
            BudgetEstimate {
                units: BudgetUnits {
                    characters: 20_000,
                    estimated_tokens: 5_000,
                },
                can_truncate: true,
                can_summarize: true,
            }
        }
        fn contribute(
            &self,
            _: &ProviderRequest<'_>,
        ) -> JaymiResult<Option<ContextContribution>> {
            Ok(Some(ContextContribution {
                sources: vec![ContextSource::Diagnostics],
                diagnostics: Some(DiagnosticsSection {
                    diagnostics: vec![BundleDiagnostic {
                        path: None,
                        severity: "info".into(),
                        message: "y".repeat(20_000),
                        line: None,
                        column: None,
                        source: None,
                    }],
                }),
                ..ContextContribution::default()
            }))
        }
    }

    struct ForbidTruncate;
    impl ContextPolicy for ForbidTruncate {
        fn id(&self) -> &'static str {
            "forbid_truncate"
        }
        fn evaluate(&self, candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
            let mut decision =
                ContextPolicyDecision::allow("allow", candidate.provider_priority);
            if candidate.provider_id == "fixed_blob" {
                decision.can_truncate = false;
            }
            decision
        }
    }

    let mut engine = ContextEngine::new();
    engine.initialize().unwrap();
    engine.set_budget_config(ContextBudgetConfig {
        max_characters: 500,
        max_tokens: None,
        chars_per_token: 4,
        reserved_characters: 50,
    });
    engine.set_context_policies(vec![Arc::new(ForbidTruncate)]);
    engine
        .bind_providers(vec![Arc::new(FixedBlob)])
        .unwrap();

    let bundle = engine.assemble(&UserRequest::new("no truncate")).unwrap();
    assert!(bundle.diagnostics().diagnostics.is_empty());
    let report = bundle.budget().unwrap();
    assert!(report.skipped_budget.iter().any(|id| id == "fixed_blob"));
}

// ── Sensitivity filtering ──────────────────────────────────────────────────

#[test]
fn sensitivity_filtering_blocks_private_providers() {
    struct BlockPrivate;
    impl ContextPolicy for BlockPrivate {
        fn id(&self) -> &'static str {
            "block_private"
        }
        fn evaluate(&self, candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
            if candidate.sensitivity >= Sensitivity::Private {
                ContextPolicyDecision::deny("private blocked")
            } else {
                ContextPolicyDecision::allow("ok", candidate.provider_priority)
            }
        }
    }

    let engine = bound_engine();
    engine.set_context_policies(vec![Arc::new(BlockPrivate)]);
    let bundle = engine.assemble(&UserRequest::new("sensitivity")).unwrap();
    let policy = bundle.policy().unwrap();
    assert!(policy
        .decisions
        .iter()
        .any(|d| d.provider_id == "memory" && !d.included));
    assert!(policy
        .decisions
        .iter()
        .any(|d| d.provider_id == "conversation" && !d.included));
    assert!(
        policy.decisions.iter().any(|d| {
            d.included
                && matches!(d.sensitivity.as_str(), "public" | "workspace" | "project")
        }),
        "at least one non-private provider should remain included; decisions={:?}",
        policy
            .decisions
            .iter()
            .map(|d| format!("{} included={} sens={}", d.provider_id, d.included, d.sensitivity))
            .collect::<Vec<_>>()
    );
}

#[test]
fn sensitivity_recorded_on_every_inspector_row() {
    let engine = bound_engine();
    let _ = engine.assemble(&UserRequest::new("sens rows")).unwrap();
    let inspection = engine.inspect_last().unwrap();
    assert!(!inspection.providers.is_empty());
    assert!(inspection
        .providers
        .iter()
        .all(|p| !p.sensitivity.is_empty()));
}

// ── Approval requirements ──────────────────────────────────────────────────

#[test]
fn approval_requirements_block_editor_selection_until_approved() {
    let engine = bound_engine();
    engine.set_session_inputs(ContextSessionInputs {
        current_file: CurrentFileSection {
            path: Some("/proj/main.rs".into()),
            dirty: false,
            language: Some("rust".into()),
        },
        current_selection: CurrentSelectionSection {
            path: Some("/proj/main.rs".into()),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 8,
            text: Some("secret selection".into()),
        },
        ..ContextSessionInputs::default()
    });

    let blocked = engine
        .assemble(&UserRequest::read_file("/proj/main.rs"))
        .unwrap();
    assert!(blocked.current_selection().text.is_none());
    let editor = blocked
        .policy()
        .unwrap()
        .decisions
        .iter()
        .find(|d| d.provider_id == "editor")
        .expect("editor");
    assert!(!editor.included);
    assert_eq!(editor.approval_status, "pending");
    assert!(editor.requires_user_approval);

    engine.set_session_inputs(ContextSessionInputs {
        current_file: CurrentFileSection {
            path: Some("/proj/main.rs".into()),
            dirty: false,
            language: Some("rust".into()),
        },
        current_selection: CurrentSelectionSection {
            path: Some("/proj/main.rs".into()),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 8,
            text: Some("secret selection".into()),
        },
        approved_context_providers: vec!["editor".into()],
        ..ContextSessionInputs::default()
    });
    engine.invalidate_cache("approval granted");
    let allowed = engine
        .assemble(&UserRequest::read_file("/proj/main.rs"))
        .unwrap();
    assert_eq!(
        allowed.current_selection().text.as_deref(),
        Some("secret selection")
    );
}

// ── Cache invalidation ─────────────────────────────────────────────────────

#[test]
fn cache_invalidation_forces_miss_and_preserves_correctness() {
    let engine = bound_engine();
    let request = UserRequest::new("cache validation");

    let first = engine.assemble(&request).unwrap();
    let second = engine.assemble(&request).unwrap();
    assert!(engine.inspect_last().unwrap().cache_hit);
    assert_eq!(fingerprint_bundle(&first), fingerprint_bundle(&second));

    engine.invalidate_cache("files_changed");
    let third = engine.assemble(&request).unwrap();
    assert!(!engine.inspect_last().unwrap().cache_hit);
    assert_eq!(fingerprint_bundle(&first), fingerprint_bundle(&third));

    let stats = engine.cache_stats();
    assert!(stats.hits >= 1);
    assert!(stats.misses >= 2);
    assert!(stats.invalidations >= 1);
}

#[test]
fn cache_misses_when_request_fingerprint_changes() {
    let engine = bound_engine();
    let _ = engine.assemble(&UserRequest::new("alpha")).unwrap();
    let _ = engine.assemble(&UserRequest::new("beta")).unwrap();
    let stats = engine.cache_stats();
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 2);
    assert_eq!(engine.inspect_last().unwrap().cache_status(), "miss");
}

#[test]
fn cache_not_invalidated_by_identical_workspace_rewrite() {
    let engine = bound_engine();
    engine.set_session_workspace(Some("coding".into()));
    let epoch = engine.cache_stats().epoch;
    let _ = engine.assemble(&UserRequest::new("hello")).unwrap();
    engine.set_session_workspace(Some("coding".into()));
    assert_eq!(engine.cache_stats().epoch, epoch);
    let _ = engine.assemble(&UserRequest::new("hello")).unwrap();
    assert!(engine.cache_stats().hits >= 1);
}

#[test]
fn cache_hit_skips_provider_contribute_work() {
    let engine = bound_engine();
    let calls = Arc::new(AtomicUsize::new(0));
    let marker = WorkspaceMark {
        id: "workspace_mark",
        priority: 90,
        relevance: 100,
        sensitivity: Sensitivity::Public,
        kind: "coding",
        characters: 32,
        can_truncate: true,
        contribute_calls: Arc::clone(&calls),
    };
    engine
        .bind_providers(vec![Arc::new(marker) as Arc<dyn ContextProvider>])
        .unwrap();
    engine.set_session_workspace(Some("coding".into()));
    let request = UserRequest::new("reuse me");
    let _ = engine.assemble(&request).unwrap();
    assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
    let _ = engine.assemble(&request).unwrap();
    assert!(engine.inspect_last().unwrap().cache_hit);
    assert_eq!(
        calls.load(AtomicOrdering::SeqCst),
        1,
        "cache hit must not re-call contribute"
    );
}

#[test]
fn diagnostics_change_requests_fresh_context() {
    let engine = bound_engine();
    let request = UserRequest::new("same prompt");
    let _ = engine.assemble(&request).unwrap();
    assert_eq!(engine.cache_stats().hits, 0);

    engine.set_session_inputs(ContextSessionInputs {
        diagnostics: DiagnosticsSection {
            diagnostics: vec![BundleDiagnostic {
                path: Some("/proj/main.rs".into()),
                severity: "error".into(),
                message: "unused".into(),
                line: Some(1),
                column: Some(0),
                source: None,
            }],
        },
        ..ContextSessionInputs::default()
    });
    let stats = engine.cache_stats();
    assert!(stats.invalidations >= 1);
    assert_eq!(
        stats.last_invalidation_reason.as_deref(),
        Some("diagnostics_changed")
    );
    let _ = engine.assemble(&request).unwrap();
    assert!(!engine.inspect_last().unwrap().cache_hit);
}

#[test]
fn conversation_revision_change_misses_cache() {
    let mut memory = MemoryEngine::with_store(Arc::new(InMemoryMemoryStore::new()));
    memory.initialize().unwrap();
    let memory = Arc::new(memory) as Arc<dyn MemoryEngineApi>;
    let mut projects = ProjectEngine::with_store(Arc::new(InMemoryProjectStore::new()));
    projects.initialize().unwrap();
    let projects = Arc::new(projects) as Arc<dyn ProjectEngineApi>;
    let data = temp_dir("conv-rev");
    let mut db = Database::with_data_dir(&data);
    db.initialize().unwrap();
    let db = Arc::new(db);
    let mut knowledge = SqliteKnowledgeStore::new(Arc::clone(&db));
    knowledge.initialize().unwrap();
    let knowledge = Arc::new(knowledge);
    let mut search = SearchEngine::new(Arc::clone(&knowledge), None);
    search.initialize().unwrap();
    let search = Arc::new(search) as Arc<dyn SearchEngineApi>;

    let mut engine = ContextEngine::new();
    engine.initialize().unwrap();
    engine
        .bind_sources(ContextSources {
            memory: Arc::clone(&memory),
            projects,
            search,
        })
        .unwrap();

    let conv = memory
        .create_conversation(&CreateConversationRequest {
            conversation_id: Some("conv-cache".into()),
            title: Some("cache".into()),
            project_id: None,
        })
        .unwrap();
    memory
        .set_active_conversation(Some(conv.id.as_str()))
        .unwrap();

    let request = UserRequest::new("hello again");
    let _ = engine.assemble(&request).unwrap();
    let hits_before = engine.cache_stats().hits;
    let _ = engine.assemble(&request).unwrap();
    assert!(engine.cache_stats().hits > hits_before);
    assert!(engine.inspect_last().unwrap().cache_hit);

    memory
        .append_message(&AppendMessageRequest {
            conversation_id: conv.id.as_str().to_string(),
            role: MessageRole::User,
            content: "new turn".into(),
            created_at: None,
            attachments: Vec::new(),
            references: Vec::new(),
        })
        .unwrap();

    let _ = engine.assemble(&request).unwrap();
    assert!(
        !engine.inspect_last().unwrap().cache_hit,
        "conversation revision change must miss cache"
    );
}

#[test]
fn request_fresh_context_forces_miss() {
    let engine = bound_engine();
    let request = UserRequest::new("fresh please");
    let _ = engine.assemble(&request).unwrap();
    let _ = engine.assemble(&request).unwrap();
    assert!(engine.inspect_last().unwrap().cache_hit);
    engine.request_fresh_context("planner_requested_fresh");
    let _ = engine.assemble(&request).unwrap();
    assert!(!engine.inspect_last().unwrap().cache_hit);
    assert_eq!(
        engine.cache_stats().last_invalidation_reason.as_deref(),
        Some("planner_requested_fresh")
    );
}

// ── ContextBundle immutability ─────────────────────────────────────────────

#[test]
fn context_bundle_immutability_accessors_are_stable() {
    let bundle = ContextBundle::builder()
        .active_workspace(ActiveWorkspaceSection {
            kind_id: Some("coding".into()),
        })
        .build();

    let a = bundle.workspace_kind();
    let b = bundle.workspace_kind();
    assert_eq!(a, b);
    assert_eq!(a, Some("coding"));

    let cloned = bundle.clone();
    assert_eq!(bundle, cloned);
}

#[test]
fn context_bundle_immutability_survives_session_mutation() {
    let engine = bound_engine();
    engine.set_session_workspace(Some("coding".into()));
    let snapshot = engine.assemble(&UserRequest::new("freeze me")).unwrap();
    assert_eq!(snapshot.workspace_kind(), Some("coding"));

    engine.set_session_workspace(Some("research".into()));
    assert_eq!(
        snapshot.workspace_kind(),
        Some("coding"),
        "prior ContextBundle must not change when session mutates"
    );
    assert_ne!(
        engine
            .assemble(&UserRequest::new("freeze me later"))
            .unwrap()
            .workspace_kind(),
        snapshot.workspace_kind()
    );
}

// ── ContextProvider independence ───────────────────────────────────────────

#[test]
fn context_provider_independence_decline_does_not_block_peers() {
    let keeper_calls = Arc::new(AtomicUsize::new(0));
    let mut engine = ContextEngine::new();
    engine.initialize().unwrap();
    engine
        .bind_providers(vec![
            Arc::new(DecliningProvider),
            Arc::new(WorkspaceMark {
                id: "peer",
                priority: 40,
                relevance: 90,
                sensitivity: Sensitivity::Workspace,
                kind: "coding",
                characters: 24,
                can_truncate: true,
                contribute_calls: Arc::clone(&keeper_calls),
            }),
        ])
        .unwrap();

    let bundle = engine.assemble(&UserRequest::new("independence")).unwrap();
    assert_eq!(bundle.workspace_kind(), Some("coding"));
    assert!(keeper_calls.load(AtomicOrdering::SeqCst) >= 1);

    let inspection = engine.inspect_last().unwrap();
    assert!(inspection
        .providers
        .iter()
        .any(|p| p.id == "declining" && matches!(p.outcome, ProviderInspectOutcome::Declined)));
    assert!(inspection
        .providers
        .iter()
        .any(|p| p.id == "peer" && p.outcome.contributed()));
}

#[test]
fn context_provider_independence_no_shared_mutable_state() {
    let left_calls = Arc::new(AtomicUsize::new(0));
    let right_calls = Arc::new(AtomicUsize::new(0));

    let mut engine = ContextEngine::new();
    engine.initialize().unwrap();
    engine
        .bind_providers(vec![
            Arc::new(WorkspaceMark {
                id: "left",
                priority: 60,
                relevance: 90,
                sensitivity: Sensitivity::Workspace,
                kind: "coding",
                characters: 24,
                can_truncate: true,
                contribute_calls: Arc::clone(&left_calls),
            }),
            Arc::new(SearchBlob {
                hits: 2,
                preview_chars: 8,
                contribute_calls: Arc::clone(&right_calls),
            }),
        ])
        .unwrap();

    // Distinct section types — neither provider depends on the other's state.
    let bundle = engine.assemble(&UserRequest::new("no shared state")).unwrap();
    assert_eq!(left_calls.load(AtomicOrdering::SeqCst), 1);
    assert_eq!(right_calls.load(AtomicOrdering::SeqCst), 1);
    assert_eq!(bundle.workspace_kind(), Some("coding"));
    assert_eq!(bundle.search_results().hits.len(), 2);
}

// ── Complexity lightweight assemble ─────────────────────────────────────────

#[test]
fn lightweight_greeting_skips_expensive_providers() {
    let engine = bound_engine();
    let hints = AssembleHints::new(IntentId::Unknown, Vec::<String>::new())
        .with_complexity("greeting");
    engine
        .assemble_with(&UserRequest::new("Hello!"), Some(&hints))
        .unwrap();
    let inspection = engine.inspect_last().unwrap();
    for id in [
        "memory",
        "search",
        "workspace",
        "diagnostics",
        "git_status",
        "workspace_inventory",
        "file_summaries",
    ] {
        assert!(
            inspection.providers.iter().any(|p| {
                p.id == id
                    && matches!(
                        p.outcome,
                        ProviderInspectOutcome::SkippedComplexity { .. }
                    )
            }),
            "expected {id} complexity-excluded for greeting; got {:?}",
            inspection
                .providers
                .iter()
                .find(|p| p.id == id)
                .map(|p| &p.outcome)
        );
    }
    assert!(
        !inspection.providers.iter().any(|p| {
            p.id == "conversation"
                && matches!(p.outcome, ProviderInspectOutcome::SkippedComplexity { .. })
        }),
        "conversation must not be complexity-excluded for greeting"
    );
}

#[test]
fn lightweight_general_question_keeps_memory_and_search_optional() {
    let engine = bound_engine();
    let hints = AssembleHints::new(IntentId::Unknown, Vec::<String>::new())
        .with_complexity("general_question");
    engine
        .assemble_with(
            &UserRequest::new("What is the capital of France?"),
            Some(&hints),
        )
        .unwrap();
    let inspection = engine.inspect_last().unwrap();
    for id in ["memory", "search"] {
        assert!(
            !inspection.providers.iter().any(|p| {
                p.id == id
                    && matches!(
                        p.outcome,
                        ProviderInspectOutcome::SkippedComplexity { .. }
                    )
            }),
            "{id} must not be complexity-excluded for general_question"
        );
    }
}

#[test]
fn lightweight_coding_does_not_complexity_exclude_core_providers() {
    let engine = bound_engine();
    engine.set_session_workspace(Some("coding".into()));
    engine.set_session_inputs(ContextSessionInputs {
        diagnostics: DiagnosticsSection {
            diagnostics: vec![BundleDiagnostic {
                path: None,
                severity: "warning".into(),
                message: "unused variable".into(),
                line: None,
                column: None,
                source: Some("rust-analyzer".into()),
            }],
        },
        ..ContextSessionInputs::default()
    });
    let hints = AssembleHints::new(IntentId::Unknown, Vec::<String>::new())
        .with_complexity("coding_question");
    engine
        .assemble_with(
            &UserRequest::new("How do I fix this borrow checker error?"),
            Some(&hints),
        )
        .unwrap();
    let inspection = engine.inspect_last().unwrap();
    for id in ["conversation", "workspace", "diagnostics", "project"] {
        assert!(
            !inspection.providers.iter().any(|p| {
                p.id == id
                    && matches!(
                        p.outcome,
                        ProviderInspectOutcome::SkippedComplexity { .. }
                    )
            }),
            "{id} must not be complexity-excluded for coding_question"
        );
    }
}

// ── ContextPolicy determinism ──────────────────────────────────────────────

#[test]
fn context_policy_determinism_repeated_evaluation() {
    let engine = bound_engine();
    let request = UserRequest::new("policy determinism");
    let first = engine.assemble(&request).unwrap();
    engine.invalidate_cache("validation");
    let second = engine.assemble(&request).unwrap();

    assert_eq!(first.policy(), second.policy());
    assert_eq!(
        first.policy().unwrap().active_policies,
        second.policy().unwrap().active_policies
    );
}

#[test]
fn context_policy_determinism_custom_policy_stable() {
    struct FlipFlopSafe;
    impl ContextPolicy for FlipFlopSafe {
        fn id(&self) -> &'static str {
            "flip_flop_safe"
        }
        fn evaluate(&self, candidate: &ContextPolicyCandidate<'_>) -> ContextPolicyDecision {
            // Deterministic: deny diagnostics always, allow others.
            if candidate.provider_id == "diagnostics" {
                ContextPolicyDecision::deny("diagnostics off")
            } else {
                ContextPolicyDecision::allow("ok", candidate.provider_priority)
            }
        }
    }

    let engine = bound_engine();
    engine.set_context_policies(vec![Arc::new(FlipFlopSafe)]);
    engine.set_session_workspace(Some("coding".into()));
    let request = UserRequest::new("stable custom policy");

    let a = engine.assemble(&request).unwrap();
    engine.invalidate_cache("validation");
    let b = engine.assemble(&request).unwrap();
    assert_eq!(a.policy().unwrap().decisions, b.policy().unwrap().decisions);
    assert!(a
        .policy()
        .unwrap()
        .decisions
        .iter()
        .any(|d| d.provider_id == "diagnostics" && !d.included));
}
