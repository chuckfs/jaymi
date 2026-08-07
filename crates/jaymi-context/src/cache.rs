//! ContextBundle caching — reuse recent assemblies without changing correctness.
//!
//! Entries are keyed by project, workspace, conversation **revision**, session
//! fingerprint (diagnostics / editor / permissions), and request type (plus a
//! request fingerprint and invalidation epoch so memory / filesystem / index
//! mutations cannot return stale bundles).
//!
//! Reuse is entirely ContextEngine-owned. Planner asks for a fresh assemble via
//! [`crate::ContextEngine::request_fresh_context`] and never touches keys,
//! epochs, or LRU.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use jaymi_core::UserRequest;
use jaymi_memory_engine::MemoryEngineApi;
use jaymi_project_engine::ProjectEngineApi;

use crate::bundle::{ContextBundle, ContextSessionInputs};
use crate::budget::ContextBudgetConfig;
use crate::inspector::ContextInspectorReport;
use crate::relevance::{RelevanceSignals, RequestKind};

/// Default maximum cached bundles.
pub const DEFAULT_CACHE_CAPACITY: usize = 32;

/// Identity backends used only to build cache keys (not for assemble work).
#[derive(Clone)]
pub struct CacheIdentity {
    /// Memory Engine — active conversation id + revision.
    pub memory: std::sync::Arc<dyn MemoryEngineApi>,
    /// Project Engine — open project id.
    pub projects: std::sync::Arc<dyn ProjectEngineApi>,
}

/// Cache key dimensions requested by product + correctness fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContextCacheKey {
    /// Invalidation epoch — bumps on file/project/workspace/conversation/index changes.
    pub epoch: u64,
    /// Active project id, when any.
    pub project_id: Option<String>,
    /// Active UX workspace kind.
    pub workspace_kind: Option<String>,
    /// Active conversation id, when any.
    pub conversation_id: Option<String>,
    /// Conversational revision fingerprint (`updated_at` + message count).
    ///
    /// Detects unchanged conversational state without loading the transcript.
    pub conversation_revision: u64,
    /// Fingerprint of session diagnostics / editor / permissions / search hits.
    pub session_fingerprint: u64,
    /// Active editor file path, when any.
    pub active_file: Option<String>,
    /// Canonical Intent id label (`read_file`, `search_knowledge`, `unknown`, …).
    pub request_type: String,
    /// Fingerprint of request content + structured fields (correctness).
    pub request_fingerprint: u64,
    /// Relevance threshold in effect when assembled.
    pub relevance_threshold: u8,
    /// Character budget in effect when assembled.
    pub budget_max_characters: usize,
    /// Fingerprint of active context policies.
    pub policy_fingerprint: u64,
    /// Fingerprint of Planner AssembleHints (intent + capability ids).
    pub hints_fingerprint: u64,
}

impl ContextCacheKey {
    /// Build a key from live identity + session + request.
    pub fn build(
        epoch: u64,
        identity: Option<&CacheIdentity>,
        session: &ContextSessionInputs,
        request: &UserRequest,
        signals: &RelevanceSignals,
        relevance_threshold: u8,
        budget: &ContextBudgetConfig,
        policy_fingerprint: u64,
        hints_fingerprint: u64,
    ) -> Self {
        let project_id = identity.and_then(|id| id.projects.open_project_id());
        let conversation_id = identity.and_then(|id| id.memory.active_conversation_id());
        let conversation_revision =
            conversation_revision_fingerprint(identity, conversation_id.as_deref());
        Self {
            epoch,
            project_id,
            workspace_kind: session.workspace_kind.clone(),
            conversation_id,
            conversation_revision,
            session_fingerprint: fingerprint_session(session),
            active_file: session.current_file.path.clone(),
            request_type: signals.intent.as_str().to_string(),
            request_fingerprint: fingerprint_request(request, signals.request_kind),
            relevance_threshold,
            budget_max_characters: budget.max_characters,
            policy_fingerprint,
            hints_fingerprint,
        }
    }
}

