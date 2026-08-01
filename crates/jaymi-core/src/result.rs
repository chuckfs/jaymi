//! Shared result alias for Jaymi crates.

use crate::error::JaymiError;

/// Convenient result type used at architectural boundaries.
pub type JaymiResult<T> = Result<T, JaymiError>;
