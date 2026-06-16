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
            check_expr(cx, &global.value, &mut out);
        }
        for workflow in &cx.program.workflows {
            check_stmts(cx, &workflow.body, &mut out);
            // `data` is implicitly in scope inside every workflow.
            // Add it to the analysis scope at runtime by skipping
            // `data` references.
        }
        for func in &cx.program.functions {
            check_stmts(cx, &func.body, &mut out);
        }
        out
    }
}

fn check_stmts(cx: &LintCx, stmts: &[Stmt], out: &mut Vec<Diagnostic>) {
    for stmt in stmts {
        match stmt {
            Stmt::VarDecl { value, .. } => {
                if let Some(v) = value {
                    check_expr(cx, v, out);
                }
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            ..
            } => {
                check_expr(cx, condition, out);
                check_stmts(cx, then_body, out);
                if let Some(eb) = else_body {
                    check_stmts(cx, eb, out);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    check_expr(cx, v, out);
                }
            }
            Stmt::Expr(e, _) | Stmt::Log(e, _) => check_expr(cx, e, out),
            Stmt::Foreach { iterable, body, .. } => {
                check_expr(cx, iterable, out);
                check_stmts(cx, body, out);
            }
            Stmt::On { .. } => {}
            Stmt::Assign { value, .. } => check_expr(cx, value, out),
        }
    }
}

fn check_expr(cx: &LintCx, expr: &Expr, out: &mut Vec<Diagnostic>) {
    match expr {
        Expr::Var(name) => {
            if is_known(cx, expr, name) {
                return;
            }
            if let Some((line, col)) = expr_position(cx.source, expr) {
                if cx.disabled.is_disabled("unknown-identifier", line) {
                    return;
                }
                out.push(cx.diag(
                    "unknown-identifier",
                    line,
                    col,
                    format!("Unknown identifier `{}`", name),
                    DiagnosticSeverity::Error,
                ));
            }
        }
        Expr::Member { object, .. } => {
            // `foo.bar` — only `foo` needs to be in scope; `bar` is
            // a property and the parser doesn't track schema.
            check_expr(cx, object, out);
        }
        Expr::BinaryOp { left, right, .. } => {
            check_expr(cx, left, out);
            check_expr(cx, right, out);
        }
        Expr::UnaryOp { operand, .. } => check_expr(cx, operand, out),
        Expr::Call { name, args } => {
            // `name` is a free variable — should be in scope as a
            // function. The typecheck linter already covers argument
            // types, so we only flag when the function itself is
            // unknown.
            if !is_known_function(cx, name) {
                if let Some((line, col)) = expr_position(cx.source, expr) {
                    if !cx.disabled.is_disabled("unknown-identifier", line) {
                        out.push(cx.diag(
                            "unknown-identifier",
                            line,
                            col,
                            format!("Unknown function `{}`", name),
                            DiagnosticSeverity::Error,
                        ));
                    }
                }
            }
            for arg in args {
                check_expr(cx, arg, out);
            }
        }
        Expr::Array(elems) => {
            for e in elems {
                check_expr(cx, e, out);
            }
        }
        Expr::InterpolatedString(parts) => {
            for p in parts {
                if let workflow_parser::ast::InterpPart::Expr(e) = p {
                    check_expr(cx, e, out);
                }
            }
        }
        Expr::String(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null => {}
    }
}

fn is_known(cx: &LintCx, expr: &Expr, name: &str) -> bool {
    // Builtins are always in scope (`log`, `len`, `to_string`, etc.).
    if crate::inference::builtins::builtin_for(name).is_some() {
        return true;
    }
    // `data` is the implicit workflow event payload.
    if name == "data" {
        return true;
    }
    // Otherwise, ask the inference scope. We use the line of the
    // expression itself.
    let Some((line, _)) = expr_position(cx.source, expr) else {
        return false;
    };
    let scope = cx.inference.scope_at.get(line as usize);
    if let Some(scope) = scope {
        if scope.iter().any(|b| b.name == name) {
            return true;
        }
    }
    false
}

fn is_known_function(cx: &LintCx, name: &str) -> bool {
    if crate::inference::builtins::builtin_for(name).is_some() {
        return true;
    }
    cx.inference.functions.contains_key(name)
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
