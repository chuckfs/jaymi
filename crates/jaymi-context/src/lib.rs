//! Context Engine for Jaymi.
//!
//! Assembles only the context required for the current request. The Planner
//! calls [`ContextEngine::assemble`] and does not coordinate Memory, Project,
//! Search, or session workspace state itself.

#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle, UserRequest};
use jaymi_memory_engine::{
    AssembleContextRequest, AssembledMemoryContext, MemoryEngineApi, PromotionAskDecision,
    PromotionSuggestQuery, PromotionSuggestion,
};
use jaymi_project_engine::{ProjectContext, ProjectEngineApi};
use jaymi_search::SearchEngineApi;

const NAME: &str = "context_engine";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "policy_engine",
    "permission_engine",
    "memory_engine",
];

/// Sources that contributed to an assembled context bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSource {
    /// Currently open project (Project Engine).
    ActiveProject,
    /// Prior turns / conversation-scoped memories.
    PreviousConversation,
    /// Search Engine contribution (query pending or index summary).
    SearchResults,
    /// Memories selected by the Memory Engine.
    RetrievedMemories,
    /// Active UX workspace from the experience session.
    ActiveWorkspace,
    /// Promotion suggestions derived from memory.
    PromotionSuggestions,
}

/// Lightweight search coordination included when a structured search request
/// is present. Full retrieval still happens through tools — this does not
/// execute search.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchContextHint {
    /// True when the user request carries a structured search query.
    pub structured_query_pending: bool,
    /// Free-text query preview, when any.
    pub query_preview: Option<String>,
    /// Active project search index document count, when a project is open.
    pub project_indexed_documents: Option<u64>,
}

/// Unified context for one Planner request.
///
/// Produced only by [`ContextEngine::assemble`]. The Planner must not rebuild
/// these fields from individual engines during `handle`.
#[derive(Debug, Clone)]
pub struct ContextBundle {
    /// Sources included in this bundle.
    pub sources: Vec<ContextSource>,
    /// Relevant memories for the request (never a full dump).
    pub memory: AssembledMemoryContext,
    /// Promotion suggestions (never auto-applied).
    pub promotion_suggestions: Vec<PromotionSuggestion>,
    /// Whether the Planner should ask the user about promotions.
    pub promotion_ask: PromotionAskDecision,
    /// Open project workspace context, when a project is active.
    pub project: Option<ProjectContext>,
    /// Active UX workspace kind id (`coding`, `research`, …), when set.
    pub active_workspace: Option<String>,
    /// Search coordination hint when appropriate (does not replace tool search).
    pub search: Option<SearchContextHint>,
    /// Monotonic assemble generation for diagnostics / tests.
    pub assemble_generation: u64,
}

impl Default for ContextBundle {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            memory: AssembledMemoryContext {
                memories: Vec::new(),
                project_id: None,
                conversation_id: None,
                candidate_count: 0,
                truncated: false,
            },
            promotion_suggestions: Vec::new(),
            promotion_ask: PromotionAskDecision::Defer,
            project: None,
            active_workspace: None,
            search: None,
            assemble_generation: 0,
        }
    }
}

/// Runtime sources bound after Memory / Project / Search are ready.
#[derive(Clone)]
pub struct ContextSources {
    /// Memory Engine for relevant memories and promotions.
    pub memory: Arc<dyn MemoryEngineApi>,
    /// Project Engine for open-project workspace context.
    pub projects: Arc<dyn ProjectEngineApi>,
    /// Search Engine — consulted only when appropriate (never for side effects).
    pub search: Arc<dyn SearchEngineApi>,
}

/// Context Engine — single assembler for request context.
pub struct ContextEngine {
    initialized: bool,
    sources: Mutex<Option<ContextSources>>,
    session_workspace: Mutex<Option<String>>,
    assemble_count: AtomicU64,
}

impl Default for ContextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextEngine {
    /// Create an uninitialized context engine (sources bound later).
    pub fn new() -> Self {
        Self {
            initialized: false,
            sources: Mutex::new(None),
            session_workspace: Mutex::new(None),
            assemble_count: AtomicU64::new(0),
        }
    }

    /// Bind Memory / Project / Search after those subsystems are ready.
    pub fn bind_sources(&self, sources: ContextSources) -> JaymiResult<()> {
        if !self.initialized {
            return Err(JaymiError::new(
                "context engine must be initialized before binding sources",
            ));
        }
        *self
            .sources
            .lock()
            .map_err(|_| JaymiError::new("context sources lock poisoned"))? = Some(sources);
        jaymi_logging::info("context", "context sources bound (memory+project+search)");
        Ok(())
    }

