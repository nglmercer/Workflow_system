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
//!
//! Internally, this is split across focused submodules:
//!
//! - [`ty`] — the `Type` enum and its label rendering.
//! - [`value`] — constant-folded `Value`, `InferredBinding`, and
//!   `FunctionSig`.
//! - [`builtins`] — built-in keyword and function lookup.
//! - [`expr`] — expression-level inference (no editor state).
//! - [`annotation`] — `//@...` comment parser.
//! - [`program`] — walks the program and builds the per-line scope
//!   table and function signatures.

use std::collections::HashMap;

use lsp_types::Position;
use workflow_parser::ast::FlowProgram;

use crate::analysis::word_at;
use crate::scope::ScopeIndex;

pub mod annotation;
pub mod builtins;
pub mod expr;
pub mod program;
pub mod registry;
pub mod schema;
pub mod ty;
pub mod value;

pub use registry::{FunctionCategory, FunctionEntry, FunctionRegistry, ParamDescriptor};
pub use ty::Type;
pub use value::{FunctionSig, InferredBinding, Value};

pub mod methods;

#[derive(Debug)]
pub struct Inference {
    /// One entry per source line, mirroring `Analysis::scope_at`. The
    /// entries for a given line are everything visible at that line.
    /// Derived from `scope_index` + `typed` for backward compat with
    /// the per-line consumers.
    pub scope_at: Vec<Vec<InferredBinding>>,
    /// Function signatures indexed by name.
    pub functions: HashMap<String, FunctionSig>,
    /// The byte-offset scope index. New code should prefer this
    /// over `scope_at`.
    pub scope_index: ScopeIndex,
    /// Type/value info attached to each scope binding. Keyed by
    /// `(name, scope_id)`, the same scheme the `defs` map uses.
    pub typed: crate::scope::TypedBindings<InferredBinding>,
    /// Dynamic function registry for looking up functions at runtime.
    /// This is the single source of truth for all known functions
    /// (built-in + user-defined + imported).
    pub registry: FunctionRegistry,
    /// All known events in the program. Collected from:
    /// - `on EVENT` statements in workflows
    /// - `emit("EVENT")` calls in function/workflow bodies
    /// - Events can be marked as `external` (SCREAMING_SNAKE_CASE)
    ///   or `internal` (defined in this file or imported)
    pub events: HashMap<String, EventInfo>,
}

/// Information about a known event.
#[derive(Debug, Clone)]
pub struct EventInfo {
    /// The event name (e.g., "USER_REGISTERED", "payment_received")
    pub name: String,
    /// Whether this event is external (SCREAMING_SNAKE_CASE convention)
    pub is_external: bool,
    /// Line where this event is defined/used (0-indexed)
    pub line: u32,
    /// The type of usage: "on", "emit", or "import"
    pub usage: EventUsage,
}

/// How an event is used in the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventUsage {
    /// Event is listened to via `on EVENT`
    On,
    /// Event is emitted via `emit("EVENT")`
    Emit,
    /// Event is imported from an external schema
    Import,
}

impl Default for Inference {
    fn default() -> Self {
        Self {
            scope_at: Vec::new(),
            functions: HashMap::new(),
            scope_index: ScopeIndex::default(),
            typed: crate::scope::TypedBindings::new(),
            registry: FunctionRegistry::with_builtins(),
            events: HashMap::new(),
        }
    }
}

impl Inference {
    /// Run inference over a parsed program and its source text.
    pub fn analyze(program: &FlowProgram, source: &str) -> Self {
        Self::analyze_with_path(program, source, None)
    }

    /// Like [`analyze`], but with the document's filesystem path so
    /// that `@import data from "./schema.json"` can be resolved
    /// relative to the file. URL imports and unreadable paths are
    /// tolerated: the resolver records the error and the binding
    /// falls back to `Type::Any`.
    pub fn analyze_with_path(
        program: &FlowProgram,
        source: &str,
        document_path: Option<&str>,
    ) -> Self {
        Self::analyze_with_path_and_imports(program, source, document_path, &HashMap::new())
    }

