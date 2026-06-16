//! `redundant-expression`: flags `Expr::BinaryOp` nodes whose two
//! operands are both literal values of the same type. Severity:
//! `Hint`. The motivation is to nudge users to fold the value into a
//! single literal rather than carrying a constant expression around.
//!
//! This lint is intentionally conservative: it only matches
//! `BinaryOp`s that are *fully literal*, so anything involving a
//! variable, call, or member access is ignored.

use crate::features::{Diagnostic, DiagnosticSeverity};
use crate::lint::{expr_position, Lint, LintCx};
use workflow_parser::ast::{Expr, Stmt};

pub struct RedundantExpression;

impl Lint for RedundantExpression {
    fn name(&self) -> &'static str {
        "redundant-expression"
    }

    fn run(&self, cx: &LintCx) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for w in &cx.program.workflows {
            scan_stmts(cx, &w.body, &mut out);
        }
        for f in &cx.program.functions {
            scan_stmts(cx, &f.body, &mut out);
        }
        for g in &cx.program.globals {
            scan_expr(cx, &g.value, &mut out);
        }
        out
    }
}

fn scan_stmts(cx: &LintCx, stmts: &[Stmt], out: &mut Vec<Diagnostic>) {
    for s in stmts {
        match s {
            Stmt::VarDecl { value, .. } => {
                if let Some(v) = value {
                    scan_expr(cx, v, out);
                }
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                scan_expr(cx, condition, out);
                scan_stmts(cx, then_body, out);
                if let Some(eb) = else_body {
                    scan_stmts(cx, eb, out);
                }
            }
            Stmt::Return { value } => {
                if let Some(v) = value {
                    scan_expr(cx, v, out);
                }
            }
            Stmt::Expr(e) | Stmt::Log(e) => scan_expr(cx, e, out),
            Stmt::Foreach { iterable, body, .. } => {
                scan_expr(cx, iterable, out);
                scan_stmts(cx, body, out);
            }
            Stmt::On { .. } => {}
            Stmt::Assign { value, .. } => scan_expr(cx, value, out),
        }
    }
}

fn scan_expr(cx: &LintCx, expr: &Expr, out: &mut Vec<Diagnostic>) {
    if let Expr::BinaryOp { left, right, op } = expr {
        if is_constant(left) && is_constant(right) && literals_same_type(left, right) {
            // Skip boolean operators: `true && true` and `false ||
            // false` are stylistic, not redundant.
            if !matches!(
                op,
                workflow_parser::ast::BinaryOp::And
                    | workflow_parser::ast::BinaryOp::Or
                    | workflow_parser::ast::BinaryOp::Eq
                    | workflow_parser::ast::BinaryOp::Neq
                    | workflow_parser::ast::BinaryOp::Lt
                    | workflow_parser::ast::BinaryOp::Gt
                    | workflow_parser::ast::BinaryOp::Lte
                    | workflow_parser::ast::BinaryOp::Gte
            ) {
                if let Some((line, col)) = expr_position(cx.source, expr) {
                    if !cx.disabled.is_disabled("redundant-expression", line) {
                        out.push(cx.diag(
                            "redundant-expression",
                            line,
                            col,
                            "Expression has two literal operands — fold it into a single literal",
                            DiagnosticSeverity::Hint,
                        ));
                    }
                }
            }
        }
    }
    // Recurse so nested binary ops are also reported.
    if let Expr::BinaryOp { left, right, .. } = expr {
        scan_expr(cx, left, out);
        scan_expr(cx, right, out);
    }
    if let Expr::UnaryOp { operand, .. } = expr {
        scan_expr(cx, operand, out);
    }
    if let Expr::Call { args, .. } = expr {
        for a in args {
            scan_expr(cx, a, out);
        }
    }
    if let Expr::Array(elems) = expr {
        for e in elems {
            scan_expr(cx, e, out);
        }
    }
    if let Expr::InterpolatedString(parts) = expr {
        for p in parts {
            if let workflow_parser::ast::InterpPart::Expr(e) = p {
                scan_expr(cx, e, out);
            }
        }
    }
    if let Expr::Member { object, .. } = expr {
        scan_expr(cx, object, out);
    }
}

fn is_constant(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::String(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null
    )
}

fn literals_same_type(a: &Expr, b: &Expr) -> bool {
    matches!(
        (a, b),
        (Expr::String(_), Expr::String(_))
            | (Expr::Number(_), Expr::Number(_))
            | (Expr::Bool(_), Expr::Bool(_))
            | (Expr::Null, Expr::Null)
    )
}

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
        RedundantExpression.run(&cx)
    }

    #[test]
    fn two_numeric_literals_flagged() {
        let source = r#"workflow "W" { on E
  var total = 1 + 2
}"#;
        let diags = run_lint(source);
        assert!(
            diags.iter().any(|d| d.message.contains("literal")),
            "got: {:?}",
            diags
        );
    }

    #[test]
    fn variable_in_expression_is_ok() {
        let source = r#"workflow "W" { on E
  var x = 1
  var total = x + 2
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }

    #[test]
    fn comparison_of_literals_is_silent() {
        // `1 == 2` is a comparison, not arithmetic; the lint should
        // not flag it (would be too noisy).
        let source = r#"workflow "W" { on E
  if (1 == 2) { log("nope") }
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }
}
