//! Lightweight service registry used for dependency injection.
//!
//! Subsystems resolve collaborators through the container instead of
//! constructing them directly. Global state is intentionally avoided.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use crate::error::JaymiError;
use crate::result::JaymiResult;

/// Type-keyed registry of process services.
#[derive(Default)]
pub struct ServiceContainer {
    services: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl ServiceContainer {
    /// Create an empty container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a service instance, replacing any existing value of the same type.
    pub fn register<T>(&mut self, service: T)
    where
        T: Send + Sync + 'static,
    {
        self.services.insert(TypeId::of::<T>(), Box::new(service));
    }

    /// Resolve an immutable reference to a registered service.
    pub fn resolve<T>(&self) -> JaymiResult<&T>
    where
        T: Send + Sync + 'static,
    {
        self.services
            .get(&TypeId::of::<T>())
            .and_then(|service| service.downcast_ref::<T>())
            .ok_or_else(|| {
                JaymiError::new(format!(
                    "service not registered: {}",
                    std::any::type_name::<T>()
                ))
            })
    }

    /// Resolve a mutable reference to a registered service.
    pub fn resolve_mut<T>(&mut self) -> JaymiResult<&mut T>
    where
        T: Send + Sync + 'static,
    {
        self.services
            .get_mut(&TypeId::of::<T>())
            .and_then(|service| service.downcast_mut::<T>())
            .ok_or_else(|| {
                JaymiError::new(format!(
                    "service not registered: {}",
                    std::any::type_name::<T>()
                ))
            })
    }

    /// Returns true when a service of type `T` is registered.
    pub fn contains<T>(&self) -> bool
    where
        T: Send + Sync + 'static,
    {
        self.services.contains_key(&TypeId::of::<T>())
    }

    /// Remove and return a registered service.
    pub fn take<T>(&mut self) -> Option<T>
    where
        T: Send + Sync + 'static,
    {
        self.services
            .remove(&TypeId::of::<T>())
            .and_then(|service| service.downcast::<T>().ok().map(|service| *service))
    }

    /// Number of registered services.
    pub fn len(&self) -> usize {
        self.services.len()
    }

    /// Returns true when no services are registered.
    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_and_resolves_services() {
        let mut container = ServiceContainer::new();
        container.register(42u32);
        assert_eq!(*container.resolve::<u32>().unwrap(), 42);
        assert!(container.contains::<u32>());
        assert!(!container.contains::<u64>());
    }

    #[test]
    fn resolve_missing_service_fails_gracefully() {
        let container = ServiceContainer::new();
        let error = container.resolve::<String>().unwrap_err();
        assert!(error.message().contains("service not registered"));
    }

    #[test]
    fn resolve_mut_updates_value() {
        let mut container = ServiceContainer::new();
        container.register(String::from("boot"));
        *container.resolve_mut::<String>().unwrap() = String::from("ready");
        assert_eq!(container.resolve::<String>().unwrap(), "ready");
    }
}
