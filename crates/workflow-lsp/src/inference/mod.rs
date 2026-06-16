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

pub mod annotation;
pub mod builtins;
pub mod expr;
pub mod program;
pub mod schema;
pub mod ty;
pub mod value;

pub use ty::Type;
pub use value::{FunctionSig, InferredBinding, Value};

pub mod methods;

#[derive(Debug, Clone, Default)]
pub struct Inference {
    /// One entry per source line, mirroring `Analysis::scope_at`. The
    /// entries for a given line are everything visible at that line.
    pub scope_at: Vec<Vec<InferredBinding>>,
    /// Function signatures indexed by name.
    pub functions: HashMap<String, FunctionSig>,
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
        let line_count = source.lines().count().max(1);
        let mut inference = Inference {
            scope_at: vec![Vec::new(); line_count],
            functions: HashMap::new(),
        };
        let annotations = annotation::parse_annotations(source);
        let (_schemas, import_bindings) =
            schema::resolve_schemas_for_program(&program.imports, document_path);
        program::run_program_with_imports(
            &mut inference,
            program,
            &annotations,
            &import_bindings,
        );
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

    /// Look up the type of the word at `position` in `source`.
    pub fn lookup(&self, source: &str, position: Position) -> Option<InferredBinding> {
        let word = word_at(source, position)?;
        let line_idx = position.line as usize;
        let scope = self.scope_at.get(line_idx)?;
        scope
            .iter()
            .find(|b| b.name == word)
            .cloned()
            .or_else(|| builtins::builtin_for(&word))
    }

    pub fn scope_at_position(&self, position: Position) -> &[InferredBinding] {
        self.scope_at
            .get(position.line as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
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
