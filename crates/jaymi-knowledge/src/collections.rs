//! Logical collections over indexed knowledge.

use std::collections::HashMap;
use std::path::PathBuf;

use jaymi_core::JaymiResult;

use crate::path::normalize_path;
use crate::stats::CollectionStats;
use crate::types::{KnowledgeQuery, KnowledgeSort};
use crate::SqliteKnowledgeStore;

/// Stable identifier for a known collection kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionId {
    /// User desktop folder.
    Desktop,
    /// Downloads folder.
    Downloads,
    /// Documents folder.
    Documents,
    /// Projects / developer workspaces.
    Projects,
    /// Pictures / photos.
    Pictures,
    /// Music / audio library.
    Music,
    /// Movies / videos.
    Movies,
    /// Applications / programs.
    Applications,
}

impl CollectionId {
    /// Canonical lowercase slug used in queries and diagnostics.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Downloads => "downloads",
            Self::Documents => "documents",
            Self::Projects => "projects",
            Self::Pictures => "pictures",
            Self::Music => "music",
            Self::Movies => "movies",
            Self::Applications => "applications",
        }
    }

    /// Human-readable label.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Downloads => "Downloads",
            Self::Documents => "Documents",
            Self::Projects => "Projects",
            Self::Pictures => "Pictures",
            Self::Music => "Music",
            Self::Movies => "Movies",
            Self::Applications => "Applications",
        }
    }

    /// Parse a user-facing name or slug into a collection id.
    pub fn parse(name: &str) -> Option<Self> {
        match jaymi_core::parse_collection_slug(name)? {
            "desktop" => Some(Self::Desktop),
            "downloads" => Some(Self::Downloads),
            "documents" => Some(Self::Documents),
            "projects" => Some(Self::Projects),
            "pictures" => Some(Self::Pictures),
            "music" => Some(Self::Music),
            "movies" => Some(Self::Movies),
            "applications" => Some(Self::Applications),
            _ => None,
        }
    }
}

/// One active collection rooted at an indexed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    /// Collection kind.
    pub id: CollectionId,
    /// Display name.
    pub name: String,
    /// Canonical root path in the inventory.
    pub root: PathBuf,
    /// Inventoried entries under the root (including the root when present).
    pub item_count: u64,
}

#[derive(Clone, Copy)]
struct CollectionDef {
    id: CollectionId,
    display_name: &'static str,
    match_names: &'static [&'static str],
}

const ALL_COLLECTIONS: &[CollectionDef] = &[
    CollectionDef {
        id: CollectionId::Desktop,
        display_name: "Desktop",
        match_names: &["Desktop"],
    },
    CollectionDef {
        id: CollectionId::Downloads,
        display_name: "Downloads",
        match_names: &["Downloads"],
    },
    CollectionDef {
        id: CollectionId::Documents,
        display_name: "Documents",
        match_names: &["Documents", "Docs"],
    },
    CollectionDef {
        id: CollectionId::Projects,
        display_name: "Projects",
        match_names: &["Projects", "Developer"],
    },
    CollectionDef {
        id: CollectionId::Pictures,
        display_name: "Pictures",
        match_names: &["Pictures", "Photos"],
    },
    CollectionDef {
        id: CollectionId::Music,
        display_name: "Music",
        match_names: &["Music"],
    },
    CollectionDef {
        id: CollectionId::Movies,
        display_name: "Movies",
        match_names: &["Movies", "Videos"],
    },
    CollectionDef {
        id: CollectionId::Applications,
        display_name: "Applications",
        match_names: &["Applications", "Apps"],
    },
];

impl SqliteKnowledgeStore {
    pub(crate) fn list_collections_inner(&self) -> JaymiResult<Vec<Collection>> {
        let mut collections = Vec::new();
        for def in ALL_COLLECTIONS {
            if let Some(collection) = self.resolve_collection_def(def)? {
                collections.push(collection);
            }
        }
        collections.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(collections)
    }

    pub(crate) fn resolve_collection_inner(&self, name: &str) -> JaymiResult<Option<Collection>> {
        let Some(id) = CollectionId::parse(name) else {
            return Ok(None);
        };
        let def = ALL_COLLECTIONS
            .iter()
            .find(|candidate| candidate.id == id)
            .expect("collection id always has a definition");
        self.resolve_collection_def(def)
    }

