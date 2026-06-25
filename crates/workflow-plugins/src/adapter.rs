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
