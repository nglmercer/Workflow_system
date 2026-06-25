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
    /// The name of the plugin that registered this function, if any.
    pub plugin_name: Option<String>,
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
    pub fn label(&self) -> String {
        match self {
            FunctionCategory::Core => workflow_i18n::t("lsp.category_core"),
            FunctionCategory::String => workflow_i18n::t("lsp.category_string"),
            FunctionCategory::Math => workflow_i18n::t("lsp.category_math"),
            FunctionCategory::Array => workflow_i18n::t("lsp.category_array"),
            FunctionCategory::Object => workflow_i18n::t("lsp.category_object"),
            FunctionCategory::Conversion => workflow_i18n::t("lsp.category_conversion"),
            FunctionCategory::DateTime => workflow_i18n::t("lsp.category_date_time"),
            FunctionCategory::Network => workflow_i18n::t("lsp.category_network"),
            FunctionCategory::Json => workflow_i18n::t("lsp.category_json"),
            FunctionCategory::UserDefined => workflow_i18n::t("lsp.category_user_defined"),
            FunctionCategory::Custom => workflow_i18n::t("lsp.category_custom"),
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
        registry.register_remaining_stdlib();
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
                description: Some(workflow_i18n::t("lsp.fn.message.description")),
                category: FunctionCategory::Core,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.value.description")),
                category: FunctionCategory::Core,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.value.description")),
                category: FunctionCategory::Conversion,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.value.description")),
                category: FunctionCategory::Conversion,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.event.description")),
                category: FunctionCategory::Core,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.strings.description")),
                category: FunctionCategory::String,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.separator.description")),
                category: FunctionCategory::String,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.separator.description")),
                category: FunctionCategory::String,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.string.description")),
                category: FunctionCategory::String,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.string.description")),
                category: FunctionCategory::String,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.string.description")),
                category: FunctionCategory::String,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.to.description")),
                category: FunctionCategory::String,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.substring.description")),
                category: FunctionCategory::String,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.length.description")),
                category: FunctionCategory::String,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.value.description")),
                category: FunctionCategory::Math,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.value.description")),
                category: FunctionCategory::Math,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.value.description")),
                category: FunctionCategory::Math,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.value.description")),
                category: FunctionCategory::Math,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.b.description")),
                category: FunctionCategory::Math,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.b.description")),
                category: FunctionCategory::Math,
                is_user_defined: false,
                plugin_name: None,
            },
        );

        inner.builtins.insert(
            "random".to_string(),
            FunctionEntry {
                name: "random".to_string(),
                params: vec![],
                return_type: Type::Number,
                description: Some(workflow_i18n::t("lsp.fn.random.description")),
                category: FunctionCategory::Math,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.array.description")),
                category: FunctionCategory::Array,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.array.description")),
                category: FunctionCategory::Array,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.element.description")),
                category: FunctionCategory::Array,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.array.description")),
                category: FunctionCategory::Array,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.callback.description")),
                category: FunctionCategory::Array,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.callback.description")),
                category: FunctionCategory::Array,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.initial.description")),
                category: FunctionCategory::Array,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.json.description")),
                category: FunctionCategory::Json,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.value.description")),
                category: FunctionCategory::Json,
                is_user_defined: false,
                plugin_name: None,
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
                description: Some(workflow_i18n::t("lsp.fn.value.description")),
                category: FunctionCategory::Core,
                is_user_defined: false,
                plugin_name: None,
            },
        );
    }

    /// Register the remaining standard library functions that were
    /// previously suppressed by the hardcoded keyword list.
    /// These are lightweight `Type::Any` entries — enough for the
    /// LSP to recognise them, provide completions, and suppress
    /// "unknown function" diagnostics.
    pub fn register_remaining_stdlib(&self) {
        let f = |s: &Self,
                 name: &str,
                 params: Vec<&str>,
                 ret: Type,
                 cat: FunctionCategory,
                 desc: &str| {
            s.register(FunctionEntry {
                name: name.to_string(),
                params: params
                    .into_iter()
                    .map(|p| ParamDescriptor {
                        name: p.to_string(),
                        ty: Type::Any,
                        optional: false,
                        default_value: None,
                    })
                    .collect(),
                return_type: ret,
                description: Some(desc.to_string()),
                category: cat,
                is_user_defined: false,
                plugin_name: None,
            });
        };

        // ── String ──────────────────────────────────────────
        f(self, "starts_with", vec!["string", "prefix"], Type::Bool, FunctionCategory::String, "Check if string starts with prefix");
        f(self, "ends_with", vec!["string", "suffix"], Type::Bool, FunctionCategory::String, "Check if string ends with suffix");
        f(self, "indexOf", vec!["string", "substring"], Type::Number, FunctionCategory::String, "Return index of substring (-1 if not found)");
        f(self, "formatCurrency", vec!["amount", "currency"], Type::String, FunctionCategory::String, "Format a number as currency");
        f(self, "validateEmail", vec!["email"], Type::Bool, FunctionCategory::String, "Validate an email address");

        // ── Array ───────────────────────────────────────────
        f(self, "shift", vec!["array"], Type::Any, FunctionCategory::Array, "Remove and return the first element");
        f(self, "unshift", vec!["array", "element"], Type::Any, FunctionCategory::Array, "Prepend an element to the array");
        f(self, "splice", vec!["array", "start", "delete_count"], Type::Array(Box::new(Type::Any)), FunctionCategory::Array, "Splice elements from an array");
        f(self, "slice", vec!["array", "start"], Type::Array(Box::new(Type::Any)), FunctionCategory::Array, "Return a shallow copy of a portion of an array");
        f(self, "find", vec!["array", "callback"], Type::Any, FunctionCategory::Array, "Find the first element matching a predicate");
        f(self, "some", vec!["array", "callback"], Type::Bool, FunctionCategory::Array, "Test whether any element passes a predicate");
        f(self, "every", vec!["array", "callback"], Type::Bool, FunctionCategory::Array, "Test whether all elements pass a predicate");
        f(self, "keys", vec!["object"], Type::Array(Box::new(Type::String)), FunctionCategory::Core, "Return an object's keys");
        f(self, "values", vec!["object"], Type::Array(Box::new(Type::Any)), FunctionCategory::Core, "Return an object's values");
        f(self, "entries", vec!["object"], Type::Array(Box::new(Type::Any)), FunctionCategory::Core, "Return an object's [key, value] pairs");
        f(self, "has", vec!["object", "key"], Type::Bool, FunctionCategory::Core, "Check if an object has a key");

        // ── Type / number ───────────────────────────────────
        f(self, "is_nan", vec!["value"], Type::Bool, FunctionCategory::Core, "Check if value is NaN");
        f(self, "is_finite", vec!["value"], Type::Bool, FunctionCategory::Core, "Check if value is finite");

        // ── Async / IO ──────────────────────────────────────
        f(self, "sleep", vec!["ms"], Type::Any, FunctionCategory::Core, "Sleep for the given milliseconds");
        f(self, "fetch", vec!["url"], Type::Any, FunctionCategory::Core, "Fetch a URL");

        // ── HTTP ────────────────────────────────────────────
        f(self, "http_get", vec!["url"], Type::Any, FunctionCategory::Core, "HTTP GET request");
        f(self, "http_post", vec!["url", "body"], Type::Any, FunctionCategory::Core, "HTTP POST request");

        // ── JSON (aliases) ──────────────────────────────────
        f(self, "json_parse", vec!["json"], Type::Any, FunctionCategory::Json, "Parse a JSON string");
        f(self, "json_stringify", vec!["value"], Type::String, FunctionCategory::Json, "Serialize a value to JSON");

        // ── Encoding ────────────────────────────────────────
        f(self, "base64_encode", vec!["data"], Type::String, FunctionCategory::Core, "Base64-encode a string");
        f(self, "base64_decode", vec!["data"], Type::String, FunctionCategory::Core, "Base64-decode a string");

        // ── Crypto / ID ─────────────────────────────────────
        f(self, "hash", vec!["data", "algorithm"], Type::String, FunctionCategory::Core, "Hash a string");
        f(self, "uuid", vec![], Type::String, FunctionCategory::Core, "Generate a UUID");

        // ── Random ──────────────────────────────────────────
        f(self, "random_int", vec!["min", "max"], Type::Number, FunctionCategory::Math, "Random integer in [min, max]");
        f(self, "random_float", vec![], Type::Number, FunctionCategory::Math, "Random float in [0, 1)");

        // ── Date / time ─────────────────────────────────────
        f(self, "now", vec![], Type::Number, FunctionCategory::Core, "Current Unix timestamp in milliseconds");
        f(self, "timestamp", vec![], Type::Number, FunctionCategory::Core, "Alias for now()");
        f(self, "date", vec![], Type::String, FunctionCategory::Core, "Current date as ISO-8601 string");
        f(self, "time", vec![], Type::String, FunctionCategory::Core, "Current time as ISO-8601 string");

        // ── Math (extended) ─────────────────────────────────
        f(self, "clamp", vec!["value", "min", "max"], Type::Number, FunctionCategory::Math, "Clamp value to [min, max]");
        f(self, "lerp", vec!["a", "b", "t"], Type::Number, FunctionCategory::Math, "Linear interpolation");
        f(self, "step", vec!["edge", "x"], Type::Number, FunctionCategory::Math, "Step function");
        f(self, "smoothstep", vec!["edge0", "edge1", "x"], Type::Number, FunctionCategory::Math, "Smooth interpolation");
        f(self, "map_range", vec!["value", "in_min", "in_max", "out_min", "out_max"], Type::Number, FunctionCategory::Math, "Map value from one range to another");
        f(self, "remap", vec!["value", "in_min", "in_max", "out_min", "out_max"], Type::Number, FunctionCategory::Math, "Alias for map_range");
        f(self, "normalize", vec!["value", "min", "max"], Type::Number, FunctionCategory::Math, "Normalize value to [0, 1]");
        f(self, "degrees", vec!["radians"], Type::Number, FunctionCategory::Math, "Convert radians to degrees");
        f(self, "radians", vec!["degrees"], Type::Number, FunctionCategory::Math, "Convert degrees to radians");

        // ── Trigonometry ────────────────────────────────────
        f(self, "sin", vec!["x"], Type::Number, FunctionCategory::Math, "Sine");
        f(self, "cos", vec!["x"], Type::Number, FunctionCategory::Math, "Cosine");
        f(self, "tan", vec!["x"], Type::Number, FunctionCategory::Math, "Tangent");
        f(self, "asin", vec!["x"], Type::Number, FunctionCategory::Math, "Arc sine");
        f(self, "acos", vec!["x"], Type::Number, FunctionCategory::Math, "Arc cosine");
        f(self, "atan", vec!["x"], Type::Number, FunctionCategory::Math, "Arc tangent");
        f(self, "atan2", vec!["y", "x"], Type::Number, FunctionCategory::Math, "Arc tangent of y/x");

        // ── Power / log / misc ──────────────────────────────
        f(self, "pow", vec!["base", "exp"], Type::Number, FunctionCategory::Math, "Exponentiation");
        f(self, "sqrt", vec!["x"], Type::Number, FunctionCategory::Math, "Square root");
        f(self, "exp", vec!["x"], Type::Number, FunctionCategory::Math, "e^x");
        f(self, "log2", vec!["x"], Type::Number, FunctionCategory::Math, "Base-2 logarithm");
        f(self, "log10", vec!["x"], Type::Number, FunctionCategory::Math, "Base-10 logarithm");
        f(self, "sign", vec!["x"], Type::Number, FunctionCategory::Math, "Sign of a number (-1, 0, or 1)");
        f(self, "mod", vec!["a", "b"], Type::Number, FunctionCategory::Math, "Modulo");
        f(self, "clamp_min", vec!["value", "min"], Type::Number, FunctionCategory::Math, "Clamp to minimum");
        f(self, "clamp_max", vec!["value", "max"], Type::Number, FunctionCategory::Math, "Clamp to maximum");
        f(self, "abs_diff", vec!["a", "b"], Type::Number, FunctionCategory::Math, "Absolute difference");
        f(self, "signum", vec!["x"], Type::Number, FunctionCategory::Math, "Sign of a number");
        f(self, "hypot", vec!["x", "y"], Type::Number, FunctionCategory::Math, "Hypotenuse");
        f(self, "cbrt", vec!["x"], Type::Number, FunctionCategory::Math, "Cube root");
        f(self, "exp2", vec!["x"], Type::Number, FunctionCategory::Math, "2^x");
        f(self, "expm1", vec!["x"], Type::Number, FunctionCategory::Math, "e^x − 1");
        f(self, "ln_1p", vec!["x"], Type::Number, FunctionCategory::Math, "ln(1 + x)");
        f(self, "log_add_exp", vec!["a", "b"], Type::Number, FunctionCategory::Math, "log(exp(a) + exp(b))");
        f(self, "log_sum_exp", vec!["a", "b"], Type::Number, FunctionCategory::Math, "Alias for log_add_exp");
        f(self, "log1p", vec!["x"], Type::Number, FunctionCategory::Math, "ln(1 + x)");

        // ── Hyperbolic ──────────────────────────────────────
        f(self, "sinh", vec!["x"], Type::Number, FunctionCategory::Math, "Hyperbolic sine");
        f(self, "cosh", vec!["x"], Type::Number, FunctionCategory::Math, "Hyperbolic cosine");
        f(self, "tanh", vec!["x"], Type::Number, FunctionCategory::Math, "Hyperbolic tangent");
        f(self, "asinh", vec!["x"], Type::Number, FunctionCategory::Math, "Inverse hyperbolic sine");
        f(self, "acosh", vec!["x"], Type::Number, FunctionCategory::Math, "Inverse hyperbolic cosine");
        f(self, "atanh", vec!["x"], Type::Number, FunctionCategory::Math, "Inverse hyperbolic tangent");

        // ── Low-level float ─────────────────────────────────
        f(self, "exp_m1", vec!["x"], Type::Number, FunctionCategory::Math, "e^x − 1");
        f(self, "powi", vec!["x", "n"], Type::Number, FunctionCategory::Math, "Integer power");
        f(self, "powf", vec!["x", "y"], Type::Number, FunctionCategory::Math, "Float power");
        f(self, "recip", vec!["x"], Type::Number, FunctionCategory::Math, "1/x");
        f(self, "mul_add", vec!["a", "b", "c"], Type::Number, FunctionCategory::Math, "a * b + c (fused multiply-add)");
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
            plugin_name: None,
        };
        self.register(entry);
    }

    /// Register a plugin function with metadata.
    ///
    /// Plugin functions are registered with `category: FunctionCategory::Custom`
    /// and `is_user_defined: false`. The `plugin_name` field identifies which
    /// plugin registered the function.
    pub fn register_plugin(
        &self,
        name: &str,
        params: Vec<ParamDescriptor>,
        return_type: Type,
        description: Option<String>,
        plugin_name: &str,
    ) {
        let entry = FunctionEntry {
            name: name.to_string(),
            params,
            return_type,
            description,
            category: FunctionCategory::Custom,
            is_user_defined: false,
            plugin_name: Some(plugin_name.to_string()),
        };
        self.register(entry);
    }

    /// Register multiple plugin functions in a single lock acquisition.
    pub fn register_plugin_batch(
        &self,
        plugin_name: &str,
        entries: Vec<FunctionEntry>,
    ) {
        let mut inner = self.inner.write().unwrap();
        for mut entry in entries {
            entry.category = FunctionCategory::Custom;
            entry.is_user_defined = false;
            entry.plugin_name = Some(plugin_name.to_string());
            inner.functions.insert(entry.name.clone(), entry);
        }
    }

    /// Unregister all functions from a specific plugin.
    pub fn unregister_plugin(&self, plugin_name: &str) {
        let mut inner = self.inner.write().unwrap();
        inner
            .functions
            .retain(|_, entry| entry.plugin_name.as_deref() != Some(plugin_name));
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

    /// Get all plugin-registered functions (category == Custom and plugin_name is Some).
    pub fn plugin_functions(&self) -> Vec<FunctionEntry> {
        let inner = self.inner.read().unwrap();
        inner
            .functions
            .values()
            .filter(|f| f.plugin_name.is_some())
            .cloned()
            .collect()
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
            description: Some(workflow_i18n::t("lsp.fn.x.description")),
            category: FunctionCategory::UserDefined,
            is_user_defined: true,
                plugin_name: None,
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
                plugin_name: None,
        };
        registry.register(entry);
        let user_fns = registry.user_functions();
        assert!(user_fns.iter().any(|f| f.name == "imported_func"));
    }
}
