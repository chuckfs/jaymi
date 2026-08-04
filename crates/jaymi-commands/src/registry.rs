//! Command registration surface.

use std::collections::HashMap;
use std::sync::RwLock;

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};

use crate::descriptor::CommandDescriptor;
use crate::search::filter_commands;

const NAME: &str = "command-registry";
const DEPENDENCIES: &[&str] = &[];

/// Registry of invokable commands (metadata only).
///
/// Plugins register additional [`CommandDescriptor`]s here. Execution handlers
/// are bound by the host (Application / UI) against `descriptor.id`.
#[derive(Default)]
pub struct CommandRegistry {
    initialized: bool,
    commands: RwLock<HashMap<String, CommandDescriptor>>,
}

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRegistry")
            .field("initialized", &self.initialized)
            .field("command_count", &self.len())
            .finish()
    }
}

impl CommandRegistry {
    /// Create an empty, uninitialized registry.
    pub fn new() -> Self {
        Self {
            initialized: false,
            commands: RwLock::new(HashMap::new()),
        }
    }

    /// Register a command descriptor.
    ///
    /// Fails when `id` is already registered (plugins must use unique ids).
    pub fn register(&self, descriptor: CommandDescriptor) -> JaymiResult<()> {
        self.ensure_initialized()?;
        let mut guard = self
            .commands
            .write()
            .map_err(|_| JaymiError::new("command registry lock poisoned"))?;
        if guard.contains_key(&descriptor.id) {
            return Err(JaymiError::new(format!(
                "command already registered: {}",
                descriptor.id
            )));
        }
        guard.insert(descriptor.id.clone(), descriptor);
        Ok(())
    }

    /// Register many descriptors; stops on the first failure.
    pub fn register_all(
        &self,
        descriptors: impl IntoIterator<Item = CommandDescriptor>,
    ) -> JaymiResult<()> {
        for descriptor in descriptors {
            self.register(descriptor)?;
        }
        Ok(())
    }

    /// Look up a command by id.
    pub fn get(&self, id: &str) -> JaymiResult<Option<CommandDescriptor>> {
        self.ensure_initialized()?;
        let guard = self
            .commands
            .read()
            .map_err(|_| JaymiError::new("command registry lock poisoned"))?;
        Ok(guard.get(id).cloned())
    }

    /// Whether `id` is registered.
    pub fn contains(&self, id: &str) -> JaymiResult<bool> {
        Ok(self.get(id)?.is_some())
    }

    /// All registered commands (stable sort by title, then id).
    pub fn list(&self) -> JaymiResult<Vec<CommandDescriptor>> {
        self.ensure_initialized()?;
        let guard = self
            .commands
            .read()
            .map_err(|_| JaymiError::new("command registry lock poisoned"))?;
        let mut out: Vec<_> = guard.values().cloned().collect();
        out.sort_by(|a, b| a.title.cmp(&b.title).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    /// Fuzzy-filter commands for the palette.
    pub fn search(&self, query: &str) -> JaymiResult<Vec<CommandDescriptor>> {
        let all = self.list()?;
        Ok(filter_commands(&all, query))
    }

    /// Number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.read().map(|guard| guard.len()).unwrap_or(0)
    }

    /// Whether the registry has no commands.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn ensure_initialized(&self) -> JaymiResult<()> {
        if !self.initialized {
            return Err(JaymiError::new("command registry is not initialized"));
        }
        Ok(())
    }
}

impl Lifecycle for CommandRegistry {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        HealthReport::new(
            NAME,
            self.initialized,
            self.initialized,
            self.version(),
            DEPENDENCIES,
        )
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        if let Ok(mut guard) = self.commands.write() {
            guard.clear();
        }
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{builtin_descriptors, ids, CommandCategory, CommandSource};

    fn ready_registry() -> CommandRegistry {
        let mut registry = CommandRegistry::new();
        registry.initialize().unwrap();
        registry.register_all(builtin_descriptors()).unwrap();
        registry
    }

    #[test]
    fn builtins_register_and_list() {
        let registry = ready_registry();
        assert!(registry.len() >= 19);
        assert!(registry.contains(ids::SAVE).unwrap());
        let save = registry.get(ids::SAVE).unwrap().unwrap();
        assert_eq!(save.title, "Save");
        assert_eq!(save.category, CommandCategory::File);
        assert_eq!(save.source, CommandSource::Builtin);
    }

    #[test]
    fn duplicate_id_rejected() {
        let registry = ready_registry();
        let err = registry
            .register(crate::CommandDescriptor::builtin(
                ids::SAVE,
                "Save Again",
                CommandCategory::File,
            ))
            .unwrap_err();
        assert!(err.message().contains("already registered"));
    }

    #[test]
    fn plugin_can_register_extension_command() {
        let registry = ready_registry();
        registry
            .register(
                crate::CommandDescriptor::plugin(
                    "ext.demo.hello",
                    "Hello from Plugin",
                    CommandCategory::Extension,
                )
                .with_keywords(["hello", "demo"]),
            )
            .unwrap();
        assert!(registry.contains("ext.demo.hello").unwrap());
        let hits = registry.search("hello plugin").unwrap();
        assert!(hits.iter().any(|cmd| cmd.id == "ext.demo.hello"));
    }

    #[test]
    fn search_matches_title_and_keywords() {
        let registry = ready_registry();
        let hits = registry.search("term").unwrap();
        assert!(hits.iter().any(|cmd| cmd.id == ids::TOGGLE_TERMINAL));
        let hits = registry.search("scm").unwrap();
        assert!(hits.iter().any(|cmd| cmd.id == ids::TOGGLE_GIT));
    }

    #[test]
    fn uninitialized_registry_rejects_register() {
        let registry = CommandRegistry::new();
        let err = registry
            .register(crate::CommandDescriptor::builtin(
                "x",
                "X",
                CommandCategory::File,
            ))
            .unwrap_err();
        assert!(err.message().contains("not initialized"));
    }
}
