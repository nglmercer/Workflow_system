//! In-process API for the Flow LSP, designed to be called directly from a
//! host editor without going through the JSON-RPC wire protocol.
//!
//! The standalone `flow-lsp` binary still speaks LSP over stdio via
//! `main.rs`, but the editor can import this module and call
//! [`completions_at`] / [`hover_at`] / [`diagnostics_at`] synchronously
//! for zero-overhead integration.
//!
//! Internally, this is split across focused submodules:
//!
//! - [`completion`] — scope-aware and member-access completions, plus
//!   the built-in keyword/function list.
//! - [`typecheck`] — argument-type-mismatch diagnostics.

use lsp_types::{Position, Range};

use crate::inference;
use crate::inference::EventUsage;
use crate::lint::{self, LintCx};
use crate::state::ServerState;

mod completion;
mod typecheck;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single completion entry, decoupled from `lsp_types::CompletionItem`
/// so the host editor can render it however it wants.
#[derive(Debug, Clone)]
pub struct Completion {
    pub label: String,
    pub detail: Option<String>,
    /// What to actually insert. If `None`, the label is used.
    pub insert_text: Option<String>,
    /// What kind of symbol this is.
    pub kind: CompletionKind,
    /// The text edit range and new text, if provided by the LSP.
    pub text_edit: Option<CompletionTextEdit>,
}

/// A text edit operation for completion.
#[derive(Debug, Clone)]
pub struct CompletionTextEdit {
    /// The range of text to replace (start line, start col, end line, end col).
    pub range: (u32, u32, u32, u32),
    /// The new text to insert.
    pub new_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Function,
    Variable,
    Value,
    Property,
    Field,
    File,
}

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A diagnostic message (error, warning, etc.) for a range of text.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Start line (0-indexed)
    pub start_line: u32,
    /// Start column (0-indexed)
    pub start_col: u32,
    /// End line (0-indexed)
    pub end_line: u32,
    /// End column (0-indexed)
    pub end_col: u32,
    /// The diagnostic message
    pub message: String,
    /// Severity level
    pub severity: DiagnosticSeverity,
    /// Optional source (e.g., "type-checker")
    pub source: Option<String>,
    /// Optional LSP `Range` (preferred over the four-uint fields when
    /// available). Set by lints that produce a precise `Span` via
    /// `workflow_parser::find_expr_range` or similar.
    pub range: Option<Range>,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Compute diagnostics for the entire document.
pub fn diagnostics_at(state: &ServerState, uri: &str) -> Vec<Diagnostic> {
    let Some(source) = state.get_document(uri) else {
        return Vec::new();
    };
    let Some(analysis) = state.get_analysis(uri) else {
        return Vec::new();
    };
    let inference = state.get_inference(uri);

    let mut diagnostics = Vec::new();

    // Check for parse errors
    if let Some(parse_error) = &analysis.parse_error {
        diagnostics.push(Diagnostic {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 0,
            message: parse_error.clone(),
            severity: DiagnosticSeverity::Error,
            source: Some("parser".to_string()),
            range: None,
        });
    }

    // Type checking diagnostics
    if let Some(inference) = inference {
        diagnostics.extend(typecheck::check_type_mismatches(source, inference));
    }

    // Lint passes — only run when the program parsed successfully.
    if let (Some(inference), Some(program)) = (inference, analysis.program.as_ref()) {
        let disabled = lint::parse_disable_directives(source);
        let cx = LintCx {
            source,
            analysis,
            inference,
            program,
            disabled: &disabled,
        };
        diagnostics.extend(lint::run_all(&cx));
    }

    diagnostics
}

