//! Provider framework for Jaymi.
//!
//! Providers connect Jaymi to resources. They expose consistent interfaces and
//! never make decisions. The Planner never communicates with external systems
//! directly — every interaction flows through providers via tools.

#![forbid(unsafe_code)]

pub mod categories;
pub mod lifecycle;
pub mod manager;
pub mod provider;

pub use categories::ProviderCategory;
pub use lifecycle::ProviderLifecycle;
pub use manager::ProviderManager;
pub use provider::{Provider, ProviderIdentity};
