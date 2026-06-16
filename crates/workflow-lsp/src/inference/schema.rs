//! Schema-to-type conversion for `@import data from ...` bindings.
//!
//! When the user writes
//!
//! ```flow
//! @import data from "./schema.json"
//! ```
//!
//! the parser produces an `ImportStmt` with the path, and the LSP's
//! `update_document` resolves it via [`resolve_schemas_for_program`]
//! (defined in this module) and pushes the result as an
//! [`InferredBinding`] into every line of the inference scope. From
//! there the rest of the inference / completion / hover machinery
//! picks it up: `data.users` resolves to whatever the JSON schema
//! says, member completions come from the [`Type::Object`] keys, and
//! workflow destructure params can be typed from the schema too.

use std::path::Path;

use workflow_domain::schema::{DataSchema, SchemaSource};
use workflow_parser::ast::ImportSource;

use super::ty::Type;
use super::value::InferredBinding;

/// Convert a [`serde_json::Value`] to a Flow [`Type`]. The mapping is:
///
/// | JSON | Flow `Type` |
/// | --- | --- |
/// | `null` | `Type::Null` |
/// | `true` / `false` | `Type::Bool` |
/// | number | `Type::Number` |
/// | string | `Type::String` |
/// | array | `Type::Array(<first element>)` (homogeneous-array assumption) |
/// | object | `Type::Object(<key→type>)` |
///
/// Arrays with zero elements become `Type::Array(Type::Any)`. Any
/// shape the user provides is mapped recursively.
pub fn json_to_type(value: &serde_json::Value) -> Type {
    match value {
        serde_json::Value::Null => Type::Null,
        serde_json::Value::Bool(_) => Type::Bool,
        serde_json::Value::Number(_) => Type::Number,
        serde_json::Value::String(_) => Type::String,
        serde_json::Value::Array(items) => {
            let inner = items.first().map(json_to_type).unwrap_or(Type::Any);
            Type::Array(Box::new(inner))
        }
        serde_json::Value::Object(map) => {
            let fields = map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_type(v)))
                .collect();
            Type::Object(fields)
        }
    }
}

