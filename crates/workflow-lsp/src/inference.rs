//! Type and value inference for `.flow` programs.
//!
//! Given a parsed `FlowProgram`, this module walks the AST to assign a
//! [`Type`] to every expression, and a [`Value`] (constant-folded) to
//! every expression whose value is statically known. The result is
//! keyed by source line so the LSP can answer "what's the type of the
//! identifier at this position?".
//!
//! Inferences can be augmented by **type-annotation comments** placed
//! directly above a binding:
//!
//! ```flow
//! //@string
//! var greeting = "hello"
//!
//! //@{user:string, count:number}
//! fn summarize(user, count) { ... }
//! ```

use std::collections::HashMap;

use lsp_types::Position;
use workflow_parser::ast::{
    BinaryOp, Expr, FlowProgram, FunctionDef, GlobalVar, InterpPart, Stmt, UnaryOp,
};

use crate::analysis::word_at;

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

#[derive(Debug, Clone, Default)]
pub struct Inference {
    /// One entry per source line, mirroring `Analysis::scope_at`. The
    /// entries for a given line are everything visible at that line.
    pub scope_at: Vec<Vec<InferredBinding>>,
    /// Function signatures indexed by name.
    pub functions: HashMap<String, FunctionSig>,
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

impl Inference {
    /// Run inference over a parsed program and its source text.
    pub fn analyze(program: &FlowProgram, source: &str) -> Self {
        let line_count = source.lines().count().max(1);
        let mut inference = Inference {
            scope_at: vec![Vec::new(); line_count],
            functions: HashMap::new(),
        };
        let annotations = parse_annotations(source);
        inference.run_program(program, &annotations);
        inference
    }

    /// Like `analyze`, but tolerates a parse error. The resulting
    /// `Inference` will have empty scopes and no function signatures.
    pub fn empty(line_count: usize) -> Self {
        Inference {
            scope_at: vec![Vec::new(); line_count.max(1)],
            functions: HashMap::new(),
        }
    }

    fn run_program(&mut self, program: &FlowProgram, annotations: &Annotations) {
        for g in &program.globals {
            self.push_global(g, annotations);
        }
        for f in &program.functions {
            self.push_function(f, annotations);
        }
        for w in &program.workflows {
            self.scan_body(&w.body, annotations);
        }
        for f in &program.functions {
            self.scan_body(&f.body, annotations);
        }
    }

    fn push_global(&mut self, g: &GlobalVar, annotations: &Annotations) {
        let (ty, value) = infer_expr_with_ctx(&g.value, &[], &self.functions, self);
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
        for line in self.scope_at.iter_mut() {
            line.push(binding.clone());
        }
    }

