//! Dynamic function registry for the Flow LSP.
//!
//! Instead of hardcoding built-in functions, this module provides a
//! registry where functions can be registered at runtime. This enables:
//! - Cross-file imports to register their functions dynamically
//! - External plugins to add custom functions
//! - Runtime inference of function signatures
//!
//! The registry is the single source of truth for all known functions.
//! When the LSP needs to check if a function exists or get its signature,
//! it queries the registry.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::ty::Type;
use super::value::FunctionSig;

/// A parameter descriptor for a registered function.
#[derive(Debug, Clone)]
pub struct ParamDescriptor {
    pub name: String,
    pub ty: Type,
    pub optional: bool,
    pub default_value: Option<String>,
}

/// A registered function's metadata.
#[derive(Debug, Clone)]
pub struct FunctionEntry {
    pub name: String,
    pub params: Vec<ParamDescriptor>,
    pub return_type: Type,
    pub description: Option<String>,
    pub category: FunctionCategory,
    /// Whether this function was defined in user code (imported from .flow files)
    /// vs being a built-in provided by the runtime.
    pub is_user_defined: bool,
}

/// Categories for organizing functions in completion/hover UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionCategory {
    /// Core language functions (log, len, etc.)
    Core,
    /// String manipulation functions
    String,
    /// Math functions
    Math,
    /// Array/list functions
    Array,
    /// Object/map functions
    Object,
    /// Type conversion functions
    Conversion,
    /// Date/time functions
    DateTime,
    /// HTTP/network functions
    Network,
    /// JSON functions
    Json,
    /// User-defined functions (from imports or local definitions)
    UserDefined,
    /// Custom functions registered by plugins
    Custom,
}

impl FunctionCategory {
    pub fn label(&self) -> &'static str {
        match self {
            FunctionCategory::Core => "Core",
            FunctionCategory::String => "String",
            FunctionCategory::Math => "Math",
            FunctionCategory::Array => "Array",
            FunctionCategory::Object => "Object",
            FunctionCategory::Conversion => "Conversion",
            FunctionCategory::DateTime => "Date/Time",
            FunctionCategory::Network => "Network",
            FunctionCategory::Json => "JSON",
            FunctionCategory::UserDefined => "User Defined",
            FunctionCategory::Custom => "Custom",
        }
    }
}

/// The dynamic function registry.
///
/// Thread-safe via `Arc<RwLock<...>>` so multiple LSP requests can
/// read concurrently while imports register new functions.
#[derive(Debug, Clone)]
pub struct FunctionRegistry {
    inner: Arc<RwLock<FunctionRegistryInner>>,
}

#[derive(Debug, Default)]
struct FunctionRegistryInner {
    functions: HashMap<String, FunctionEntry>,
    /// Built-in functions provided by the runtime.
    builtins: HashMap<String, FunctionEntry>,
}