/// Hash active conversation `(updated_at, message_count)` into a stable u64.
fn conversation_revision_fingerprint(
    identity: Option<&CacheIdentity>,
    conversation_id: Option<&str>,
) -> u64 {
    let Some(identity) = identity else {
        return 0;
    };
    let Some(conversation_id) = conversation_id else {
        return 0;
    };
    let Ok(Some((updated_at, message_count))) =
        identity.memory.conversation_revision(conversation_id)
    else {
        return 0;
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    updated_at.hash(&mut hasher);
    message_count.hash(&mut hasher);
    hasher.finish()
}

/// Fingerprint session inputs that affect providers (diagnostics, editor, …).
pub fn fingerprint_session(session: &ContextSessionInputs) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session.workspace_kind.hash(&mut hasher);
    session.project_open.hash(&mut hasher);
    session.project_indexed_documents.hash(&mut hasher);
    session.current_file.path.hash(&mut hasher);
    session.current_file.dirty.hash(&mut hasher);
    session.current_file.language.hash(&mut hasher);
    session.current_selection.path.hash(&mut hasher);
    session.current_selection.start_line.hash(&mut hasher);
    session.current_selection.start_column.hash(&mut hasher);
    session.current_selection.end_line.hash(&mut hasher);
    session.current_selection.end_column.hash(&mut hasher);
    session.current_selection.text.hash(&mut hasher);
    for file in &session.open_files.files {
        file.path.hash(&mut hasher);
        file.dirty.hash(&mut hasher);
        file.active.hash(&mut hasher);
    }
    for diag in &session.diagnostics.diagnostics {
        diag.path.hash(&mut hasher);
        diag.severity.hash(&mut hasher);
        diag.message.hash(&mut hasher);
        diag.line.hash(&mut hasher);
        diag.column.hash(&mut hasher);
        diag.source.hash(&mut hasher);
    }
    session.git_status.is_repository.hash(&mut hasher);
    session.git_status.branch.hash(&mut hasher);
    session.git_status.summary.hash(&mut hasher);
    session.git_status.modified_count.hash(&mut hasher);
    session.git_status.staged_count.hash(&mut hasher);
    session.git_status.untracked_count.hash(&mut hasher);
    session.git_status.conflict_count.hash(&mut hasher);
    session.git_status.head_sha.hash(&mut hasher);
    session.git_status.head_short.hash(&mut hasher);
    for path in &session.git_status.dirty_paths {
        path.hash(&mut hasher);
    }
    for path in &session.git_status.staged_paths {
        path.hash(&mut hasher);
    }
    for path in &session.git_status.untracked_paths {
        path.hash(&mut hasher);
    }
    for path in &session.git_status.conflict_paths {
        path.hash(&mut hasher);
    }
    for commit in &session.git_status.recent_commits {
        commit.sha.hash(&mut hasher);
        commit.short_sha.hash(&mut hasher);
        commit.subject.hash(&mut hasher);
        commit.author.hash(&mut hasher);
        commit.relative_time.hash(&mut hasher);
    }
    for path in &session.git_status.sample_paths {
        path.hash(&mut hasher);
    }
    session.workspace_inventory.root.hash(&mut hasher);
    session.workspace_inventory.file_count.hash(&mut hasher);
    session.workspace_inventory.directory_count.hash(&mut hasher);
    session.workspace_inventory.status.hash(&mut hasher);
    for path in &session.workspace_inventory.sample_paths {
        path.hash(&mut hasher);
    }
    for entry in &session.file_summaries.entries {
        entry.path.hash(&mut hasher);
        entry.language.hash(&mut hasher);
        entry.line_count.hash(&mut hasher);
        entry.summary.hash(&mut hasher);
    }
    for entry in &session.permissions.entries {
        entry.category.hash(&mut hasher);
        entry.action.hash(&mut hasher);
        entry.decision.hash(&mut hasher);
        entry.resource.hash(&mut hasher);
    }
    for hit in &session.search_hits {
        hit.item_id.hash(&mut hasher);
        hit.title.hash(&mut hasher);
        hit.path.hash(&mut hasher);
        hit.score.hash(&mut hasher);
        hit.match_reason.hash(&mut hasher);
        hit.preview.hash(&mut hasher);
        hit.line.hash(&mut hasher);
        hit.column.hash(&mut hasher);
    }
    for id in &session.approved_context_providers {
        id.hash(&mut hasher);
    }
    for id in &session.active_capabilities.capability_ids {
        id.hash(&mut hasher);
    }
    if let Some(snapshot) = &session.workspace_snapshot {
        snapshot.hash(&mut hasher);
    }
    if let Some(snapshot) = &session.editor_snapshot {
        snapshot.hash(&mut hasher);
    }
    if let Some(snapshot) = &session.project_snapshot {
        snapshot.hash(&mut hasher);
    }
    if let Some(snapshot) = &session.git_snapshot {
        snapshot.hash(&mut hasher);
    }
    if let Some(snapshot) = &session.runtime_snapshot {
        snapshot.hash(&mut hasher);
    }
    if let Some(snapshot) = &session.workspace_memory_snapshot {
        snapshot.hash(&mut hasher);
    }
    hasher.finish()
}