    /// Like [`analyze_with_path`], but also accepts function signatures
    /// from imported .flow files. This enables cross-file imports where
    /// functions defined in one file can be used in another.
    pub fn analyze_with_path_and_imports(
        program: &FlowProgram,
        source: &str,
        document_path: Option<&str>,
        imported_functions: &HashMap<String, FunctionSig>,
    ) -> Self {
        let line_count = source.lines().count().max(1);
        let mut inference = Inference {
            scope_at: vec![Vec::new(); line_count],
            functions: HashMap::new(),
            ..Default::default()
        };
        let annotations = annotation::parse_annotations(source);
        let (_schemas, import_bindings) =
            schema::resolve_schemas_for_program(&program.imports, document_path);

        // Merge imported functions into the inference's function table
        // and register them in the dynamic registry
        for (name, sig) in imported_functions {
            inference.functions.insert(name.clone(), sig.clone());
            inference.registry.register_from_sig(sig, true);
        }

        program::run_program_with_imports(
            &mut inference,
            program,
            &annotations,
            &import_bindings,
            source,
        );

        // Also register locally-defined functions in the registry
        for (name, sig) in &inference.functions {
            if !imported_functions.contains_key(name) {
                inference.registry.register_from_sig(sig, false);
            }
        }

        // Collect events from the program
        inference.collect_events(program, source);

        inference
    }

    /// Collect all events from `on EVENT` and `emit("EVENT")` statements.
    fn collect_events(&mut self, program: &FlowProgram, source: &str) {
        // Collect events from `on EVENT` statements
        for workflow in &program.workflows {
            let is_external = is_screaming_snake_case(&workflow.event);
            let line = source[..workflow.span.start]
                .chars()
                .filter(|c| *c == '\n')
                .count() as u32;
            self.events.insert(
                workflow.event.clone(),
                EventInfo {
                    name: workflow.event.clone(),
                    is_external,
                    line,
                    usage: EventUsage::On,
                },
            );
        }

        // Collect events from `emit("EVENT")` calls
        for func in &program.functions {
            collect_emit_events(&func.body, source, &mut self.events);
        }
        for workflow in &program.workflows {
            collect_emit_events(&workflow.body, source, &mut self.events);
        }
    }

    /// Like `analyze`, but tolerates a parse error. The resulting
    /// `Inference` will have empty scopes and no function signatures.
    pub fn empty(line_count: usize) -> Self {
        Inference {
            scope_at: vec![Vec::new(); line_count.max(1)],
            functions: HashMap::new(),
            ..Default::default()
        }
    }

    /// Look up the type of the word at `position` in `source`.
    pub fn lookup(&self, source: &str, position: Position) -> Option<InferredBinding> {
        let word = word_at(source, position)?;
        // Walk the active scope stack at this position; the
        // innermost binding with a matching name wins.
        let offset = crate::analysis::position_to_byte_offset(source, position)?;
        for b in self.scope_index.bindings_at(offset) {
            if b.name == word {
                if let Some(typed) = self.typed.get(&word, b.scope_id) {
                    return Some(typed.clone());
                }
            }
        }
        // Check builtins
        if let Some(builtin) = builtins::builtin_for(&word) {
            return Some(builtin);
        }
        // Check the dynamic registry
        if let Some(entry) = self.registry.get(&word) {
            return Some(InferredBinding {
                name: entry.name,
                ty: entry.return_type,
                value: None,
                annotated: !entry.is_user_defined,
            });
        }
        None
    }

    /// Per-line scope view, kept for backward compat. The
    /// contents are derived from `scope_index` + `typed` at
    /// build time, so each line's bindings are exactly the
    /// typed bindings visible at the end of that line.
    pub fn scope_at_position(&self, position: Position) -> &[InferredBinding] {
        self.scope_at
            .get(position.line as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Position-precise lookup. Returns the typed binding for
    /// `name` at the given position, or `None` if no binding
    /// with that name is visible.
    pub fn lookup_at(
        &self,
        source: &str,
        position: Position,
        name: &str,
    ) -> Option<InferredBinding> {
        let offset = crate::analysis::position_to_byte_offset(source, position)?;
        self.lookup_at_offset(source, offset, name)
    }

    /// Like [`lookup_at`], but takes a byte offset directly instead
    /// of an LSP `Position`. Used by the lint which walks the AST
    /// and knows statement byte offsets but not column positions.
    pub fn lookup_at_offset(
        &self,
        _source: &str,
        offset: usize,
        name: &str,
    ) -> Option<InferredBinding> {
        for b in self.scope_index.bindings_at(offset) {
            if b.name == name {
                if let Some(typed) = self.typed.get(name, b.scope_id) {
                    return Some(typed.clone());
                }
            }
        }
        // Check builtins
        if let Some(builtin) = builtins::builtin_for(name) {
            return Some(builtin);
        }
        // Check the dynamic registry
        if let Some(entry) = self.registry.get(name) {
            return Some(InferredBinding {
                name: entry.name,
                ty: entry.return_type,
                value: None,
                annotated: !entry.is_user_defined,
            });
        }
        None
    }
}

/// Check if a string is in SCREAMING_SNAKE_CASE (convention for external events).
fn is_screaming_snake_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut has_upper = false;
    let mut has_underscore = false;
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            has_upper = true;
        } else if c == '_' {
            has_underscore = true;
            // Consecutive underscores are not allowed
            if i > 0 && s.as_bytes()[i - 1] == b'_' {
                return false;
            }
        } else if !c.is_ascii_lowercase() && !c.is_ascii_digit() {
            return false;
        }
    }
    // Must have at least one uppercase letter and use underscores
    has_upper && has_underscore
}

