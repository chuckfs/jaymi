//! Project-scoped knowledge search and isolation.
//!
//! The Project Engine owns which knowledge belongs to a project. Search and
//! retrieval go through this module so results stay inside project boundaries.

use std::path::PathBuf;

use jaymi_core::{JaymiError, JaymiResult, SearchRequest};
use jaymi_memory_engine::{
    project_decision_from_record, MemoryQuery, MemoryRecord, MemoryScope,
};

use crate::context::{ProjectDecisionEntry, ProjectFileEntry, ProjectParsedContent, ProjectTaskEntry};
use crate::engine::{ProjectEngine, ProjectEngineApi};
use crate::types::Project;

/// Kind of knowledge hit returned for a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectKnowledgeKind {
    /// Indexed inventory file.
    File,
    /// Parsed / normalized content under the project root.
    ParsedContent,
    /// Project memory (non-task).
    Memory,
    /// Project-attached conversation transcript.
    Conversation,
    /// Project task memory.
    Task,
    /// Architecture decision or architecture document.
    Architecture,
    /// Important / documentation file.
    Document,
}

impl ProjectKnowledgeKind {
    /// Stable label for summaries.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::ParsedContent => "parsed_content",
            Self::Memory => "memory",
            Self::Conversation => "conversation",
            Self::Task => "task",
            Self::Architecture => "architecture",
            Self::Document => "document",
        }
    }
}

/// Query for project-scoped knowledge retrieval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKnowledgeQuery {
    /// Project that owns the knowledge boundary.
    pub project_id: String,
    /// Free-text query.
    pub text: String,
    /// Maximum hits to return.
    pub limit: Option<usize>,
}

impl Default for ProjectKnowledgeQuery {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            text: String::new(),
            limit: Some(24),
        }
    }
}

/// One knowledge hit inside a project boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKnowledgeHit {
    /// Hit kind.
    pub kind: ProjectKnowledgeKind,
    /// Display title.
    pub title: String,
    /// Detail / snippet.
    pub detail: String,
    /// Optional filesystem path.
    pub path: Option<PathBuf>,
    /// Deterministic score (higher is better).
    pub score: u32,
    /// Owning project id.
    pub project_id: String,
}

const DEFAULT_KNOWLEDGE_LIMIT: usize = 24;

