//! Inferred Flow types and their string labels.

/// A Flow type. Kept intentionally small — we don't try to be a full
/// Hindley-Milner engine, we just need enough precision for hover and
/// completion to be useful.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    String,
    Number,
    Bool,
    Null,
    Array(Box<Type>),
    Object(Vec<(String, Type)>),
    Function { params: Vec<Type>, ret: Box<Type> },
    Any,
}

impl Type {
    /// Short single-token label like `string`, `number`, `T[]`, `{a:T, b:U}`.
    pub fn label(&self) -> String {
        match self {
            Type::String => "string".to_string(),
            Type::Number => "number".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Null => "null".to_string(),
            Type::Array(inner) => format!("{}[]", inner.label()),
            Type::Object(fields) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.label()))
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            }
            Type::Function { params, ret } => {
                let p: Vec<String> = params.iter().map(|p| p.label()).collect();
                format!("({}) -> {}", p.join(", "), ret.label())
            }
            Type::Any => "any".to_string(),
        }
    }
}
