//! Project Engine — first-class persistent projects.
//!
//! Architecture: Planner → Project Engine → Project Store
//!
//! The Planner requests one [`ProjectContext`]. The Project Engine decides what
//! belongs (files, conversations, memories, search index, recent work, architecture).

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use jaymi_core::{EntityId, HealthReport, JaymiError, JaymiResult, Lifecycle, SearchRequest};
use jaymi_knowledge::{KnowledgeQuery, KnowledgeSort, RecentKind};
use jaymi_projects::structure::JaymiProjectLayout;

use crate::context::{
    ProjectArchitectureItem, ProjectContext, ProjectContextSources, ProjectConversationEntry,
    ProjectFileEntry, ProjectRecentWorkItem, ProjectSearchIndex, DEFAULT_ARCHITECTURE_LIMIT,
    DEFAULT_CONVERSATION_LIMIT, DEFAULT_CONVERSATION_MESSAGE_LIMIT, DEFAULT_IMPORTANT_DOC_LIMIT,
    DEFAULT_INDEXED_FILE_LIMIT, DEFAULT_PARSED_CONTENT_LIMIT, DEFAULT_RECENT_LIMIT,
};
use crate::knowledge::{self, ProjectKnowledgeHit, ProjectKnowledgeQuery};
use crate::store::{ProjectStore, SqliteProjectStore};
use crate::types::{
    slugify_project_name, CreateProjectRequest, Project, ProjectHealth, ProjectStats,
    ProjectStatus, ProjectType,
};

const NAME: &str = "project_engine";
const DEPENDENCIES: &[&str] = &[
    "configuration",
    "logging",
    "database",
    "memory_engine",
    "search_engine",
];

/// Consumer-facing Project Engine API (Planner-facing surface).
pub trait ProjectEngineApi: Send + Sync {
    /// Create a persistent project.
    fn create(&self, request: &CreateProjectRequest) -> JaymiResult<Project>;

    /// Open a project and assemble its context.
    ///
    /// Owns session-open state. Application must not call this for user session
    /// open — the Planner orchestrates open (PE state + Memory hint + resume).
    fn open(&self, project_id: &str) -> JaymiResult<ProjectContext>;

    /// Close the session-open project, when any.
    ///
    /// Owns clearing session-open state. Application must not call this for user
    /// session close — the Planner orchestrates close (PE state + Memory hint).
    fn close(&self) -> JaymiResult<Option<Project>>;

    /// Soft-delete a project.
    fn delete(&self, project_id: &str) -> JaymiResult<()>;

    /// List active projects.
    fn list(&self) -> JaymiResult<Vec<Project>>;

    /// Load a project by id.
    fn get(&self, project_id: &str) -> JaymiResult<Option<Project>>;

    /// Find an active project by display name or slug.
    fn find_by_name(&self, name: &str) -> JaymiResult<Option<Project>>;

    /// Current open / active workspace project id, when any.
    fn open_project_id(&self) -> Option<String>;

    /// Active workspace project id (same as [`Self::open_project_id`]).
    fn active_project_id(&self) -> Option<String> {
        self.open_project_id()
    }

    /// Switch the active workspace to another project (same as [`Self::open`]).
    /// Does not clear the active conversation.
    fn switch(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.open(project_id)
    }

    /// Assemble one ProjectContext for the Planner (never manually gathered).
    fn assemble_context(&self, project_id: &str) -> JaymiResult<ProjectContext>;

    /// Assemble context for a specific id or the session-open project.
    fn project_context(&self, project_id: Option<&str>) -> JaymiResult<Option<ProjectContext>>;

    /// Search knowledge owned by a project (isolated from other projects).
    fn search_knowledge(
        &self,
        query: &crate::knowledge::ProjectKnowledgeQuery,
    ) -> JaymiResult<Vec<crate::knowledge::ProjectKnowledgeHit>>;

    /// Aggregate diagnostics.
    fn stats(&self) -> JaymiResult<ProjectStats>;

    /// Subsystem health.
    fn health(&self) -> JaymiResult<ProjectHealth>;
}

/// Centralized Project Engine.
pub struct ProjectEngine {
    initialized: bool,
    store: Arc<dyn ProjectStore>,
    open_project: Mutex<Option<String>>,
    sources: Mutex<Option<ProjectContextSources>>,
}