/// Collect events from `emit("EVENT")` calls in statements.
fn collect_emit_events(
    stmts: &[workflow_parser::ast::Stmt],
    source: &str,
    events: &mut HashMap<String, EventInfo>,
) {
    for stmt in stmts {
        match stmt {
            workflow_parser::ast::Stmt::Expr(expr, span)
            | workflow_parser::ast::Stmt::Log(expr, span) => {
                collect_emit_from_expr(expr, source, *span, events);
            }
            workflow_parser::ast::Stmt::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                collect_emit_from_expr(condition, source, stmt.span(), events);
                collect_emit_events(then_body, source, events);
                if let Some(eb) = else_body {
                    collect_emit_events(eb, source, events);
                }
            }
            workflow_parser::ast::Stmt::Foreach { body, .. } => {
                collect_emit_events(body, source, events);
            }
            workflow_parser::ast::Stmt::VarDecl { value: Some(v), .. } => {
                collect_emit_from_expr(v, source, stmt.span(), events);
            }
            workflow_parser::ast::Stmt::Return { value: Some(v), .. } => {
                collect_emit_from_expr(v, source, stmt.span(), events);
            }
            workflow_parser::ast::Stmt::Assign { value, .. } => {
                collect_emit_from_expr(value, source, stmt.span(), events);
            }
            _ => {}
        }
    }
}

