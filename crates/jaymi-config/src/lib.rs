//! Configuration for the Jaymi personal AI environment.
//!
//! Layer 0 foundation: configuration is a dependency of every other component.

#![forbid(unsafe_code)]

use jaymi_core::JaymiResult;

/// Application configuration skeleton.
#[derive(Debug, Default, Clone)]
pub struct Config;

impl Config {
    /// Load configuration from the default local location.
    ///
    /// Intentionally unimplemented in the architectural skeleton.
    pub fn load() -> JaymiResult<Self> {
        Ok(Self)
    }
}
