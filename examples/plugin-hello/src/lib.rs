use plugin_system::{PluginContext, PluginMetadata};
use std::collections::HashMap;

pub struct HelloPlugin {
    messages: HashMap<String, String>,
    config: HashMap<String, String>,
}

impl Default for HelloPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[plugin_system::plugin_export]
impl HelloPlugin {
    pub fn new() -> Self {
        let mut config = HashMap::new();
        config.insert("greeting".to_string(), "Hello".to_string());
        config.insert("version".to_string(), "1.0.0".to_string());
        config.insert("author".to_string(), "Workflow System".to_string());

        Self {
            messages: HashMap::new(),
            config,
        }
    }

    fn metadata(&self) -> PluginMetadata {
        plugin_system::plugin_metadata! {
            name: "hello",
            version: "0.1.0",
            authors: ["Workflow System"],
            dependencies: []
        }
    }

    fn on_load(&mut self, _ctx: &PluginContext) {
        log::info!("HelloPlugin loaded");
    }

    fn on_unload(&mut self) {
        log::info!("HelloPlugin unloading");
    }

    /// Say hello with an optional custom message.
    #[plugin_system::command("say_hello")]
    pub fn say_hello(&mut self, name: String) -> String {
        let greeting = format!("Hello, {}!", name);
        log::info!("{}", greeting);
        greeting
    }

    /// Store a message for later retrieval.
    #[plugin_system::command("store_message")]
    pub fn store_message(&mut self, key: String, value: String) -> String {
        self.messages.insert(key.clone(), value.clone());
        format!("Stored message for key '{}'", key)
    }

    /// Retrieve a stored message by key.
    #[plugin_system::command("get_message")]
    pub fn get_message(&self, key: &str) -> Option<String> {
        self.messages.get(key).cloned()
    }

    /// List all stored message keys.
    #[plugin_system::command("list_messages")]
    pub fn list_messages(&self) -> Vec<String> {
        self.messages.keys().cloned().collect()
    }

    /// Echo back whatever is passed in.
    #[plugin_system::command("echo")]
    pub fn echo(&self, message: String) -> String {
        message
    }

    /// Get a config value by key.
    #[plugin_system::command("get_config")]
    pub fn get_config(&self, key: &str) -> Option<String> {
        self.config.get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_system::Plugin;

    #[test]
    fn metadata_is_correct() {
        let plugin = HelloPlugin::new();
        let meta = plugin.metadata();
        assert_eq!(meta.name, "hello");
        assert_eq!(meta.version, "0.1.0");
    }

    #[test]
    fn say_hello_works() {
        let mut plugin = HelloPlugin::new();
        let result = plugin.say_hello("World".to_string());
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn store_and_retrieve_message() {
        let mut plugin = HelloPlugin::new();
        plugin.store_message("greeting".to_string(), "Hi there!".to_string());
        assert_eq!(
            plugin.get_message("greeting"),
            Some("Hi there!".to_string())
        );
    }

    #[test]
    fn echo_returns_input() {
        let plugin = HelloPlugin::new();
        let result = plugin.echo("test message".to_string());
        assert_eq!(result, "test message");
    }

    #[test]
    fn config_values() {
        let plugin = HelloPlugin::new();
        assert_eq!(plugin.get_config("greeting"), Some("Hello".to_string()));
        assert_eq!(plugin.get_config("version"), Some("1.0.0".to_string()));
    }
}