impl FunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(FunctionRegistryInner::default())),
        }
    }

    /// Create a registry pre-populated with standard built-in functions.
    pub fn with_builtins() -> Self {
        let registry = Self::new();
        registry.register_standard_builtins();
        registry
    }

    /// Register a standard set of built-in functions.
    /// These are the minimal functions that every Flow runtime provides.
    pub fn register_standard_builtins(&self) {
        let mut inner = self.inner.write().unwrap();

        // Core functions
        inner.builtins.insert(
            "log".to_string(),
            FunctionEntry {
                name: "log".to_string(),
                params: vec![ParamDescriptor {
                    name: "message".to_string(),
                    ty: Type::Any,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Any,
                description: Some("Log a message to the console".to_string()),
                category: FunctionCategory::Core,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "len".to_string(),
            FunctionEntry {
                name: "len".to_string(),
                params: vec![ParamDescriptor {
                    name: "value".to_string(),
                    ty: Type::Any,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Number,
                description: Some("Get the length of a string or array".to_string()),
                category: FunctionCategory::Core,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "to_string".to_string(),
            FunctionEntry {
                name: "to_string".to_string(),
                params: vec![ParamDescriptor {
                    name: "value".to_string(),
                    ty: Type::Any,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::String,
                description: Some("Convert a value to a string".to_string()),
                category: FunctionCategory::Conversion,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "to_number".to_string(),
            FunctionEntry {
                name: "to_number".to_string(),
                params: vec![ParamDescriptor {
                    name: "value".to_string(),
                    ty: Type::String,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Number,
                description: Some("Convert a string to a number".to_string()),
                category: FunctionCategory::Conversion,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "emit".to_string(),
            FunctionEntry {
                name: "emit".to_string(),
                params: vec![ParamDescriptor {
                    name: "event".to_string(),
                    ty: Type::String,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Any,
                description: Some("Emit an event".to_string()),
                category: FunctionCategory::Core,
                is_user_defined: false,
            },
        );

        // String functions
        inner.builtins.insert(
            "concat".to_string(),
            FunctionEntry {
                name: "concat".to_string(),
                params: vec![ParamDescriptor {
                    name: "strings".to_string(),
                    ty: Type::Array(Box::new(Type::String)),
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::String,
                description: Some("Concatenate multiple strings".to_string()),
                category: FunctionCategory::String,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "join".to_string(),
            FunctionEntry {
                name: "join".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "array".to_string(),
                        ty: Type::Array(Box::new(Type::Any)),
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "separator".to_string(),
                        ty: Type::String,
                        optional: false,
                        default_value: None,
                    },
                ],
                return_type: Type::String,
                description: Some("Join array elements into a string".to_string()),
                category: FunctionCategory::String,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "split".to_string(),
            FunctionEntry {
                name: "split".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "string".to_string(),
                        ty: Type::String,
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "separator".to_string(),
                        ty: Type::String,
                        optional: false,
                        default_value: None,
                    },
                ],
                return_type: Type::Array(Box::new(Type::String)),
                description: Some("Split a string into an array".to_string()),
                category: FunctionCategory::String,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "trim".to_string(),
            FunctionEntry {
                name: "trim".to_string(),
                params: vec![ParamDescriptor {
                    name: "string".to_string(),
                    ty: Type::String,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::String,
                description: Some("Remove whitespace from both ends of a string".to_string()),
                category: FunctionCategory::String,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "upper".to_string(),
            FunctionEntry {
                name: "upper".to_string(),
                params: vec![ParamDescriptor {
                    name: "string".to_string(),
                    ty: Type::String,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::String,
                description: Some("Convert string to uppercase".to_string()),
                category: FunctionCategory::String,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "lower".to_string(),
            FunctionEntry {
                name: "lower".to_string(),
                params: vec![ParamDescriptor {
                    name: "string".to_string(),
                    ty: Type::String,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::String,
                description: Some("Convert string to lowercase".to_string()),
                category: FunctionCategory::String,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "replace".to_string(),
            FunctionEntry {
                name: "replace".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "string".to_string(),
                        ty: Type::String,
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "from".to_string(),
                        ty: Type::String,
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "to".to_string(),
                        ty: Type::String,
                        optional: false,
                        default_value: None,
                    },
                ],
                return_type: Type::String,
                description: Some("Replace occurrences of a substring".to_string()),
                category: FunctionCategory::String,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "contains".to_string(),
            FunctionEntry {
                name: "contains".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "string".to_string(),
                        ty: Type::String,
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "substring".to_string(),
                        ty: Type::String,
                        optional: false,
                        default_value: None,
                    },
                ],
                return_type: Type::Bool,
                description: Some("Check if string contains substring".to_string()),
                category: FunctionCategory::String,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "substr".to_string(),
            FunctionEntry {
                name: "substr".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "string".to_string(),
                        ty: Type::String,
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "start".to_string(),
                        ty: Type::Number,
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "length".to_string(),
                        ty: Type::Number,
                        optional: true,
                        default_value: None,
                    },
                ],
                return_type: Type::String,
                description: Some("Extract a substring".to_string()),
                category: FunctionCategory::String,
                is_user_defined: false,
            },
        );

        // Math functions
        inner.builtins.insert(
            "abs".to_string(),
            FunctionEntry {
                name: "abs".to_string(),
                params: vec![ParamDescriptor {
                    name: "value".to_string(),
                    ty: Type::Number,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Number,
                description: Some("Get absolute value".to_string()),
                category: FunctionCategory::Math,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "ceil".to_string(),
            FunctionEntry {
                name: "ceil".to_string(),
                params: vec![ParamDescriptor {
                    name: "value".to_string(),
                    ty: Type::Number,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Number,
                description: Some("Round up to nearest integer".to_string()),
                category: FunctionCategory::Math,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "floor".to_string(),
            FunctionEntry {
                name: "floor".to_string(),
                params: vec![ParamDescriptor {
                    name: "value".to_string(),
                    ty: Type::Number,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Number,
                description: Some("Round down to nearest integer".to_string()),
                category: FunctionCategory::Math,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "round".to_string(),
            FunctionEntry {
                name: "round".to_string(),
                params: vec![ParamDescriptor {
                    name: "value".to_string(),
                    ty: Type::Number,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Number,
                description: Some("Round to nearest integer".to_string()),
                category: FunctionCategory::Math,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "max".to_string(),
            FunctionEntry {
                name: "max".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "a".to_string(),
                        ty: Type::Number,
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "b".to_string(),
                        ty: Type::Number,
                        optional: false,
                        default_value: None,
                    },
                ],
                return_type: Type::Number,
                description: Some("Get maximum of two values".to_string()),
                category: FunctionCategory::Math,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "min".to_string(),
            FunctionEntry {
                name: "min".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "a".to_string(),
                        ty: Type::Number,
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "b".to_string(),
                        ty: Type::Number,
                        optional: false,
                        default_value: None,
                    },
                ],
                return_type: Type::Number,
                description: Some("Get minimum of two values".to_string()),
                category: FunctionCategory::Math,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "random".to_string(),
            FunctionEntry {
                name: "random".to_string(),
                params: vec![],
                return_type: Type::Number,
                description: Some("Get a random number between 0 and 1".to_string()),
                category: FunctionCategory::Math,
                is_user_defined: false,
            },
        );

        // Array functions
        inner.builtins.insert(
            "sort".to_string(),
            FunctionEntry {
                name: "sort".to_string(),
                params: vec![ParamDescriptor {
                    name: "array".to_string(),
                    ty: Type::Array(Box::new(Type::Any)),
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Array(Box::new(Type::Any)),
                description: Some("Sort an array".to_string()),
                category: FunctionCategory::Array,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "reverse".to_string(),
            FunctionEntry {
                name: "reverse".to_string(),
                params: vec![ParamDescriptor {
                    name: "array".to_string(),
                    ty: Type::Array(Box::new(Type::Any)),
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Array(Box::new(Type::Any)),
                description: Some("Reverse an array".to_string()),
                category: FunctionCategory::Array,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "push".to_string(),
            FunctionEntry {
                name: "push".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "array".to_string(),
                        ty: Type::Array(Box::new(Type::Any)),
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "element".to_string(),
                        ty: Type::Any,
                        optional: false,
                        default_value: None,
                    },
                ],
                return_type: Type::Array(Box::new(Type::Any)),
                description: Some("Add element to end of array".to_string()),
                category: FunctionCategory::Array,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "pop".to_string(),
            FunctionEntry {
                name: "pop".to_string(),
                params: vec![ParamDescriptor {
                    name: "array".to_string(),
                    ty: Type::Array(Box::new(Type::Any)),
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Any,
                description: Some("Remove and return last element".to_string()),
                category: FunctionCategory::Array,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "map".to_string(),
            FunctionEntry {
                name: "map".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "array".to_string(),
                        ty: Type::Array(Box::new(Type::Any)),
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "callback".to_string(),
                        ty: Type::Any, // Function type
                        optional: false,
                        default_value: None,
                    },
                ],
                return_type: Type::Array(Box::new(Type::Any)),
                description: Some("Transform each element".to_string()),
                category: FunctionCategory::Array,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "filter".to_string(),
            FunctionEntry {
                name: "filter".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "array".to_string(),
                        ty: Type::Array(Box::new(Type::Any)),
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "callback".to_string(),
                        ty: Type::Any, // Function type
                        optional: false,
                        default_value: None,
                    },
                ],
                return_type: Type::Array(Box::new(Type::Any)),
                description: Some("Filter elements by predicate".to_string()),
                category: FunctionCategory::Array,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "reduce".to_string(),
            FunctionEntry {
                name: "reduce".to_string(),
                params: vec![
                    ParamDescriptor {
                        name: "array".to_string(),
                        ty: Type::Array(Box::new(Type::Any)),
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "callback".to_string(),
                        ty: Type::Any, // Function type
                        optional: false,
                        default_value: None,
                    },
                    ParamDescriptor {
                        name: "initial".to_string(),
                        ty: Type::Any,
                        optional: true,
                        default_value: None,
                    },
                ],
                return_type: Type::Any,
                description: Some("Reduce array to single value".to_string()),
                category: FunctionCategory::Array,
                is_user_defined: false,
            },
        );

        // JSON functions
        inner.builtins.insert(
            "parse".to_string(),
            FunctionEntry {
                name: "parse".to_string(),
                params: vec![ParamDescriptor {
                    name: "json".to_string(),
                    ty: Type::String,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::Any,
                description: Some("Parse JSON string".to_string()),
                category: FunctionCategory::Json,
                is_user_defined: false,
            },
        );

        inner.builtins.insert(
            "stringify".to_string(),
            FunctionEntry {
                name: "stringify".to_string(),
                params: vec![ParamDescriptor {
                    name: "value".to_string(),
                    ty: Type::Any,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::String,
                description: Some("Convert value to JSON string".to_string()),
                category: FunctionCategory::Json,
                is_user_defined: false,
            },
        );

        // Type functions
        inner.builtins.insert(
            "type_of".to_string(),
            FunctionEntry {
                name: "type_of".to_string(),
                params: vec![ParamDescriptor {
                    name: "value".to_string(),
                    ty: Type::Any,
                    optional: false,
                    default_value: None,
                }],
                return_type: Type::String,
                description: Some("Get the type name of a value".to_string()),
                category: FunctionCategory::Core,
                is_user_defined: false,
            },
        );
    }

    /// Register a function in the registry.
    pub fn register(&self, entry: FunctionEntry) {
        let mut inner = self.inner.write().unwrap();
        inner.functions.insert(entry.name.clone(), entry);
    }

    /// Register a function from a FunctionSig (for backward compatibility).
    pub fn register_from_sig(&self, sig: &FunctionSig, is_user_defined: bool) {
        let entry = FunctionEntry {
            name: sig.name.clone(),
            params: sig
                .params
                .iter()
                .enumerate()
                .map(|(i, name)| ParamDescriptor {
                    name: name.clone(),
                    ty: sig.param_types.get(i).cloned().unwrap_or(Type::Any),
                    optional: false,
                    default_value: None,
                })
                .collect(),
            return_type: sig.ret.clone(),
            description: None,
            category: if is_user_defined {
                FunctionCategory::UserDefined
            } else {
                FunctionCategory::Core
            },
            is_user_defined,
        };
        self.register(entry);
    }

    /// Look up a function by name.
    pub fn get(&self, name: &str) -> Option<FunctionEntry> {
        let inner = self.inner.read().unwrap();
        // Check user functions first, then builtins
        inner
            .functions
            .get(name)
            .or_else(|| inner.builtins.get(name))
            .cloned()
    }

    /// Check if a function exists in the registry.
    pub fn contains(&self, name: &str) -> bool {
        let inner = self.inner.read().unwrap();
        inner.functions.contains_key(name) || inner.builtins.contains_key(name)
    }

    /// Get all registered function names.
    pub fn function_names(&self) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        let mut names: Vec<String> = inner
            .functions
            .keys()
            .chain(inner.builtins.keys())
            .cloned()
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Get all functions in a category.
    pub fn functions_in_category(&self, category: FunctionCategory) -> Vec<FunctionEntry> {
        let inner = self.inner.read().unwrap();
        inner
            .functions
            .values()
            .chain(inner.builtins.values())
            .filter(|f| f.category == category)
            .cloned()
            .collect()
    }

    /// Get all user-defined functions.
    pub fn user_functions(&self) -> Vec<FunctionEntry> {
        let inner = self.inner.read().unwrap();
        inner
            .functions
            .values()
            .filter(|f| f.is_user_defined)
            .cloned()
            .collect()
    }

    /// Get all built-in functions.
    pub fn builtin_functions(&self) -> Vec<FunctionEntry> {
        let inner = self.inner.read().unwrap();
        inner.builtins.values().cloned().collect()
    }

    /// Clear all user-defined functions (but keep builtins).
    pub fn clear_user_functions(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.functions.clear();
    }

    /// Create a new registry with only built-ins (for testing).
    pub fn empty() -> Self {
        Self::new()
    }
}

impl Default for FunctionRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_builtins() {
        let registry = FunctionRegistry::with_builtins();
        assert!(registry.contains("log"));
        assert!(registry.contains("len"));
        assert!(registry.contains("to_string"));
    }

    #[test]
    fn registry_register_user_function() {
        let registry = FunctionRegistry::with_builtins();
        let entry = FunctionEntry {
            name: "my_func".to_string(),
            params: vec![ParamDescriptor {
                name: "x".to_string(),
                ty: Type::Number,
                optional: false,
                default_value: None,
            }],
            return_type: Type::Number,
            description: Some("My custom function".to_string()),
            category: FunctionCategory::UserDefined,
            is_user_defined: true,
        };
        registry.register(entry);
        assert!(registry.contains("my_func"));
        assert!(registry.get("my_func").unwrap().is_user_defined);
    }

    #[test]
    fn registry_user_functions_list() {
        let registry = FunctionRegistry::with_builtins();
        let entry = FunctionEntry {
            name: "imported_func".to_string(),
            params: vec![],
            return_type: Type::Any,
            description: None,
            category: FunctionCategory::UserDefined,
            is_user_defined: true,
        };
        registry.register(entry);
        let user_fns = registry.user_functions();
        assert!(user_fns.iter().any(|f| f.name == "imported_func"));
    }
}
