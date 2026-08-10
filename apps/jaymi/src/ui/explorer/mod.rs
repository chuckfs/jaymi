//! Project Explorer — reusable VS Code-style file tree for Coding Workspace.
//!
//! The Coding workspace owns [`ExplorerState`]. This module owns rendering and
//! emits [`ExplorerEvent`] values; Application applies them through Planner.

mod events;
mod icons;
mod render;

pub use events::ExplorerEvent;
pub use icons::paint_disclosure;
pub use render::render_explorer;
