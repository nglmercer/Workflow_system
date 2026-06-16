//! `unused-binding`: reports `var` declarations whose name is never
//! referenced in any expression after the declaration. Severity:
//! `Hint`. Skips globals and function parameters (they may be used
//! by other functions or across files).

use std::collections::HashSet;

use crate::features::{Diagnostic, DiagnosticSeverity};
use crate::lint::{expr_position, Lint, LintCx};
use workflow_parser::ast::{Expr, Stmt};

pub struct UnusedBinding;

impl Lint for UnusedBinding {
    fn name(&self) -> &'static str {
        "unused-binding"
    }

    fn run(&self, cx: &LintCx) -> Vec<Diagnostic> {
        // Collect all variable *references* (every `Expr::Var` name)
        // in the program.
        let mut references: HashSet<String> = HashSet::new();
        for global in &cx.program.globals {
            collect_refs(&global.value, &mut references);
        }
        for w in &cx.program.workflows {
            collect_stmts_refs(&w.body, &mut references);
        }
        for f in &cx.program.functions {
            collect_stmts_refs(&f.body, &mut references);
        }

        let mut out = Vec::new();
        // Globals: skip — they may be referenced by other files.
        // Function parameters: skip — they may be used by callers.
        // Locals and foreach items: check.
        for w in &cx.program.workflows {
            scan_locals(cx, &w.body, &mut out, &references);
        }
        for f in &cx.program.functions {
            scan_locals(cx, &f.body, &mut out, &references);
        }
        out
    }
}

fn collect_stmts_refs(stmts: &[Stmt], refs: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::VarDecl { value, .. } => {
                if let Some(v) = value {
                    collect_refs(v, refs);
                }
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                collect_refs(condition, refs);
                collect_stmts_refs(then_body, refs);
                if let Some(eb) = else_body {
                    collect_stmts_refs(eb, refs);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    collect_refs(v, refs);
                }
            }
            Stmt::Expr(e, _) | Stmt::Log(e, _) => collect_refs(e, refs),
            Stmt::Foreach { iterable, body, .. } => {
                collect_refs(iterable, refs);
                collect_stmts_refs(body, refs);
            }
            Stmt::On { .. } => {}
            Stmt::Assign { value, .. } => collect_refs(value, refs),
        }
    }
}

fn collect_refs(expr: &Expr, refs: &mut HashSet<String>) {
    match expr {
        Expr::Var(name) => {
            refs.insert(name.clone());
        }
        Expr::Member { object, .. } => collect_refs(object, refs),
        Expr::BinaryOp { left, right, .. } => {
            collect_refs(left, refs);
            collect_refs(right, refs);
        }
        Expr::UnaryOp { operand, .. } => collect_refs(operand, refs),
        Expr::Call { args, .. } => {
            for a in args {
                collect_refs(a, refs);
            }
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_refs(e, refs);
            }
        }
        Expr::InterpolatedString(parts) => {
            for p in parts {
                if let workflow_parser::ast::InterpPart::Expr(e) = p {
                    collect_refs(e, refs);
                }
            }
        }
        Expr::String(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null => {}
    }
}

fn scan_locals(cx: &LintCx, stmts: &[Stmt], out: &mut Vec<Diagnostic>, refs: &HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::VarDecl { name, value, .. } if !refs.contains(name) => {
                if let Some((line, col)) = var_decl_position(cx, name, value) {
                    if cx.disabled.is_disabled("unused-binding", line) {
                        continue;
                    }
                    out.push(cx.diag(
                        "unused-binding",
                        line,
                        col,
                        format!("Unused local variable `{}`", name),
                        DiagnosticSeverity::Hint,
                    ));
                }
            }
            Stmt::Foreach { item_var, body, .. } => {
                if !refs.contains(item_var) {
                    if let Some((line, col)) = foreach_position(cx, item_var) {
                        if !cx.disabled.is_disabled("unused-binding", line) {
                            out.push(cx.diag(
                                "unused-binding",
                                line,
                                col,
                                format!("Unused foreach item `{}`", item_var),
                                DiagnosticSeverity::Hint,
                            ));
                        }
                    }
                }
                scan_locals(cx, body, out, refs);
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                scan_locals(cx, then_body, out, refs);
                if let Some(eb) = else_body {
                    scan_locals(cx, eb, out, refs);
                }
            }
            _ => {}
        }
    }
}

/// Locate a `var name = ...` declaration by searching the source for
/// the keyword followed by the name.
fn var_decl_position(cx: &LintCx, name: &str, value: &Option<Expr>) -> Option<(u32, u32)> {
    if let Some(value) = value {
        if let Some((line, col)) = expr_position(cx.source, value) {
            // Walk back from the value to find the `var` keyword on
            // the same line.
            let line_text = cx.source.lines().nth(line as usize)?;
            let col_us = col as usize;
            let prefix = &line_text[..col_us.min(line_text.len())];
            if let Some(idx) = prefix.rfind("var") {
                let after_var = &prefix[idx + 3..];
                let trimmed = after_var.trim_start();
                if trimmed.starts_with(name) {
                    let col = idx + 3 + (after_var.len() - trimmed.len());
                    return Some((line, col as u32));
                }
            }
        }
    }
    // Fallback: search the whole source for the first `var name`.
    find_substring_line(cx.source, &format!("var {}", name))
}

/// Locate a `foreach (item in …)` declaration.
fn foreach_position(cx: &LintCx, item_var: &str) -> Option<(u32, u32)> {
    find_substring_line(cx.source, &format!("foreach ({} in", item_var))
}

fn find_substring_line(source: &str, needle: &str) -> Option<(u32, u32)> {
    let idx = source.find(needle)? as u32;
    let (line, col) = workflow_parser::ast::byte_to_line_col(source, idx as usize);
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
        UnusedBinding.run(&cx)
    }

    #[test]
    fn unused_local_is_hint() {
        let source = r#"workflow "W" { on E
  var unused = 42
  log("hello")
}"#;
        let diags = run_lint(source);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("unused") && d.severity == DiagnosticSeverity::Hint),
            "got: {:?}",
            diags
        );
    }

    #[test]
    fn used_local_is_ok() {
        let source = r#"workflow "W" { on E
  var used = 42
  log(used)
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }

    #[test]
    fn unused_foreach_item_is_hint() {
        let source = r#"workflow "W" { on E
  var xs = [1, 2, 3]
  foreach (item in xs) {
    log("loop")
  }
}"#;
        let diags = run_lint(source);
        assert!(
            diags.iter().any(|d| d.message.contains("foreach item")),
            "got: {:?}",
            diags
        );
    }

    #[test]
    fn global_unused_is_silent() {
        // Globals may be used by other files, so we don't flag them.
        let source = r#"var g = 42
workflow "W" { on E
  log("hello")
}"#;
        let diags = run_lint(source);
        assert!(diags.is_empty(), "got: {:?}", diags);
    }
}
