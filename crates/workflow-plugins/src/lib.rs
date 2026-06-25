pub mod adapter;
pub mod loader;
pub mod manager;
pub mod registry;

pub use adapter::PluginActionHandler;
pub use loader::WorkflowPluginLoader;
pub use manager::WorkflowPluginManager;
pub use registry::{NativeFunction, ObjectGetter, PluginFunctionRegistry};

// Re-export plugin-system types for convenience
pub use plugin_macros::{command, plugin_export};
pub use plugin_system::{
    Plugin, PluginContext, PluginError, PluginManager, PluginMetadata, PluginResult,
};
