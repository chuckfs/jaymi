//! Extension-based icons for explorer rows (no giant match on every call site).

use jaymi_capabilities::ExplorerNode;

/// Folder glyph pair: collapsed / expanded prefix + folder.
pub fn folder_icon(expanded: bool) -> &'static str {
    if expanded {
        "▾📂"
    } else {
        "▸📁"
    }
}

/// File glyph based on basename / extension.
pub fn file_icon(node: &ExplorerNode) -> &'static str {
    icon_for_name(&node.name)
}

fn icon_for_name(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if let Some(icon) = special_basename_icon(&lower) {
        return icon;
    }
    let ext = std::path::Path::new(&lower)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    extension_icon(ext)
}

fn special_basename_icon(name: &str) -> Option<&'static str> {
    const SPECIAL: &[(&str, &str)] = &[
        ("dockerfile", "🐳"),
        ("makefile", "🔧"),
        ("gnumakefile", "🔧"),
        ("cmakelists.txt", "🔧"),
        ("cargo.toml", "📦"),
        ("cargo.lock", "📦"),
        ("package.json", "📦"),
        ("readme", "📘"),
        ("readme.md", "📘"),
        ("license", "⚖️"),
        ("licence", "⚖️"),
    ];
    SPECIAL
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, icon)| *icon)
}

fn extension_icon(ext: &str) -> &'static str {
    const GROUPS: &[(&[&str], &str)] = &[
        (&["rs"], "🦀"),
        (&["ts", "tsx", "mts", "cts"], "🔷"),
        (&["js", "jsx", "mjs", "cjs"], "🟨"),
        (&["py"], "🐍"),
        (&["go"], "🐹"),
        (&["java", "kt", "kts"], "☕"),
        (&["c", "h", "cc", "cpp", "cxx", "hpp", "hxx"], "💠"),
        (&["cs"], "💜"),
        (&["swift"], "🧡"),
        (&["rb"], "💎"),
        (&["php"], "🐘"),
        (&["html", "htm"], "🌐"),
        (&["css", "scss", "less"], "🎨"),
        (&["json", "jsonc"], "{ }"),
        (&["toml", "yaml", "yml", "ini", "cfg", "conf", "env"], "⚙️"),
        (&["md", "markdown", "txt", "rst"], "📝"),
        (&["sh", "bash", "zsh", "fish", "ps1"], "💻"),
        (&["sql"], "🗃️"),
        (&["svg", "png", "jpg", "jpeg", "gif", "webp"], "🖼️"),
        (&["lock"], "🔒"),
    ];
    for (exts, icon) in GROUPS {
        if exts.contains(&ext) {
            return icon;
        }
    }
    "📄"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extensions_resolve() {
        assert_eq!(icon_for_name("lib.rs"), "🦀");
        assert_eq!(icon_for_name("App.tsx"), "🔷");
        assert_eq!(icon_for_name("Cargo.toml"), "📦");
        assert_eq!(icon_for_name("notes.md"), "📝");
        assert_eq!(icon_for_name("mystery.zzz"), "📄");
    }
}
