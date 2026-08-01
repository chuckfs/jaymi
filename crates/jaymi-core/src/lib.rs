//! Shared types and core architecture primitives for Jaymi.
//!
//! Jaymi is an intelligent environment that coordinates models, tools, and
//! providers through a single conversational interface. This crate holds the
//! foundational types shared across every subsystem.

#![forbid(unsafe_code)]

pub mod container;
pub mod error;
pub mod health;
pub mod id;
pub mod lifecycle;
pub mod request;
pub mod result;
pub mod state;

pub use container::ServiceContainer;
pub use error::JaymiError;
pub use health::HealthReport;
pub use id::EntityId;
pub use lifecycle::Lifecycle;
pub use request::UserRequest;
pub use result::JaymiResult;
pub use state::AppState;
