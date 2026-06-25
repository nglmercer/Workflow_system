pub mod adapter;
pub mod loader;
pub mod manager;

pub use adapter::PluginActionHandler;
pub use loader::WorkflowPluginLoader;
pub use manager::WorkflowPluginManager;

// Re-export plugin-system types for convenience
pub use plugin_macros::{command, plugin_export};
pub use plugin_system::{
    Plugin, PluginContext, PluginError, PluginManager, PluginMetadata, PluginResult,
};
