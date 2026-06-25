use std::path::Path;

use plugin_system::PluginManager;
use workflow_engine::RuleEngine;
use workflow_parser::evaluator::FlowEvaluator;

use crate::adapter::PluginActionHandler;
use crate::loader::WorkflowPluginLoader;
use crate::registry::PluginFunctionRegistry;

/// High-level manager that loads workflow plugins and bridges them into
/// the `RuleEngine` as `ActionHandler` implementations, and into the
/// `FlowEvaluator` as native functions and object getters.
pub struct WorkflowPluginManager {
    plugin_manager: PluginManager,
    loader: WorkflowPluginLoader,
    function_registry: PluginFunctionRegistry,
}

impl WorkflowPluginManager {
    /// Create a new manager that will load plugins from the given directory.
    pub fn new(plugin_dir: impl AsRef<Path>) -> Self {
        Self {
            plugin_manager: PluginManager::new(),
            loader: WorkflowPluginLoader::new(plugin_dir),
            function_registry: PluginFunctionRegistry::new(),
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

    /// Returns a reference to the shared function registry.
    pub fn function_registry(&self) -> &PluginFunctionRegistry {
        &self.function_registry
    }

    /// Returns a mutable reference to the shared function registry.
    pub fn function_registry_mut(&mut self) -> &mut PluginFunctionRegistry {
        &mut self.function_registry
    }

    /// Consume the manager and return the function registry.
    /// Useful when you only need the registry after loading plugins.
    pub fn into_registry(self) -> PluginFunctionRegistry {
        self.function_registry
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
                Ok(actual_name) => {
                    log::info!("Loaded workflow plugin: {}", actual_name);
                    self.register_plugin_functions(&actual_name);
                    loaded.push(actual_name);
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
        let name = self.plugin_manager.load_plugin(path)?;
        self.register_plugin_functions(&name);
        Ok(name)
    }

    fn register_plugin_functions(&mut self, plugin_name: &str) {
        let plugin_arc = match self.plugin_manager.get_plugin_arc(plugin_name) {
            Ok(p) => p,
            Err(_) => return,
        };

        let interface_data = {
            let plugin = plugin_arc.read().unwrap();
            plugin.interface_data()
        };

        if let Some(data) = interface_data {
            // Register commands as native functions
            if let Some(commands) = data.get("commands").and_then(|v| v.as_array()) {
                for cmd in commands {
                    if let Some(name) = cmd.get("name").and_then(|v| v.as_str()) {
                        let param_names: Vec<String> = cmd
                            .get("params")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();

                        let plugin_name_clone = plugin_name.to_string();
                        let method_name = name.to_string();
                        let plugin_manager_registry = self.plugin_manager.registry();
                        let captured_params = param_names.clone();

                        let plugin_name_captured = plugin_name_clone.clone();
                        let method_name_captured = method_name.clone();

                        self.function_registry.register_function(
                            &method_name,
                            param_names,
                            &format!("Plugin command {} from {}", method_name, plugin_name_clone),
                            &plugin_name_clone,
                            Box::new(move |args| {
                                let registry = plugin_manager_registry.read().unwrap();
                                if let Some(plugin_arc) = registry.get_by_name(&plugin_name_captured)
                                {
                                    let mut plugin = plugin_arc.write().unwrap();
                                    let mut args_obj = serde_json::Map::new();
                                    for (i, param_name) in captured_params.iter().enumerate() {
                                        if let Some(val) = args.get(i) {
                                            args_obj.insert(param_name.clone(), val.clone());
                                        }
                                    }
                                    return plugin
                                        .handle_command(
                                            &method_name_captured,
                                            serde_json::Value::Object(args_obj),
                                        )
                                        .unwrap_or(serde_json::Value::Null);
                                }
                                serde_json::Value::Null
                            }),
                        );
                    }
                }
            }

            // Register objects
            if let Some(objects) = data.get("objects").and_then(|v| v.as_object()) {
                for (obj_name, obj_data) in objects {
                    let plugin_name_clone = plugin_name.to_string();
                    let obj_data_clone = obj_data.clone();

                    self.function_registry.register_object(
                        obj_name,
                        &format!("Plugin object {} from {}", obj_name, plugin_name_clone),
                        vec![], // We could extract fields from obj_data if we wanted more detail
                        Box::new(move |path| {
                            if path.is_empty() {
                                return Some(obj_data_clone.clone());
                            }
                            let mut current = &obj_data_clone;
                            for part in path.split('.') {
                                if let Some(val) = current.get(part) {
                                    current = val;
                                } else {
                                    return None;
                                }
                            }
                            Some(current.clone())
                        }),
                    );
                }
            }
        }
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

    /// Inject all registered native functions and object getters into a
    /// `FlowEvaluator`. This makes plugin-provided functions callable
    /// from `.flow` files and plugin objects accessible via `${}`.
    ///
    /// # Example
    /// ```ignore
    /// let mut evaluator = FlowEvaluator::new();
    /// plugin_manager.inject_into_evaluator(&mut evaluator);
    ///
    /// // Now .flow files can call:
    /// // let response = http_get("https://api.example.com")
    /// // let base_url = ${config.base_url}
    /// ```
    pub fn inject_into_evaluator(&self, evaluator: &mut FlowEvaluator) {
        let func_names = self.function_registry.function_names();
        let obj_names = self.function_registry.object_names();

        // Inject native functions
        for name in &func_names {
            let registry = self.function_registry.clone();
            let name_clone = name.clone();
            let func: workflow_parser::evaluator::NativeFunction = Box::new(move |args| {
                registry.call_function(&name_clone, args).unwrap_or_default()
            });
            evaluator.register_native_function(name, func);
            log::info!("Injected plugin function '{}' into evaluator", name);
        }

        // Inject object getters
        for name in &obj_names {
            let registry = self.function_registry.clone();
            let name_clone = name.clone();
            let getter: workflow_parser::evaluator::ObjectGetter = Box::new(move |path| {
                registry.get_object(&name_clone, path)
            });
            evaluator.register_object_getter(name, getter);
            log::info!("Injected plugin object '{}' into evaluator", name);
        }
    }

    /// Execute a command on a loaded plugin.
    pub fn call_plugin(
        &self,
        name: &str,
        method: &str,
        args: serde_json::Value,
    ) -> Result<Option<serde_json::Value>, plugin_system::PluginError> {
        self.plugin_manager.with_plugin_mut(name, |plugin| {
            plugin.handle_command(method, args)
        })
    }

    /// Reload a plugin by name (unload + load from original path).
    pub fn reload_plugin(&mut self, name: &str) -> Result<(), plugin_system::PluginError> {
        self.plugin_manager.reload_plugin(name)
    }
}
