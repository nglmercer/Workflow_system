//! Method and property tables for Flow primitive types.
//!
//! `.flow` is a workflow DSL, not a general-purpose language — there's
//! no need to ship a full standard library. But for the basic
//! primitives (`string`, `number`, `bool`, `array`, `object`) we
//! provide a small, opinionated set of methods/properties that makes
//! the language feel like a real scripting surface for the common
//! "shape-of-data" operations a rule body actually does:
//!
//! - `string.length`, `string.toUpperCase()`, `string.toLowerCase()`,
//!   `string.trim()`, `string.contains(needle)`, `string.startsWith(p)`,
//!   `string.endsWith(p)`, `string.replace(from, to)`, `string.split(sep)`,
//!   `string.toNumber()`.
//! - `number.toString()`, `number.toFixed(digits)`.
//! - `bool.toString()`.
//! - `array.length`, `array.contains(needle)`, `array.first()`,
//!   `array.last()`, `array.reverse()`, `array.join(sep)`.
//! - `object.keys()`, `object.values()`.
//!
//! Methods and properties are kept separate because properties
//! (`length`) don't take arguments and don't need parentheses, while
//! methods do. Both lists are referenced by the type-aware completion
//! builder and the `Expr::Member` inference so hover and completions
//! agree on the shape of the type.

use super::ty::Type;

/// A method exposed on a primitive type. The signature is positional
/// (no overloads, no optional args, no varargs) — the workflow DSL
/// doesn't have any of those concepts so we don't model them.
#[derive(Debug, Clone)]
pub struct Method {
    pub name: &'static str,
    /// Parameter types, in order. Used for documentation and for the
    /// `Expr::Call` arg-type check (which we don't implement today
    /// but the data is here when we do).
    pub params: &'static [Type],
    /// The method's return type.
    pub ret: Type,
    /// Short single-line summary, surfaced in completion and hover.
    pub doc: &'static str,
}

/// A read-only property exposed on a primitive type. Distinct from
/// methods because it doesn't take arguments.
#[derive(Debug, Clone)]
pub struct Property {
    /// The property's name. Owned so we can synthesize properties
    /// from JSON-schema object keys (which are user strings, not
    /// `'static`).
    pub name: String,
    pub ty: Type,
    pub doc: &'static str,
}

// Method tables can't be `const` because `Type::Array(Box::new(...))`
// uses a non-const `Box::new`. We build them once per call and return
// a borrowed slice. The functions are pure so the cost is a few
// short-lived allocations on first use per LSP session; once warmed
// up the static slices are reused.

fn string_methods() -> Vec<Method> {
    vec![
        Method {
            name: "toUpperCase",
            params: &[],
            ret: Type::String,
            doc: "Returns the string with all characters upper-cased.",
        },
        Method {
            name: "toLowerCase",
            params: &[],
            ret: Type::String,
            doc: "Returns the string with all characters lower-cased.",
        },
        Method {
            name: "trim",
            params: &[],
            ret: Type::String,
            doc: "Returns the string with leading and trailing whitespace removed.",
        },
        Method {
            name: "contains",
            params: &[Type::String],
            ret: Type::Bool,
            doc: "True if the string contains `needle` as a substring.",
        },
        Method {
            name: "startsWith",
            params: &[Type::String],
            ret: Type::Bool,
            doc: "True if the string starts with `prefix`.",
        },
        Method {
            name: "endsWith",
            params: &[Type::String],
            ret: Type::Bool,
            doc: "True if the string ends with `suffix`.",
        },
        Method {
            name: "replace",
            params: &[Type::String, Type::String],
            ret: Type::String,
            doc: "Returns a new string with the first `from` replaced by `to`.",
        },
        Method {
            name: "split",
            params: &[Type::String],
            ret: Type::Array(Box::new(Type::String)),
            doc: "Splits the string by `sep` and returns an array of substrings.",
        },
        Method {
            name: "toNumber",
            params: &[],
            ret: Type::Number,
            doc: "Parses the string as a number. Returns 0 on failure.",
        },
    ]
}

fn number_methods() -> Vec<Method> {
    vec![
        Method {
            name: "toString",
            params: &[],
            ret: Type::String,
            doc: "Returns the number rendered as a string.",
        },
        Method {
            name: "toFixed",
            params: &[Type::Number],
            ret: Type::String,
            doc: "Returns the number as a string with `digits` decimal places.",
        },
    ]
}

fn bool_methods() -> Vec<Method> {
    vec![Method {
        name: "toString",
        params: &[],
        ret: Type::String,
        doc: "Returns the boolean as `\"true\"` or `\"false\"`.",
    }]
}