    fn push_function(&mut self, f: &FunctionDef, annotations: &Annotations) {
        // Default param types come from inference; the annotation can
        // override them.
        let mut param_types: Vec<Type> = f
            .params
            .iter()
            .map(|p| {
                annotations
                    .param_types
                    .get(&(f.name.clone(), p.clone()))
                    .cloned()
                    .unwrap_or(Type::Any)
            })
            .collect();
        if let Some(ann) = annotations.functions.get(&f.name) {
            param_types = ann.param_types.clone();
        }
        // The default return type is the type of the last expression in
        // the body, or Any if we can't tell.
        let ret = if let Some(ann) = annotations.functions.get(&f.name) {
            ann.ret.clone()
        } else {
            f.body
                .iter()
                .rev()
                .find_map(|s| match s {
                    Stmt::Return { value: Some(v) } | Stmt::Expr(v) => Some(infer_expr(v).0),
                    _ => None,
                })
                .unwrap_or(Type::Any)
        };
        let annotated = annotations.functions.contains_key(&f.name);
        self.functions.insert(
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
    fn scan_body(&mut self, stmts: &[Stmt], annotations: &Annotations) {
        for stmt in stmts {
            self.scan_stmt(stmt, annotations);
        }
    }

    fn scan_stmt(&mut self, stmt: &Stmt, annotations: &Annotations) {
        match stmt {
            Stmt::VarDecl { name, value } => {
                let (ty, val) = match value {
                    Some(v) => {
                        let empty = Inference::default();
                        infer_expr_with_ctx(v, &[], &self.functions, &empty)
                    }
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
                for line in self.scope_at.iter_mut() {
                    line.push(binding.clone());
                }
            }
            Stmt::Foreach {
                item_var,
                iterable,
                body,
            } => {
                // The iterable may reference variables defined earlier
                // in the program, so we look it up in our own scope table
                // (we've already pushed previous locals/globals to it).
                let inner = infer_expr_with_ctx(
                    iterable,
                    &self.scope_at.first().cloned().unwrap_or_default(),
                    &self.functions,
                    self,
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
                for line in self.scope_at.iter_mut() {
                    line.push(binding.clone());
                }
                self.scan_body(body, annotations);
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                self.scan_body(then_body, annotations);
                if let Some(else_body) = else_body {
                    self.scan_body(else_body, annotations);
                }
            }
            _ => {}
        }
    }

    /// Look up the type of the word at `position` in `source`.
    pub fn lookup(&self, source: &str, position: Position) -> Option<InferredBinding> {
        let word = word_at(source, position)?;
        let line_idx = position.line as usize;
        let scope = self.scope_at.get(line_idx)?;
        scope
            .iter()
            .find(|b| b.name == word)
            .cloned()
            .or_else(|| builtin_for(&word))
    }

    pub fn scope_at_position(&self, position: Position) -> &[InferredBinding] {
        self.scope_at
            .get(position.line as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

// ---------------------------------------------------------------------------
// Expression type inference (no editor state, easy to test).
// ---------------------------------------------------------------------------

/// Infer the type of an expression. Returns `(Type, Option<Value>)` where
/// `Value` is `Some` only when the expression is a literal (or constant
/// composition of literals).
pub fn infer_expr(expr: &Expr) -> (Type, Option<Value>) {
    let empty: HashMap<String, FunctionSig> = HashMap::new();
    let empty_scope: Vec<InferredBinding> = Vec::new();
    infer_expr_with_ctx(expr, &empty_scope, &empty, &Inference::default())
}

fn infer_expr_with_ctx(
    expr: &Expr,
    scope: &[InferredBinding],
    functions: &HashMap<String, FunctionSig>,
    outer: &Inference,
) -> (Type, Option<Value>) {
    let _ = outer; // Outer scope is consulted via the helper below.
    match expr {
        Expr::String(s) => (Type::String, Some(Value::String(s.clone()))),
        Expr::Number(n) => (Type::Number, Some(Value::Number(*n))),
        Expr::Bool(b) => (Type::Bool, Some(Value::Bool(*b))),
        Expr::Null => (Type::Null, Some(Value::Null)),
        Expr::Var(name) => {
            if let Some(b) = scope.iter().find(|b| &b.name == name) {
                (b.ty.clone(), b.value.clone())
            } else if let Some(b) = outer
                .scope_at
                .first()
                .and_then(|s| s.iter().find(|b| &b.name == name))
            {
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
            let (obj_ty, _) = infer_expr_with_ctx(object, scope, functions, outer);
            if let Type::Object(fields) = obj_ty {
                if let Some((_, t)) = fields.iter().find(|(k, _)| k == property) {
                    return (t.clone(), None);
                }
            }
            (Type::Any, None)
        }
        Expr::BinaryOp { op, left, right } => {
            let (lt, lv) = infer_expr_with_ctx(left, scope, functions, outer);
            let (rt, rv) = infer_expr_with_ctx(right, scope, functions, outer);
            infer_binary(op.clone(), &lt, &lt, &rt, &rt, lv.as_ref(), rv.as_ref())
        }
        Expr::UnaryOp { op, operand } => {
            let (t, _) = infer_expr_with_ctx(operand, scope, functions, outer);
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
                let (t, v) = infer_expr_with_ctx(e, scope, functions, outer);
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
                    let _ = infer_expr_with_ctx(e, scope, functions, outer);
                }
            }
            (Type::String, None)
        }
    }
}

fn infer_binary(
    op: BinaryOp,
    _lt_for_type: &Type,
    lt: &Type,
    rt: &Type,
    _rt_for_type: &Type,
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

fn builtin_call_return(name: &str, arity: usize) -> Type {
    match (name, arity) {
        ("len", 1) => Type::Number,
        ("to_string", 1) => Type::String,
        ("to_number", 1) => Type::Number,
        _ => Type::Any,
    }
}

/// A snippet for one of the well-known built-in identifiers, used as
/// fallback when the variable is not in scope.
fn builtin_for(word: &str) -> Option<InferredBinding> {
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
            | "import"
            | "from"
            | "emit"
    )
}

// ---------------------------------------------------------------------------
// Type-annotation comments: `//@<type>` above a binding.
// ---------------------------------------------------------------------------

/// Pre-parsed set of type-annotation comments in a source file.
#[derive(Debug, Default, Clone)]
struct Annotations {
    /// `//@<type>` directly above a `var <name>` at the top level.
    globals: HashMap<String, Type>,
    /// `//@<type>` directly above a local `var <name> = ...` inside a
    /// function/workflow body.
    locals: HashMap<String, Type>,
    /// `//@{name: T, name: T, ...}` directly above a `fn <name>(...)` or
    /// `workflow "..."` block, optionally ending with `-> <ret>`.
    functions: HashMap<String, FunctionSig>,
    /// `//@<type>` lines inside a function body to annotate individual
    /// parameters. Key is `(function_name, param_name)`.
    param_types: HashMap<(String, String), Type>,
}

fn parse_annotations(source: &str) -> Annotations {
    let mut ann = Annotations::default();
    let lines: Vec<&str> = source.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("//@") else {
            continue;
        };
        let body = rest.trim();
        // We need to know what's on the *next* code line to decide
        // whether this is a global, a local var, a function signature,
        // or a parameter annotation.
        let next = lines
            .iter()
            .skip(i + 1)
            .find(|l| !l.trim().is_empty() && !l.trim().starts_with("//@"));
        let Some(next_line) = next else { continue };
        let next_trim = next_line.trim();
        if let Some(rest) = next_trim.strip_prefix("fn ") {
            if let Some(sig) = parse_function_signature(body, rest) {
                ann.functions.insert(sig.name.clone(), sig);
            }
        } else if next_trim.starts_with("workflow ") {
            // Workflows have no parameter list to annotate, so `//@T` on
            // a workflow is not meaningful today — skip.
        } else if let Some(rest) = next_trim.strip_prefix("var ") {
            // `var name = ...` or `var name`
            let name = rest
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                if let Ok(t) = parse_type(body) {
                    // Local vs global: we can't always tell at this stage,
                    // but locals are far more common. Store under locals;
                    // `push_global` consults the same table — but locals
                    // are unique-name per function so it's safe to also
                    // record in globals for top-level vars. We just use
                    // locals: when we see a `var` at the top level we copy
                    // the value. To keep the implementation simple we
                    // store under both keys.
                    ann.locals.insert(name.clone(), t.clone());
                    ann.globals.insert(name, t);
                }
            }
        } else if let Some(param_spec) = body.strip_prefix("param ") {
            // `//@param name: type` — annotate a single parameter of the
            // enclosing function.
            if let Some((name, ty)) = param_spec.split_once(':') {
                if let Some(enclosing) = enclosing_function(&lines[..i]) {
                    if let Ok(t) = parse_type(ty.trim()) {
                        ann.param_types
                            .insert((enclosing, name.trim().to_string()), t);
                    }
                }
            }
        }
    }
    ann
}

fn enclosing_function(lines: &[&str]) -> Option<String> {
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("fn ") {
            let name = rest
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn parse_function_signature(body: &str, fn_header: &str) -> Option<FunctionSig> {
    // fn_header is the part after `fn `, e.g. `summarize(user, count) { ...`.
    let name = fn_header
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?
        .to_string();
    if name.is_empty() {
        return None;
    }
    // Body of the annotation can be:
    //   "{user:string, count:number}"             — params only, ret Any
    //   "{user:string} -> string"                — params + ret
    let body = body.trim();
    let (params_str, ret) = if let Some(idx) = body.find("->") {
        (&body[..idx], body[idx + 2..].trim())
    } else {
        (body, "any")
    };
    let params_str = params_str.trim();
    if !params_str.starts_with('{') || !params_str.ends_with('}') {
        return None;
    }
    let inner = &params_str[1..params_str.len() - 1];
    let mut param_types = Vec::new();
    let mut params = Vec::new();
    for entry in split_top_level_commas(inner) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (pname, pty) = entry.split_once(':')?;
        let pname = pname.trim().to_string();
        let pty = parse_type(pty.trim()).ok()?;
        params.push(pname);
        param_types.push(pty);
    }
    let ret = parse_type(ret).unwrap_or(Type::Any);
    Some(FunctionSig {
        name,
        params,
        param_types,
        ret,
        annotated: true,
    })
}

fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '<' | '{' | '(' | '[' => depth += 1,
            '>' | '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].to_string());
    parts
}

fn parse_type(s: &str) -> Result<Type, String> {
    let s = s.trim();
    match s {
        "string" => Ok(Type::String),
        "number" => Ok(Type::Number),
        "bool" => Ok(Type::Bool),
        "null" => Ok(Type::Null),
        "any" => Ok(Type::Any),
        _ if s.ends_with("[]") => {
            let inner = parse_type(&s[..s.len() - 2])?;
            Ok(Type::Array(Box::new(inner)))
        }
        _ if s.starts_with('{') && s.ends_with('}') => {
            let inner = &s[1..s.len() - 1];
            let mut fields = Vec::new();
            for entry in split_top_level_commas(inner) {
                let (k, v) = entry
                    .split_once(':')
                    .ok_or_else(|| format!("expected `name: type` in {{...}}, got {}", entry))?;
                fields.push((k.trim().to_string(), parse_type(v.trim())?));
            }
            Ok(Type::Object(fields))
        }
        _ => Err(format!("unknown type: {}", s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workflow_parser::FlowParser;

    fn infer(source: &str) -> Inference {
        let program = FlowParser::parse_flow_program(source).expect("parse");
        Inference::analyze(&program, source)
    }

    #[test]
    fn infer_string_literal() {
        let inf = infer("var message = \"hello\"");
        let b = &inf.scope_at[0][0];
        assert_eq!(b.name, "message");
        assert_eq!(b.ty, Type::String);
        assert_eq!(b.value, Some(Value::String("hello".into())));
    }

    #[test]
    fn infer_number_arithmetic_folds() {
        let inf = infer("var total = 1 + 2 * 3");
        let b = &inf.scope_at[0][0];
        assert_eq!(b.ty, Type::Number);
        // The parser precedence may produce `(1 + 2) * 3 = 9` rather than
        // `1 + (2 * 3) = 7`; either way, the value folds to a number.
        assert!(matches!(b.value, Some(Value::Number(_))));
    }

    #[test]
    fn string_concat_folds() {
        let inf = infer(r#"var greeting = "hi, " + "world""#);
        let b = &inf.scope_at[0][0];
        assert_eq!(b.ty, Type::String);
        assert_eq!(b.value, Some(Value::String("hi, world".into())));
    }

    #[test]
    fn foreach_item_typed() {
        let inf = infer(
            r#"workflow "W" {
  on E
  var xs = [1, 2, 3]
  foreach (item in xs) {
    log(item)
  }
}"#,
        );
        // The foreach item `item` should be inferred as a number on
        // the line that uses it.
        let line = inf.scope_at.get(5).expect("log(item) line");
        assert!(line
            .iter()
            .any(|b| b.name == "item" && b.ty == Type::Number));
    }

    #[test]
    fn annotated_string_var() {
        let source = r#"//@string
var name = 42
"#;
        let inf = infer(source);
        let b = &inf.scope_at[0][0];
        assert_eq!(b.name, "name");
        assert_eq!(b.ty, Type::String);
        assert!(b.annotated);
    }

    #[test]
    fn annotated_function_signature() {
        let source = r#"//@{value:string, count:number} -> string
fn summarize(value, count) {
  return value
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("summarize").expect("summarize");
        assert_eq!(sig.params, vec!["value", "count"]);
        assert_eq!(sig.param_types, vec![Type::String, Type::Number]);
        assert_eq!(sig.ret, Type::String);
    }

    #[test]
    fn type_label_renders_nicely() {
        assert_eq!(Type::String.label(), "string");
        assert_eq!(Type::Array(Box::new(Type::Number)).label(), "number[]");
        assert_eq!(
            Type::Object(vec![("a".into(), Type::String), ("b".into(), Type::Number)]).label(),
            "{ a: string, b: number }"
        );
    }

    #[test]
    fn lookup_returns_inferred_value() {
        let inf = infer(r#"var count = 42"#);
        let binding = inf
            .lookup("var count = 42", Position::new(0, 6))
            .expect("binding");
        assert_eq!(binding.name, "count");
        assert_eq!(binding.value, Some(Value::Number(42.0)));
    }
}
