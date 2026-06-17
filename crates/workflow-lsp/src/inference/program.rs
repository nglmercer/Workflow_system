//! Program-level inference: walks a `FlowProgram` and produces the
//! `Inference` result (per-line scopes + function signatures +
//! typed bindings).
//!
//! The walker uses the byte-offset [`ScopeIndex`] from
//! [`crate::scope`] as the source of truth. Type information is
//! attached to each binding via a side table
//! (`Inference::typed`) keyed by `(name, scope_id)`. The legacy
//! per-line `scope_at` table is derived from the scope index for
//! backward compatibility with consumers that haven't been
//! ported to the new byte-offset lookup.

use workflow_parser::ast::{Expr, FlowProgram, ImportStmt, Stmt};

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
    /// just the last statement. Recurses into `if` and `foreach`
    /// bodies so a `return` deep inside a block is still picked up.
    pub fn infer_return_type(&self, body: &[Stmt], scope: &[InferredBinding]) -> Type {
        let mut ret: Option<Type> = None;
        self.collect_return_type(body, scope, &mut ret);
        if ret.is_none() {
            if let Some(last_expr) = body.iter().rev().find_map(|s| match s {
                Stmt::Expr(v, _) => Some(v),
                _ => None,
            }) {
                let (t, _) = infer_expr_with_ctx(last_expr, scope, self.functions, &[]);
                ret = Some(t);
            }
        }
        ret.unwrap_or(Type::Any)
    }

    fn collect_return_type(
        &self,
        body: &[Stmt],
        scope: &[InferredBinding],
        ret: &mut Option<Type>,
    ) {
        for stmt in body {
            match stmt {
                Stmt::Return { value: Some(v), .. } => {
                    let (t, _) = infer_expr_with_ctx(v, scope, self.functions, &[]);
                    *ret = Some(narrow(ret.take(), t));
                }
                Stmt::Return { value: None, .. } => {
                    // Bare `return` — the function may return null,
                    // but we only narrow if no other site has produced
                    // a concrete type.
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.collect_return_type(then_body, scope, ret);
                    if let Some(eb) = else_body {
                        self.collect_return_type(eb, scope, ret);
                    }
                }
                Stmt::Foreach { body: inner, .. } => {
                    self.collect_return_type(inner, scope, ret);
                }
                _ => {}
            }
        }
    }

    /// Infer a parameter's type from how it's used inside the function
    /// body.
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
                ..
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
            Stmt::Return { value: Some(v), .. } => self.collect_param_usage_in_expr(v, param, out),
            Stmt::Return { value: None, .. } => {}
            Stmt::Expr(v, _) | Stmt::Log(v, _) => self.collect_param_usage_in_expr(v, param, out),
            Stmt::Foreach { iterable, body, .. } => {
                self.collect_param_usage_in_expr(iterable, param, out);
                for s in body {
                    self.collect_param_usage(s, param, out);
                }
            }
            Stmt::On { .. } => {}
            Stmt::Assign { value, .. } => self.collect_param_usage_in_expr(value, param, out),
        }
    }

    fn collect_param_usage_in_expr(&self, expr: &Expr, param: &str, out: &mut Option<Type>) {
        if let Some(t) = self.param_type_in_context(expr, param) {
            *out = Some(narrow(out.take(), t));
        }
    }

    fn param_type_in_context(&self, expr: &Expr, param: &str) -> Option<Type> {
        match expr {
            Expr::BinaryOp { op, left, right } => {
                use workflow_parser::ast::BinaryOp::*;
                let l_uses = uses_param(left, param);
                let r_uses = uses_param(right, param);
                if !l_uses && !r_uses {
                    return None;
                }
                match op {
                    Eq | Neq | Lt | Gt | Lte | Gte => {
                        if l_uses && r_uses {
                            return None;
                        }
                        let other = if l_uses { right } else { left };
                        let (t, _) = infer_expr_with_ctx(other, &[], self.functions, &[]);
                        Some(t)
                    }
                    Add | Sub | Mul | Div | Mod | And | Or => {
                        let (t, _) = infer_expr_with_ctx(expr, &[], self.functions, &[]);
                        Some(t)
                    }
                }
            }
            Expr::Call { name, args } => {
                if let Some(sig) = self.functions.get(name) {
                    for (i, a) in args.iter().enumerate() {
                        if uses_param(a, param) {
                            if let Some(t) = sig.param_types.get(i) {
                                return Some(t.clone());
                            }
                        }
                    }
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

fn narrow(a: Option<Type>, b: Type) -> Type {
    match a {
        None => b,
        Some(Type::Any) => b,
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
    source: &str,
) {
    run_program_with_imports(inference, program, annotations, &[], source)
}

/// Like [`run_program`], but takes an explicit list of bindings to
/// push into scope for every line. This is how imported data schemas
/// (`@import data from ...`) enter the inference: the LSP resolves
/// the import's path or inline object, converts it to a [`Type`],
/// and passes the resulting bindings through this entry point.
pub fn run_program_with_imports(
    inference: &mut super::Inference,
    program: &FlowProgram,
    annotations: &Annotations,
    import_bindings: &[InferredBinding],
    source: &str,
) {
    // Build the byte-offset scope index from the AST. This is the
    // authoritative scope source — the per-line `scope_at` table
    // below is derived from it.
    use crate::scope::build_scope_index;
    inference.scope_index = build_scope_index(program, source);
    inference.typed = crate::scope::TypedBindings::new();

    // Now walk the program and attach type information to each
    // binding the scope index recorded. We do this by walking the
    // AST in a separate pass — the scope index already gives us
    // the (name, scope_id) keys.
    infer_globals(inference, program, annotations);
    infer_functions(inference, program, annotations);
    infer_workflows(inference, program, annotations, import_bindings);
    infer_imports(inference, &program.imports, import_bindings);

    // Project the typed bindings into the per-line table. We use
    // the line's *end* so any binding declared on the same line
    // shows up to the rest of the line. This matches the legacy
    // behaviour closely enough for the existing consumers.
    use crate::analysis::byte_offset_of_line_end;
    let line_count = source.lines().count().max(1);
    inference.scope_at = vec![Vec::new(); line_count];
    for line_idx in 0..line_count {
        let byte_offset = byte_offset_of_line_end(source, line_idx);
        let bindings = inference.scope_index.bindings_at(byte_offset);
        for b in bindings {
            if let Some(typed) = inference.typed.get(b.name, b.scope_id) {
                inference.scope_at[line_idx].push(typed.clone());
            }
        }
    }
}

fn infer_globals(
    inference: &mut super::Inference,
    program: &FlowProgram,
    annotations: &Annotations,
) {
    // For each global, find the scope binding the scope index
    // recorded, compute the type, and attach it.
    for g in &program.globals {
        let (ty, value) = infer_expr_with_ctx(&g.value, &[], &inference.functions, &[]);
        let (ty, annotated) = annotations
            .globals
            .get(&g.name)
            .map(|t| (t.clone(), true))
            .unwrap_or((ty, false));
        let scope_id = lookup_scope_for(inference, &g.name, g.span.start);
        if let Some(sid) = scope_id {
            inference.typed.insert(
                &g.name,
                sid,
                InferredBinding {
                    name: g.name.clone(),
                    ty,
                    value,
                    annotated,
                },
            );
        }
    }
}

fn infer_functions(
    inference: &mut super::Inference,
    program: &FlowProgram,
    annotations: &Annotations,
) {
    for f in &program.functions {
        let walker = Walker::new(&inference.functions);
        let mut param_types: Vec<Type> = f
            .params
            .iter()
            .map(|p| {
                annotations
                    .param_types
                    .get(&(f.name.clone(), p.clone()))
                    .cloned()
                    .unwrap_or_else(|| walker.infer_param_type(&f.body, p))
            })
            .collect();
        if let Some(ann) = annotations.functions.get(&f.name) {
            param_types = ann.param_types.clone();
        }
        let param_scope: Vec<InferredBinding> = f
            .params
            .iter()
            .cloned()
            .zip(param_types.iter().cloned())
            .map(|(name, ty)| InferredBinding {
                name,
                ty,
                value: None,
                annotated: false,
            })
            .collect();
        let ret = if let Some(ann) = annotations.functions.get(&f.name) {
            if ann.ret != Type::Any {
                ann.ret.clone()
            } else {
                walker.infer_return_type(&f.body, &param_scope)
            }
        } else {
            walker.infer_return_type(&f.body, &param_scope)
        };
        let annotated = annotations.functions.contains_key(&f.name);
        inference.functions.insert(
            f.name.clone(),
            FunctionSig {
                name: f.name.clone(),
                params: f.params.clone(),
                param_types: param_types.clone(),
                ret: ret.clone(),
                annotated,
            },
        );
        // The function name itself is a module-level binding.
        // Type it as its return type.
        let sid = lookup_scope_for(inference, &f.name, f.span.start);
        if let Some(sid) = sid {
            inference.typed.insert(
                &f.name,
                sid,
                InferredBinding {
                    name: f.name.clone(),
                    ty: ret.clone(),
                    value: None,
                    annotated,
                },
            );
        }
        // Function parameters are bound in the function's own
        // scope. Look up the binding by name and attach the
        // parameter type. We use the *first* scope where `p`
        // appears; for non-shadowing parameters this is the
        // function scope.
        for (idx, p) in f.params.iter().enumerate() {
            let ty = param_types.get(idx).cloned().unwrap_or(Type::Any);
            if let Some(sid) = lookup_scope_for(inference, p, f.span.start) {
                inference.typed.insert(
                    p,
                    sid,
                    InferredBinding {
                        name: p.clone(),
                        ty,
                        value: None,
                        annotated: param_types
                            .get(idx)
                            .map(|t| t != &Type::Any)
                            .unwrap_or(false),
                    },
                );
            }
        }
    }
}

fn infer_workflows(
    inference: &mut super::Inference,
    program: &FlowProgram,
    annotations: &Annotations,
    import_bindings: &[InferredBinding],
) {
    for w in &program.workflows {
        // Workflow destructure params: visible at the workflow's
        // start. Type from the matching import, or fall back to
        // Any.
        let event_binding = import_bindings.iter().find(|b| b.name == w.event);
        for p in &w.params {
            if p == "_rename" {
                continue;
            }
            let ty = import_bindings
                .iter()
                .find_map(|b| match &b.ty {
                    Type::Object(fields) => {
                        fields.iter().find(|(k, _)| k == p).map(|(_, t)| t.clone())
                    }
                    _ => None,
                })
                .or_else(|| event_binding.map(|b| b.ty.clone()))
                .unwrap_or(Type::Any);
            if let Some(sid) = lookup_scope_for(inference, p, w.span.start) {
                inference.typed.insert(
                    p,
                    sid,
                    InferredBinding {
                        name: p.clone(),
                        ty,
                        value: None,
                        annotated: import_bindings
                            .iter()
                            .any(|b| matches!(b.ty, Type::Object(_))),
                    },
                );
            }
        }
        // `data` and the event name are also workflow bindings.
        // Both inherit their type from the matching `@import`
        // binding when there is one — otherwise hovering on
        // `on USER_REGISTERED` (or the `data` carrier) would
        // report `any` even though the schema is sitting right
        // there in the imports list. We mark `annotated` only when
        // the type came from a real import so completion/hover can
        // tell a typed event from a generic one.
        for &(name, _kind) in &[
            ("data", BindingKind::EventPayload),
            (&w.event, BindingKind::WorkflowEvent),
        ] {
            if let Some(sid) = lookup_scope_for(inference, name, w.span.start) {
                let (ty, annotated) = match event_binding {
                    Some(b) => (b.ty.clone(), true),
                    None => (Type::Any, false),
                };
                inference.typed.insert(
                    name,
                    sid,
                    InferredBinding {
                        name: name.to_string(),
                        ty,
                        value: None,
                        annotated,
                    },
                );
            }
        }
        // Walk the workflow body and attach types to local
        // bindings.
        infer_body_bindings(inference, w.span.start, &w.body, annotations);
    }
    // Function bodies: type locals.
    for f in &program.functions {
        infer_body_bindings(inference, f.span.start, &f.body, annotations);
    }
}

fn infer_body_bindings(
    inference: &mut super::Inference,
    _scope_anchor: usize,
    stmts: &[Stmt],
    annotations: &Annotations,
) {
    // Build a per-line scope table for the body's region so
    // expressions can resolve the right typed bindings.
    for stmt in stmts {
        match stmt {
            Stmt::VarDecl {
                name, value, span, ..
            } => {
                // Find the scope binding the scope index recorded.
                let sid = lookup_scope_for(inference, name, span.start);
                if let Some(sid) = sid {
                    let (ty, val) = match value {
                        Some(v) => {
                            let typed_scope = scope_at_offset(inference, span.start);
                            infer_expr_with_ctx(v, &typed_scope, &inference.functions, &[])
                        }
                        None => (Type::Any, None),
                    };
                    let (ty, annotated) = annotations
                        .locals
                        .get(name)
                        .map(|t| (t.clone(), true))
                        .unwrap_or((ty, false));
                    inference.typed.insert(
                        name,
                        sid,
                        InferredBinding {
                            name: name.clone(),
                            ty,
                            value: val,
                            annotated,
                        },
                    );
                }
            }
            Stmt::Assign {
                name, value, span, ..
            } => {
                // Re-resolve the existing binding's type from the
                // RHS expression.
                let sid = lookup_scope_for(inference, name, span.start);
                if let Some(sid) = sid {
                    let typed_scope = scope_at_offset(inference, span.start);
                    let (ty, val) =
                        infer_expr_with_ctx(value, &typed_scope, &inference.functions, &[]);
                    // Update the existing typed entry (or insert a
                    // new one if there isn't one yet).
                    inference.typed.insert(
                        name,
                        sid,
                        InferredBinding {
                            name: name.clone(),
                            ty,
                            value: val,
                            annotated: false,
                        },
                    );
                }
            }
            Stmt::Foreach {
                item_var,
                iterable,
                body,
                span,
                ..
            } => {
                // The item binding lives in the foreach body scope,
                // which starts at the first statement — not at the
                // `foreach` keyword. Use the body's start so the
                // scope index can find the binding.
                let body_start = body.first().map(|s| s.span().start).unwrap_or(span.start);
                let sid = lookup_scope_for(inference, item_var, body_start);
                if let Some(sid) = sid {
                    let typed_scope = scope_at_offset(inference, span.start);
                    let (inner_ty, _) =
                        infer_expr_with_ctx(iterable, &typed_scope, &inference.functions, &[]);
                    let item_ty = match inner_ty {
                        Type::Array(inner) => *inner,
                        _ => Type::Any,
                    };
                    inference.typed.insert(
                        item_var,
                        sid,
                        InferredBinding {
                            name: item_var.clone(),
                            ty: item_ty,
                            value: None,
                            annotated: false,
                        },
                    );
                }
                // Recurse into the body.
                infer_body_bindings(inference, _scope_anchor, body, annotations);
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                infer_body_bindings(inference, _scope_anchor, then_body, annotations);
                if let Some(eb) = else_body {
                    infer_body_bindings(inference, _scope_anchor, eb, annotations);
                }
            }
            _ => {}
        }
    }
}

fn infer_imports(
    inference: &mut super::Inference,
    imports: &[ImportStmt],
    import_bindings: &[InferredBinding],
) {
    // Each import binding is a module-level binding named after
    // the import's `name` field. We find the scope binding the
    // scope index recorded (it was added in `walk_program` with
    // kind=Import) and attach the type info.
    //
    // The lookup MUST happen at the import's `span.start`, not at
    // offset 0: the import binding is only visible from its
    // declaration onward, so querying at 0 returns `None` for any
    // import declared past the first byte. Without the correct
    // offset the typed binding never gets attached, hover falls
    // back to `any`, and member completions stop working.
    for ib in import_bindings {
        let offset = imports
            .iter()
            .find(|imp| imp.name == ib.name)
            .map(|imp| imp.span.start)
            .unwrap_or(0);
        if let Some(sid) = lookup_scope_for(inference, &ib.name, offset) {
            inference.typed.insert(&ib.name, sid, ib.clone());
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindingKind {
    EventPayload,
    WorkflowEvent,
}

/// Find the scope id that contains the binding for `name` at
/// `offset`. Returns the innermost scope id, so shadowing
/// declarations win.
fn lookup_scope_for(inference: &super::Inference, name: &str, offset: usize) -> Option<usize> {
    inference
        .scope_index
        .bindings_at(offset)
        .into_iter()
        .find(|b| b.name == name)
        .map(|b| b.scope_id)
}

/// Collect the typed bindings visible at `offset`. Used as the
/// inference context when walking expressions inside a body.
fn scope_at_offset(inference: &super::Inference, offset: usize) -> Vec<InferredBinding> {
    inference
        .scope_index
        .bindings_at(offset)
        .into_iter()
        .filter_map(|b| inference.typed.get(b.name, b.scope_id).cloned())
        .collect()
}

#[allow(dead_code)]
pub(crate) fn fold_value(expr: &Expr) -> Option<Value> {
    infer_expr(expr).1
}
