//! Search Engine — single entry point for all retrieval.
//!
//! Hides Knowledge Store / Content Intelligence / embedding implementation
//! details from Planner tools. No internet.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use jaymi_core::{
    HealthReport, JaymiError, JaymiResult, Lifecycle, MetadataFilters, SearchRequest,
};
use jaymi_database::Database;
use jaymi_knowledge::{
    normalize_path, KnowledgeItem, KnowledgeQuery, KnowledgeSort, KnowledgeStore, RecentKind,
    SqliteKnowledgeStore,
};
use jaymi_providers::EmbeddingProvider;
use jaymi_understanding::{ContentIntelligence, ContentIntelligenceApi};

use crate::hybrid_rank::{
    fuse_relevance, ranking_now_unix, recency_score, semantic_signal_from_similarity, RankSignals,
};
use crate::result::{MatchReason, SearchHit, SearchResults};
use crate::stats::{SearchHealth, SearchStats};
use crate::strategy::{select_strategy, SearchStrategy};

const NAME: &str = "search_engine";
const DEPENDENCIES: &[&str] = &["configuration", "logging", "database", "knowledge"];
const PREVIEW_CHARS: usize = 200;
const DEFAULT_LIMIT: usize = 100;
const SEMANTIC_MIN_SIMILARITY: f32 = 0.12;

/// Consumer-facing Search Engine surface.
pub trait SearchEngineApi: Send + Sync {
    /// Execute a search request and return ranked results.
    fn search(&self, request: &SearchRequest) -> JaymiResult<SearchResults>;

    /// Aggregate diagnostics statistics.
    fn stats(&self) -> JaymiResult<SearchStats>;

    /// Subsystem health for diagnostics.
    fn health(&self) -> JaymiResult<SearchHealth>;
}

#[derive(Default)]
struct RuntimeStats {
    search_count: u64,
    total_duration_ms: u64,
    last_strategy: Option<String>,
    last_duration_ms: Option<u64>,
    last_hit_count: Option<usize>,
    last_citation_count: Option<usize>,
    citations_generated: u64,
}

/// Optional semantic retrieval dependencies (provider + embedding store).
#[derive(Clone)]
pub struct SemanticDeps {
    /// Shared database (embeddings table only — not content blobs on SearchHit).
    pub database: Arc<Database>,
    /// Model-agnostic embedding provider.
    pub provider: Arc<dyn EmbeddingProvider>,
}

/// Unified Search Engine backed by the Knowledge Store.
///
/// Optional Content Intelligence supplies previews / FTS when content is already
/// normalized. Optional [`SemanticDeps`] enables meaning-based retrieval.
pub struct SearchEngine {
    initialized: bool,
    knowledge: Arc<SqliteKnowledgeStore>,
    content: Option<Arc<ContentIntelligenceApi>>,
    semantic: Option<SemanticDeps>,
    runtime: Mutex<RuntimeStats>,
}

impl SearchEngine {
    /// Create an uninitialized search engine.
    pub fn new(
        knowledge: Arc<SqliteKnowledgeStore>,
        content: Option<Arc<ContentIntelligenceApi>>,
    ) -> Self {
        Self::with_semantic(knowledge, content, None)
    }

    /// Create an uninitialized search engine with optional semantic retrieval.
    pub fn with_semantic(
        knowledge: Arc<SqliteKnowledgeStore>,
        content: Option<Arc<ContentIntelligenceApi>>,
        semantic: Option<SemanticDeps>,
    ) -> Self {
        Self {
            initialized: false,
            knowledge,
            content,
            semantic,
            runtime: Mutex::new(RuntimeStats::default()),
        }
    }

    /// Borrow the knowledge store (indexing / tests only — not Planner path).
    pub fn knowledge(&self) -> &Arc<SqliteKnowledgeStore> {
        &self.knowledge
    }

    /// True when a usable embedding provider is configured.
    pub fn semantic_available(&self) -> bool {
        self.semantic
            .as_ref()
            .map(|deps| deps.provider.embedding_status().available)
            .unwrap_or(false)
    }

    fn ensure_ready(&self) -> JaymiResult<()> {
        if self.initialized {
            Ok(())
        } else {
            Err(JaymiError::new("search engine is not initialized"))
        }
    }