impl ProjectEngine {
    /// Create an engine backed by SQLite.
    pub fn new(store: Arc<SqliteProjectStore>) -> Self {
        Self::with_store(store)
    }

    /// Create an engine with an arbitrary store backend.
    pub fn with_store(store: Arc<dyn ProjectStore>) -> Self {
        Self {
            initialized: false,
            store,
            open_project: Mutex::new(None),
            sources: Mutex::new(None),
        }
    }

    /// Bind Memory / Knowledge / Search backends used for context assembly.
    pub fn bind_sources(&self, sources: ProjectContextSources) -> JaymiResult<()> {
        let mut guard = self
            .sources
            .lock()
            .map_err(|_| JaymiError::new("project sources lock"))?;
        *guard = Some(sources);
        Ok(())
    }

    fn ensure_ready(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("project engine is not initialized"))
        }
    }

    fn sources(&self) -> JaymiResult<ProjectContextSources> {
        self.sources
            .lock()
            .map_err(|_| JaymiError::new("project sources lock"))?
            .clone()
            .ok_or_else(|| JaymiError::new("project context sources are not bound"))
    }

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0)
    }

    fn initialize_jaymi_dir(root: &std::path::Path) -> JaymiResult<()> {
        let layout = JaymiProjectLayout::for_root(root);
        fs::create_dir_all(&layout.jaymi_dir).map_err(|error| {
            JaymiError::new(format!(
                "failed to create {}: {error}",
                layout.jaymi_dir.display()
            ))
        })?;
        for path in [
            &layout.conversations,
            &layout.memories,
            &layout.tasks,
            &layout.artifacts,
            &layout.cache,
        ] {
            fs::create_dir_all(path).map_err(|error| {
                JaymiError::new(format!("failed to create {}: {error}", path.display()))
            })?;
        }
        if !layout.project_json.exists() {
            let body = format!(
                "{{\n  \"root\": \"{}\",\n  \"created_by\": \"jaymi-project-engine\"\n}}\n",
                root.display()
            );
            fs::write(&layout.project_json, body).map_err(|error| {
                JaymiError::new(format!(
                    "failed to write {}: {error}",
                    layout.project_json.display()
                ))
            })?;
        }
        Ok(())
    }

    fn assemble_with_sources(
        &self,
        project: Project,
        is_open: bool,
        sources: &ProjectContextSources,
    ) -> JaymiResult<ProjectContext> {
        let mut memories = sources
            .memory
            .restore_project_memories(project.id.as_str())?;
        memories.name = project.name.clone();

        let conversation_ids = memories.conversation_ids.clone();
        let mut conversations = Vec::new();
        for conversation_id in conversation_ids.iter().take(DEFAULT_CONVERSATION_LIMIT) {
            if let Some(conversation) = sources.memory.load_conversation(conversation_id)? {
                conversations.push(ProjectConversationEntry::from_conversation(
                    &conversation,
                    DEFAULT_CONVERSATION_MESSAGE_LIMIT,
                ));
            } else {
                conversations.push(ProjectConversationEntry {
                    conversation_id: conversation_id.clone(),
                    title: None,
                    project_id: Some(project.id.as_str().to_string()),
                    updated_at: 0,
                    message_count: 0,
                    messages: Vec::new(),
                });
            }
        }

        let mut indexed_files = Vec::new();
        let mut search_index = ProjectSearchIndex {
            has_root: project.root_directory.is_some(),
            ..ProjectSearchIndex::default()
        };

        if let Some(root) = &project.root_directory {
            let prefix = root
                .canonicalize()
                .unwrap_or_else(|_| root.clone())
                .to_string_lossy()
                .into_owned();
            let items = sources.knowledge.query(KnowledgeQuery {
                path_prefix: Some(prefix.clone()),
                files_only: false,
                sort: KnowledgeSort::RecentlyModified,
                limit: Some(DEFAULT_INDEXED_FILE_LIMIT),
                ..KnowledgeQuery::default()
            })?;
            search_index.indexed_file_count =
                items.iter().filter(|item| !item.is_directory).count() as u64;
            search_index.indexed_folder_count =
                items.iter().filter(|item| item.is_directory).count() as u64;
            indexed_files = items
                .iter()
                .filter(|item| !item.is_directory)
                .take(DEFAULT_INDEXED_FILE_LIMIT)
                .map(ProjectFileEntry::from_knowledge)
                .collect();

            let search_health = sources.search.health().ok();
            search_index.search_healthy = search_health
                .as_ref()
                .map(|health| health.healthy)
                .unwrap_or(false);
            search_index.detail = search_health
                .map(|health| health.detail)
                .unwrap_or_else(|| format!("root={prefix}"));

            // Touch folder search so the index path is exercised for the project.
            let _ = sources.search.search(&SearchRequest::folder(root, false));
        } else {
            search_index.detail = "project has no root directory".into();
        }

        let mut important_documents: Vec<ProjectFileEntry> = memories
            .important_files
            .iter()
            .filter_map(|record| {
                let path = PathBuf::from(record.content.trim());
                if path.as_os_str().is_empty() {
                    return None;
                }
                Some(ProjectFileEntry {
                    path: path.clone(),
                    filename: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| record.summary.clone()),
                    extension: path
                        .extension()
                        .map(|ext| ext.to_string_lossy().to_ascii_lowercase()),
                    size: 0,
                    is_directory: false,
                    modified: Some(record.updated_at),
                })
            })
            .take(DEFAULT_IMPORTANT_DOC_LIMIT)
            .collect();

        // Supplement with inventory docs under the root.
        if let Some(root) = &project.root_directory {
            let prefix = root
                .canonicalize()
                .unwrap_or_else(|_| root.clone())
                .to_string_lossy()
                .into_owned();
            for ext in ["md", "txt", "pdf", "docx"] {
                if important_documents.len() >= DEFAULT_IMPORTANT_DOC_LIMIT {
                    break;
                }
                let docs = sources.knowledge.query(KnowledgeQuery {
                    path_prefix: Some(prefix.clone()),
                    extension: Some(ext.into()),
                    files_only: true,
                    sort: KnowledgeSort::RecentlyModified,
                    limit: Some(DEFAULT_IMPORTANT_DOC_LIMIT),
                    ..KnowledgeQuery::default()
                })?;
                for item in docs {
                    if important_documents.len() >= DEFAULT_IMPORTANT_DOC_LIMIT {
                        break;
                    }
                    if important_documents
                        .iter()
                        .any(|existing| existing.path == item.path)
                    {
                        continue;
                    }
                    important_documents.push(ProjectFileEntry::from_knowledge(&item));
                }
            }
        }

        let mut architecture_documents: Vec<ProjectArchitectureItem> = memories
            .architecture_decisions
            .iter()
            .map(|record| ProjectArchitectureItem {
                source: "memory".into(),
                title: record.summary.clone(),
                detail: record.content.clone(),
                path: None,
            })
            .take(DEFAULT_ARCHITECTURE_LIMIT)
            .collect();

        if let Some(root) = &project.root_directory {
            let prefix = root
                .canonicalize()
                .unwrap_or_else(|_| root.clone())
                .to_string_lossy()
                .into_owned();
            let arch_files = sources.knowledge.query(KnowledgeQuery {
                path_prefix: Some(prefix),
                name_contains: Some("architect".into()),
                files_only: true,
                sort: KnowledgeSort::RecentlyModified,
                limit: Some(DEFAULT_ARCHITECTURE_LIMIT),
                ..KnowledgeQuery::default()
            })?;
            for item in arch_files {
                if architecture_documents.len() >= DEFAULT_ARCHITECTURE_LIMIT {
                    break;
                }
                architecture_documents.push(ProjectArchitectureItem {
                    source: "file".into(),
                    title: item.filename.clone(),
                    detail: item.path.display().to_string(),
                    path: Some(item.path.clone()),
                });
            }
        }

        let mut recent_work = Vec::new();
        for record in memories.all_memories() {
            recent_work.push(ProjectRecentWorkItem {
                kind: "memory".into(),
                title: record.summary.clone(),
                reference: record.id.as_str().to_string(),
                at: record.updated_at,
            });
        }
        for conversation in &conversations {
            recent_work.push(ProjectRecentWorkItem {
                kind: "conversation".into(),
                title: conversation
                    .title
                    .clone()
                    .unwrap_or_else(|| conversation.conversation_id.clone()),
                reference: conversation.conversation_id.clone(),
                at: conversation.updated_at,
            });
        }
        for file in indexed_files.iter().take(DEFAULT_RECENT_LIMIT) {
            recent_work.push(ProjectRecentWorkItem {
                kind: "file".into(),
                title: file.filename.clone(),
                reference: file.path.display().to_string(),
                at: file.modified.unwrap_or(0),
            });
        }
        // Prefer knowledge "recent" listing when a root exists.
        if let Some(root) = &project.root_directory {
            let _ = root;
            if let Ok(recent) = sources
                .knowledge
                .recent(RecentKind::Modified, DEFAULT_RECENT_LIMIT)
            {
                for item in recent {
                    if let Some(root) = &project.root_directory {
                        let root_key = root
                            .canonicalize()
                            .unwrap_or_else(|_| root.clone())
                            .to_string_lossy()
                            .into_owned();
                        if !item.path.starts_with(&root_key)
                            && !item
                                .path
                                .to_string_lossy()
                                .starts_with(root.to_string_lossy().as_ref())
                        {
                            continue;
                        }
                    }
                    recent_work.push(ProjectRecentWorkItem {
                        kind: "file".into(),
                        title: item.filename.clone(),
                        reference: item.path.display().to_string(),
                        at: item.modified.or(item.last_modified).unwrap_or(0),
                    });
                }
            }
        }
        recent_work.sort_by(|left, right| {
            right
                .at
                .cmp(&left.at)
                .then(left.reference.cmp(&right.reference))
        });
        recent_work.dedup_by(|left, right| left.reference == right.reference);
        recent_work.truncate(DEFAULT_RECENT_LIMIT);

        let tasks = knowledge::tasks_from_memories(&memories);
        let decisions = knowledge::decisions_from_memories(&memories);
        let documentation: Vec<ProjectFileEntry> = important_documents
            .iter()
            .filter(|doc| {
                doc.extension
                    .as_deref()
                    .map(|ext| matches!(ext, "md" | "txt" | "rst" | "adoc"))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let parsed_content = knowledge::assemble_parsed_content(
            sources,
            &indexed_files,
            DEFAULT_PARSED_CONTENT_LIMIT,
        )?;
        search_index.detail = format!(
            "{}; parsed={} docs={} tasks={} decisions={} architecture={}",
            search_index.detail,
            parsed_content.len(),
            documentation.len(),
            tasks.len(),
            decisions.len(),
            architecture_documents.len()
        );

        Ok(ProjectContext {
            project,
            is_open,
            indexed_files,
            conversations,
            memories,
            search_index,
            important_documents,
            documentation,
            recent_work,
            architecture_documents,
            parsed_content,
            tasks,
            decisions,
        })
    }
}

