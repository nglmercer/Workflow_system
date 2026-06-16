//! Program-level inference: walks a `FlowProgram` and produces the
//! `Inference` result (per-line scopes + function signatures).

use workflow_parser::ast::{Expr, FlowProgram, FunctionDef, GlobalVar, Stmt};

use super::annotation::Annotations;
use super::expr::{infer_expr, infer_expr_with_ctx};
use super::ty::Type;
use super::value::{FunctionSig, InferredBinding, Value};

/// Helpers shared across the program walker.
pub struct Walker<'a> {
    pub functions: &'a std::collections::HashMap<String, FunctionSig>,
}

impl<'a> Walker<'a> {
    pub fn new(functions: &'a std::collections::HashMap<String, FunctionSig>) -> Self {
        Self { functions }
    }

    /// Infer the return type of a function body, considering every
    /// `return <expr>` site (and the trailing expression) rather than
    /// just the last statement.
    pub fn infer_return_type(&self, body: &[Stmt]) -> Type {
        let mut ret: Option<Type> = None;
        for stmt in body {
            match stmt {
                Stmt::Return { value: Some(v) } => {
                    let (t, _) = infer_expr_with_ctx(v, &[], self.functions, &[]);
                    ret = Some(narrow(ret, t));
                }
                Stmt::Return { value: None } => {
                    // Bare `return` — the function may return null, but
                    // we only narrow if no other site has produced a
                    // concrete type.
                }
                _ => {}
            }
        }
        // If no explicit return was found, the trailing expression's
        // type is the implicit return.
        if ret.is_none() {
            if let Some(last_expr) = body.iter().rev().find_map(|s| match s {
                Stmt::Expr(v) => Some(v),
                _ => None,
            }) {
                let (t, _) = infer_expr_with_ctx(last_expr, &[], self.functions, &[]);
                ret = Some(t);
            }
        }
        ret.unwrap_or(Type::Any)
    }

    /// Infer a parameter's type from how it's used inside the function
    /// body. We scan the body for any expression that uses the
    /// parameter and use the surrounding expression's inferred type
    /// (e.g. if `user` is compared with `==` to a `string` literal,
    /// then `user: string` is a reasonable guess).
    pub fn infer_param_type(&self, body: &[Stmt], param: &str) -> Type {
        let mut inferred: Option<Type> = None;
        for stmt in body {
            self.collect_param_usage(stmt, param, &mut inferred);
        }
        inferred.unwrap_or(Type::Any)
    }