    fn record_timing(
        &self,
        strategy: SearchStrategy,
        duration_ms: u64,
        hit_count: usize,
        citation_count: usize,
    ) -> JaymiResult<()> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| JaymiError::new("search engine stats lock poisoned"))?;
        runtime.search_count = runtime.search_count.saturating_add(1);
        runtime.total_duration_ms = runtime.total_duration_ms.saturating_add(duration_ms);
        runtime.last_strategy = Some(strategy.as_str().to_string());
        runtime.last_duration_ms = Some(duration_ms);
        runtime.last_hit_count = Some(hit_count);
        runtime.last_citation_count = Some(citation_count);
        runtime.citations_generated = runtime
            .citations_generated
            .saturating_add(citation_count as u64);
        Ok(())
    }

    fn execute(&self, request: &SearchRequest) -> JaymiResult<SearchResults> {
        self.ensure_ready()?;
        let started = Instant::now();
        let mut strategy = select_strategy(request);
        // Upgrade free-text to semantic when embeddings are available.
        if matches!(strategy, SearchStrategy::FreeText) && self.semantic_available() {
            strategy = SearchStrategy::Semantic;
        }
        let limit = request.limit.unwrap_or(DEFAULT_LIMIT).max(1);

        let (mut hits, candidate_count) = if request.metadata.list_collections {
            let hits = self.search_collections(limit)?;
            let count = hits.len();
            (hits, count)
        } else if let Some(name) = &request.metadata.collection {
            self.search_collection(
                name,
                request.metadata.collection_immediate,
                limit,
                request,
                strategy,
            )?
        } else {
            self.search_inventory(request, strategy, limit)?
        };

        // Guarantee explainable provenance on every returned hit.
        crate::citation::ensure_hit_previews(&mut hits);
        let citation_count = hits.len();

        let duration_ms = started.elapsed().as_millis() as u64;
        self.record_timing(strategy, duration_ms, hits.len(), citation_count)?;

        Ok(SearchResults {
            hits,
            strategy,
            duration_ms,
            candidate_count,
        })
    }

    fn search_collections(&self, limit: usize) -> JaymiResult<Vec<SearchHit>> {
        let collections = self.knowledge.list_collections()?;
        let mut hits = collections
            .into_iter()
            .map(|collection| {
                let signals = RankSignals {
                    metadata: 50,
                    ..RankSignals::default()
                };
                SearchHit {
                    item_id: format!("collection:{}", collection.name),
                    title: collection.name.clone(),
                    path: collection.root.clone(),
                    score: fuse_relevance(&signals),
                    signals,
                    match_reason: MatchReason::Collection,
                    preview: Some(format!("{} items", collection.item_count)),
                    matching_section: None,
                    snippet: None,
                    is_directory: true,
                }
            })
            .collect::<Vec<_>>();
        sort_hits(&mut hits);
        hits.truncate(limit);
        Ok(hits)
    }

    fn search_collection(
        &self,
        name: &str,
        immediate: bool,
        limit: usize,
        request: &SearchRequest,
        strategy: SearchStrategy,
    ) -> JaymiResult<(Vec<SearchHit>, usize)> {
        let Some(_collection) = self.knowledge.resolve_collection(name)? else {
            return Ok((Vec::new(), 0));
        };
        let items = self.knowledge.items_in_collection(
            name,
            immediate,
            Some(limit.saturating_mul(4).max(limit)),
        )?;
        let candidate_count = items.len();
        let hits = self.rank_items(items, request, strategy, limit)?;
        Ok((hits, candidate_count))
    }

    fn search_inventory(
        &self,
        request: &SearchRequest,
        strategy: SearchStrategy,
        limit: usize,
    ) -> JaymiResult<(Vec<SearchHit>, usize)> {
        let free_text = request
            .free_text
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());

        let has_content_meta = request.metadata.has_content_filters();

        // Structured metadata alone — never touch FTS.
        if matches!(strategy, SearchStrategy::StructuredMetadata)
            || (has_content_meta
                && free_text.is_none()
                && !matches!(
                    strategy,
                    SearchStrategy::Filename | SearchStrategy::Extension | SearchStrategy::Folder
                ))
        {
            return self.search_structured_metadata(request, strategy, limit);
        }

        // Free-text / semantic / combined with body search uses FTS and optional
        // vector similarity. Content metadata filters remain an independent SQL
        // constraint and also contribute a metadata signal.
        if matches!(
            strategy,
            SearchStrategy::FreeText | SearchStrategy::Semantic | SearchStrategy::Combined
        ) && free_text.is_some()
        {
            let query = free_text.unwrap();
            let (mut hits, mut candidate_count) = if matches!(
                strategy,
                SearchStrategy::Semantic | SearchStrategy::Combined
            ) && self.semantic_available()
            {
                self.search_with_semantic(request, strategy, limit, query)?
            } else {
                self.search_with_content_fts(request, strategy, limit, query)?
            };
            if has_content_meta {
                let meta_map = self.metadata_hits_by_source(request, limit)?;
                hits.retain(|hit| meta_map.contains_key(&hit.item_id));
                candidate_count = candidate_count.max(meta_map.len());
                for hit in &mut hits {
                    if let Some(meta) = meta_map.get(&hit.item_id) {
                        if let Some(scored) =
                            self.score_metadata_hit(meta, request, strategy)?
                        {
                            let prior_reason = hit.match_reason.clone();
                            hit.merge_signals(&scored.signals);
                            if prior_reason != scored.match_reason {
                                hit.match_reason = MatchReason::Combined {
                                    parts: vec![
                                        prior_reason.as_str(),
                                        scored.match_reason.as_str(),
                                    ],
                                };
                            }
                            if hit.matching_section.is_none() {
                                hit.matching_section = scored.matching_section.clone();
                            }
                        }
                    }
                }
                sort_hits(&mut hits);
            }
            return Ok((hits, candidate_count));
        }

        let items = self.fetch_candidates(request, strategy, limit)?;
        let candidate_count = items.len();
        let hits = self.rank_items(items, request, strategy, limit)?;
        Ok((hits, candidate_count))
    }

    fn metadata_hits_by_source(
        &self,
        request: &SearchRequest,
        limit: usize,
    ) -> JaymiResult<std::collections::BTreeMap<String, jaymi_understanding::ContentMetadataHit>>
    {
        let Some(api) = &self.content else {
            return Ok(std::collections::BTreeMap::new());
        };
        let hits = api.search_metadata(&request.metadata, limit.saturating_mul(4).max(limit))?;
        Ok(hits
            .into_iter()
            .map(|hit| (hit.source_id.clone(), hit))
            .collect())
    }

    /// Structured content metadata search (SQL only — independent of FTS).
    fn search_structured_metadata(
        &self,
        request: &SearchRequest,
        strategy: SearchStrategy,
        limit: usize,
    ) -> JaymiResult<(Vec<SearchHit>, usize)> {
        let Some(api) = &self.content else {
            // Without content intelligence, fall back to inventory browse filters.
            let items = self.fetch_candidates(request, SearchStrategy::Metadata, limit)?;
            let candidate_count = items.len();
            let hits = self.rank_items(items, request, SearchStrategy::Metadata, limit)?;
            return Ok((hits, candidate_count));
        };

        let meta_hits = api.search_metadata(&request.metadata, limit.saturating_mul(4).max(limit))?;
        let candidate_count = meta_hits.len();
        let mut hits = Vec::new();
        for meta in meta_hits {
            if let Some(hit) = self.score_metadata_hit(&meta, request, strategy)? {
                hits.push(hit);
            }
        }

        // Also include pure inventory browse when both content + inventory filters.
        if request.metadata.has_inventory_filters() && !request.metadata.list_collections {
            let items = self.fetch_candidates(request, SearchStrategy::Metadata, limit)?;
            for item in items {
                if let Some(hit) = self.score_item(&item, request, SearchStrategy::Metadata)? {
                    let key = hit.item_id.clone();
                    if let Some(existing) = hits.iter_mut().find(|h| h.item_id == key) {
                        existing.merge_signals(&hit.signals);
                    } else if !request.metadata.has_content_filters() {
                        hits.push(hit);
                    }
                }
            }
        }

        sort_hits(&mut hits);
        hits.truncate(limit);
        Ok((hits, candidate_count))
    }

    fn score_metadata_hit(
        &self,
        meta: &jaymi_understanding::ContentMetadataHit,
        request: &SearchRequest,
        strategy: SearchStrategy,
    ) -> JaymiResult<Option<SearchHit>> {
        let filters = &request.metadata;
        let mut signals = RankSignals::default();
        let mut reasons: Vec<String> = Vec::new();
        let mut primary = MatchReason::Metadata;
        let mut matching_section = None;

        if let Some(want) = filters
            .content_type
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
        {
            if meta.content_type.to_ascii_lowercase() != want {
                return Ok(None);
            }
            signals.metadata = signals.metadata.saturating_add(70);
            primary = MatchReason::MetadataContentType;
            reasons.push("content_type".into());
        }

        if let Some(want) = filters
            .language
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
        {
            match meta.language.as_ref().map(|value| value.to_ascii_lowercase()) {
                Some(have) if have == want => {
                    signals.metadata = signals.metadata.saturating_add(65);
                    if reasons.is_empty() {
                        primary = MatchReason::MetadataLanguage;
                    }
                    reasons.push("language".into());
                }
                _ => return Ok(None),
            }
        }

        if let Some(want) = filters
            .author
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
        {
            match meta.author.as_ref().map(|value| value.to_ascii_lowercase()) {
                Some(have) if have.contains(&want) => {
                    signals.metadata = signals.metadata.saturating_add(85);
                    if reasons.is_empty() {
                        primary = MatchReason::MetadataAuthor;
                    }
                    reasons.push("author".into());
                }
                _ => return Ok(None),
            }
        }

        if let Some(want) = filters
            .tag
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
        {
            if meta
                .tags
                .iter()
                .any(|tag| tag.to_ascii_lowercase() == want)
            {
                signals.metadata = signals.metadata.saturating_add(80);
                if reasons.is_empty() {
                    primary = MatchReason::MetadataTag;
                }
                reasons.push("tag".into());
            } else {
                return Ok(None);
            }
        }

        if let Some(want) = filters
            .heading_contains
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
        {
            if let Some(heading) = meta
                .headings
                .iter()
                .find(|heading| heading.text.to_ascii_lowercase().contains(&want))
            {
                signals.metadata = signals.metadata.saturating_add(90);
                matching_section = Some(heading.text.clone());
                if reasons.is_empty() {
                    primary = MatchReason::MetadataHeading;
                }
                reasons.push("heading".into());
            } else {
                return Ok(None);
            }
        }

        if let Some(want) = filters
            .title_contains
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
        {
            match meta.title.as_ref().map(|value| value.to_ascii_lowercase()) {
                Some(have) if have.contains(&want) => {
                    signals.title = signals.title.saturating_add(75);
                    if reasons.is_empty() {
                        primary = MatchReason::MetadataTitle;
                    }
                    reasons.push("title".into());
                }
                _ => return Ok(None),
            }
        }

        let date_active = filters.modified_after.is_some()
            || filters.modified_before.is_some()
            || filters.created_after.is_some()
            || filters.created_before.is_some()
            || filters.extracted_after.is_some()
            || filters.extracted_before.is_some();
        if date_active {
            signals.metadata = signals.metadata.saturating_add(55);
            if reasons.is_empty() {
                primary = MatchReason::MetadataDate;
            }
            reasons.push("date".into());
        }

        if signals.is_empty() {
            signals.metadata = 40;
            primary = MatchReason::Metadata;
        }

        let path = PathBuf::from(&meta.source_id);
        let item = self.knowledge.get_by_path(&path)?;
        if let Some(item) = &item {
            if !passes_metadata(item, &request.metadata) {
                return Ok(None);
            }
            signals.recency = recency_score(item.modified, ranking_now_unix());
        } else {
            signals.recency = recency_score(meta.modified, ranking_now_unix());
        }

        if matches!(strategy, SearchStrategy::Combined) && reasons.len() > 1 {
            primary = MatchReason::Combined {
                parts: reasons.clone(),
            };
        }

        let preview = matching_section.clone().or_else(|| meta.title.clone());
        Ok(Some(SearchHit {
            item_id: meta.source_id.clone(),
            title: meta.title.clone().unwrap_or_else(|| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| meta.source_id.clone())
            }),
            path,
            score: fuse_relevance(&signals),
            signals,
            match_reason: primary,
            preview: preview.clone(),
            matching_section,
            snippet: preview,
            is_directory: item.map(|value| value.is_directory).unwrap_or(false),
        }))
    }

    /// Semantic retrieval: embed the query, rank by vector similarity, and merge
    /// independent FTS / filename hits into one ordered list.
    fn search_with_semantic(
        &self,
        request: &SearchRequest,
        strategy: SearchStrategy,
        limit: usize,
        query: &str,
    ) -> JaymiResult<(Vec<SearchHit>, usize)> {
        let (mut hits, mut candidate_count) =
            self.search_with_content_fts(request, strategy, limit, query)?;

        let Some(deps) = &self.semantic else {
            return Ok((hits, candidate_count));
        };
        if !deps.provider.embedding_status().available {
            return Ok((hits, candidate_count));
        }

        let vectors = deps.provider.embed(&[query.to_string()])?;
        let Some(query_vec) = vectors.into_iter().next() else {
            return Ok((hits, candidate_count));
        };

        let similar = deps.database.search_embeddings_similar(
            &query_vec.values,
            deps.provider.model_id(),
            limit.saturating_mul(4).max(limit),
            SEMANTIC_MIN_SIMILARITY,
        )?;
        candidate_count = candidate_count.saturating_add(similar.len());

        let now = ranking_now_unix();
        let mut by_path: std::collections::BTreeMap<String, SearchHit> = hits
            .drain(..)
            .map(|hit| (hit.item_id.clone(), hit))
            .collect();

        for sim in similar {
            let semantic_raw = semantic_signal_from_similarity(sim.similarity);
            if semantic_raw == 0 {
                continue;
            }
            let path = PathBuf::from(&sim.source_id);
            let item = self.knowledge.get_by_path(&path)?;
            if let Some(item) = &item {
                if !passes_metadata(item, &request.metadata) {
                    continue;
                }
                if let Some(folder) = &request.folder {
                    let normalized = normalize_path(folder)?;
                    let folder_key = normalized.to_string_lossy().into_owned();
                    let item_key = item.path.to_string_lossy();
                    let in_folder = if request.folder_immediate {
                        item.parent
                            .as_ref()
                            .map(|parent| parent.to_string_lossy() == folder_key)
                            .unwrap_or(false)
                    } else {
                        item_key == folder_key
                            || item_key.starts_with(
                                &(folder_key.clone() + std::path::MAIN_SEPARATOR_STR),
                            )
                    };
                    if !in_folder {
                        continue;
                    }
                }
            } else if request.folder.is_some() || request.metadata.is_active() {
                // Folder / metadata filters require an inventory row.
                continue;
            }

            let mut signals = RankSignals {
                semantic: semantic_raw,
                ..RankSignals::default()
            };
            signals.recency = recency_score(item.as_ref().and_then(|value| value.modified), now);

            let title = self
                .content
                .as_ref()
                .and_then(|api| {
                    api.get_by_source_id(&sim.source_id)
                        .ok()
                        .flatten()
                        .and_then(|content| content.title)
                })
                .or_else(|| item.as_ref().map(|value| value.filename.clone()))
                .unwrap_or_else(|| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| sim.source_id.clone())
                });

            let preview = self.preview_for(&path);
            let semantic_hit = SearchHit {
                item_id: sim.source_id.clone(),
                title,
                path,
                score: fuse_relevance(&signals),
                signals,
                match_reason: MatchReason::Semantic,
                preview: preview.clone(),
                matching_section: None,
                snippet: preview,
                is_directory: item.map(|value| value.is_directory).unwrap_or(false),
            };

            if let Some(existing) = by_path.get_mut(&sim.source_id) {
                let prior = existing.match_reason.clone();
                existing.merge_signals(&semantic_hit.signals);
                if prior != MatchReason::Semantic {
                    existing.match_reason = MatchReason::Combined {
                        parts: vec![prior.as_str(), MatchReason::Semantic.as_str()],
                    };
                }
            } else {
                by_path.insert(sim.source_id, semantic_hit);
            }
        }

        hits = by_path.into_values().collect();
        sort_hits(&mut hits);
        hits.truncate(limit);
        Ok((hits, candidate_count))
    }

    /// Free-text / combined search: FTS over normalized content + filename inventory.
    ///
    /// Each strategy contributes independent signals; hybrid fusion produces one score.
    fn search_with_content_fts(
        &self,
        request: &SearchRequest,
        strategy: SearchStrategy,
        limit: usize,
        query: &str,
    ) -> JaymiResult<(Vec<SearchHit>, usize)> {
        let fetch_limit = limit.saturating_mul(4).max(limit).min(10_000);
        let mut by_path: std::collections::BTreeMap<String, SearchHit> =
            std::collections::BTreeMap::new();
        let now = ranking_now_unix();

        // Content full-text hits (words / phrases / exact matches).
        // When a folder is set, constrain FTS at the database layer for project isolation.
        if let Some(api) = &self.content {
            let path_prefix = match &request.folder {
                Some(folder) if !request.folder_immediate => {
                    Some(normalize_path(folder)?.to_string_lossy().into_owned())
                }
                _ => None,
            };
            let fts_hits = api.search_full_text_in_prefix(query, path_prefix.as_deref(), fetch_limit)?;
            for content_hit in fts_hits {
                let Some(ranked) = crate::content_rank::rank_content_match(
                    query,
                    content_hit.title.as_deref(),
                    &content_hit.plain_text,
                    &content_hit.sections,
                ) else {
                    continue;
                };

                let path = PathBuf::from(&content_hit.source_id);
                let item = self.knowledge.get_by_path(&path)?;
                if let Some(item) = &item {
                    if !passes_metadata(item, &request.metadata) {
                        continue;
                    }
                    if let Some(ext) = request
                        .extension
                        .as_ref()
                        .map(|value| value.trim().trim_start_matches('.'))
                        .filter(|value| !value.is_empty())
                    {
                        let want = ext.to_ascii_lowercase();
                        match &item.extension {
                            Some(have) if have.to_ascii_lowercase() == want => {}
                            _ => continue,
                        }
                    }
                    if let Some(folder) = &request.folder {
                        let normalized = normalize_path(folder)?;
                        let folder_key = normalized.to_string_lossy().into_owned();
                        let item_key = item.path.to_string_lossy();
                        let in_folder = if request.folder_immediate {
                            item.parent
                                .as_ref()
                                .map(|parent| parent.to_string_lossy() == folder_key)
                                .unwrap_or(false)
                        } else {
                            item_key == folder_key
                                || item_key.starts_with(
                                    &(folder_key.clone() + std::path::MAIN_SEPARATOR_STR),
                                )
                        };
                        if !in_folder {
                            continue;
                        }
                    }
                } else if request.extension.is_some()
                    || request.folder.is_some()
                    || request.metadata.is_active()
                {
                    // Combined filters require an inventory row.
                    continue;
                }

                let title = content_hit
                    .title
                    .clone()
                    .or_else(|| item.as_ref().map(|value| value.filename.clone()))
                    .unwrap_or_else(|| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| content_hit.source_id.clone())
                    });

                let mut signals = ranked.signals.clone();
                signals.recency =
                    recency_score(item.as_ref().and_then(|value| value.modified), now);

                let hit = SearchHit {
                    item_id: content_hit.source_id.clone(),
                    title,
                    path: path.clone(),
                    score: fuse_relevance(&signals),
                    signals,
                    match_reason: ranked.reason,
                    preview: ranked.snippet.clone().or_else(|| {
                        let text = content_hit.plain_text.trim();
                        if text.is_empty() {
                            None
                        } else {
                            Some(text.chars().take(PREVIEW_CHARS).collect())
                        }
                    }),
                    matching_section: ranked.matching_section,
                    snippet: ranked.snippet,
                    is_directory: item.map(|value| value.is_directory).unwrap_or(false),
                };
                by_path.insert(content_hit.source_id, hit);
            }
        }

        // Filename / path inventory matches (still useful alongside content).
        let items = self.fetch_candidates(request, strategy, limit)?;
        let candidate_count = items.len().saturating_add(by_path.len());
        for item in items {
            if let Some(hit) = self.score_item(&item, request, strategy)? {
                let key = item.path.to_string_lossy().into_owned();
                if let Some(existing) = by_path.get_mut(&key) {
                    let prior_reason = existing.match_reason.clone();
                    existing.merge_signals(&hit.signals);
                    if matches!(strategy, SearchStrategy::Combined)
                        || prior_reason != hit.match_reason
                    {
                        existing.match_reason = MatchReason::Combined {
                            parts: vec![prior_reason.as_str(), hit.match_reason.as_str()],
                        };
                    }
                    if existing.matching_section.is_none() {
                        existing.matching_section = hit.matching_section.clone();
                    }
                    if existing.snippet.is_none() {
                        existing.snippet = hit.snippet.clone();
                    }
                    if existing.preview.is_none() {
                        existing.preview = hit.preview.clone();
                    }
                } else {
                    by_path.insert(key, hit);
                }
            }
        }

        let mut hits: Vec<SearchHit> = by_path.into_values().collect();
        sort_hits(&mut hits);
        hits.truncate(limit);
        Ok((hits, candidate_count))
    }

    fn fetch_candidates(
        &self,
        request: &SearchRequest,
        strategy: SearchStrategy,
        limit: usize,
    ) -> JaymiResult<Vec<KnowledgeItem>> {
        let meta = &request.metadata;
        let fetch_limit = limit.saturating_mul(4).max(limit).min(10_000);

        if meta.recently_modified {
            return self.knowledge.recent(RecentKind::Modified, fetch_limit);
        }
        if meta.recently_created {
            return self.knowledge.recent(RecentKind::Created, fetch_limit);
        }

        let mut query = KnowledgeQuery {
            limit: Some(fetch_limit),
            ..KnowledgeQuery::default()
        };

        if let Some(folder) = &request.folder {
            let normalized = normalize_path(folder)?;
            let key = normalized.to_string_lossy().into_owned();
            if request.folder_immediate {
                query.parent = Some(key);
            } else {
                query.path_prefix = Some(key);
            }
        }

        if let Some(ext) = &request.extension {
            let trimmed = ext.trim().trim_start_matches('.').to_ascii_lowercase();
            if !trimmed.is_empty() {
                query.extension = Some(trimmed);
            }
        }

        if let Some(name) = &request.filename {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                query.name_contains = Some(trimmed.to_string());
            }
        } else if let Some(text) = &request.free_text {
            let trimmed = text.trim();
            if !trimmed.is_empty()
                && matches!(
                    strategy,
                    SearchStrategy::FreeText
                        | SearchStrategy::Semantic
                        | SearchStrategy::Combined
                )
            {
                query.name_contains = Some(trimmed.to_string());
            }
        }

        query.files_only = meta.files_only;
        query.directories_only = meta.directories_only;
        query.hidden_only = meta.hidden_only;
        query.empty_folders = meta.empty_folders;
        if meta.largest {
            query.files_only = true;
            query.sort = KnowledgeSort::Largest;
        }

        if matches!(
            strategy,
            SearchStrategy::FreeText | SearchStrategy::Semantic | SearchStrategy::Combined
        ) && request.free_text.is_some()
            && query.name_contains.is_some()
        {
            let primary = self.knowledge.query(query.clone())?;
            if !primary.is_empty() {
                return Ok(primary);
            }
            query.name_contains = None;
            query.limit = Some(fetch_limit);
            return self.knowledge.query(query);
        }

        self.knowledge.query(query)
    }

    fn rank_items(
        &self,
        items: Vec<KnowledgeItem>,
        request: &SearchRequest,
        strategy: SearchStrategy,
        limit: usize,
    ) -> JaymiResult<Vec<SearchHit>> {
        let mut hits = Vec::new();
        for item in items {
            if let Some(hit) = self.score_item(&item, request, strategy)? {
                hits.push(hit);
            }
        }
        sort_hits(&mut hits);
        hits.truncate(limit);
        Ok(hits)
    }

    fn score_item(
        &self,
        item: &KnowledgeItem,
        request: &SearchRequest,
        strategy: SearchStrategy,
    ) -> JaymiResult<Option<SearchHit>> {
        let filename_lower = item.filename.to_ascii_lowercase();
        let path_lower = item.path.to_string_lossy().to_ascii_lowercase();
        let mut signals = RankSignals::default();
        let mut reasons: Vec<String> = Vec::new();
        let mut primary_reason = MatchReason::Metadata;
        let mut preview = self.preview_for(&item.path);
        let mut matching_section = None;
        let mut snippet = None;

        if let Some(name) = request
            .filename
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            let needle = name.to_ascii_lowercase();
            if filename_lower == needle {
                signals.filename = signals.filename.saturating_add(100);
                primary_reason = MatchReason::FilenameExact;
                reasons.push("filename_exact".into());
            } else if filename_lower.contains(&needle) {
                signals.filename = signals.filename.saturating_add(80);
                primary_reason = MatchReason::FilenameContains;
                reasons.push("filename_contains".into());
            } else {
                return Ok(None);
            }
        }

        if let Some(ext) = request
            .extension
            .as_ref()
            .map(|value| value.trim().trim_start_matches('.'))
            .filter(|value| !value.is_empty())
        {
            let want = ext.to_ascii_lowercase();
            match &item.extension {
                Some(have) if have.to_ascii_lowercase() == want => {
                    signals.metadata = signals.metadata.saturating_add(70);
                    if reasons.is_empty() {
                        primary_reason = MatchReason::Extension;
                    }
                    reasons.push("extension".into());
                }
                _ => return Ok(None),
            }
        }

        if let Some(folder) = &request.folder {
            let normalized = normalize_path(folder)?;
            let folder_key = normalized.to_string_lossy().into_owned();
            let in_folder = if request.folder_immediate {
                item.parent
                    .as_ref()
                    .map(|parent| parent.to_string_lossy() == folder_key)
                    .unwrap_or(false)
            } else {
                let item_key = item.path.to_string_lossy();
                item_key == folder_key
                    || item_key.starts_with(&(folder_key.clone() + std::path::MAIN_SEPARATOR_STR))
            };
            if !in_folder {
                return Ok(None);
            }
            signals.metadata = signals
                .metadata
                .saturating_add(if request.folder_immediate { 60 } else { 50 });
            if reasons.is_empty() {
                primary_reason = MatchReason::Folder;
            }
            reasons.push("folder".into());
        }

        if let Some(text) = request
            .free_text
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            let needle = text.to_ascii_lowercase();
            let mut matched = false;
            if filename_lower == needle {
                signals.filename = signals.filename.saturating_add(100);
                primary_reason = MatchReason::FilenameExact;
                reasons.push("filename_exact".into());
                matched = true;
            } else if filename_lower.contains(&needle) || path_lower.contains(&needle) {
                signals.filename = signals.filename.saturating_add(80);
                primary_reason = MatchReason::FreeTextFilename;
                reasons.push("free_text_filename".into());
                matched = true;
            }
            // Content body matches are handled by FTS in search_with_content_fts.
            // Keep a lightweight preview check only when FTS is unavailable.
            if self.content.is_none() {
                if let Some(text_preview) = preview.as_ref() {
                    if text_preview.to_ascii_lowercase().contains(&needle) {
                        signals.full_text = signals.full_text.saturating_add(35);
                        if !matched {
                            primary_reason = MatchReason::FreeTextContent;
                        }
                        reasons.push("free_text_content".into());
                        matched = true;
                    }
                }
            }
            if !matched {
                return Ok(None);
            }
        }

        if !passes_metadata(item, &request.metadata) {
            return Ok(None);
        }

        if signals.filename == 0
            && signals.title == 0
            && signals.metadata == 0
            && signals.full_text == 0
        {
            signals.metadata = metadata_base_score(&request.metadata);
            primary_reason = if request.metadata.collection.is_some() {
                MatchReason::Collection
            } else {
                MatchReason::Metadata
            };
            reasons.push(primary_reason.as_str());
        }

        signals.recency = recency_score(item.modified, ranking_now_unix());

        if matches!(strategy, SearchStrategy::Combined) && reasons.len() > 1 {
            primary_reason = MatchReason::Combined {
                parts: reasons.clone(),
            };
        }

        let title = self
            .content
            .as_ref()
            .and_then(|api| {
                api.get_by_source_id(&item.path.to_string_lossy())
                    .ok()
                    .flatten()
                    .and_then(|content| {
                        matching_section = content
                            .enrichment
                            .sections
                            .first()
                            .map(|section| section.title.clone());
                        content.title
                    })
            })
            .unwrap_or_else(|| item.filename.clone());

        if preview.is_none() {
            preview = self.preview_for(&item.path);
        }
        if snippet.is_none() {
            snippet = preview.clone();
        }

        Ok(Some(SearchHit {
            item_id: item.path.to_string_lossy().into_owned(),
            title,
            path: item.path.clone(),
            score: fuse_relevance(&signals),
            signals,
            match_reason: primary_reason,
            preview,
            matching_section,
            snippet,
            is_directory: item.is_directory,
        }))
    }

    fn preview_for(&self, path: &Path) -> Option<String> {
        let api = self.content.as_ref()?;
        let source_id = path.to_string_lossy();
        let content = api.get_by_source_id(&source_id).ok().flatten()?;
        let text = content.plain_text.trim();
        if text.is_empty() {
            return None;
        }
        let preview: String = text.chars().take(PREVIEW_CHARS).collect();
        Some(preview)
    }

    fn snapshot_stats(&self) -> JaymiResult<SearchStats> {
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| JaymiError::new("search engine stats lock poisoned"))?;
        let average = if runtime.search_count == 0 {
            0
        } else {
            runtime.total_duration_ms / runtime.search_count
        };
        Ok(SearchStats {
            search_count: runtime.search_count,
            average_query_time_ms: average,
            last_strategy: runtime.last_strategy.clone(),
            last_duration_ms: runtime.last_duration_ms,
            last_hit_count: runtime.last_hit_count,
            last_citation_count: runtime.last_citation_count,
            citations_generated: runtime.citations_generated,
        })
    }
}

