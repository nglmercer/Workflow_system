//! Lint passes for `.flow` programs.
//!
//! A lint is a small, focused check that walks the AST (and the
//! inference result) and reports diagnostics. Lints are run by
//! `features::diagnostics_at` after the parser and typechecker have
//! done their work.
//!
//! Each lint is implemented as a zero-sized type that implements the
//! [`Lint`] trait. The trait takes a shared [`LintCx`] (a bundle of
//! everything a lint might need) and returns a `Vec<Diagnostic>`.
//!
//! Adding a new lint is a three-step process:
//! 1. Add a new file under this directory with a `pub struct X;`
//!    type and an `impl Lint for X` block.
//! 2. Add a unit test in the same file.
//! 3. Register the lint in [`run_all`] below.
//!
//! Lints are run in registration order; diagnostics from earlier
//! lints appear first in the panel. Order is rarely significant for
//! correctness, but it can affect test stability.
use lsp_types::Position;
use workflow_parser::ast::FlowProgram;

use crate::analysis::Analysis;
use crate::features::{Diagnostic, DiagnosticSeverity};
use crate::inference::Inference;

mod redundant_expression;
mod unknown_identifier;
mod unused_binding;
mod unused_workflow;

/// A bundle of everything a lint pass might need. All fields are
/// shared (`&`-borrowed) and the context is rebuilt for every
/// `diagnostics_at` call.
pub struct LintCx<'a> {
    pub source: &'a str,
    pub analysis: &'a Analysis,
    pub inference: &'a Inference,
    pub program: &'a FlowProgram,
    /// Set of `(lint_name, line)` pairs that should be suppressed.
    /// The set is built by [`parse_disable_directives`] from
    /// `// flow-lint:disable=lint-a,lint-b` comments.
    pub disabled: &'a DisabledSet,
}

/// A set of `(lint_name, line)` suppression pairs. The set is queried
/// via [`LintCx::is_disabled`] to decide whether a given diagnostic
/// should be emitted.
#[derive(Debug, Default, Clone)]
pub struct DisabledSet {
    /// Map of `line -> set of lint names suppressed on that line`.
    by_line: std::collections::BTreeMap<u32, Vec<String>>,
}

impl DisabledSet {
    /// True if the given `lint_name` is suppressed on `line`. The
    /// suppression applies to the *next* non-comment line after the
    /// `// flow-lint:disable=...` directive, which the caller is
    /// expected to have resolved when populating the set.
    pub fn is_disabled(&self, lint_name: &str, line: u32) -> bool {
        self.by_line
            .get(&line)
            .map(|names| names.iter().any(|n| n == lint_name))
            .unwrap_or(false)
    }

    /// True if the `name` is a valid identifier for the disable
    /// directive (alphabetic + `-` and `_`).
    fn is_valid_name(name: &str) -> bool {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }
}

impl<'a> LintCx<'a> {
    /// Convenience: emit a diagnostic at the given `line` (0-indexed),
    /// with a default `range` and `source` matching the lint name.
    /// Returns the diagnostic so the caller can push it.
    pub fn diag(
        &self,
        lint_name: &str,
        line: u32,
        col: u32,
        message: impl Into<String>,
        severity: DiagnosticSeverity,
    ) -> Diagnostic {
        Diagnostic {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
            message: message.into(),
            severity,
            source: Some(lint_name.to_string()),
            range: None,
        }
    }
}

/// A single lint pass. Implementations are zero-sized and stateless.
pub trait Lint {
    /// Stable identifier used in the `source` field of produced
    /// diagnostics and in `// flow-lint:disable=` comments. Must be
    /// unique across the lint registry.
    fn name(&self) -> &'static str;

    /// Run the pass and return all diagnostics it produces. The
    /// returned diagnostics are merged into the editor's diagnostic
    /// list verbatim.
    fn run(&self, cx: &LintCx) -> Vec<Diagnostic>;
}

/// Run every registered lint and concatenate the results. Order is
/// stable: lints are run in the order they are listed here, so
/// test-only changes should keep this order predictable.
pub fn run_all(cx: &LintCx) -> Vec<Diagnostic> {
    let passes: [&dyn Lint; 4] = [
        &unknown_identifier::UnknownIdentifier,
        &unused_binding::UnusedBinding,
        &unused_workflow::UnusedWorkflow,
        &redundant_expression::RedundantExpression,
    ];
    let mut out = Vec::new();
    for pass in passes {
        out.extend(pass.run(cx));
    }
    out
}

/// Walk `source` and collect all `// flow-lint:disable=…` directives.
/// A directive suppresses the named lints on the *next* non-comment,
/// non-blank line. This is the same convention rustc/clippy use for
/// `#[allow(...)]` attributes.
pub fn parse_disable_directives(source: &str) -> DisabledSet {
    let mut set = DisabledSet::default();
    let mut pending: Vec<String> = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("// flow-lint:disable=") {
            // Reset pending for the new directive — the previous
            // pending was unclaimed and is discarded.
            pending = rest
                .split(',')
                .map(str::trim)
                .filter(|n| !n.is_empty() && DisabledSet::is_valid_name(n))
                .map(str::to_string)
                .collect();
        } else if !trimmed.is_empty() && !trimmed.starts_with("//") {
            // First non-blank, non-comment line after a directive.
            if !pending.is_empty() {
                for name in &pending {
                    set.by_line
                        .entry(idx as u32)
                        .or_default()
                        .push(name.clone());
                }
                pending.clear();
            }
        }
        // Blank/comment lines: keep `pending` intact.
    }
    set
}

/// Helper: try to get a `(line, col)` 0-based position for an `Expr`
/// in `source` using the parser's heuristic. Returns `None` if the
/// expression can't be located.
pub fn expr_position(source: &str, expr: &workflow_parser::ast::Expr) -> Option<(u32, u32)> {
    expr_position_nth(source, expr, 1)
}

/// Like [`expr_position`], but for `Var` nodes, uses the `n`th
/// occurrence (1-indexed) of the identifier. Other expression types
/// ignore `n`.
pub fn expr_position_nth(
    source: &str,
    expr: &workflow_parser::ast::Expr,
    n: usize,
) -> Option<(u32, u32)> {
    let span = workflow_parser::find_expr_range_nth(source, expr, n)?;
    span.to_line_col(source).map(|(sl, sc, _, _)| (sl, sc))
}

/// Helper: build a `Position` 0-based for a `(line, col)` pair.
pub fn pos(line: u32, col: u32) -> Position {
    Position {
        line,
        character: col,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_disable_directives_basic() {
        let source = "// flow-lint:disable=unknown-identifier\nvar x = 42\n";
        let set = parse_disable_directives(source);
        assert!(set.is_disabled("unknown-identifier", 1));
        assert!(!set.is_disabled("unknown-identifier", 0));
        assert!(!set.is_disabled("unused-binding", 1));
    }

    #[test]
    fn parse_disable_directives_multiple_names() {
        let source = "// flow-lint:disable=unknown-identifier,unused-binding\nvar x = 42\n";
        let set = parse_disable_directives(source);
        assert!(set.is_disabled("unknown-identifier", 1));
        assert!(set.is_disabled("unused-binding", 1));
    }

    #[test]
    fn parse_disable_directives_blank_lines_consumed() {
        // Blank lines between the directive and the next code line
        // should NOT consume the pending directive.
        let source = "// flow-lint:disable=unknown-identifier\n\n\nvar x = 42\n";
        let set = parse_disable_directives(source);
        assert!(set.is_disabled("unknown-identifier", 3));
    }
}
