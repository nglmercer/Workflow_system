//! Dynamic data-schema imports for `.flow` programs.
//!
//! The `data` binding that a workflow's `on EVENT ({a, b})` clause
//! destructures can be statically typed when the schema is known at
//! edit time. The grammar accepts three import forms (relative path,
//! remote URL, and inline object literal) and the parser stores them
//! in [`SchemaSource`]. This module is the synchronous resolver that
//! turns a [`SchemaSource`] into a [`serde_json::Value`] the type
//! checker can use.
//!
//! - [`SchemaSource::Inline`] is always resolved immediately — it
//!   is the value the parser already produced.
//! - [`SchemaSource::Path`] is resolved by reading the file at
//!   `base_dir.join(path)` (or just `path` if it's absolute).
//!   Resolution failures are collected on [`DataSchema::errors`]
//!   rather than aborting inference, so an unloadable schema
//!   degrades gracefully into `Type::Any`.
//! - [`SchemaSource::Url`] is left unresolved by the sync API
//!   because the LSP edit loop is single-threaded. A future async
//!   resolver can fetch the URL on a worker thread and feed the
//!   result back through the state. For now, callers that hit a
//!   `Url` should treat it as `Type::Any`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where an imported data schema comes from. The grammar produces one
/// of these for every `@import data from ...` statement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "value")]
pub enum SchemaSource {
    /// Inline JSON value (typically an object literal). The value is
    /// stored verbatim and used as the schema.
    Inline(serde_json::Value),
    /// A filesystem path. The resolver joins it against the
    /// importing file's directory.
    Path(String),
    /// A `http://` / `https://` URL. The synchronous resolver does
    /// not fetch it; callers should treat the schema as `Type::Any`
    /// (with a diagnostic, if they care to emit one).
    Url(String),
}

impl SchemaSource {
    /// True if the source can be resolved by the sync [`resolve_source`]
    /// helper. URL sources need an async fetch and always return
    /// `false` here.
    pub fn is_sync_resolvable(&self) -> bool {
        !matches!(self, SchemaSource::Url(_))
    }

    /// The raw path or URL string, when the source is one of those
    /// two variants. Returns `None` for inline sources.
    pub fn path_or_url(&self) -> Option<&str> {
        match self {
            SchemaSource::Path(p) | SchemaSource::Url(p) => Some(p),
            SchemaSource::Inline(_) => None,
        }
    }
}

/// A resolved data schema, including the resolved JSON value (if
/// resolution succeeded) and any errors that came up. The LSP builds
/// one of these per `program.imports` entry and uses the resolved
/// value to type the import's binding in [`crate::inference`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSchema {
    /// The binding name (`"data"` for `@import data from ...`, or the
    /// user-chosen name for `import <name> from ...`).
    pub name: String,
    /// The original source the user wrote.
    pub source: SchemaSource,
    /// The resolved JSON value, if `resolve_source` succeeded. When
    /// `None`, callers should fall back to `Type::Any` for the
    /// binding and may surface `errors` as diagnostics.
    pub resolved: Option<serde_json::Value>,
    /// Resolution errors (file-not-found, bad JSON, URL source in a
    /// sync context, …). The struct is still produced on error so
    /// the caller can attach a diagnostic without losing the
    /// binding.
    pub errors: Vec<String>,
}

impl DataSchema {
    /// Convenience constructor for an inline (object literal) import.
    pub fn from_inline(name: impl Into<String>, value: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            source: SchemaSource::Inline(value),
            resolved: None,
            errors: Vec::new(),
        }
    }

    /// Convenience constructor for a path-or-URL string. The
    /// resolver picks `Url` for `http(s)://` and `Path` otherwise.
    pub fn from_path_or_url(name: impl Into<String>, raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let source = if raw.starts_with("http://") || raw.starts_with("https://") {
            SchemaSource::Url(raw)
        } else {
            SchemaSource::Path(raw)
        };
        Self {
            name: name.into(),
            source,
            resolved: None,
            errors: Vec::new(),
        }
    }
}