fn sort_hits(hits: &mut [SearchHit]) {
    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.title.cmp(&right.title))
    });
}

fn passes_metadata(item: &KnowledgeItem, meta: &MetadataFilters) -> bool {
    if meta.files_only && item.is_directory {
        return false;
    }
    if meta.directories_only && !item.is_directory {
        return false;
    }
    if meta.hidden_only && !item.hidden {
        return false;
    }
    true
}

fn metadata_base_score(meta: &MetadataFilters) -> u32 {
    if meta.largest {
        45
    } else if meta.recently_modified || meta.recently_created {
        40
    } else if meta.hidden_only || meta.empty_folders {
        35
    } else {
        30
    }
}

impl SearchEngineApi for SearchEngine {
    fn search(&self, request: &SearchRequest) -> JaymiResult<SearchResults> {
        let results = self.execute(request)?;
        jaymi_logging::info(
            "search",
            format!(
                "strategy={} hits={} citations={} duration_ms={}",
                results.strategy,
                results.len(),
                results.citations().len(),
                results.duration_ms
            ),
        );
        Ok(results)
    }

    fn stats(&self) -> JaymiResult<SearchStats> {
        self.ensure_ready()?;
        self.snapshot_stats()
    }

    fn health(&self) -> JaymiResult<SearchHealth> {
        let report = self.health_check();
        let statistics = self.snapshot_stats().unwrap_or_default();
        let detail = if !report.initialized {
            "search engine is not initialized".to_string()
        } else {
            format!(
                "searches={} avg_ms={} strategy={} citations={} last_citations={}",
                statistics.search_count,
                statistics.average_query_time_ms,
                statistics
                    .last_strategy
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                statistics.citations_generated,
                statistics
                    .last_citation_count
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string())
            )
        };
        Ok(SearchHealth {
            initialized: report.initialized,
            healthy: report.healthy && report.initialized,
            version: report.version,
            detail,
            statistics,
        })
    }
}