/// Fingerprint request content and structured fields that affect providers.
pub fn fingerprint_request(request: &UserRequest, kind: RequestKind) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    kind.as_str().hash(&mut hasher);
    request.content.hash(&mut hasher);
    hash_opt_path(&mut hasher, request.directory.as_ref());
    hash_opt_path(&mut hasher, request.project_tree.as_ref());
    hash_opt_path(&mut hasher, request.file.as_ref());
    if let Some(write) = &request.write_file {
        write.path.hash(&mut hasher);
        write.content.hash(&mut hasher);
    }
    if let Some(manage) = &request.manage_path {
        manage.command.hash(&mut hasher);
        manage.path.hash(&mut hasher);
        hash_opt_path(&mut hasher, manage.destination.as_ref());
    }
    if let Some(search) = &request.search {
        search.free_text.hash(&mut hasher);
        search.filename.hash(&mut hasher);
    }
    if let Some(pk) = &request.project_knowledge {
        pk.project_id.hash(&mut hasher);
        pk.text.hash(&mut hasher);
        pk.limit.hash(&mut hasher);
    }
    request.open_project_id.hash(&mut hasher);
    request.close_project.hash(&mut hasher);
    request.discover.hash(&mut hasher);
    if let Some(kind) = &request.discovery_kind {
        format!("{kind:?}").hash(&mut hasher);
    }
    hash_opt_path(&mut hasher, request.index_root.as_ref());
    if request.terminal.is_some() {
        format!("{:?}", request.terminal).hash(&mut hasher);
    }
    if request.git.is_some() {
        format!("{:?}", request.git).hash(&mut hasher);
    }
    if request.lsp.is_some() {
        format!("{:?}", request.lsp).hash(&mut hasher);
    }
    hasher.finish()
}

fn hash_opt_path(hasher: &mut impl Hasher, path: Option<&std::path::PathBuf>) {
    if let Some(path) = path {
        path.hash(hasher);
    } else {
        0u8.hash(hasher);
    }
}

/// Cached assemble payload (bundle + inspector snapshot).
#[derive(Debug, Clone)]
pub struct ContextCacheEntry {
    /// Assembled bundle (restamped on hit).
    pub bundle: ContextBundle,
    /// Inspector decisions from the original miss assemble.
    pub inspection: ContextInspectorReport,
}

/// Cache statistics for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextCacheStats {
    /// Successful lookups.
    pub hits: u64,
    /// Misses that triggered a full assemble.
    pub misses: u64,
    /// Explicit invalidations / epoch bumps.
    pub invalidations: u64,
    /// Current epoch.
    pub epoch: u64,
    /// Entries currently stored.
    pub entries: usize,
    /// Last invalidation reason, when any.
    pub last_invalidation_reason: Option<String>,
}

/// LRU ContextBundle cache.
#[derive(Debug, Default)]
pub struct ContextBundleCache {
    capacity: usize,
    epoch: u64,
    entries: HashMap<ContextCacheKey, ContextCacheEntry>,
    order: VecDeque<ContextCacheKey>,
    hits: u64,
    misses: u64,
    invalidations: u64,
    last_invalidation_reason: Option<String>,
}

