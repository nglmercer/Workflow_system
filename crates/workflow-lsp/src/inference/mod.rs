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
pub mod ty;
pub mod value;

pub use ty::Type;
pub use value::{FunctionSig, InferredBinding, Value};

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
        let line_count = source.lines().count().max(1);
        let mut inference = Inference {
            scope_at: vec![Vec::new(); line_count],
            functions: HashMap::new(),
        };
        let annotations = annotation::parse_annotations(source);
        program::run_program(&mut inference, program, &annotations);
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
}