impl ProjectEngine {
    /// Search knowledge belonging to one project (files, content, memories, tasks, …).
    pub(crate) fn search_knowledge_with_sources(
        &self,
        query: &ProjectKnowledgeQuery,
        sources: &crate::context::ProjectContextSources,
    ) -> JaymiResult<Vec<ProjectKnowledgeHit>> {
        let project_id = query.project_id.trim();
        if project_id.is_empty() {
            return Err(JaymiError::new(
                "project knowledge search requires project_id",
            ));
        }
        let Some(project) = self.get(project_id)? else {
            return Err(JaymiError::new(format!("project not found: {project_id}")));
        };
        let text = query.text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let limit = query
            .limit
            .unwrap_or(DEFAULT_KNOWLEDGE_LIMIT)
            .max(1)
            .min(200);
        let needle = text.to_ascii_lowercase();
        let mut hits = Vec::new();

        if let Some(root) = &project.root_directory {
            let mut request = SearchRequest::free_text(text);
            request.folder = Some(root.clone());
            request.limit = Some(limit);
            let results = sources.search.search(&request)?;
            for hit in results.hits {
                hits.push(ProjectKnowledgeHit {
                    kind: ProjectKnowledgeKind::File,
                    title: hit.title.clone(),
                    detail: hit
                        .snippet
                        .clone()
                        .or(hit.preview.clone())
                        .unwrap_or_default(),
                    path: Some(hit.path.clone()),
                    score: hit.score.saturating_add(40),
                    project_id: project.id.as_str().to_string(),
                });
            }
        }

        let memories = sources.memory.retrieve(&MemoryQuery {
            scope: Some(MemoryScope::Project),
            project_id: Some(project.id.as_str().to_string()),
            text: Some(text.to_string()),
            limit: Some(limit),
            ..MemoryQuery::default()
        })?;
        for record in memories {
            hits.push(memory_hit(&project, &record, &needle));
        }

        // Conversations: match transcript text inside the project boundary.
        let conversation_ids = sources
            .memory
            .list_conversations_for_project(project.id.as_str())?;
        for meta in conversation_ids.into_iter().take(limit) {
            let Some(conversation) = sources
                .memory
                .load_conversation(meta.id.as_str())?
            else {
                continue;
            };
            let matched = conversation.messages.iter().find(|message| {
                message
                    .content
                    .to_ascii_lowercase()
                    .contains(&needle)
            });
            let Some(message) = matched else {
                continue;
            };
            hits.push(ProjectKnowledgeHit {
                kind: ProjectKnowledgeKind::Conversation,
                title: meta
                    .title
                    .clone()
                    .unwrap_or_else(|| meta.id.as_str().to_string()),
                detail: message.content.clone(),
                path: None,
                score: 70,
                project_id: project.id.as_str().to_string(),
            });
        }

        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then(left.title.cmp(&right.title))
                .then(left.kind.as_str().cmp(right.kind.as_str()))
        });
        hits.dedup_by(|left, right| {
            left.kind == right.kind
                && left.title == right.title
                && left.detail == right.detail
                && left.path == right.path
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

fn memory_hit(project: &Project, record: &MemoryRecord, needle: &str) -> ProjectKnowledgeHit {
    let kind = match record.kind.as_deref() {
        Some("task") | Some("todo") => ProjectKnowledgeKind::Task,
        Some("architecture_decision") | Some("architecture") => ProjectKnowledgeKind::Architecture,
        Some("important_file") => ProjectKnowledgeKind::Document,
        _ => ProjectKnowledgeKind::Memory,
    };
    let decision = project_decision_from_record(record);
    let detail = decision
        .as_ref()
        .map(|value| {
            if value.reasoning.trim().is_empty() {
                value.description.clone()
            } else {
                format!("{}\nWhy: {}", value.description, value.reasoning)
            }
        })
        .unwrap_or_else(|| record.content.clone());
    let hay = format!(
        "{} {} {}",
        record.summary,
        record.content,
        record.metadata_json
    )
    .to_ascii_lowercase();
    let score = if hay.contains(needle) { 80 } else { 50 };
    let path = decision
        .as_ref()
        .and_then(|value| {
            if value.related_files.len() == 1 {
                Some(std::path::PathBuf::from(&value.related_files[0]))
            } else {
                None
            }
        });
    ProjectKnowledgeHit {
        kind,
        title: record.summary.clone(),
        detail,
        path,
        score,
        project_id: project.id.as_str().to_string(),
    }
}

/// Build parsed-content entries for indexed files under the project root.
pub(crate) fn assemble_parsed_content(
    sources: &crate::context::ProjectContextSources,
    indexed_files: &[ProjectFileEntry],
    limit: usize,
) -> JaymiResult<Vec<ProjectParsedContent>> {
    let Some(content) = &sources.content else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for file in indexed_files.iter().take(limit) {
        let source_id = jaymi_knowledge::normalize_path(&file.path)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| file.path.to_string_lossy().into_owned());
        let Some(parsed) = content.get_by_source_id(&source_id)? else {
            continue;
        };
        let preview: String = parsed.plain_text.chars().take(240).collect();
        out.push(ProjectParsedContent {
            source_id,
            path: file.path.clone(),
            title: parsed.title.clone(),
            content_type: parsed.content_type.clone(),
            preview,
        });
    }
    Ok(out)
}

/// Surface project tasks from categorized project memory.
pub(crate) fn tasks_from_memories(
    memories: &jaymi_memory_engine::ProjectContext,
) -> Vec<ProjectTaskEntry> {
    memories
        .tasks
        .iter()
        .map(|record| ProjectTaskEntry {
            memory_id: record.id.as_str().to_string(),
            summary: record.summary.clone(),
            content: record.content.clone(),
            updated_at: record.updated_at,
        })
        .collect()
}

/// Surface the structured decision log from architecture decision memories.
pub(crate) fn decisions_from_memories(
    memories: &jaymi_memory_engine::ProjectContext,
) -> Vec<ProjectDecisionEntry> {
    memories
        .architecture_decisions
        .iter()
        .filter_map(|record| {
            let decision = project_decision_from_record(record)?;
            Some(ProjectDecisionEntry {
                memory_id: decision.memory_id,
                timestamp: decision.timestamp,
                title: decision.title,
                description: decision.description,
                reasoning: decision.reasoning,
                related_files: decision.related_files,
                related_conversations: decision.related_conversations,
            })
        })
        .collect()
}
