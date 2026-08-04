//! Project Explorer — reusable VS Code-style file tree for Coding Workspace.
//!
//! The Coding workspace owns [`ExplorerState`]. This module owns rendering and
//! emits [`ExplorerEvent`] values; Application applies them through Planner.

mod events;
mod icons;
mod render;

pub use events::ExplorerEvent;
pub use icons::{file_icon, folder_icon};
pub use render::render_explorer;

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::ExplorerNode;

    #[test]
    fn icons_vary_by_extension_without_defaulting_all_files() {
        let rust = ExplorerNode {
            name: "main.rs".into(),
            path: "/a/main.rs".into(),
            is_dir: false,
            children: vec![],
        };
        let ts = ExplorerNode {
            name: "app.ts".into(),
            path: "/a/app.ts".into(),
            is_dir: false,
            children: vec![],
        };
        let unknown = ExplorerNode {
            name: "data.bin".into(),
            path: "/a/data.bin".into(),
            is_dir: false,
            children: vec![],
        };
        assert_ne!(file_icon(&rust), file_icon(&unknown));
        assert_ne!(file_icon(&ts), file_icon(&rust));
        assert_eq!(folder_icon(false), "▸📁");
        assert_eq!(folder_icon(true), "▾📂");
    }
}