/// Resolve a [`SchemaSource`] to a [`serde_json::Value`], using
/// `base_dir` as the root for relative paths. The function is
/// synchronous: it does not fetch URLs (see [`SchemaSource::Url`]).
pub fn resolve_source(
    source: &SchemaSource,
    base_dir: Option<&Path>,
) -> Result<serde_json::Value, String> {
    match source {
        SchemaSource::Inline(v) => Ok(v.clone()),
        SchemaSource::Path(p) => {
            let candidate = if Path::new(p).is_absolute() {
                PathBuf::from(p)
            } else if let Some(base) = base_dir {
                base.join(p)
            } else {
                PathBuf::from(p)
            };
            let s = std::fs::read_to_string(&candidate).map_err(|e| {
                format!(
                    "Failed to read schema at {}: {}",
                    candidate.display(),
                    e
                )
            })?;
            serde_json::from_str(&s).map_err(|e| {
                format!(
                    "Failed to parse schema at {}: {}",
                    candidate.display(),
                    e
                )
            })
        }
        SchemaSource::Url(u) => Err(format!(
            "Schema at URL {} cannot be resolved synchronously; use a relative path or inline object for now",
            u
        )),
    }
}

/// Resolve every import in `imports` to a [`DataSchema`]. The
/// returned vector is the same length and order as `imports`. Any
/// resolution failure is collected on the per-entry `errors` vector
/// rather than aborting — this lets the LSP keep producing
/// completions and hovers even when one schema is unloadable.
pub fn resolve_imports(
    imports: &[(String, SchemaSource)],
    base_dir: Option<&Path>,
) -> Vec<DataSchema> {
    imports
        .iter()
        .map(|(name, source)| {
            let mut schema = DataSchema {
                name: name.clone(),
                source: source.clone(),
                resolved: None,
                errors: Vec::new(),
            };
            match resolve_source(source, base_dir) {
                Ok(v) => schema.resolved = Some(v),
                Err(e) => schema.errors.push(e),
            }
            schema
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_schema(contents: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "workflow-system-schema-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("schema.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn inline_source_resolves_immediately() {
        let value = serde_json::json!({ "users": [], "meta": [] });
        let source = SchemaSource::Inline(value.clone());
        let resolved = resolve_source(&source, None).unwrap();
        assert_eq!(resolved, value);
    }

    #[test]
    fn path_source_resolves_relative_to_base_dir() {
        let path = write_temp_schema(r#"{ "users": [], "meta": [] }"#);
        let base = path.parent().unwrap();
        let source = SchemaSource::Path("schema.json".to_string());
        let resolved = resolve_source(&source, Some(base)).unwrap();
        assert!(resolved.get("users").is_some());
    }

    #[test]
    fn path_source_missing_file_is_error_not_panic() {
        let source = SchemaSource::Path("/this/path/does/not/exist.json".to_string());
        let result = resolve_source(&source, None);
        assert!(result.is_err());
    }

    #[test]
    fn url_source_is_not_sync_resolvable() {
        let source = SchemaSource::Url("https://example.com/schema.json".to_string());
        assert!(!source.is_sync_resolvable());
        let result = resolve_source(&source, None);
        assert!(result.is_err());
    }

    #[test]
    fn from_path_or_url_distinguishes_url_from_path() {
        let from_url = DataSchema::from_path_or_url("data", "https://example.com/schema.json");
        assert!(matches!(from_url.source, SchemaSource::Url(_)));
        let from_path = DataSchema::from_path_or_url("data", "./schema.json");
        assert!(matches!(from_path.source, SchemaSource::Path(_)));
    }

    #[test]
    fn resolve_imports_returns_one_per_input_in_order() {
        let imports = vec![
            (
                "a".to_string(),
                SchemaSource::Inline(serde_json::json!({ "x": 1 })),
            ),
            ("b".to_string(), SchemaSource::Url("https://x".into())),
        ];
        let out = resolve_imports(&imports, None);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "a");
        assert!(out[0].resolved.is_some());
        assert!(out[0].errors.is_empty());
        assert_eq!(out[1].name, "b");
        assert!(out[1].resolved.is_none());
        assert!(!out[1].errors.is_empty());
    }
}
