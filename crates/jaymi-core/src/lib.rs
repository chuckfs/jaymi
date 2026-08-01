//! Shared types and core architecture primitives for Jaymi.
//!
//! Jaymi is an intelligent environment that coordinates models, tools, and
//! providers through a single conversational interface. This crate holds the
//! foundational types shared across every subsystem.

#![forbid(unsafe_code)]

pub mod error;
pub mod id;
pub mod request;
pub mod result;

pub use error::JaymiError;
pub use id::EntityId;
pub use request::UserRequest;
pub use result::JaymiResult;