impl ProjectEngineApi for ProjectEngine {
    fn create(&self, request: &CreateProjectRequest) -> JaymiResult<Project> {
        self.ensure_ready()?;
        let name = request.name.trim();
        if name.is_empty() {
            return Err(JaymiError::new("create project requires a name"));
        }
        let now = Self::now();
        let id = request
            .project_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("project:{}", slugify_project_name(name)));

        if let Some(existing) = self.store.get(&id)? {
            if existing.status != ProjectStatus::Deleted {
                return Err(JaymiError::new(format!("project already exists: {id}")));
            }
        }

        if let Some(root) = &request.root_directory {
            Self::initialize_jaymi_dir(root)?;
        }

        let project = Project {
            id: EntityId::new(id),
            name: name.to_string(),
            description: request
                .description
                .clone()
                .unwrap_or_default()
                .trim()
                .to_string(),
            root_directory: request.root_directory.clone(),
            created_at: now,
            updated_at: now,
            last_opened_at: None,
            project_type: request.project_type.unwrap_or(ProjectType::General),
            status: ProjectStatus::Active,
        };
        self.store.upsert(&project)?;
        jaymi_logging::info(
            "project",
            format!(
                "created project id={} name={} type={}",
                project.id.as_str(),
                project.name,
                project.project_type
            ),
        );
        Ok(project)
    }

    fn open(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.ensure_ready()?;
        let id = project_id.trim();
        if id.is_empty() {
            return Err(JaymiError::new("open project requires project_id"));
        }
        let Some(mut project) = self.store.get(id)? else {
            return Err(JaymiError::new(format!("project not found: {id}")));
        };
        if project.status == ProjectStatus::Deleted {
            return Err(JaymiError::new(format!(
                "cannot open deleted project: {id}"
            )));
        }
        let now = Self::now();
        project.last_opened_at = Some(now);
        project.updated_at = now;
        self.store.upsert(&project)?;
        {
            let mut guard = self
                .open_project
                .lock()
                .map_err(|_| JaymiError::new("open project lock"))?;
            *guard = Some(project.id.as_str().to_string());
        }
        jaymi_logging::info(
            "project",
            format!(
                "opened project id={} name={}",
                project.id.as_str(),
                project.name
            ),
        );
        self.assemble_context(project.id.as_str())
    }

    fn close(&self) -> JaymiResult<Option<Project>> {
        self.ensure_ready()?;
        let previous = {
            let mut guard = self
                .open_project
                .lock()
                .map_err(|_| JaymiError::new("open project lock"))?;
            guard.take()
        };
        let Some(project_id) = previous else {
            return Ok(None);
        };
        let project = self.store.get(&project_id)?;
        jaymi_logging::info("project", format!("closed project id={project_id}"));
        Ok(project)
    }

    fn delete(&self, project_id: &str) -> JaymiResult<()> {
        self.ensure_ready()?;
        let id = project_id.trim();
        if id.is_empty() {
            return Err(JaymiError::new("delete project requires project_id"));
        }
        let now = Self::now();
        if !self.store.delete(id, now)? {
            return Err(JaymiError::new(format!("project not found: {id}")));
        }
        {
            let mut guard = self
                .open_project
                .lock()
                .map_err(|_| JaymiError::new("open project lock"))?;
            if guard.as_deref() == Some(id) {
                *guard = None;
            }
        }
        jaymi_logging::info("project", format!("deleted project id={id}"));
        Ok(())
    }

    fn list(&self) -> JaymiResult<Vec<Project>> {
        self.ensure_ready()?;
        self.store.list_active()
    }

    fn get(&self, project_id: &str) -> JaymiResult<Option<Project>> {
        self.ensure_ready()?;
        self.store.get(project_id)
    }

    fn find_by_name(&self, name: &str) -> JaymiResult<Option<Project>> {
        self.ensure_ready()?;
        self.store.find_by_name(name)
    }

    fn open_project_id(&self) -> Option<String> {
        self.open_project
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn assemble_context(&self, project_id: &str) -> JaymiResult<ProjectContext> {
        self.ensure_ready()?;
        let id = project_id.trim();
        if id.is_empty() {
            return Err(JaymiError::new("assemble_context requires project_id"));
        }
        let Some(project) = self.store.get(id)? else {
            return Err(JaymiError::new(format!("project not found: {id}")));
        };
        if project.status == ProjectStatus::Deleted {
            return Err(JaymiError::new(format!(
                "cannot assemble context for deleted project: {id}"
            )));
        }
        let is_open = self.open_project_id().as_deref() == Some(project.id.as_str());
        let sources = self.sources()?;
        let context = self.assemble_with_sources(project, is_open, &sources)?;
        jaymi_logging::info(
            "project",
            format!(
                "assembled project context id={} entries={} files={} conversations={}",
                context.project.id.as_str(),
                context.entry_count(),
                context.indexed_files.len(),
                context.conversations.len()
            ),
        );
        Ok(context)
    }

    fn project_context(&self, project_id: Option<&str>) -> JaymiResult<Option<ProjectContext>> {
        self.ensure_ready()?;
        let target = project_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| self.open_project_id());
        let Some(target) = target else {
            return Ok(None);
        };
        Ok(Some(self.assemble_context(&target)?))
    }

    fn search_knowledge(
        &self,
        query: &ProjectKnowledgeQuery,
    ) -> JaymiResult<Vec<ProjectKnowledgeHit>> {
        self.ensure_ready()?;
        let sources = self.sources()?;
        self.search_knowledge_with_sources(query, &sources)
    }

    fn stats(&self) -> JaymiResult<ProjectStats> {
        self.ensure_ready()?;
        Ok(ProjectStats {
            active_count: self.store.count_by_status(ProjectStatus::Active)?,
            deleted_count: self.store.count_by_status(ProjectStatus::Deleted)?,
            open_project_id: self.open_project_id(),
        })
    }

    fn health(&self) -> JaymiResult<ProjectHealth> {
        let statistics = if self.initialized {
            self.stats().unwrap_or_default()
        } else {
            ProjectStats::default()
        };
        let sources_bound = self
            .sources
            .lock()
            .ok()
            .map(|guard| guard.is_some())
            .unwrap_or(false);
        Ok(ProjectHealth {
            initialized: self.initialized,
            healthy: self.initialized,
            version: env!("CARGO_PKG_VERSION").to_string(),
            detail: format!(
                "active_count={} deleted={} active={} sources={}",
                statistics.active_count,
                statistics.deleted_count,
                statistics
                    .open_project_id
                    .clone()
                    .unwrap_or_else(|| "-".into()),
                if sources_bound { "bound" } else { "unbound" }
            ),
            statistics,
        })
    }
}

