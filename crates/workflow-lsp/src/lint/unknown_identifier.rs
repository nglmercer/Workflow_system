//! `unknown-identifier`: reports references to names that aren't in
//! scope. Severity: `Error`. Skips known builtins (handled by
//! `inference::builtins`) and skips member-access property names (the
//! property side of `foo.bar` is not required to be a known symbol).

use crate::features::{Diagnostic, DiagnosticSeverity};
use crate::lint::{expr_position, Lint, LintCx};
use workflow_parser::ast::{Expr, FlowProgram, Stmt};

pub struct UnknownIdentifier;

impl Lint for UnknownIdentifier {
    fn name(&self) -> &'static str {
        "unknown-identifier"
    }

    fn run(&self, cx: &LintCx) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for global in &cx.program.globals {
            check_expr(cx, &global.value, global.span.start, &mut out);
        }
        for workflow in &cx.program.workflows {
            check_stmts(cx, &workflow.body, &mut out);
        }
        for func in &cx.program.functions {
            check_stmts(cx, &func.body, &mut out);
        }
        out
    }
}

fn check_stmts(cx: &LintCx, stmts: &[Stmt], out: &mut Vec<Diagnostic>) {
    for stmt in stmts {
        // Use the statement's byte offset as the search anchor
        // for Var lookups inside it. This ensures we find the
        // correct occurrence of each name — the one inside this
        // statement, not an earlier one in a different scope.
        let anchor = stmt.span().start;
        match stmt {
            Stmt::VarDecl { value, .. } => {
                if let Some(v) = value {
                    check_expr(cx, v, anchor, out);
                }
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                check_expr(cx, condition, anchor, out);
                check_stmts(cx, then_body, out);
                if let Some(eb) = else_body {
                    check_stmts(cx, eb, out);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    check_expr(cx, v, anchor, out);
                }
            }
            Stmt::Expr(e, _) | Stmt::Log(e, _) => check_expr(cx, e, anchor, out),
            Stmt::Foreach { iterable, body, .. } => {
                check_expr(cx, iterable, anchor, out);
                check_stmts(cx, body, out);
            }
            Stmt::On { .. } => {}
            Stmt::Assign { value, .. } => check_expr(cx, value, anchor, out),
        }
    }
}