/// Resolve every `ImportStmt` in the program to a `DataSchema` plus
/// the inferred `InferredBinding` to push into the inference scope.
/// The base directory comes from the document's filesystem path so
/// relative imports resolve against the `.flow` file's location.
pub fn resolve_schemas_for_program(
    imports: &[workflow_parser::ast::ImportStmt],
    document_path: Option<&str>,
) -> (Vec<DataSchema>, Vec<InferredBinding>) {
    let base_dir = document_path.and_then(|p| Path::new(p).parent());
    let pairs: Vec<(String, SchemaSource)> = imports
        .iter()
        .map(|import| {
            let source = match &import.source {
                ImportSource::Path(p) => {
                    if p.starts_with("http://") || p.starts_with("https://") {
                        SchemaSource::Url(p.clone())
                    } else {
                        SchemaSource::Path(p.clone())
                    }
                }
                ImportSource::Inline(v) => SchemaSource::Inline(v.clone()),
            };
            (import.name.clone(), source)
        })
        .collect();

    let schemas = workflow_domain::schema::resolve_imports(&pairs, base_dir);
    let bindings = schemas
        .iter()
        .filter_map(|s| {
            let value = s.resolved.as_ref()?;
            let ty = json_to_type(value);
            Some(InferredBinding {
                name: s.name.clone(),
                ty,
                value: None,
                annotated: true,
            })
        })
        .collect();
    (schemas, bindings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn primitives_map_one_to_one() {
        assert_eq!(json_to_type(&json!(null)), Type::Null);
        assert_eq!(json_to_type(&json!(true)), Type::Bool);
        assert_eq!(json_to_type(&json!(42)), Type::Number);
        assert_eq!(json_to_type(&json!("x")), Type::String);
    }

    #[test]
    fn object_becomes_object_type_with_field_types() {
        let value = json!({ "users": [], "meta": { "count": 0, "source": "" } });
        let ty = json_to_type(&value);
        match ty {
            Type::Object(fields) => {
                assert_eq!(fields.len(), 2);
                let users = fields.iter().find(|(k, _)| k == "users").unwrap();
                assert!(matches!(users.1, Type::Array(_)));
                let meta = fields.iter().find(|(k, _)| k == "meta").unwrap();
                match &meta.1 {
                    Type::Object(inner) => {
                        assert!(inner.iter().any(|(k, _)| k == "count"));
                        assert!(inner.iter().any(|(k, _)| k == "source"));
                    }
                    other => panic!("expected inner object, got {:?}", other),
                }
            }
            other => panic!("expected object, got {:?}", other),
        }
    }

    #[test]
    fn empty_array_becomes_array_of_any() {
        let ty = json_to_type(&json!([]));
        assert_eq!(ty, Type::Array(Box::new(Type::Any)));
    }

    #[test]
    fn array_of_objects_uses_first_object_as_inner_type() {
        let ty = json_to_type(&json!([{ "a": 1 }, { "a": 2 }]));
        match ty {
            Type::Array(inner) => {
                assert!(matches!(*inner, Type::Object(_)));
            }
            other => panic!("expected array, got {:?}", other),
        }
    }

    #[test]
    fn empty_array_schema_resolves_to_array_of_any() {
        // An inline array of nothing should still produce a
        // well-typed binding (Array of Any) so member completion
        // and length access still work.
        let value = json!([]);
        assert_eq!(json_to_type(&value), Type::Array(Box::new(Type::Any)));
    }

    #[test]
    fn multiple_inline_imports_do_not_clobber_each_other() {
        // Each `@import` is a separate binding. The two schemas
        // produce two distinct `InferredBinding`s in the same
        // order as the imports; nothing overwrites anything.
        use workflow_parser::ast::{ImportSource, ImportStmt};
        let imports = vec![
            ImportStmt {
                name: "USER_REGISTERED".to_string(),
                source: ImportSource::Inline(json!({ "email": "", "plan": "" })),
            },
            ImportStmt {
                name: "BATCH_START".to_string(),
                source: ImportSource::Inline(json!({ "items": [] })),
            },
        ];
        let (schemas, bindings) = resolve_schemas_for_program(&imports, None);
        assert_eq!(schemas.len(), 2);
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].name, "USER_REGISTERED");
        assert_eq!(bindings[1].name, "BATCH_START");
        // The first binding should be an object with `email` and
        // `plan` keys, the second with an `items` key. Neither
        // overwrites the other.
        match &bindings[0].ty {
            Type::Object(fields) => {
                assert!(fields.iter().any(|(k, _)| k == "email"));
                assert!(fields.iter().any(|(k, _)| k == "plan"));
                assert!(!fields.iter().any(|(k, _)| k == "items"));
            }
            other => panic!("expected object, got {:?}", other),
        }
        match &bindings[1].ty {
            Type::Object(fields) => {
                assert!(fields.iter().any(|(k, _)| k == "items"));
                assert!(!fields.iter().any(|(k, _)| k == "email"));
            }
            other => panic!("expected object, got {:?}", other),
        }
    }

    #[test]
    fn relative_path_import_resolves_against_document_dir() {
        // Pass a `document_path` so the resolver can find a
        // sibling schema file. The base directory is the parent
        // of the document; we use the existing
        // `examples/nested_data.json` file as the target.
        use workflow_parser::ast::{ImportSource, ImportStmt};
        let doc =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/advanced.flow");
        let imports = vec![ImportStmt {
            name: "data".to_string(),
            source: ImportSource::Path("./nested_data.json".to_string()),
        }];
        let (_schemas, bindings) = resolve_schemas_for_program(&imports, doc.to_str());
        assert_eq!(bindings.len(), 1);
        assert!(matches!(bindings[0].ty, Type::Object(_)));
    }
}