impl ContextBundleCache {
    /// Create a cache with the given capacity (minimum 1).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            ..Self::default()
        }
    }

    /// Current invalidation epoch.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Lookup a cached entry.
    pub fn get(&mut self, key: &ContextCacheKey) -> Option<ContextCacheEntry> {
        if let Some(entry) = self.entries.get(key).cloned() {
            self.hits = self.hits.saturating_add(1);
            // Refresh LRU order.
            if let Some(index) = self.order.iter().position(|item| item == key) {
                if let Some(item) = self.order.remove(index) {
                    self.order.push_back(item);
                }
            }
            Some(entry)
        } else {
            self.misses = self.misses.saturating_add(1);
            None
        }
    }

    /// Store an entry under `key`.
    pub fn insert(&mut self, key: ContextCacheKey, entry: ContextCacheEntry) {
        if self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), entry);
            if let Some(index) = self.order.iter().position(|item| item == &key) {
                if let Some(item) = self.order.remove(index) {
                    self.order.push_back(item);
                }
            }
            return;
        }
        while self.entries.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, entry);
    }

    /// Drop all entries and bump the epoch.
    pub fn invalidate(&mut self, reason: impl Into<String>) {
        self.entries.clear();
        self.order.clear();
        self.epoch = self.epoch.saturating_add(1);
        self.invalidations = self.invalidations.saturating_add(1);
        self.last_invalidation_reason = Some(reason.into());
    }

    /// Snapshot stats for diagnostics.
    pub fn stats(&self) -> ContextCacheStats {
        ContextCacheStats {
            hits: self.hits,
            misses: self.misses,
            invalidations: self.invalidations,
            epoch: self.epoch,
            entries: self.entries.len(),
            last_invalidation_reason: self.last_invalidation_reason.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextBundle;

    fn sample_key(epoch: u64, request_type: &str) -> ContextCacheKey {
        ContextCacheKey {
            epoch,
            project_id: Some("proj".into()),
            workspace_kind: Some("coding".into()),
            conversation_id: Some("conv".into()),
            conversation_revision: 1,
            session_fingerprint: 2,
            active_file: Some("/tmp/a.rs".into()),
            request_type: request_type.into(),
            request_fingerprint: 1,
            relevance_threshold: 40,
            budget_max_characters: 32_000,
            policy_fingerprint: 0,
            hints_fingerprint: 0,
        }
    }

    #[test]
    fn cache_hit_and_invalidate() {
        let mut cache = ContextBundleCache::with_capacity(2);
        let key = sample_key(0, "chat");
        cache.insert(
            key.clone(),
            ContextCacheEntry {
                bundle: ContextBundle::default(),
                inspection: ContextInspectorReport::default(),
            },
        );
        assert!(cache.get(&key).is_some());
        assert_eq!(cache.stats().hits, 1);
        cache.invalidate("project_changed");
        assert!(cache.get(&key).is_none());
        assert_eq!(cache.stats().epoch, 1);
        assert_eq!(cache.stats().invalidations, 1);
    }

    #[test]
    fn lru_evicts_oldest() {
        let mut cache = ContextBundleCache::with_capacity(2);
        let a = sample_key(0, "a");
        let b = sample_key(0, "b");
        let c = sample_key(0, "c");
        let entry = || ContextCacheEntry {
            bundle: ContextBundle::default(),
            inspection: ContextInspectorReport::default(),
        };
        cache.insert(a.clone(), entry());
        cache.insert(b.clone(), entry());
        cache.insert(c.clone(), entry());
        assert!(cache.get(&a).is_none());
        assert!(cache.get(&b).is_some());
        assert!(cache.get(&c).is_some());
    }

    #[test]
    fn session_fingerprint_changes_with_diagnostics() {
        let mut a = ContextSessionInputs::default();
        let mut b = ContextSessionInputs::default();
        assert_eq!(fingerprint_session(&a), fingerprint_session(&b));
        b.diagnostics.diagnostics.push(crate::BundleDiagnostic {
            path: Some("/x.rs".into()),
            severity: "error".into(),
            message: "boom".into(),
            line: Some(1),
            column: Some(0),
            source: None,
        });
        assert_ne!(fingerprint_session(&a), fingerprint_session(&b));
        a.diagnostics = b.diagnostics.clone();
        assert_eq!(fingerprint_session(&a), fingerprint_session(&b));
    }
}