fn array_methods() -> Vec<Method> {
    vec![
        Method {
            name: "contains",
            params: &[Type::Any],
            ret: Type::Bool,
            doc: "True if the array contains `needle` (uses `==` comparison).",
        },
        Method {
            name: "first",
            params: &[],
            ret: Type::Any,
            doc: "Returns the first element, or `null` if the array is empty.",
        },
        Method {
            name: "last",
            params: &[],
            ret: Type::Any,
            doc: "Returns the last element, or `null` if the array is empty.",
        },
        Method {
            name: "reverse",
            params: &[],
            ret: Type::Array(Box::new(Type::Any)),
            doc: "Returns a new array with the elements in reverse order.",
        },
        Method {
            name: "join",
            params: &[Type::String],
            ret: Type::String,
            doc: "Joins the elements with `sep` into a single string.",
        },
    ]
}

fn object_methods() -> Vec<Method> {
    vec![
        Method {
            name: "keys",
            params: &[],
            ret: Type::Array(Box::new(Type::String)),
            doc: "Returns an array of the object's keys.",
        },
        Method {
            name: "values",
            params: &[],
            ret: Type::Array(Box::new(Type::Any)),
            doc: "Returns an array of the object's values.",
        },
    ]
}

/// Return the list of methods available on `ty`. The result is a
/// fresh `Vec`; callers may keep it. The cost is a few short-lived
/// allocations per call but the bodies are tiny (under 10 entries
/// each) so we don't bother with a `OnceLock` cache.
pub fn methods_for(ty: &Type) -> Vec<Method> {
    match ty {
        Type::String => string_methods(),
        Type::Number => number_methods(),
        Type::Bool => bool_methods(),
        Type::Array(_) => array_methods(),
        Type::Object(_) => object_methods(),
        _ => Vec::new(),
    }
}

/// Return the list of properties available on `ty`. Only
/// `string.length` and `array.length` are defined today. Returns a
/// fresh `Vec`; callers may keep it.
pub fn properties_for(ty: &Type) -> Vec<Property> {
    match ty {
        Type::String => vec![Property {
            name: "length".to_string(),
            ty: Type::Number,
            doc: "Number of characters in the string.",
        }],
        Type::Array(_) => vec![Property {
            name: "length".to_string(),
            ty: Type::Number,
            doc: "Number of elements in the array.",
        }],
        // Object properties come from the schema (i.e. an imported
        // `@import data from ...` or an inline object literal). The
        // member access inference resolves them by name, so the
        // completion list can be built from the same field list.
        Type::Object(fields) => fields
            .iter()
            .map(|(k, v)| Property {
                name: k.clone(),
                ty: v.clone(),
                doc: "",
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Look up a single method by name on `ty`. Returns `None` if the
/// method doesn't exist on this type. Returns an owned `Method` (the
/// underlying table is built per call so we can't return a reference
/// into it cheaply).
pub fn method_for(ty: &Type, name: &str) -> Option<Method> {
    methods_for(ty).into_iter().find(|m| m.name == name)
}

/// Look up a single property by name on `ty`. Returns `None` if the
/// property doesn't exist on this type.
pub fn property_for(ty: &Type, name: &str) -> Option<Property> {
    properties_for(ty).into_iter().find(|p| p.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_has_length_and_methods() {
        let props = properties_for(&Type::String);
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].name, "length");
        assert_eq!(props[0].ty, Type::Number);

        let methods = methods_for(&Type::String);
        assert!(methods.iter().any(|m| m.name == "toUpperCase"));
        assert!(methods.iter().any(|m| m.name == "toLowerCase"));
        assert!(methods.iter().any(|m| m.name == "trim"));
        assert!(methods.iter().any(|m| m.name == "contains"));
    }

    #[test]
    fn number_has_to_string() {
        let methods = methods_for(&Type::Number);
        assert!(methods.iter().any(|m| m.name == "toString"));
        assert!(methods.iter().any(|m| m.name == "toFixed"));
    }

    #[test]
    fn array_has_length_and_first() {
        let props = properties_for(&Type::Array(Box::new(Type::Any)));
        assert_eq!(props[0].name, "length");
        assert_eq!(props[0].ty, Type::Number);

        let methods = methods_for(&Type::Array(Box::new(Type::String)));
        assert!(methods.iter().any(|m| m.name == "first"));
        assert!(methods.iter().any(|m| m.name == "join"));
    }

    #[test]
    fn method_for_returns_some_for_known() {
        let m = method_for(&Type::String, "toUpperCase");
        assert!(m.is_some());
        assert_eq!(m.unwrap().ret, Type::String);

        let p = property_for(&Type::String, "length");
        assert!(p.is_some());
        assert_eq!(p.unwrap().ty, Type::Number);
    }

    #[test]
    fn method_for_returns_none_for_unknown() {
        assert!(method_for(&Type::String, "frobnicate").is_none());
        assert!(property_for(&Type::String, "size").is_none());
    }
}
