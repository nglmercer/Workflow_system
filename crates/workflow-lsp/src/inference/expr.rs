//! Expression-level type and value inference.
//!
//! These functions walk a single `Expr` and produce a `(Type, Option<Value>)`
//! pair. They do not mutate any global state, so they're easy to unit-test
//! in isolation.

use std::collections::HashMap;

use workflow_parser::ast::{BinaryOp, Expr, InterpPart, UnaryOp};

use super::builtins::builtin_call_return;
use super::ty::Type;
use super::value::{FunctionSig, InferredBinding, Value};

/// Infer the type of an expression with no surrounding scope. Returns
/// `(Type, Option<Value>)` where `Value` is `Some` only when the
/// expression is a literal (or constant composition of literals).
pub fn infer_expr(expr: &Expr) -> (Type, Option<Value>) {
    let empty: HashMap<String, FunctionSig> = HashMap::new();
    let empty_scope: Vec<InferredBinding> = Vec::new();
    infer_expr_with_ctx(expr, &empty_scope, &empty, &[])
}

pub fn infer_expr_with_ctx(
    expr: &Expr,
    scope: &[InferredBinding],
    functions: &HashMap<String, FunctionSig>,
    outer_scope: &[InferredBinding],
) -> (Type, Option<Value>) {
    match expr {
        Expr::String(s) => (Type::String, Some(Value::String(s.clone()))),
        Expr::Number(n) => (Type::Number, Some(Value::Number(*n))),
        Expr::Bool(b) => (Type::Bool, Some(Value::Bool(*b))),
        Expr::Null => (Type::Null, Some(Value::Null)),
        Expr::Var(name) => {
            if let Some(b) = scope.iter().find(|b| &b.name == name) {
                (b.ty.clone(), b.value.clone())
            } else if let Some(b) = outer_scope.iter().find(|b| &b.name == name) {
                (b.ty.clone(), b.value.clone())
            } else if let Some(sig) = functions.get(name) {
                (
                    Type::Function {
                        params: sig.param_types.clone(),
                        ret: Box::new(sig.ret.clone()),
                    },
                    None,
                )
            } else {
                (Type::Any, None)
            }
        }
        Expr::Member { object, property } => {
            let (obj_ty, _) = infer_expr_with_ctx(object, scope, functions, outer_scope);
            if let Type::Object(fields) = obj_ty {
                if let Some((_, t)) = fields.iter().find(|(k, _)| k == property) {
                    return (t.clone(), None);
                }
            }
            (Type::Any, None)
        }
        Expr::BinaryOp { op, left, right } => {
            let (lt, lv) = infer_expr_with_ctx(left, scope, functions, outer_scope);
            let (rt, rv) = infer_expr_with_ctx(right, scope, functions, outer_scope);
            infer_binary(op.clone(), &lt, &rt, lv.as_ref(), rv.as_ref())
        }
        Expr::UnaryOp { op, operand } => {
            let (t, _) = infer_expr_with_ctx(operand, scope, functions, outer_scope);
            match op {
                UnaryOp::Not => (Type::Bool, None),
                UnaryOp::Neg => (t, None),
            }
        }
        Expr::Call { name, args } => {
            if let Some(sig) = functions.get(name) {
                (sig.ret.clone(), None)
            } else {
                // Built-ins: known signatures, no value fold.
                (builtin_call_return(name, args.len()), None)
            }
        }
        Expr::Array(elements) => {
            let mut inner: Option<Type> = None;
            let mut values: Vec<Value> = Vec::new();
            let mut all_folded = true;
            for e in elements {
                let (t, v) = infer_expr_with_ctx(e, scope, functions, outer_scope);
                inner = Some(match inner {
                    None => t,
                    Some(prev) if prev == t => t,
                    Some(_) => Type::Any,
                });
                match v {
                    Some(val) => values.push(val),
                    None => all_folded = false,
                }
            }
            (
                Type::Array(Box::new(inner.unwrap_or(Type::Any))),
                if all_folded {
                    Some(Value::Array(values))
                } else {
                    None
                },
            )
        }
        Expr::InterpolatedString(parts) => {
            for part in parts {
                if let InterpPart::Expr(e) = part {
                    let _ = infer_expr_with_ctx(e, scope, functions, outer_scope);
                }
            }
            (Type::String, None)
        }
    }
}

fn infer_binary(
    op: BinaryOp,
    lt: &Type,
    rt: &Type,
    lv: Option<&Value>,
    rv: Option<&Value>,
) -> (Type, Option<Value>) {
    use BinaryOp::*;
    use Value::*;
    match op {
        Add if matches!(lt, Type::String) || matches!(rt, Type::String) => {
            let value = match (lv, rv) {
                (Some(String(a)), Some(String(b))) => Some(String(format!("{}{}", a, b))),
                (Some(Number(a)), Some(String(b))) => Some(String(format!("{}{}", a, b))),
                (Some(String(a)), Some(Number(b))) => Some(String(format!("{}{}", a, b))),
                _ => None,
            };
            (Type::String, value)
        }
        Add | Sub | Mul | Div | Mod => {
            let value = match (lv, rv) {
                (Some(Number(a)), Some(Number(b))) => Some(match op {
                    Add => Number(a + b),
                    Sub => Number(a - b),
                    Mul => Number(a * b),
                    Div => Number(a / b),
                    Mod => Number(a % b),
                    _ => unreachable!(),
                }),
                _ => None,
            };
            (Type::Number, value)
        }
        Eq | Neq => (Type::Bool, None),
        Lt | Gt | Lte | Gte => (Type::Bool, None),
        And | Or => (Type::Bool, None),
    }
}
