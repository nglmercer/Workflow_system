//! Built-in keywords, functions, and their known signatures.

use lsp_types::Position;

use super::ty::Type;
use super::value::InferredBinding;
use crate::analysis::word_at;

/// Look up the return type of a built-in function call. Returns `Any` if
/// the name/arity pair is not a known built-in.
pub fn builtin_call_return(name: &str, arity: usize) -> Type {
    match (name, arity) {
        ("len", 1) => Type::Number,
        ("to_string", 1) => Type::String,
        ("to_number", 1) => Type::Number,
        _ => Type::Any,
    }
}

/// Look up the type of a built-in function's argument. Returns `None` if
/// the function isn't a known built-in, or `Some(Any)` if the built-in
/// accepts multiple shapes (e.g. `len`).
pub fn builtin_arg_type(name: &str, arity: usize) -> Option<Type> {
    match (name, arity) {
        ("len", 1) => Some(Type::Any),
        ("to_string", 1) => Some(Type::Any),
        ("to_number", 1) => Some(Type::String),
        ("log", 1) => Some(Type::Any),
        _ => None,
    }
}

/// A snippet for one of the well-known built-in identifiers, used as
/// fallback when the variable is not in scope.
pub fn builtin_for(word: &str) -> Option<InferredBinding> {
    if !is_builtin_keyword(word) {
        return None;
    }
    Some(InferredBinding {
        name: word.to_string(),
        ty: Type::Any,
        value: None,
        annotated: true,
    })
}

/// Convenience used by the inference engine's `lookup` to pick up a
/// built-in identifier directly from the source text.
#[allow(dead_code)]
pub fn lookup_builtin_at(source: &str, position: Position) -> Option<InferredBinding> {
    let word = word_at(source, position)?;
    builtin_for(&word)
}

fn is_builtin_keyword(word: &str) -> bool {
    matches!(
        word,
        "var"
            | "fn"
            | "workflow"
            | "on"
            | "if"
            | "else"
            | "foreach"
            | "in"
            | "return"
            | "log"
            | "len"
            | "to_string"
            | "to_number"
            | "true"
            | "false"
            | "null"
            | "emit"
            | "formatCurrency"
            | "validateEmail"
            | "greet"
            | "abs"
            | "ceil"
            | "floor"
            | "round"
            | "max"
            | "min"
            | "random"
            | "now"
            | "timestamp"
            | "date"
            | "time"
            | "concat"
            | "join"
            | "split"
            | "trim"
            | "upper"
            | "lower"
            | "replace"
            | "contains"
            | "starts_with"
            | "ends_with"
            | "substr"
            | "indexOf"
            | "sort"
            | "reverse"
            | "push"
            | "pop"
            | "shift"
            | "unshift"
            | "splice"
            | "slice"
            | "map"
            | "filter"
            | "reduce"
            | "find"
            | "some"
            | "every"
            | "keys"
            | "values"
            | "entries"
            | "has"
            | "parse"
            | "stringify"
            | "type_of"
            | "is_nan"
            | "is_finite"
            | "sleep"
            | "fetch"
            | "http_get"
            | "http_post"
            | "json_parse"
            | "json_stringify"
            | "base64_encode"
            | "base64_decode"
            | "hash"
            | "uuid"
            | "random_int"
            | "random_float"
            | "clamp"
            | "lerp"
            | "step"
            | "smoothstep"
            | "map_range"
            | "remap"
            | "normalize"
            | "degrees"
            | "radians"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "atan2"
            | "pow"
            | "sqrt"
            | "exp"
            | "log2"
            | "log10"
            | "sign"
            | "mod"
            | "clamp_min"
            | "clamp_max"
            | "abs_diff"
            | "signum"
            | "hypot"
            | "cbrt"
            | "exp2"
            | "expm1"
            | "ln_1p"
            | "log_add_exp"
            | "log_sum_exp"
            | "log1p"
            | "sinh"
            | "cosh"
            | "tanh"
            | "asinh"
            | "acosh"
            | "atanh"
            | "exp_m1"
            | "powi"
            | "powf"
            | "recip"
            | "mul_add"
    )
}
