use std::path::Path;

use plugin_system::PluginManager;
use workflow_engine::RuleEngine;

use crate::adapter::PluginActionHandler;
use crate::loader::WorkflowPluginLoader;

/// High-level manager that loads workflow plugins and bridges them into
/// the `RuleEngine` as `ActionHandler` implementations.
pub struct WorkflowPluginManager {
    plugin_manager: PluginManager,
    loader: WorkflowPluginLoader,
}

impl WorkflowPluginManager {
    /// Create a new manager that will load plugins from the given directory.
    pub fn new(plugin_dir: impl AsRef<Path>) -> Self {
        Self {
            plugin_manager: PluginManager::new(),
            loader: WorkflowPluginLoader::new(plugin_dir),
        }
    }

    /// Returns a reference to the underlying `PluginManager`.
    pub fn inner(&self) -> &PluginManager {
        &self.plugin_manager
    }

    /// Returns a mutable reference to the underlying `PluginManager`.
    pub fn inner_mut(&mut self) -> &mut PluginManager {
        &mut self.plugin_manager
    }

    /// Returns the plugin directory path.
    pub fn plugin_dir(&self) -> &Path {
        self.loader.dir()
    }

    /// Discover and load all plugins from the plugin directory.
    /// Returns the names of successfully loaded plugins.
    pub fn load_all(&mut self) -> Vec<String> {
        let discovered = self.loader.discover();
        let mut loaded = Vec::new();

        for (name, loader) in discovered {
            match self
                .plugin_manager
                .load_plugin_from_loader(loader.as_ref(), &name)
            {
                Ok(name) => {
                    log::info!("Loaded workflow plugin: {}", name);
                    loaded.push(name);
                }
                Err(e) => {
                    log::error!("Failed to load plugin '{}': {}", name, e);
                }
            }
        }

        loaded
    }

    /// Load a single plugin by path.
    pub fn load_plugin(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<String, plugin_system::PluginError> {
        self.plugin_manager.load_plugin(path)
    }

    /// Unload a plugin by name.
    pub fn unload_plugin(&mut self, name: &str) -> Result<(), plugin_system::PluginError> {
        self.plugin_manager.unload_plugin(name)
    }

    /// List all loaded plugin names.
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugin_manager.plugin_names()
    }

    /// Check if a plugin is loaded.
    pub fn is_loaded(&self, name: &str) -> bool {
        self.plugin_manager.is_loaded(name)
    }

    /// Get metadata for a loaded plugin.
    pub fn plugin_metadata(&self, name: &str) -> Option<plugin_system::PluginMetadata> {
        self.plugin_manager.plugin_metadata(name)
    }

    /// Create `ActionHandler` adapters for all loaded plugins and register
    /// them with the given `RuleEngine`.
    ///
    /// Each plugin becomes an `ActionHandler` whose `action_type()` returns
    /// the plugin's name. When the engine dispatches an action with a
    /// matching `action_type`, the call is forwarded to the plugin's
    /// `handle_command()` method.
    pub fn register_handlers(&self, engine: &mut RuleEngine) {
        let names = self.plugin_manager.plugin_names();
        for name in &names {
            if let Ok(plugin_arc) = self.plugin_manager.get_plugin_arc(name) {
                let handler = PluginActionHandler::new(name.clone(), plugin_arc);
                engine.register_handler(Box::new(handler));
                log::info!("Registered plugin '{}' as action handler", name);
            }
        }
    }

    /// Execute a command on a loaded plugin.
    pub fn call_plugin(
        &self,
        name: &str,
        method: &str,
        args: serde_json::Value,
    ) -> Result<Option<serde_json::Value>, plugin_system::PluginError> {
        self.plugin_manager
            .with_plugin_mut(name, |plugin| plugin.handle_command(method, args))
    }

    /// Reload a plugin by name (unload + load from original path).
    pub fn reload_plugin(&mut self, name: &str) -> Result<(), plugin_system::PluginError> {
        self.plugin_manager.reload_plugin(name)
    }
}
