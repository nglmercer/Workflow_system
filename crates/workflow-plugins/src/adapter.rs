use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use plugin_system::Plugin;
use workflow_domain::{ActionParams, TriggerContext, WorkflowResult};
use workflow_engine::ActionHandler;

/// Wraps a dynamically loaded `Plugin` as an `ActionHandler` so it can be
/// registered in the `RuleEngine`. When the engine dispatches an action whose
/// `action_type` matches the plugin's name, the call is forwarded to the
/// plugin's `handle_command` method.
pub struct PluginActionHandler {
    plugin_name: String,
    plugin: Arc<RwLock<Box<dyn Plugin>>>,
}

impl PluginActionHandler {
    pub fn new(plugin_name: String, plugin: Arc<RwLock<Box<dyn Plugin>>>) -> Self {
        Self {
            plugin_name,
            plugin,
        }
    }
}

#[async_trait]
impl ActionHandler for PluginActionHandler {
    fn action_type(&self) -> &str {
        &self.plugin_name
    }

    async fn execute(
        &self,
        params: &Option<ActionParams>,
        _context: &TriggerContext,
    ) -> WorkflowResult<serde_json::Value> {
        let method = params
            .as_ref()
            .and_then(|p| p.get("method"))
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let args = match params {
            Some(p) => serde_json::to_value(p).unwrap_or_default(),
            None => serde_json::Value::Null,
        };

        let mut plugin = self
            .plugin
            .write()
            .map_err(|e| workflow_domain::WorkflowError::Plugin(e.to_string()))?;

        plugin.handle_command(method, args).ok_or_else(|| {
            workflow_domain::WorkflowError::Plugin(format!(
                "Plugin '{}' returned no result for command '{}'",
                self.plugin_name, method
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handler(name: &str) -> PluginActionHandler {
        let plugin: Arc<RwLock<Box<dyn Plugin>>> = Arc::new(RwLock::new(Box::new(MockPlugin {
            name: name.to_string(),
        })));
        PluginActionHandler::new(name.to_string(), plugin)
    }

    #[test]
    fn action_type_returns_plugin_name() {
        let handler = make_handler("my_plugin");
        assert_eq!(handler.action_type(), "my_plugin");
    }

    #[test]
    fn handler_stores_plugin_name() {
        let handler = make_handler("test_plugin");
        assert_eq!(handler.plugin_name, "test_plugin");
    }

    // MockPlugin is a minimal Plugin implementation for testing.
    // We test at the unit level rather than integration level
    // because the Plugin trait requires a full runtime context.
    struct MockPlugin {
        name: String,
    }

    impl Plugin for MockPlugin {
        fn metadata(&self) -> plugin_system::PluginMetadata {
            plugin_system::PluginMetadata {
                name: self.name.clone(),
                version: "0.1.0".to_string(),
                authors: vec!["test".to_string()],
                dependencies: vec![],
            }
        }

        fn on_load(&mut self, _ctx: &plugin_system::context::PluginContext) {}
        fn on_unload(&mut self) {}
        fn plugin_type_name(&self) -> &'static str { "mock" }

        fn handle_command(
            &mut self,
            method: &str,
            args: serde_json::Value,
        ) -> Option<serde_json::Value> {
            match method {
                "echo" => Some(args),
                "greet" => {
                    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("world");
                    Some(serde_json::json!(format!("Hello, {}!", name)))
                }
                _ => None,
            }
        }
    }
}
