//! Projects — first-class workspaces in Jaymi.
//!
//! On-disk layout helpers for the hidden `.jaymi` directory. Persistent project
//! identity and lifecycle live in the Project Engine (`jaymi-project-engine`).

#![forbid(unsafe_code)]

pub mod structure;
