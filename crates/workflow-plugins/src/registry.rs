use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A native function callable from `.flow` files.
///
/// Receives arguments as `serde_json::Value` and returns a value.
/// The function name is registered separately (e.g., `http_get`, `csv_parse`).
pub type NativeFunction = Box<dyn Fn(&[serde_json::Value]) -> serde_json::Value + Send + Sync>;

/// An object getter for `${plugin_name.path}` access in `.flow` files.
///
/// Receives a dot-separated path (e.g., `"config.base_url"`) and returns
/// the value at that path, or `None` if not found.
pub type ObjectGetter = Box<dyn Fn(&str) -> Option<serde_json::Value> + Send + Sync>;

/// Metadata about a registered native function, for LSP/IDE support.
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    /// The function name as it appears in `.flow` (e.g., `http_get`).
    pub name: String,
    /// Parameter names in order (e.g., `["url", "options"]`).
    pub params: Vec<String>,
    /// Human-readable description.
    pub description: String,
    /// Category for LSP grouping (e.g., "HTTP", "CSV", "Custom").
    pub category: String,
    /// Return type description (e.g., "Value", "String", "Number").
    pub return_type: String,
}

/// Metadata about a registered object, for LSP/IDE support.
#[derive(Debug, Clone)]
pub struct ObjectSignature {
    /// The plugin name used as prefix (e.g., `config`, `store`).
    pub plugin_name: String,
    /// Human-readable description.
    pub description: String,
    /// Available fields/paths with their types.
    pub fields: Vec<ObjectField>,
}

/// A field within an object signature.
#[derive(Debug, Clone)]
pub struct ObjectField {
    /// Dot-separated path relative to the plugin name (e.g., `"base_url"`).
    pub path: String,
    /// Type description (e.g., `"String"`, `"Number"`).
    pub type_desc: String,
    /// Human-readable description.
    pub description: String,
}

/// Thread-safe registry for plugin-provided native functions and objects.
///
/// This registry is shared between the `.flow` evaluator (for runtime dispatch)
/// and the LSP (for autocompletion and hover info).
#[derive(Clone)]
pub struct PluginFunctionRegistry {
    inner: Arc<RwLock<PluginFunctionRegistryInner>>,
}

struct PluginFunctionRegistryInner {
    /// Native functions callable from `.flow` (e.g., `http_get`).
    native_functions: HashMap<String, NativeFunction>,
    /// Object getters for `${plugin.path}` access (e.g., `config`).
    object_getters: HashMap<String, ObjectGetter>,
    /// Function signatures for LSP/IDE metadata.
    function_signatures: Vec<FunctionSignature>,
    /// Object signatures for LSP/IDE metadata.
    object_signatures: Vec<ObjectSignature>,
}

impl PluginFunctionRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(PluginFunctionRegistryInner {
                native_functions: HashMap::new(),
                object_getters: HashMap::new(),
                function_signatures: Vec::new(),
                object_signatures: Vec::new(),
            })),
        }
    }

    /// Register a native function callable from `.flow` files.
    pub fn register_function(
        &self,
        name: &str,
        params: Vec<String>,
        description: &str,
        category: &str,
        func: NativeFunction,
    ) {
        let mut inner = self.inner.write().unwrap();
        inner
            .native_functions
            .insert(name.to_string(), func);
        inner.function_signatures.push(FunctionSignature {
            name: name.to_string(),
            params,
            description: description.to_string(),
            category: category.to_string(),
            return_type: "Value".to_string(),
        });
    }

    /// Register an object getter for `${plugin_name.path}` access in `.flow`.
    pub fn register_object(
        &self,
        plugin_name: &str,
        description: &str,
        fields: Vec<ObjectField>,
        getter: ObjectGetter,
    ) {
        let mut inner = self.inner.write().unwrap();
        inner
            .object_getters
            .insert(plugin_name.to_string(), getter);
        inner.object_signatures.push(ObjectSignature {
            plugin_name: plugin_name.to_string(),
            description: description.to_string(),
            fields,
        });
    }

    /// Call a native function by name.
    pub fn call_function(&self, name: &str, args: &[serde_json::Value]) -> Option<serde_json::Value> {
        let inner = self.inner.read().unwrap();
        inner.native_functions.get(name).map(|f| f(args))
    }

    /// Get an object value by plugin name and path.
    pub fn get_object(&self, plugin_name: &str, path: &str) -> Option<serde_json::Value> {
        let inner = self.inner.read().unwrap();
        inner
            .object_getters
            .get(plugin_name)
            .and_then(|getter| getter(path))
    }

    /// Check if a native function is registered.
    pub fn has_function(&self, name: &str) -> bool {
        let inner = self.inner.read().unwrap();
        inner.native_functions.contains_key(name)
    }

    /// Check if an object getter is registered.
    pub fn has_object(&self, plugin_name: &str) -> bool {
        let inner = self.inner.read().unwrap();
        inner.object_getters.contains_key(plugin_name)
    }

    /// Get all registered function names.
    pub fn function_names(&self) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        inner.native_functions.keys().cloned().collect()
    }

    /// Get all registered object plugin names.
    pub fn object_names(&self) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        inner.object_getters.keys().cloned().collect()
    }

    /// Get all function signatures (for LSP).
    pub fn function_signatures(&self) -> Vec<FunctionSignature> {
        let inner = self.inner.read().unwrap();
        inner.function_signatures.clone()
    }

    /// Get all object signatures (for LSP).
    pub fn object_signatures(&self) -> Vec<ObjectSignature> {
        let inner = self.inner.read().unwrap();
        inner.object_signatures.clone()
    }

    /// Get a function signature by name.
    pub fn get_function_signature(&self, name: &str) -> Option<FunctionSignature> {
        let inner = self.inner.read().unwrap();
        inner
            .function_signatures
            .iter()
            .find(|s| s.name == name)
            .cloned()
    }
}