    pub(crate) fn collection_stats_inner(&self) -> JaymiResult<CollectionStats> {
        let collections = self.list_collections_inner()?;
        Ok(CollectionStats {
            collection_count: collections.len() as u64,
            total_items: collections.iter().map(|item| item.item_count).sum(),
            names: collections.into_iter().map(|item| item.name).collect(),
        })
    }

    fn resolve_collection_def(&self, def: &CollectionDef) -> JaymiResult<Option<Collection>> {
        let mut ranked: HashMap<String, (PathBuf, u64, u8)> = HashMap::new();

        let mut candidates = candidate_roots(def);
        for path in self.inventory_named_directories(def)? {
            candidates.push((path, 2));
        }

        for (path, priority) in candidates {
            let Ok(normalized) = normalize_path(&path) else {
                continue;
            };
            let key = normalized.to_string_lossy().into_owned();
            if let Some((_, _, existing_priority)) = ranked.get(&key) {
                if priority >= *existing_priority {
                    continue;
                }
            }
            let count = self.coverage_count(&key)?;
            if count == 0 {
                continue;
            }
            ranked.insert(key, (normalized, count, priority));
        }

        let best = ranked.into_values().max_by(|left, right| {
            right
                .2
                .cmp(&left.2)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.0.cmp(&right.0))
        });

        Ok(best.map(|(root, item_count, _)| Collection {
            id: def.id,
            name: def.display_name.to_string(),
            root,
            item_count,
        }))
    }

    fn inventory_named_directories(&self, def: &CollectionDef) -> JaymiResult<Vec<PathBuf>> {
        let mut matches = Vec::new();
        for name in def.match_names {
            let rows = self.query_untracked(KnowledgeQuery {
                name_contains: Some((*name).to_string()),
                directories_only: true,
                limit: Some(200),
                ..KnowledgeQuery::default()
            })?;
            for row in rows {
                if row.filename.eq_ignore_ascii_case(name) {
                    matches.push(row.path);
                }
            }
        }
        Ok(matches)
    }

    fn coverage_count(&self, root_key: &str) -> JaymiResult<u64> {
        let items = self.query_untracked(KnowledgeQuery {
            path_prefix: Some(root_key.to_string()),
            sort: KnowledgeSort::Path,
            limit: None,
            ..KnowledgeQuery::default()
        })?;
        Ok(items.len() as u64)
    }
}

fn candidate_roots(def: &CollectionDef) -> Vec<(PathBuf, u8)> {
    let mut candidates = Vec::new();

    for path in os_well_known_paths(def.id) {
        candidates.push((path, 0));
    }

    if let Some(home) = dirs::home_dir() {
        for name in def.match_names {
            candidates.push((home.join(name), 1));
        }
        if def.id == CollectionId::Projects {
            candidates.push((home.join("src"), 1));
        }
        if def.id == CollectionId::Applications {
            candidates.push((home.join("Applications"), 1));
        }
    }

    if def.id == CollectionId::Applications && cfg!(target_os = "macos") {
        candidates.push((PathBuf::from("/Applications"), 0));
    }

    candidates
}

fn os_well_known_paths(id: CollectionId) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    match id {
        CollectionId::Desktop => {
            if let Some(path) = dirs::desktop_dir() {
                paths.push(path);
            }
        }
        CollectionId::Downloads => {
            if let Some(path) = dirs::download_dir() {
                paths.push(path);
            }
        }
        CollectionId::Documents => {
            if let Some(path) = dirs::document_dir() {
                paths.push(path);
            }
        }
        CollectionId::Pictures => {
            if let Some(path) = dirs::picture_dir() {
                paths.push(path);
            }
        }
        CollectionId::Music => {
            if let Some(path) = dirs::audio_dir() {
                paths.push(path);
            }
        }
        CollectionId::Movies => {
            if let Some(path) = dirs::video_dir() {
                paths.push(path);
            }
        }
        CollectionId::Projects | CollectionId::Applications => {}
    }
    paths
}
