//! Known logical collection names shared by Planner routing and discovery.

/// Canonical collection slugs Jaymi understands.
pub const COLLECTION_SLUGS: &[&str] = &[
    "desktop",
    "downloads",
    "documents",
    "projects",
    "pictures",
    "music",
    "movies",
    "applications",
];

/// Resolve a user-facing name to a canonical collection slug.
pub fn parse_collection_slug(name: &str) -> Option<&'static str> {
    let trimmed = name.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    for (slug, aliases) in COLLECTION_ALIASES {
        if lower == *slug || aliases.iter().any(|alias| *alias == lower) {
            return Some(*slug);
        }
    }
    None
}

const COLLECTION_ALIASES: &[(&str, &[&str])] = &[
    ("desktop", &["desktop"]),
    ("downloads", &["download", "downloads"]),
    ("documents", &["document", "documents", "docs"]),
    ("projects", &["project", "projects", "developer"]),
    ("pictures", &["picture", "pictures", "photos", "photo"]),
    ("music", &["music", "audio"]),
    ("movies", &["movie", "movies", "video", "videos"]),
    ("applications", &["application", "applications", "apps", "programs"]),
];

/// True when `name` refers to a known collection.
pub fn is_known_collection_name(name: &str) -> bool {
    parse_collection_slug(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_aliases() {
        assert_eq!(parse_collection_slug("Downloads"), Some("downloads"));
        assert_eq!(parse_collection_slug("download"), Some("downloads"));
        assert_eq!(parse_collection_slug("projects"), Some("projects"));
        assert_eq!(parse_collection_slug("/tmp/Downloads"), None);
    }
}
