//! Constant-folded values and the bindings/scopes that hold them.

use super::ty::Type;

/// A value we managed to fold out of literal expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Array(Vec<Value>),
}

/// A binding inferred for a particular line in the source.
#[derive(Debug, Clone)]
pub struct InferredBinding {
    pub name: String,
    pub ty: Type,
    /// The literal value, if we could constant-fold it.
    pub value: Option<Value>,
    /// True if the type came from a `//@...` annotation rather than
    /// inference. We render annotations slightly differently in hover.
    pub annotated: bool,
}

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub name: String,
    pub params: Vec<String>,
    pub param_types: Vec<Type>,
    pub ret: Type,
    /// True if the return type was given by an annotation rather than
    /// inferred (default `Any`).
    pub annotated: bool,
}