impl Lifecycle for ProjectEngine {
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
        let health = self.health().unwrap_or(ProjectHealth {
            initialized: self.initialized,
            healthy: false,
            version: env!("CARGO_PKG_VERSION").to_string(),
            detail: "unavailable".into(),
            statistics: ProjectStats::default(),
        });
        HealthReport::new(
            NAME,
            health.initialized,
            health.healthy,
            self.version(),
            DEPENDENCIES,
        )
        .with_details(vec![
            (
                "status".into(),
                if health.healthy { "ok" } else { "degraded" }.into(),
            ),
            ("detail".into(), health.detail),
        ])
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        if let Ok(mut guard) = self.open_project.lock() {
            *guard = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryProjectStore;

    #[test]
    fn create_open_close_list_and_delete_without_sources_errors_on_assemble() {
        let mut engine = ProjectEngine::with_store(Arc::new(InMemoryProjectStore::new()));
        engine.initialize().unwrap();

        let created = engine
            .create(&CreateProjectRequest {
                project_id: Some("project:jaymi".into()),
                name: "Jaymi".into(),
                description: Some("Personal AI environment".into()),
                root_directory: None,
                project_type: Some(ProjectType::Code),
            })
            .unwrap();
        assert_eq!(created.name, "Jaymi");

        let listed = engine.list().unwrap();
        assert_eq!(listed.len(), 1);

        // Opening requires context sources in Slice 2.
        let opened = engine.open(created.id.as_str());
        assert!(opened.is_err());

        {
            let mut guard = engine.open_project.lock().unwrap();
            *guard = Some(created.id.as_str().to_string());
            let now = ProjectEngine::now();
            let mut project = created.clone();
            project.last_opened_at = Some(now);
            project.updated_at = now;
            engine.store.upsert(&project).unwrap();
        }
        assert_eq!(
            engine.open_project_id().as_deref(),
            Some(created.id.as_str())
        );
        let closed = engine.close().unwrap().unwrap();
        assert_eq!(closed.id, created.id);

        engine.delete(created.id.as_str()).unwrap();
        assert!(engine.list().unwrap().is_empty());
    }

    #[test]
    fn create_with_root_initializes_jaymi_directory() {
        let root = std::env::temp_dir().join(format!(
            "jaymi-project-root-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let mut engine = ProjectEngine::with_store(Arc::new(InMemoryProjectStore::new()));
        engine.initialize().unwrap();
        engine
            .create(&CreateProjectRequest {
                project_id: None,
                name: "Demo".into(),
                description: None,
                root_directory: Some(PathBuf::from(&root)),
                project_type: None,
            })
            .unwrap();

        let layout = JaymiProjectLayout::for_root(&root);
        assert!(layout.jaymi_dir.is_dir());
        assert!(layout.project_json.is_file());
        assert!(layout.conversations.is_dir());
        let _ = fs::remove_dir_all(&root);
    }
}