/// Compute completions for the given cursor position.
pub fn completions_at(
    state: &ServerState,
    uri: &str,
    line: usize,
    character: usize,
) -> Vec<Completion> {
    let Some(source) = state.get_document(uri) else {
        return Vec::new();
    };
    let position = Position {
        line: line as u32,
        character: character as u32,
    };
    let inference = state.get_inference(uri);
    let document_path = uri.strip_prefix("file://");
    completion::build_completions(inference, source, position, document_path)
        .into_iter()
        .map(|item| {
            completion::into_completion_with_type(item, inference, source, position, format_value)
        })
        .collect()
}

/// Compute hover documentation for the given cursor position.
pub fn hover_at(state: &ServerState, uri: &str, line: usize, character: usize) -> Option<String> {
    let source = state.get_document(uri)?;
    let analysis = state.get_analysis(uri)?;
    let inference = state.get_inference(uri);
    let position = Position {
        line: line as u32,
        character: character as u32,
    };

    // First try the analysis lookup (works for local symbols)
    if let Some(symbol) = analysis.lookup(source, position) {
        let mut body = String::new();
        if let Some(detail) = &symbol.detail {
            body.push_str(detail);
            body.push_str("\n\n");
        }

        if let Some(inference) = inference {
            // Check if this is a function and show its signature
            if let Some(sig) = inference.functions.get(&symbol.name) {
                let ret_label = sig.ret.label();
                if sig.annotated {
                    body.push_str(&format!("`//@{}`\n\n", ret_label));
                } else {
                    body.push_str(&format!("**returns:** `{}`\n\n", ret_label));
                }
                // Show parameter types if available
                if !sig.param_types.is_empty() {
                    let params: Vec<String> = sig
                        .params
                        .iter()
                        .zip(sig.param_types.iter())
                        .map(|(name, ty)| format!("{}: {}", name, ty.label()))
                        .collect();
                    body.push_str(&format!("**params:** `({})`\n\n", params.join(", ")));
                }
            } else if let Some(binding) = inference.lookup(source, position) {
                if binding.annotated {
                    body.push_str(&format!("`//@{}`\n\n", binding.ty.label()));
                } else {
                    body.push_str(&format!("**type:** `{}`\n\n", binding.ty.label()));
                }
                if let Some(value) = &binding.value {
                    body.push_str(&format!("**value:** `{}`\n\n", format_value(value)));
                }
            }
        }

        if let Some(docs) = &symbol.documentation {
            body.push_str(docs);
        }
        if !body.is_empty() {
            return Some(body);
        }
    }

    // Fallback: check if the word at position is a function in the registry
    if let Some(inference) = inference {
        if let Some(word) = crate::analysis::word_at(source, position) {
            // Check local functions first
            if let Some(sig) = inference.functions.get(&word) {
                let ret_label = sig.ret.label();
                let mut body = format!("**fn** `{}`\n\n", word);
                body.push_str(&format!("**returns:** `{}`\n\n", ret_label));
                if !sig.params.is_empty() {
                    let params: Vec<String> = sig
                        .params
                        .iter()
                        .zip(sig.param_types.iter())
                        .map(|(name, ty)| format!("{}: {}", name, ty.label()))
                        .collect();
                    body.push_str(&format!("**params:** `({})`\n\n", params.join(", ")));
                }
                return Some(body);
            }
            // Check registry functions
            if let Some(entry) = inference.registry.get(&word) {
                let mut body = if entry.is_user_defined {
                    format!("**fn** `{}` (imported)\n\n", word)
                } else {
                    format!("**fn** `{}` (built-in)\n\n", word)
                };

                // Show category
                body.push_str(&format!("**category:** {}\n\n", entry.category.label()));

                // Show return type
                body.push_str(&format!("**returns:** `{}`\n\n", entry.return_type.label()));

                // Show parameters
                if !entry.params.is_empty() {
                    let params: Vec<String> = entry
                        .params
                        .iter()
                        .map(|p| {
                            if p.optional {
                                format!("{}?: {}", p.name, p.ty.label())
                            } else {
                                format!("{}: {}", p.name, p.ty.label())
                            }
                        })
                        .collect();
                    body.push_str(&format!("**params:** `({})`\n\n", params.join(", ")));
                }

                // Show description if available
                if let Some(desc) = &entry.description {
                    body.push_str(desc);
                }

                return Some(body);
            }

            // Check if the word is an event
            if let Some(event_info) = inference.events.get(&word) {
                let mut body = format!("**event** `{}`\n\n", word);

                // Show event type
                if event_info.is_external {
                    body.push_str("**type:** external (SCREAMING_SNAKE_CASE)\n\n");
                } else {
                    body.push_str("**type:** internal\n\n");
                }

                // Show usage
                let usage_desc = match event_info.usage {
                    EventUsage::On => "Listened to by a workflow",
                    EventUsage::Emit => "Emitted by code",
                    EventUsage::Import => "Imported from external schema",
                };
                body.push_str(&format!("**usage:** {}\n\n", usage_desc));

                // Show line number
                body.push_str(&format!("**defined at:** line {}\n", event_info.line + 1));

                return Some(body);
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn format_value(v: &inference::Value) -> String {
    match v {
        inference::Value::String(s) => format!("\"{}\"", s),
        inference::Value::Number(n) => {
            if n.fract() == 0.0 {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        }
        inference::Value::Bool(b) => format!("{}", b),
        inference::Value::Null => "null".to_string(),
        inference::Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(format_value).collect();
            format!("[{}]", parts.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ServerState;

    #[test]
    fn diagnostics_detect_type_mismatch() {
        let source = r#"fn double(x) {
  return x * 2
}

var message = "hello"
var result = double(message)"#;

        let mut state = ServerState::new();
        let uri = "file:///test.flow";
        state.update_document(uri, source);

        let diagnostics = diagnostics_at(&state, uri);

        // Should have a type mismatch warning for double(message)
        assert!(
            !diagnostics.is_empty(),
            "Expected at least one diagnostic, got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("Type mismatch")
                    && d.message.contains("number")
                    && d.message.contains("string")),
            "Expected type mismatch diagnostic, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn diagnostics_no_error_for_correct_types() {
        let source = r#"fn double(x) {
  return x * 2
}

var num = 42
var result = double(num)"#;

        let mut state = ServerState::new();
        let uri = "file:///test.flow";
        state.update_document(uri, source);

        let diagnostics = diagnostics_at(&state, uri);

        // Should have no type mismatch warnings
        assert!(diagnostics
            .iter()
            .all(|d| !d.message.contains("Type mismatch")));
    }

    /// End-to-end regression test: `examples/advanced.flow` exercises
    /// every feature the LSP needs to handle (function params,
    /// workflow destructure params, nested foreach, the `//@T,T`
    /// per-parameter shortcut, and SCREAMING_SNAKE_CASE event names
    /// that should be treated as external). Before the fix, this
    /// file produced five "Unknown identifier" errors and five
    /// "listens for X but no emit was found" hints. After the fix,
    /// the diagnostics list is empty.
    #[test]
    fn examples_advanced_flow_lints_clean() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/advanced.flow");
        let source =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {:?}: {}", path, e));

        let mut state = ServerState::new();
        let uri = format!("file://{}", path.to_string_lossy());
        state.update_document(&uri, &source);

        let diagnostics = diagnostics_at(&state, &uri);
        if !diagnostics.is_empty() {
            let formatted: Vec<String> = diagnostics
                .iter()
                .map(|d| {
                    format!(
                        "{} Ln {}, Col {}: {}",
                        match d.severity {
                            DiagnosticSeverity::Error => "error",
                            DiagnosticSeverity::Warning => "warning",
                            DiagnosticSeverity::Info => "info",
                            DiagnosticSeverity::Hint => "hint",
                        },
                        d.start_line + 1,
                        d.start_col + 1,
                        d.message
                    )
                })
                .collect();
            panic!(
                "expected zero diagnostics on examples/advanced.flow, got:\n{}",
                formatted.join("\n")
            );
        }
    }
}