    /// True when Memory / Project / Search sources are bound.
    pub fn sources_bound(&self) -> bool {
        self.sources
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Record the active UX workspace kind for the next assemble (session state).
    pub fn set_session_workspace(&self, workspace_kind: Option<String>) {
        if let Ok(mut guard) = self.session_workspace.lock() {
            *guard = workspace_kind;
        }
    }

    /// Active UX workspace kind id, when set.
    pub fn session_workspace(&self) -> Option<String> {
        self.session_workspace
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Number of successful `assemble` calls since boot (tests / diagnostics).
    pub fn assemble_count(&self) -> u64 {
        self.assemble_count.load(Ordering::Relaxed)
    }

    /// Build only the context required for the current request.
    ///
    /// This is the sole Planner entry point for request context. Internally
    /// coordinates Memory Engine, Project Engine, Search Engine (when
    /// appropriate), and active workspace/session state.
    pub fn assemble(&self, request: &UserRequest) -> JaymiResult<ContextBundle> {
        if !self.initialized {
            return Err(JaymiError::new("context engine is not initialized"));
        }
        let sources = self
            .sources
            .lock()
            .map_err(|_| JaymiError::new("context sources lock poisoned"))?
            .clone()
            .ok_or_else(|| JaymiError::new("context engine sources are not bound"))?;

        let mut included = Vec::new();

        let memory = sources.memory.assemble_context(&AssembleContextRequest {
            text: request.content.clone(),
            conversation_id: sources.memory.active_conversation_id(),
            project_id: None,
            limit: Some(12),
            ..AssembleContextRequest::default()
        })?;
        included.push(ContextSource::RetrievedMemories);
        if memory.conversation_id.is_some() {
            included.push(ContextSource::PreviousConversation);
        }

        let promotion_suggestions =
            sources.memory.suggest_promotions(&PromotionSuggestQuery {
                conversation_id: sources.memory.active_conversation_id(),
                project_id: sources.memory.active_project_id(),
                min_importance: None,
                limit: Some(5),
            })?;
        let promotion_ask = PromotionAskDecision::from_suggestions(&promotion_suggestions);
        if !promotion_suggestions.is_empty() {
            included.push(ContextSource::PromotionSuggestions);
        }

        let project = match sources.projects.project_context(None) {
            Ok(context) => context,
            Err(error) => {
                jaymi_logging::warn(
                    "context",
                    format!("project context unavailable: {}", error.message()),
                );
                None
            }
        };
        if project.is_some() {
            included.push(ContextSource::ActiveProject);
        }

        let active_workspace = self.session_workspace();
        if active_workspace.is_some() {
            included.push(ContextSource::ActiveWorkspace);
        }

        let search = self.coordinate_search(&sources, request, project.as_ref());
        if search.is_some() {
            included.push(ContextSource::SearchResults);
        }

        let assemble_generation = self.assemble_count.fetch_add(1, Ordering::Relaxed) + 1;

        jaymi_logging::info(
            "context",
            format!(
                "assembled context memories={} candidates={} truncated={} project={} workspace={:?} search={} generation={}",
                memory.len(),
                memory.candidate_count,
                memory.truncated,
                project
                    .as_ref()
                    .map(|ctx| ctx.project.name.as_str())
                    .unwrap_or("-"),
                active_workspace,
                search.is_some(),
                assemble_generation
            ),
        );

        Ok(ContextBundle {
            sources: included,
            memory,
            promotion_suggestions,
            promotion_ask,
            project,
            active_workspace,
            search,
            assemble_generation,
        })
    }

    /// Search coordination without executing retrieval tools.
    fn coordinate_search(
        &self,
        sources: &ContextSources,
        request: &UserRequest,
        project: Option<&ProjectContext>,
    ) -> Option<SearchContextHint> {
        let structured = request.search.as_ref();
        let project_indexed = project.map(|ctx| ctx.search_index.indexed_file_count);

        // Appropriate when a structured search is present or an open project
        // exposes index health — never runs SearchEngine::search here.
        if structured.is_none() && project_indexed.is_none() {
            let _ = sources.search; // keep Search Engine wired for future scoped use
            return None;
        }

        Some(SearchContextHint {
            structured_query_pending: structured.is_some(),
            query_preview: structured.and_then(|search| {
                search
                    .free_text
                    .clone()
                    .or_else(|| search.filename.clone())
            }),
            project_indexed_documents: project_indexed,
        })
    }
}

impl Lifecycle for ContextEngine {
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
        let bound = self.sources_bound();
        let healthy = self.initialized && bound;
        HealthReport::new(
            NAME,
            self.initialized,
            healthy,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            (
                "status".to_string(),
                if healthy {
                    "operational".to_string()
                } else if self.initialized {
                    "awaiting_sources".to_string()
                } else {
                    "not_initialized".to_string()
                },
            ),
            ("sources_bound".to_string(), bound.to_string()),
            (
                "assemble_count".to_string(),
                self.assemble_count().to_string(),
            ),
            (
                "note".to_string(),
                "Planner request context is assembled here".to_string(),
            ),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        if let Ok(mut guard) = self.sources.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.session_workspace.lock() {
            *guard = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_memory_engine::{InMemoryMemoryStore, MemoryEngine};
    use jaymi_project_engine::{InMemoryProjectStore, ProjectEngine};
    use jaymi_search::SearchEngine;
    use jaymi_core::Lifecycle;
    use jaymi_database::Database;
    use jaymi_knowledge::SqliteKnowledgeStore;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jaymi-context-unit-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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

        let data = temp_dir();
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

    #[test]
    fn lifecycle_reports_memory_dependency() {
        let engine = ContextEngine::new();
        let health = engine.health_check();
        assert!(health.dependencies.contains(&"memory_engine".to_string()));
        assert!(!health.healthy);
    }

    #[test]
    fn assemble_increments_generation_and_includes_memory_source() {
        let engine = bound_engine();
        let first = engine.assemble(&UserRequest::new("hello")).unwrap();
        let second = engine.assemble(&UserRequest::new("hello again")).unwrap();
        assert_eq!(first.assemble_generation, 1);
        assert_eq!(second.assemble_generation, 2);
        assert!(first.sources.contains(&ContextSource::RetrievedMemories));
        assert_eq!(engine.assemble_count(), 2);
        assert!(engine.health_check().healthy);
    }
}