    fn collect_param_usage(&self, stmt: &Stmt, param: &str, out: &mut Option<Type>) {
        match stmt {
            Stmt::VarDecl { value: Some(v), .. } => self.collect_param_usage_in_expr(v, param, out),
            Stmt::VarDecl { value: None, .. } => {}
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                self.collect_param_usage_in_expr(condition, param, out);
                for s in then_body {
                    self.collect_param_usage(s, param, out);
                }
                if let Some(eb) = else_body {
                    for s in eb {
                        self.collect_param_usage(s, param, out);
                    }
                }
            }
            Stmt::Return { value: Some(v) } => self.collect_param_usage_in_expr(v, param, out),
            Stmt::Return { value: None } => {}
            Stmt::Expr(v) | Stmt::Log(v) => self.collect_param_usage_in_expr(v, param, out),
            Stmt::Foreach { iterable, body, .. } => {
                self.collect_param_usage_in_expr(iterable, param, out);
                for s in body {
                    self.collect_param_usage(s, param, out);
                }
            }
            Stmt::On { .. } => {}
        }
    }

    fn collect_param_usage_in_expr(&self, expr: &Expr, param: &str, out: &mut Option<Type>) {
        if let Some(t) = self.param_type_in_context(expr, param) {
            *out = Some(narrow(out.take(), t));
        }
    }

    /// Given an expression that mentions `param`, return the type we can
    /// infer for `param` from the surrounding context (e.g. the other
    /// operand of a comparison, the argument position of a known call).
    /// Returns `None` if no constraint can be extracted.
    fn param_type_in_context(&self, expr: &Expr, param: &str) -> Option<Type> {
        match expr {
            Expr::BinaryOp { op, left, right } => {
                use workflow_parser::ast::BinaryOp::*;
                let l_uses = uses_param(left, param);
                let r_uses = uses_param(right, param);
                // We need at least one side to use the param, otherwise
                // the expression is unrelated to it.
                if !l_uses && !r_uses {
                    return None;
                }
                match op {
                    // Comparisons: the param takes the type of the
                    // *other* operand (the one not equal to it).
                    Eq | Neq | Lt | Gt | Lte | Gte => {
                        if l_uses && r_uses {
                            // Both sides use the param — give up.
                            return None;
                        }
                        let other = if l_uses { right } else { left };
                        let (t, _) = infer_expr_with_ctx(other, &[], self.functions, &[]);
                        Some(t)
                    }
                    // Arithmetic / logical: result type constrains
                    // both operands to it.
                    Add | Sub | Mul | Div | Mod | And | Or => {
                        let (t, _) = infer_expr_with_ctx(expr, &[], self.functions, &[]);
                        Some(t)
                    }
                }
            }
            Expr::Call { name, args } => {
                // If the called function has known param types, find
                // which arg position uses `param` and use that.
                if let Some(sig) = self.functions.get(name) {
                    for (i, a) in args.iter().enumerate() {
                        if uses_param(a, param) {
                            if let Some(t) = sig.param_types.get(i) {
                                return Some(t.clone());
                            }
                        }
                    }
                    // Fall through to the built-in case.
                }
                if let Some(t) = super::builtins::builtin_arg_type(name, args.len()) {
                    return Some(t);
                }
                None
            }
            Expr::UnaryOp { operand, .. } => {
                if uses_param(operand, param) {
                    let (t, _) = infer_expr_with_ctx(expr, &[], self.functions, &[]);
                    Some(t)
                } else {
                    None
                }
            }
            Expr::Array(elements) => {
                if elements.iter().any(|e| uses_param(e, param)) {
                    Some(Type::Array(Box::new(Type::Any)))
                } else {
                    None
                }
            }
            Expr::InterpolatedString(parts) => {
                if parts.iter().any(|p| {
                    matches!(p,
                    workflow_parser::ast::InterpPart::Expr(e) if uses_param(e, param))
                }) {
                    Some(Type::String)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// True if `expr` mentions `param` as a free variable.
fn uses_param(expr: &Expr, param: &str) -> bool {
    match expr {
        Expr::String(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null => false,
        Expr::Var(name) => name == param,
        Expr::Member { object, .. } => uses_param(object, param),
        Expr::BinaryOp { left, right, .. } => uses_param(left, param) || uses_param(right, param),
        Expr::UnaryOp { operand, .. } => uses_param(operand, param),
        Expr::Call { args, .. } => args.iter().any(|a| uses_param(a, param)),
        Expr::Array(elements) => elements.iter().any(|e| uses_param(e, param)),
        Expr::InterpolatedString(parts) => parts.iter().any(|p| match p {
            workflow_parser::ast::InterpPart::Text(_) => false,
            workflow_parser::ast::InterpPart::Expr(e) => uses_param(e, param),
        }),
    }
}

/// Narrow two inferred types: prefer a concrete one over `Any`, otherwise
/// require both sides to agree.
fn narrow(a: Option<Type>, b: Type) -> Type {
    match a {
        None => b,
        Some(prev) if prev == Type::Any => b,
        Some(prev) if b == Type::Any => prev,
        Some(prev) if prev == b => prev,
        Some(_) => Type::Any,
    }
}

// ---------------------------------------------------------------------------
// Program walker
// ---------------------------------------------------------------------------

pub fn run_program(
    inference: &mut super::Inference,
    program: &FlowProgram,
    annotations: &Annotations,
) {
    for g in &program.globals {
        push_global(inference, g, annotations);
    }
    for f in &program.functions {
        push_function(inference, f, annotations);
    }
    for w in &program.workflows {
        scan_body(inference, &w.body, annotations);
    }
    for f in &program.functions {
        scan_body(inference, &f.body, annotations);
    }
}

fn push_global(inference: &mut super::Inference, g: &GlobalVar, annotations: &Annotations) {
    let (ty, value) = infer_expr_with_ctx(&g.value, &[], &inference.functions, &[]);
    let (ty, annotated) = annotations
        .globals
        .get(&g.name)
        .map(|t| (t.clone(), true))
        .unwrap_or((ty, false));
    let binding = InferredBinding {
        name: g.name.clone(),
        ty,
        value,
        annotated,
    };
    for line in inference.scope_at.iter_mut() {
        line.push(binding.clone());
    }
}

fn push_function(inference: &mut super::Inference, f: &FunctionDef, annotations: &Annotations) {
    // First pass: collect param/return types from inference, then let
    // annotations override them.
    let walker = Walker::new(&inference.functions);
    let mut param_types: Vec<Type> = f
        .params
        .iter()
        .map(|p| {
            // Per-param annotation wins; otherwise infer from body.
            annotations
                .param_types
                .get(&(f.name.clone(), p.clone()))
                .cloned()
                .unwrap_or_else(|| walker.infer_param_type(&f.body, p))
        })
        .collect();
    if let Some(ann) = annotations.functions.get(&f.name) {
        // A full function-signature annotation overrides per-param
        // inference (and any lone `//@param` hints).
        param_types = ann.param_types.clone();
    }
    // Return type: annotation wins; otherwise infer from every return
    // site in the body, falling back to the trailing expression.
    let ret = if let Some(ann) = annotations.functions.get(&f.name) {
        ann.ret.clone()
    } else {
        walker.infer_return_type(&f.body)
    };
    let annotated = annotations.functions.contains_key(&f.name);
    inference.functions.insert(
        f.name.clone(),
        FunctionSig {
            name: f.name.clone(),
            params: f.params.clone(),
            param_types,
            ret,
            annotated,
        },
    );
}

/// Walk a statement body and add local bindings to every line's scope.
/// Loops add their item variable to subsequent lines.
pub fn scan_body(inference: &mut super::Inference, stmts: &[Stmt], annotations: &Annotations) {
    for stmt in stmts {
        scan_stmt(inference, stmt, annotations);
    }
}

fn scan_stmt(inference: &mut super::Inference, stmt: &Stmt, annotations: &Annotations) {
    match stmt {
        Stmt::VarDecl { name, value } => {
            let (ty, val) = match value {
                Some(v) => infer_expr_with_ctx(v, &[], &inference.functions, &[]),
                None => (Type::Any, None),
            };
            let (ty, annotated) = annotations
                .locals
                .get(name)
                .map(|t| (t.clone(), true))
                .unwrap_or((ty, false));
            let binding = InferredBinding {
                name: name.clone(),
                ty,
                value: val,
                annotated,
            };
            for line in inference.scope_at.iter_mut() {
                line.push(binding.clone());
            }
        }
        Stmt::Foreach {
            item_var,
            iterable,
            body,
        } => {
            let inner = infer_expr_with_ctx(
                iterable,
                &inference.scope_at.first().cloned().unwrap_or_default(),
                &inference.functions,
                &[],
            )
            .0;
            let ty = match inner {
                Type::Array(inner) => *inner,
                _ => Type::Any,
            };
            let binding = InferredBinding {
                name: item_var.clone(),
                ty,
                value: None,
                annotated: false,
            };
            for line in inference.scope_at.iter_mut() {
                line.push(binding.clone());
            }
            scan_body(inference, body, annotations);
        }
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            scan_body(inference, then_body, annotations);
            if let Some(else_body) = else_body {
                scan_body(inference, else_body, annotations);
            }
        }
        _ => {}
    }
}

#[allow(dead_code)]
pub(crate) fn fold_value(expr: &Expr) -> Option<Value> {
    infer_expr(expr).1
}