impl Lifecycle for SearchEngine {
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
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.initialized = false;
        if let Ok(mut runtime) = self.runtime.lock() {
            *runtime = RuntimeStats::default();
        }
        Ok(())
    }
}

/// Map a discovery query kind onto a Search Engine request.
pub fn request_from_discovery(kind: &jaymi_core::DiscoveryQueryKind) -> SearchRequest {
    use jaymi_core::DiscoveryQueryKind;
    match kind {
        DiscoveryQueryKind::All => SearchRequest {
            limit: Some(10_000),
            ..SearchRequest::default()
        },
        DiscoveryQueryKind::ByExtension { extension } => SearchRequest::extension(extension),
        DiscoveryQueryKind::ByFolder { path, immediate } => {
            SearchRequest::folder(path.clone(), *immediate)
        }
        DiscoveryQueryKind::Collections => SearchRequest {
            metadata: MetadataFilters {
                list_collections: true,
                ..MetadataFilters::default()
            },
            limit: Some(1_000),
            ..SearchRequest::default()
        },
        DiscoveryQueryKind::ByCollection { name, immediate } => SearchRequest {
            metadata: MetadataFilters {
                collection: Some(name.clone()),
                collection_immediate: *immediate,
                ..MetadataFilters::default()
            },
            limit: Some(10_000),
            ..SearchRequest::default()
        },
        DiscoveryQueryKind::RecentlyModified => SearchRequest {
            metadata: MetadataFilters {
                recently_modified: true,
                ..MetadataFilters::default()
            },
            limit: Some(100),
            ..SearchRequest::default()
        },
        DiscoveryQueryKind::RecentlyCreated => SearchRequest {
            metadata: MetadataFilters {
                recently_created: true,
                ..MetadataFilters::default()
            },
            limit: Some(100),
            ..SearchRequest::default()
        },
        DiscoveryQueryKind::Largest => SearchRequest {
            metadata: MetadataFilters {
                largest: true,
                files_only: true,
                ..MetadataFilters::default()
            },
            limit: Some(100),
            ..SearchRequest::default()
        },
        DiscoveryQueryKind::Hidden => SearchRequest {
            metadata: MetadataFilters {
                hidden_only: true,
                ..MetadataFilters::default()
            },
            limit: Some(10_000),
            ..SearchRequest::default()
        },
        DiscoveryQueryKind::EmptyFolders => SearchRequest {
            metadata: MetadataFilters {
                empty_folders: true,
                directories_only: true,
                ..MetadataFilters::default()
            },
            limit: Some(10_000),
            ..SearchRequest::default()
        },
    }
}

/// Convert search hits into filesystem entries for Planner tool output.
pub fn hits_to_entries(hits: &[SearchHit]) -> Vec<jaymi_core::FileEntry> {
    use jaymi_core::{EntryType, FileEntry};
    hits.iter()
        .map(|hit| {
            let entry_type = if hit.is_directory {
                EntryType::Directory
            } else {
                EntryType::File
            };
            FileEntry::new(hit.title.clone(), entry_type, hit.path.clone(), 0, None)
        })
        .collect()
}