fn check_expr(cx: &LintCx, expr: &Expr, after_byte: usize, out: &mut Vec<Diagnostic>) {
    match expr {
        Expr::Var(name) => {
            if is_known(cx, name, after_byte) {
                return;
            }
            // For the diagnostic position, use the statement anchor.
            // This may not be the exact column of the Var, but it's
            // close enough and always correct for line/col reporting.
            if let Some((line, col)) = find_name_position(cx.source, name, after_byte) {
                if cx.disabled.is_disabled("unknown-identifier", line) {
                    return;
                }
                out.push(cx.diag(
                    "unknown-identifier",
                    line,
                    col,
                    workflow_i18n::tf("lsp.lint_unknown_identifier", &[("name", name)]),
                    DiagnosticSeverity::Error,
                ));
            }
        }
        Expr::Member { object, .. } => {
            check_expr(cx, object, after_byte, out);
        }
        Expr::BinaryOp { left, right, .. } => {
            check_expr(cx, left, after_byte, out);
            check_expr(cx, right, after_byte, out);
        }
        Expr::UnaryOp { operand, .. } => check_expr(cx, operand, after_byte, out),
        Expr::Call { name, args } => {
            if !is_known_function(cx, name) {
                if let Some((line, col)) = expr_position(cx.source, expr) {
                    if !cx.disabled.is_disabled("unknown-identifier", line) {
                        out.push(cx.diag(
                            "unknown-identifier",
                            line,
                            col,
                            workflow_i18n::tf("lsp.lint_unknown_function", &[("name", name)]),
                            DiagnosticSeverity::Error,
                        ));
                    }
                }
            }
            for arg in args {
                check_expr(cx, arg, after_byte, out);
            }
        }
        Expr::Array(elems) => {
            for e in elems {
                check_expr(cx, e, after_byte, out);
            }
        }
        Expr::InterpolatedString(parts) => {
            for p in parts {
                if let workflow_parser::ast::InterpPart::Expr(e) = p {
                    check_expr(cx, e, after_byte, out);
                }
            }
        }
        Expr::String(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null => {}
    }
}

/// Check whether `name` is known in scope at `after_byte`.
fn is_known(cx: &LintCx, name: &str, after_byte: usize) -> bool {
    if crate::inference::builtins::builtin_for(name).is_some() {
        return true;
    }
    if name == "data" {
        return true;
    }
    if cx.program.imports.iter().any(|i| i.name == name) {
        return true;
    }
    cx.inference
        .lookup_at_offset(cx.source, after_byte, name)
        .is_some()
}

fn is_known_function(cx: &LintCx, name: &str) -> bool {
    if crate::inference::builtins::builtin_for(name).is_some() {
        return true;
    }
    if cx.inference.functions.contains_key(name) {
        return true;
    }
    // Check the dynamic registry
    if cx.inference.registry.contains(name) {
        return true;
    }
    false
}

/// Find the (line, col) of `name` in source at or after `after_byte`.
fn find_name_position(source: &str, name: &str, after_byte: usize) -> Option<(u32, u32)> {
    let bytes = source.as_bytes();
    let name_bytes = name.as_bytes();
    // Search from after_byte forward for the identifier.
    let mut i = after_byte;
    while i + name_bytes.len() <= bytes.len() {
        if &bytes[i..i + name_bytes.len()] == name_bytes {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_idx = i + name_bytes.len();
            let after_ok = after_idx == bytes.len() || !is_ident_byte(bytes[after_idx]);
            if before_ok && after_ok {
                let span = workflow_parser::Span::new(i, after_idx);
                return span.to_line_col(source).map(|(sl, sc, _, _)| (sl, sc));
            }
        }
        i += 1;
    }
    // Fallback: search from the beginning of the source.
    let mut i = 0;
    while i + name_bytes.len() <= bytes.len() {
        if &bytes[i..i + name_bytes.len()] == name_bytes {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_idx = i + name_bytes.len();
            let after_ok = after_idx == bytes.len() || !is_ident_byte(bytes[after_idx]);
            if before_ok && after_ok {
                let span = workflow_parser::Span::new(i, after_idx);
                return span.to_line_col(source).map(|(sl, sc, _, _)| (sl, sc));
            }
        }
        i += 1;
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[allow(dead_code)]
fn _ensure_flow_program_in_scope(_: &FlowProgram) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::{parse_disable_directives, LintCx};
    use crate::state::ServerState;
    use workflow_parser::FlowParser;

    fn run_lint(source: &str) -> Vec<Diagnostic> {
        let mut state = ServerState::new();
        let uri = "file:///test.flow";
        state.update_document(uri, source);
        let analysis = state.get_analysis(uri).expect("analysis");
        let inference = state.get_inference(uri).expect("inference");
        let program = FlowParser::parse_flow_program(source).expect("parse");
        let disabled = parse_disable_directives(source);
        let cx = LintCx {
            source,
            analysis,
            inference,
            program: &program,
            disabled: &disabled,
        };
        UnknownIdentifier.run(&cx)
    }

    #[test]
    fn unknown_variable_is_error() {
        let source = r#"workflow "W" { on E
  log(undefined_var)
}"#;
        let diags = run_lint(source);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Unknown identifier")
                    && d.message.contains("undefined_var")),
            "got: {:?}",
            diags
        );
    }

    #[test]
    fn defined_variable_is_ok() {
        let source = r#"workflow "W" { on E
  var x = 42
  log(x)
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }

    #[test]
    fn builtins_are_known() {
        let source = r#"workflow "W" { on E
  log("hi")
  var n = len("hello")
  log(to_string(n))
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }

    #[test]
    fn disable_directive_suppresses() {
        let source = r#"workflow "W" { on E
  // flow-lint:disable=unknown-identifier
  log(undefined_var)
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }

    #[test]
    fn unknown_function_is_error() {
        let source = r#"workflow "W" { on E
  notAFunction(42)
}"#;
        let diags = run_lint(source);
        assert!(
            diags.iter().any(|d| d.message.contains("Unknown function")),
            "got: {:?}",
            diags
        );
    }
}