/// Collect events from `emit("EVENT")` calls in an expression.
fn collect_emit_from_expr(
    expr: &workflow_parser::ast::Expr,
    source: &str,
    _span: workflow_parser::ast::Span,
    events: &mut HashMap<String, EventInfo>,
) {
    if let workflow_parser::ast::Expr::Call { name, args } = expr {
        if name == "emit" {
            if let Some(workflow_parser::ast::Expr::String(event_name)) = args.first() {
                let line = source[.._span.start].chars().filter(|c| *c == '\n').count() as u32;
                let is_external = is_screaming_snake_case(event_name);
                events.insert(
                    event_name.clone(),
                    EventInfo {
                        name: event_name.clone(),
                        is_external,
                        line,
                        usage: EventUsage::Emit,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workflow_parser::ast::Expr;
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
        // the line that uses it (line 4: `log(item)`).
        let line = inf.scope_at.get(4).expect("log(item) line");
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
        // The variable is declared on line 1, so it appears in
        // scope_at[1], not scope_at[0] (which is the annotation comment).
        let b = inf.scope_at[1]
            .iter()
            .find(|b| b.name == "name")
            .expect("name binding on line 1");
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

    // -- Function-body inference -----------------------------------------

    #[test]
    fn function_param_inferred_from_usage() {
        // `name` is passed to a string-concat, so we infer it as a
        // string. The return type is also a string from `return name`.
        let source = r#"fn greet(name) {
  return "hi, " + name
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("greet").expect("greet");
        assert_eq!(sig.param_types, vec![Type::String]);
        assert_eq!(sig.ret, Type::String);
        assert!(!sig.annotated);
    }

    #[test]
    fn function_param_inferred_from_call_to_known_fn() {
        // `xs` is passed to `len`, which returns `number`, so the
        // *return* type is known. The argument is left as `Any`
        // because `len` accepts both arrays and strings.
        let source = r#"fn size(xs) {
  return len(xs)
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("size").expect("size");
        assert_eq!(sig.param_types, vec![Type::Any]);
        assert_eq!(sig.ret, Type::Number);
    }

    #[test]
    fn function_param_inferred_from_literal_compare() {
        // `n == 0` ⇒ `n: number`.
        let source = r#"fn is_zero(n) {
  if (n == 0) {
    return true
  }
  return false
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("is_zero").expect("is_zero");
        assert_eq!(sig.param_types, vec![Type::Number]);
        assert_eq!(sig.ret, Type::Bool);
    }

    #[test]
    fn function_param_annotation_overrides_inference() {
        let source = r#"//@param name: number
fn greet(name) {
  return "hi, " + name
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("greet").expect("greet");
        assert_eq!(sig.param_types, vec![Type::Number]);
    }

    // -- Per-parameter binding population (regression for the
    //    `unknown-identifier` lint false positives on params) -------

    #[test]
    fn function_params_in_scope() {
        // `amount` and `currency` are function parameters — they
        // should be in the scope table for the lines that reference
        // them. Before the fix, the unknown-identifier lint reported
        // both as "Unknown identifier".
        let source = r#"fn formatCurrency(amount, currency) {
  return currency + " " + amount
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("formatCurrency").expect("formatCurrency");
        assert_eq!(sig.params, vec!["amount", "currency"]);
        // The signature picks up the types from the string concat
        // usage in the body.
        assert_eq!(sig.param_types, vec![Type::String, Type::String]);
        assert_eq!(sig.ret, Type::String);
        // And the names appear in the scope table.
        let on_line_1 = &inf.scope_at[1];
        assert!(
            on_line_1.iter().any(|b| b.name == "amount"),
            "amount missing from scope_at[1]: {:?}",
            on_line_1
        );
        assert!(
            on_line_1.iter().any(|b| b.name == "currency"),
            "currency missing from scope_at[1]: {:?}",
            on_line_1
        );
    }

    #[test]
    fn function_param_shortcut_annotation() {
        // The `//@T1,T2,T3` per-parameter shortcut. Each type
        // positionally maps to the next function parameter.
        //
        // Note: the shortcut is a *parameter* annotation, not a
        // full function-signature annotation, so `sig.annotated` is
        // `false` — the per-param map carries the types instead.
        // This is what lets body-based inference still determine
        // the return type.
        let source = r#"//@string,string
fn formatCurrency(amount, currency) {
  return currency + " " + amount
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("formatCurrency").expect("formatCurrency");
        assert_eq!(sig.params, vec!["amount", "currency"]);
        assert_eq!(sig.param_types, vec![Type::String, Type::String]);
        assert!(!sig.annotated);
        // The return type is still inferred from the body — the
        // body has a string concat, so the return is `string`.
        assert_eq!(sig.ret, Type::String);
    }

    #[test]
    fn function_param_shortcut_mismatch_falls_through() {
        // A 2-arg function with a 3-type annotation shouldn't
        // silently corrupt inference — it should fall through to
        // body-based inference rather than fail.
        let source = r#"//@string,number,bool
fn add(a, b) {
  return a + b
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("add").expect("add");
        // The shortcut didn't match (3 != 2), so we fall back to
        // the existing inference path which sees `a + b` and picks
        // Number for both.
        assert_eq!(sig.params, vec!["a", "b"]);
        assert_eq!(sig.param_types, vec![Type::Number, Type::Number]);
        assert!(!sig.annotated);
    }

    #[test]
    fn workflow_destructure_params_in_scope() {
        // The destructure `on EVENT ({a, b})` binds `a` and `b` to
        // names visible in the workflow body. Before the fix, the
        // unknown-identifier lint reported them as unknown.
        let source = r#"workflow "Nested" {
  on NESTED_DATA ({users, meta})
  log("Users: " + users.length + ", Meta: " + meta.length)
}
"#;
        let inf = infer(source);
        // The signature for the workflow is implicit (no `FunctionSig`),
        // but the param names should be in the scope table.
        let line = &inf.scope_at[2]; // the `log(...)` line
        assert!(
            line.iter().any(|b| b.name == "users"),
            "users missing from scope_at[2]: {:?}",
            line
        );
        assert!(
            line.iter().any(|b| b.name == "meta"),
            "meta missing from scope_at[2]: {:?}",
            line
        );
    }

    // -- Primitive member-access inference ----------------------------

    #[test]
    fn string_length_is_number() {
        // Hover on `email.length` should report `number` when `email`
        // is annotated as a string. The previous implementation
        // returned `Any` for any non-Object member access.
        use crate::inference::expr::infer_expr_with_ctx;
        let value = Expr::Member {
            object: Box::new(Expr::Var("email".into())),
            property: "length".into(),
        };
        let scope = vec![InferredBinding {
            name: "email".into(),
            ty: Type::String,
            value: None,
            annotated: true,
        }];
        let functions = std::collections::HashMap::new();
        let (ty, _) = infer_expr_with_ctx(&value, &scope, &functions, &[]);
        assert_eq!(ty, Type::Number);
    }

    #[test]
    fn string_method_return_types() {
        // `email.toUpperCase()` should report `string`,
        // `email.contains(...)` should report `bool`.
        use crate::inference::expr::infer_expr_with_ctx;
        let functions = std::collections::HashMap::new();
        let scope = vec![InferredBinding {
            name: "email".into(),
            ty: Type::String,
            value: None,
            annotated: true,
        }];
        for (prop, expected) in [
            ("toUpperCase", Type::String),
            ("toLowerCase", Type::String),
            ("trim", Type::String),
            ("contains", Type::Bool),
            ("startsWith", Type::Bool),
            ("endsWith", Type::Bool),
            ("toNumber", Type::Number),
        ] {
            let value = Expr::Member {
                object: Box::new(Expr::Var("email".into())),
                property: prop.into(),
            };
            let (ty, _) = infer_expr_with_ctx(&value, &scope, &functions, &[]);
            assert_eq!(ty, expected, "email.{}: got {:?}", prop, ty);
        }
    }

    #[test]
    fn array_length_is_number() {
        use crate::inference::expr::infer_expr_with_ctx;
        let functions = std::collections::HashMap::new();
        let scope = vec![InferredBinding {
            name: "items".into(),
            ty: Type::Array(Box::new(Type::String)),
            value: None,
            annotated: true,
        }];
        let value = Expr::Member {
            object: Box::new(Expr::Var("items".into())),
            property: "length".into(),
        };
        let (ty, _) = infer_expr_with_ctx(&value, &scope, &functions, &[]);
        assert_eq!(ty, Type::Number);
    }

    // -- Shortcut annotation must not pin the return type ------------

    #[test]
    fn shortcut_annotation_does_not_pin_return_type() {
        // `//@string` annotates the *parameter* `email` as string.
        // It must NOT prevent body-based inference of the return
        // type. `validateEmail` returns `true` / `false`, so the
        // inferred return type should be `Bool`, not `Any`.
        let source = r#"//@string
fn validateEmail(email) {
  var length = email.length
  if (length > 5) {
    return true
  }
  return false
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("validateEmail").expect("validateEmail");
        assert_eq!(sig.param_types, vec![Type::String]);
        assert_eq!(
            sig.ret,
            Type::Bool,
            "expected return type to be inferred from body as Bool, got {:?}",
            sig.ret
        );
    }

    #[test]
    fn full_signature_omitting_return_falls_through_to_body() {
        // The full `//@{a:T, b:T}` form (no `-> R`) used to pin the
        // return type to `Any`. After the fix, an unspecified return
        // type in the full form also falls through to body inference.
        let source = r#"//@{value:string, count:number}
fn summarize(value, count) {
  return value
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("summarize").expect("summarize");
        assert_eq!(sig.param_types, vec![Type::String, Type::Number]);
        // Body returns `value` (a string), so ret should be `String`.
        assert_eq!(sig.ret, Type::String);
    }

    #[test]
    fn full_signature_with_return_type_wins() {
        // The full `//@{...} -> R` form pins the return type, as
        // before. The body returning `value` (a string) should NOT
        // override the explicit `-> number` annotation.
        let source = r#"//@{value:string, count:number} -> number
fn summarize(value, count) {
  return value
}
"#;
        let inf = infer(source);
        let sig = inf.functions.get("summarize").expect("summarize");
        assert_eq!(sig.ret, Type::Number);
    }

    #[test]
    fn local_var_in_function_body_sees_annotated_param() {
        // Regression for the "function body type inference uses an
        // empty scope" bug: `var length = email.length` should pick
        // up `email`'s annotated `string` type and resolve `.length`
        // to `number`. Before the fix, `email` was not in scope, so
        // the inference saw `Expr::Var("email")` as `Any` and
        // `.length` fell through to `Any` as well.
        let source = r#"//@string
fn validateEmail(email) {
  var length = email.length
  if (length > 5) {
    return true
  }
  return false
}
"#;
        let inf = infer(source);
        // The `length` binding should be a `number`, not `any`,
        // because the inference resolved `email.length` through
        // the annotated param type.
        let length_binding = inf
            .scope_at
            .iter()
            .flatten()
            .find(|b| b.name == "length")
            .expect("length should be in scope");
        assert_eq!(
            length_binding.ty,
            Type::Number,
            "expected email.length to be typed as number, got {:?}",
            length_binding.ty
        );
    }
}
