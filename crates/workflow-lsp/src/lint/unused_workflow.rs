//! `unused-workflow`: reports workflows that either have no `on`
//! clause or whose event is never emitted anywhere. Severity: `Hint`.
//! Functions and globals are not flagged (they may be called from
//! outside the file or used as entry points).

use std::collections::HashSet;

use crate::features::{Diagnostic, DiagnosticSeverity};
use crate::lint::{Lint, LintCx};
use workflow_parser::ast::{Expr, Stmt};

pub struct UnusedWorkflow;

impl Lint for UnusedWorkflow {
    fn name(&self) -> &'static str {
        "unused-workflow"
    }

    fn run(&self, cx: &LintCx) -> Vec<Diagnostic> {
        // Collect every `Expr::Call` name in the program. Treat any
        // call to `emit("FOO")` as evidence that `FOO` is dispatched.
        // We also treat plain `FOO(...)` (when `FOO` matches a
        // workflow's event name) as dispatch, but conservatively only
        // flag workflows whose event is *also* never passed to
        // `emit`. This avoids false-positives on user-defined
        // helper functions.
        let mut emitted_events: HashSet<String> = HashSet::new();
        for w in &cx.program.workflows {
            collect_emits(&w.body, &mut emitted_events);
        }
        for f in &cx.program.functions {
            collect_emits(&f.body, &mut emitted_events);
        }

        let mut out = Vec::new();
        for w in &cx.program.workflows {
            if w.event.is_empty() {
                if !cx.disabled.is_disabled("unused-workflow", 0) {
                    out.push(cx.diag(
                        "unused-workflow",
                        0,
                        0,
                        format!("Workflow \"{}\" has no `on` clause", w.name),
                        DiagnosticSeverity::Hint,
                    ));
                }
                continue;
            }
            if !emitted_events.contains(&w.event) {
                if let Some((line, col)) = workflow_header_position(cx, &w.name) {
                    if cx.disabled.is_disabled("unused-workflow", line) {
                        continue;
                    }
                    out.push(cx.diag(
                        "unused-workflow",
                        line,
                        col,
                        format!(
                            "Workflow \"{}\" listens for `{}` but no `emit(\"{}\")` was found",
                            w.name, w.event, w.event
                        ),
                        DiagnosticSeverity::Hint,
                    ));
                }
            }
        }
        out
    }
}

fn collect_emits(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Log(e) => collect_emits_in_expr(e, out),
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                collect_emits_in_expr(condition, out);
                collect_emits(then_body, out);
                if let Some(eb) = else_body {
                    collect_emits(eb, out);
                }
            }
            Stmt::Return { value } => {
                if let Some(v) = value {
                    collect_emits_in_expr(v, out);
                }
            }
            Stmt::VarDecl { value, .. } => {
                if let Some(v) = value {
                    collect_emits_in_expr(v, out);
                }
            }
            Stmt::Foreach { iterable, body, .. } => {
                collect_emits_in_expr(iterable, out);
                collect_emits(body, out);
            }
            Stmt::On { .. } => {}
        }
    }
}

fn collect_emits_in_expr(expr: &Expr, out: &mut HashSet<String>) {
    if let Expr::Call { name, args } = expr {
        if name == "emit" {
            if let Some(Expr::String(s)) = args.first() {
                out.insert(s.clone());
            }
        }
        for a in args {
            collect_emits_in_expr(a, out);
        }
    }
    if let Expr::BinaryOp { left, right, .. } = expr {
        collect_emits_in_expr(left, out);
        collect_emits_in_expr(right, out);
    }
    if let Expr::UnaryOp { operand, .. } = expr {
        collect_emits_in_expr(operand, out);
    }
    if let Expr::Member { object, .. } = expr {
        collect_emits_in_expr(object, out);
    }
    if let Expr::Array(elems) = expr {
        for e in elems {
            collect_emits_in_expr(e, out);
        }
    }
    if let Expr::InterpolatedString(parts) = expr {
        for p in parts {
            if let workflow_parser::ast::InterpPart::Expr(e) = p {
                collect_emits_in_expr(e, out);
            }
        }
    }
}

fn workflow_header_position(cx: &LintCx, name: &str) -> Option<(u32, u32)> {
    let needle = format!("workflow \"{}\"", name);
    let idx = cx.source.find(&needle)? as u32;
    let (line, col) = workflow_parser::ast::byte_to_line_col(cx.source, idx as usize);
    Some((line, col))
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
        UnusedWorkflow.run(&cx)
    }

    #[test]
    fn workflow_with_no_emit_is_hint() {
        let source = r#"workflow "W" {
  on NEVER_EMITTED
  log("hi")
}"#;
        let diags = run_lint(source);
        assert!(
            diags.iter().any(|d| d.message.contains("NEVER_EMITTED")),
            "got: {:?}",
            diags
        );
    }

    #[test]
    fn workflow_with_emit_is_ok() {
        let source = r#"workflow "A" {
  on HELLO
  emit("HELLO")
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }
}
