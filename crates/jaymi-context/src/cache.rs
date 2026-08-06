//! ContextBundle caching — reuse recent assemblies without changing correctness.
//!
//! Entries are keyed by project, workspace, conversation, active file, and
//! request type (plus a request fingerprint and invalidation epoch so memory /
//! filesystem / index mutations cannot return stale bundles).

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
    /// Memory Engine — active conversation id.
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
    /// Active editor file path, when any.
    pub active_file: Option<String>,
    /// Coarse request type (`chat`, `file_read`, `search`, …).
    pub request_type: String,
    /// Fingerprint of request content + structured fields (correctness).
    pub request_fingerprint: u64,
    /// Relevance threshold in effect when assembled.
    pub relevance_threshold: u8,
    /// Character budget in effect when assembled.
    pub budget_max_characters: usize,
    /// Fingerprint of active context policies.
    pub policy_fingerprint: u64,
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
    ) -> Self {
        let project_id = identity.and_then(|id| id.projects.open_project_id());
        let conversation_id = identity.and_then(|id| id.memory.active_conversation_id());
        Self {
            epoch,
            project_id,
            workspace_kind: session.workspace_kind.clone(),
            conversation_id,
            active_file: session.current_file.path.clone(),
            request_type: signals.request_kind.as_str().to_string(),
            request_fingerprint: fingerprint_request(request, signals.request_kind),
            relevance_threshold,
            budget_max_characters: budget.max_characters,
            policy_fingerprint,
        }
    }
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
            active_file: Some("/tmp/a.rs".into()),
            request_type: request_type.into(),
            request_fingerprint: 1,
            relevance_threshold: 40,
            budget_max_characters: 32_000,
            policy_fingerprint: 0,
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
}