impl Default for PluginFunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_is_empty() {
        let reg = PluginFunctionRegistry::new();
        assert!(reg.function_names().is_empty());
        assert!(reg.object_names().is_empty());
    }

    #[test]
    fn register_and_call_function() {
        let reg = PluginFunctionRegistry::new();
        reg.register_function(
            "add",
            vec!["a".to_string(), "b".to_string()],
            "Adds two numbers",
            "Math",
            Box::new(|args| {
                let a = args[0].as_f64().unwrap_or(0.0);
                let b = args[1].as_f64().unwrap_or(0.0);
                serde_json::json!(a + b)
            }),
        );
        assert!(reg.has_function("add"));
        assert!(!reg.has_function("subtract"));
        let result = reg.call_function("add", &[serde_json::json!(2.0), serde_json::json!(3.0)]);
        assert_eq!(result, Some(serde_json::json!(5.0)));
    }

    #[test]
    fn call_nonexistent_function_returns_none() {
        let reg = PluginFunctionRegistry::new();
        let result = reg.call_function("nope", &[]);
        assert_eq!(result, None);
    }

    #[test]
    fn function_names_returns_all_registered() {
        let reg = PluginFunctionRegistry::new();
        reg.register_function("a", vec![], "A", "Cat", Box::new(|_| serde_json::json!(1)));
        reg.register_function("b", vec![], "B", "Cat", Box::new(|_| serde_json::json!(2)));
        let mut names = reg.function_names();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn function_signatures_matches_registrations() {
        let reg = PluginFunctionRegistry::new();
        reg.register_function(
            "greet",
            vec!["name".to_string()],
            "Greets someone",
            "Social",
            Box::new(|_| serde_json::json!("hello")),
        );
        let sigs = reg.function_signatures();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "greet");
        assert_eq!(sigs[0].params, vec!["name"]);
        assert_eq!(sigs[0].category, "Social");
    }

    #[test]
    fn register_and_get_object() {
        let reg = PluginFunctionRegistry::new();
        reg.register_object(
            "config",
            "App configuration",
            vec![ObjectField {
                path: "base_url".to_string(),
                type_desc: "String".to_string(),
                description: "The base URL".to_string(),
            }],
            Box::new(|path| match path {
                "base_url" => Some(serde_json::json!("https://example.com")),
                _ => None,
            }),
        );
        assert!(reg.has_object("config"));
        assert!(!reg.has_object("store"));
        let val = reg.get_object("config", "base_url");
        assert_eq!(val, Some(serde_json::json!("https://example.com")));
        let missing = reg.get_object("config", "nonexistent");
        assert_eq!(missing, None);
    }

    #[test]
    fn object_signatures_matches_registrations() {
        let reg = PluginFunctionRegistry::new();
        reg.register_object(
            "db",
            "Database",
            vec![ObjectField {
                path: "host".to_string(),
                type_desc: "String".to_string(),
                description: "DB host".to_string(),
            }],
            Box::new(|_| None),
        );
        let sigs = reg.object_signatures();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].plugin_name, "db");
        assert_eq!(sigs[0].fields.len(), 1);
        assert_eq!(sigs[0].fields[0].path, "host");
    }

    #[test]
    fn get_function_signature_by_name() {
        let reg = PluginFunctionRegistry::new();
        reg.register_function("foo", vec![], "desc", "cat", Box::new(|_| serde_json::json!(0)));
        assert!(reg.get_function_signature("foo").is_some());
        assert!(reg.get_function_signature("bar").is_none());
    }
}
