//! `unused-workflow`: reports workflows that either have no `on`
//! clause or whose event is never emitted anywhere. Severity: `Hint`.
//! Functions and globals are not flagged (they may be called from
//! outside the file or used as entry points).
//!
//! An event is treated as **external** (and therefore not flagged) when
//! either:
//!
//! 1. The event name matches the `SCREAMING_SNAKE_CASE` convention
//!    (e.g. `USER_REGISTERED`, `BATCH_START`). This is the convention
//!    for events received from outside the file, so the hint would
//!    otherwise be a constant false positive on every realistic
//!    workflow. Workflows that *do* use `emit(...)` to dispatch the
//!    same event still aren't flagged (they're trivially non-external).
//!
//! 2. The event matches the binding name of an imported data schema
//!    (`@import NESTED_DATA from "./nested.json"` and the workflow
//!    listens for `on NESTED_DATA`). Importing a schema means the
//!    event is being consumed from an external source — the
//!    `//@external` annotation we used to accept has been removed in
//!    favour of the explicit import.
//!
//! The previous `//@external` annotation syntax is no longer
//! recognised. To make an event external, either rename it to
//! `SCREAMING_SNAKE_CASE` or import its payload schema.

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
        let mut emitted_events: HashSet<String> = HashSet::new();
        for w in &cx.program.workflows {
            collect_emits(&w.body, &mut emitted_events);
        }
        for f in &cx.program.functions {
            collect_emits(&f.body, &mut emitted_events);
        }

        // An event that has a payload schema imported for it is
        // also external: the user explicitly told us the event
        // arrives from outside the file by importing its type. The
        // schema's binding name (e.g. `data` for `@import data from
        // ...`, or `NESTED_DATA` for `import NESTED_DATA from ...`)
        // matches the workflow's event name in the typical pattern.
        let imported_event_names: HashSet<String> =
            cx.program.imports.iter().map(|i| i.name.clone()).collect();

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
            // Suppress: emitted in-file, matches an imported schema's
            // binding name, or follows the SCREAMING_SNAKE_CASE
            // convention.
            if emitted_events.contains(&w.event) {
                continue;
            }
            if imported_event_names.contains(&w.event) {
                continue;
            }
            if is_screaming_snake_case(&w.event) {
                continue;
            }
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
        out
    }
}

/// Returns true if `name` follows the `SCREAMING_SNAKE_CASE` convention
/// (e.g. `USER_REGISTERED`, `BATCH_START`, `CALCULATE`). These are
/// treated as external-event names by convention and are exempt from
/// the `unused-workflow` hint.
fn is_screaming_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && name.len() >= 2
}

fn collect_emits(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Expr(e, _) | Stmt::Log(e, _) => collect_emits_in_expr(e, out),
            Stmt::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                collect_emits_in_expr(condition, out);
                collect_emits(then_body, out);
                if let Some(eb) = else_body {
                    collect_emits(eb, out);
                }
            }
            Stmt::Return { value, .. } => {
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
            Stmt::Assign { value, .. } => collect_emits_in_expr(value, out),
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
        // Use a non-SCREAMING_SNAKE_CASE event so the SCREAMING_SNAKE_CASE
        // heuristic doesn't suppress the hint. The lint should still fire.
        let source = r#"workflow "W" {
  on lowercase_event
  log("hi")
}"#;
        let diags = run_lint(source);
        assert!(
            diags.iter().any(|d| d.message.contains("lowercase_event")),
            "got: {:?}",
            diags
        );
    }

    #[test]
    fn screaming_snake_case_is_external() {
        // SCREAMING_SNAKE_CASE events are treated as external by
        // convention and are not flagged.
        let source = r#"workflow "W" {
  on USER_REGISTERED
  log("hi")
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }

    #[test]
    fn imported_schema_marks_event_external() {
        // Importing a schema whose binding name matches the
        // workflow's event marks the event as external — the user
        // is saying "this event arrives from outside the file".
        let source = r#"@import my_external_event from { users: [], meta: [] }
workflow "W" {
  on my_external_event
  log("hi")
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }

    #[test]
    fn legacy_external_annotation_is_no_longer_recognised() {
        // The old `//@external` annotation syntax has been removed.
        // Workflows that rely on it need to be migrated to the
        // `@import data from ...` form (or rename the event to
        // SCREAMING_SNAKE_CASE). With the annotation gone, the
        // lint should fire on the lowercase event name.
        let source = r#"workflow "W" {
  //@external
  on my_external_event
  log("hi")
}"#;
        let diags = run_lint(source);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("my_external_event")),
            "expected the lint to flag the event; got: {:?}",
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
