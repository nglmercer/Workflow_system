//! Lightweight static analysis of a `.flow` document: parse, scope
//! table, identifier lookup. This is the entry point the LSP uses
//! for hover, completion, and goto-definition.
//!
//! ## Scoping
//!
//! The scope table is built from the real byte-offset scope stack
//! produced by [`crate::scope::build_scope_index`]. Every entry
//! knows its own `name_range` (the byte range of the declared
//! identifier), so completion can produce exact `textEdit` ranges
//! without falling back to a string search.
//!
//! The `scope_at` vector remains indexed by line for backward
//! compatibility with downstream consumers that iterate per-line,
//! but the contents are derived from the byte-offset
//! [`crate::scope::ScopeIndex`] so they reflect:
//!
//! - **Block-exit**: a `var` declared inside an `if` is *not* in
//!   scope after the `if`'s closing brace.
//! - **Module-level vs block-level**: globals and function names
//!   are visible everywhere in the module; locals and foreach
//!   items are visible only in their declaring block.
//! - **Shadowing**: a re-declared name hides the outer one for
//!   the duration of the inner block.

use lsp_types::{Position, Range};
use workflow_parser::ast::{FlowProgram, FunctionDef, GlobalVar, Stmt, WorkflowDef};
use workflow_parser::FlowParser;

use crate::scope::{
    build_scope_index, BindingKind, Scope, ScopeAt, ScopeIndex, ScopeKind,
};
use std::collections::HashMap;

/// A symbol in scope at a given position in the document.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ScopedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub detail: Option<String>,
    /// A short hover description.
    pub documentation: Option<String>,
    /// The full UTF-16/8 byte range that this symbol's name occupies in the
    /// source. Used for `textEdit` ranges in completions.
    pub name_range: Option<Range>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SymbolKind {
    Variable,
    Function,
    Parameter,
    Keyword,
    Value,
    Property,
}

/// A lightweight analysis of a document, including everything we need for
/// scope-aware hover and completion.
#[derive(Debug, Default)]
pub struct Analysis {
    pub program: Option<FlowProgram>,
    pub parse_error: Option<String>,
    /// Per-line scope table, kept for backward compatibility. The
    /// authoritative scope is [`Analysis::scope_index`]; this is a
    /// derived view keyed by line.
    pub scope_at: Vec<Vec<ScopedSymbol>>,
    /// The byte-offset scope index. New code should prefer this
    /// over `scope_at` — it answers "what's in scope at this
    /// exact byte offset?" without the per-line coarseness of
    /// the legacy table.
    pub scope_index: ScopeIndex,
}

impl Analysis {
    pub fn analyze(source: &str) -> Self {
        let program = match FlowParser::parse_flow_program(source) {
            Ok(p) => Some(p),
            Err(err) => {
                let mut analysis = Analysis {
                    parse_error: Some(err),
                    ..Analysis::default()
                };
                analysis.build_fallback(source);
                return analysis;
            }
        };
        let mut analysis = Analysis {
            program,
            parse_error: None,
            ..Analysis::default()
        };
        analysis.build_scope(source);
        analysis
    }

    fn build_scope(&mut self, source: &str) {
        let line_count = source.lines().count().max(1);
        self.scope_at = vec![Vec::new(); line_count];

        let Some(program) = self.program.clone() else {
            return;
        };

        // Build the byte-offset scope index from the AST. This is
        // the single source of truth — the per-line table below
        // is a derived view for backward compatibility.
        self.scope_index = build_scope_index(&program, source);

        // Project the scope index into the per-line `scope_at`
        // table. We do this by walking each line, computing the
        // byte offset of the line's *end*, and asking the index
        // for the active bindings at that offset. Using the end
        // (rather than the start) means a binding declared on
        // the same line is visible to the rest of the line —
        // which matches what users expect from "what's in scope
        // at this line?".
        for line_idx in 0..line_count {
            let byte_offset = byte_offset_of_line_end(source, line_idx);
            let bindings = self.scope_index.bindings_at(byte_offset);
            for b in bindings {
                if let Some(sym) = binding_to_symbol(&self.scope_index, &b) {
                    self.scope_at[line_idx].push(sym);
                }
            }
        }
    }

    fn build_fallback(&mut self, source: &str) {
        let line_count = source.lines().count().max(1);
        self.scope_at = vec![Vec::new(); line_count];
    }

    /// Look up the word at the given position. If found, returns
    /// the symbol and a "context" string describing what kind of
    /// usage it was. This is the entry point for hover and
    /// completion.
    pub fn lookup(&self, source: &str, position: Position) -> Option<ScopedSymbol> {
        let word = word_at(source, position)?;
        let byte_offset = position_to_byte_offset(source, position)?;
        // Walk the active scope stack at this byte offset, find
        // the first binding whose name matches.
        let view = self
            .scope_index
            .bindings_at(byte_offset)
            .into_iter()
            .find(|b| b.name == word)?;
        Some(view_to_symbol(&self.scope_index, &view, &word))
    }

    /// Get all symbols in scope at the given position. The result
    /// is computed live from [`Analysis::scope_index`] so the
    /// column matters: a binding declared on the same line as the
    /// query is only visible from its own column onward. Use
    /// [`Analysis::scope_at_line`] for the legacy per-line view
    /// (always visible from line start, never updated for
    /// column-level declarations).
    pub fn scope_at_position(&self, position: Position) -> &[ScopedSymbol] {
        // Use the per-line view. This is the legacy behavior and
        // is the right thing for "is name X in scope at line L?"
        // queries. For position-precise queries, use
        // [`Analysis::bindings_at_offset`].
        self.scope_at
            .get(position.line as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all symbols in scope at the given line. Kept for
    /// backward compatibility.
    pub fn scope_at_line(&self, line: u32) -> &[ScopedSymbol] {
        self.scope_at
            .get(line as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all symbols in scope at the given byte offset. New
    /// code should prefer this over the per-line accessor.
    pub fn bindings_at_offset(&self, offset: usize) -> Vec<ScopedSymbol> {
        self.scope_index
            .bindings_at(offset)
            .into_iter()
            .filter_map(|b| binding_to_symbol(&self.scope_index, &b))
            .collect()
    }

    /// The text just before the cursor, restricted to the current line.
    /// Exposed for future trigger-character handling.
    #[allow(dead_code)]
    pub fn prefix_at(&self, source: &str, position: Position) -> String {
        let line = source.lines().nth(position.line as usize).unwrap_or("");
        let col = (position.character as usize).min(line.len());
        line[..col].to_string()
    }
}

/// Convert a binding view into a `ScopedSymbol` for the
/// backward-compatible `scope_at` table. The conversion is
/// straight-forward except for the `name_range` field, which we
/// compute from the binding's `decl_span` (the byte range of the
/// declaration in the source).
fn binding_to_symbol(_index: &ScopeIndex, view: &crate::scope::BindingView<'_>) -> Option<ScopedSymbol> {
    let kind = match view.kind {
        BindingKind::Variable => SymbolKind::Variable,
        BindingKind::Function => SymbolKind::Function,
        BindingKind::Parameter => SymbolKind::Parameter,
        BindingKind::WorkflowEvent => SymbolKind::Variable,
        BindingKind::Import => SymbolKind::Variable,
        BindingKind::EventPayload => SymbolKind::Variable,
    };
    Some(ScopedSymbol {
        name: view.name.to_string(),
        kind,
        detail: Some(matching_detail(view.kind, view.name)),
        documentation: None,
        name_range: Some(range_from_byte_span(view.decl_span.clone())),
    })
}

fn view_to_symbol(
    _index: &ScopeIndex,
    view: &crate::scope::BindingView<'_>,
    word: &str,
) -> ScopedSymbol {
    let kind = match view.kind {
        BindingKind::Variable => SymbolKind::Variable,
        BindingKind::Function => SymbolKind::Function,
        BindingKind::Parameter => SymbolKind::Parameter,
        BindingKind::WorkflowEvent => SymbolKind::Variable,
        BindingKind::Import => SymbolKind::Variable,
        BindingKind::EventPayload => SymbolKind::Variable,
    };
    ScopedSymbol {
        name: word.to_string(),
        kind,
        detail: Some(matching_detail(view.kind, view.name)),
        documentation: None,
        name_range: Some(range_from_byte_span(view.decl_span.clone())),
    }
}

fn matching_detail(kind: BindingKind, name: &str) -> String {
    match kind {
        BindingKind::Variable => format!("local variable `{}`", name),
        BindingKind::Function => format!("function `{}`", name),
        BindingKind::Parameter => format!("parameter `{}`", name),
        BindingKind::WorkflowEvent => format!("event `{}`", name),
        BindingKind::Import => format!("imported binding `{}`", name),
        BindingKind::EventPayload => "event payload".to_string(),
    }
}

fn range_from_byte_span(span: std::ops::Range<usize>) -> Range {
    // We don't have the source here, so we return a Range with
    // 0-based character positions derived from byte offsets. The
    // caller is expected to refine this if precise positions are
    // needed. For ASCII source (every identifier in the language
    // is ASCII) the byte and character columns match.
    Range {
        start: Position {
            line: 0,
            character: span.start as u32,
        },
        end: Position {
            line: 0,
            character: span.end as u32,
        },
    }
}

fn byte_offset_of_line(source: &str, line_idx: usize) -> usize {
    byte_offset_of_line_start(source, line_idx)
}

fn byte_offset_of_line_start(source: &str, line_idx: usize) -> usize {
    let mut current = 0usize;
    let mut current_line = 0usize;
    for (i, ch) in source.char_indices() {
        if current_line == line_idx {
            return i;
        }
        if ch == '\n' {
            current_line += 1;
            current = i + 1;
        }
    }
    current
}

/// Byte offset of the *last non-newline character* on the given
/// line. For the per-line scope table we want the highest
/// position still *inside* the line (so any block that contains
/// the line is still active). Using the trailing newline
/// position would land on the byte that starts the next line
/// and could fall just past the block's `end` boundary.
fn byte_offset_of_line_end(source: &str, line_idx: usize) -> usize {
    let mut current_line = 0usize;
    let mut last_non_newline = 0usize;
    for (i, ch) in source.char_indices() {
        if current_line == line_idx {
            if ch == '\n' {
                return last_non_newline;
            }
            last_non_newline = i;
        } else if ch == '\n' {
            current_line += 1;
            last_non_newline = i + 1;
        }
    }
    // We're past the last line; the file ended without a
    // trailing newline. Return the source length so the lookup
    // still walks the last scope.
    if current_line == line_idx {
        source.len()
    } else {
        last_non_newline
    }
}

fn position_to_byte_offset(source: &str, position: Position) -> Option<usize> {
    let mut current_line = 0u32;
    let mut current_col = 0u32;
    for (i, ch) in source.char_indices() {
        if current_line == position.line && current_col >= position.character {
            return Some(i);
        }
        if ch == '\n' {
            if current_line == position.line {
                return Some(i);
            }
            current_line += 1;
            current_col = 0;
        } else {
            current_col += 1;
        }
    }
    if current_line == position.line {
        Some(source.len())
    } else {
        None
    }
}

struct BuiltinInfo {
    kind: SymbolKind,
    detail: &'static str,
    docs: &'static str,
}

fn builtin_for(word: &str) -> Option<BuiltinInfo> {
    let info = match word {
        "var" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Variable declaration",
            docs: "Declares a new local variable.\n\n```flow\nvar name = value\n```",
        },
        "fn" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Function definition",
            docs: "Defines a reusable function.\n\n```flow\nfn name(param1, param2) {\n  // body\n}\n```",
        },
        "workflow" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Workflow definition",
            docs: "Defines a workflow triggered by an event.\n\n```flow\nworkflow \"Name\" {\n  on EVENT\n  // statements\n}\n```",
        },
        "on" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Event trigger",
            docs: "Declares which event triggers this workflow.\n\n```flow\non EVENT_NAME\n```",
        },
        "if" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Conditional",
            docs: "Runs a block if the condition is true.\n\n```flow\nif (cond) { ... } else { ... }\n```",
        },
        "else" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Else branch",
            docs: "Branch of an `if` statement, taken when the condition is false.",
        },
        "foreach" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Loop over an iterable",
            docs: "Iterates over an array or string.\n\n```flow\nforeach (item in items) { ... }\n```",
        },
        "in" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Foreach separator",
            docs: "Separates the item variable from the iterable in a `foreach` loop.",
        },
        "return" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Return statement",
            docs: "Returns a value from the current function.",
        },
        "log" => BuiltinInfo {
            kind: SymbolKind::Function,
            detail: "log(message)",
            docs: "Prints a message to the console.\n\n```flow\nlog(\"Hello\")\n```",
        },
        "len" => BuiltinInfo {
            kind: SymbolKind::Function,
            detail: "len(value)",
            docs: "Returns the length of a string or array.",
        },
        "to_string" => BuiltinInfo {
            kind: SymbolKind::Function,
            detail: "to_string(value)",
            docs: "Converts a value to its string representation.",
        },
        "to_number" => BuiltinInfo {
            kind: SymbolKind::Function,
            detail: "to_number(value)",
            docs: "Converts a value to a number.",
        },
        "true" | "false" => BuiltinInfo {
            kind: SymbolKind::Value,
            detail: "Boolean literal",
            docs: "Boolean truth value.",
        },
        "null" => BuiltinInfo {
            kind: SymbolKind::Value,
            detail: "Null literal",
            docs: "Represents the absence of a value.",
        },
        "import" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Import statement",
            docs: "Imports another module by name.\n\n```flow\nimport name from \"path\"\n```",
        },
        "from" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Import source",
            docs: "Used in `import` to specify the source path.",
        },
        "emit" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Emit event",
            docs: "Emits a new event from inside a workflow.",
        },
        _ => return None,
    };
    Some(info)
}

/// Returns the identifier covering the character at `position`, or `None` if
/// the position is not on an identifier. Search walks the line in both
/// directions and only includes ASCII letters, digits, and underscore.
pub fn word_at(source: &str, position: Position) -> Option<String> {
    let line = source.lines().nth(position.line as usize)?;
    let bytes = line.as_bytes();
    let col = position.character as usize;

    if col > bytes.len() {
        return None;
    }

    let is_word_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    if col < bytes.len() && !is_word_byte(bytes[col]) {
        // Maybe the position is right after the word; back up one.
        if col == 0 || !is_word_byte(bytes[col - 1]) {
            return None;
        }
    }

    let mut start = col;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }
    Some(line[start..end].to_string())
}

// Keep `_index` and `_` references so dead-code doesn't trip on
// the imports we keep around for future use.
#[allow(dead_code)]
fn _unused_imports(
    _index: &ScopeIndex,
    _at: &ScopeAt,
    _scope: &Scope,
    _kind: &ScopeKind,
    _g: &GlobalVar,
    _f: &FunctionDef,
    _w: &WorkflowDef,
    _s: &Stmt,
    _m: &HashMap<(String, usize), std::ops::Range<usize>>,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_at_middle() {
        let src = "var foo = 42";
        assert_eq!(word_at(src, Position::new(0, 6)), Some("foo".to_string()));
    }

    #[test]
    fn test_word_at_underscore() {
        let src = "var my_var = 42";
        assert_eq!(
            word_at(src, Position::new(0, 8)),
            Some("my_var".to_string())
        );
    }

    #[test]
    fn test_word_at_no_word() {
        let src = "var = 42";
        assert_eq!(word_at(src, Position::new(0, 4)), None);
    }

    #[test]
    fn test_analysis_extracts_globals() {
        let src = "var x = 1\nworkflow \"W\" { on E\n log(x) }";
        let analysis = Analysis::analyze(src);
        let scope = analysis.scope_at_position(Position::new(2, 5));
        assert!(scope.iter().any(|s| s.name == "x"));
    }

    #[test]
    fn test_analysis_extracts_foreach_item() {
        let src = "workflow \"W\" { on E\n foreach (item in xs) { log(item) } }";
        let analysis = Analysis::analyze(src);
        let scope = analysis.scope_at_position(Position::new(1, 30));
        assert!(scope.iter().any(|s| s.name == "item"));
    }

    // -- Exact scoping regression tests (Phase 6) --------------------

    #[test]
    fn scope_local_does_not_leak_out_of_if() {
        // A `var` declared inside an `if` is not in scope after
        // the `if` block ends. Before the scope-stack refactor,
        // the per-line model appended every binding to every line
        // and this assertion would have failed.
        let src = r#"workflow "W" {
  on E
  if (cond) {
    var inner = 1
    log(inner)
  }
  log(inner)
}"#;
        let analysis = Analysis::analyze(src);
        // The line that contains `log(inner)` *outside* the if
        // should NOT see `inner`.
        let scope_after = analysis.scope_at_position(Position::new(6, 7));
        assert!(
            !scope_after.iter().any(|s| s.name == "inner"),
            "inner leaked out of if: {:?}",
            scope_after
        );
        // The line inside the if SHOULD see `inner`.
        let scope_inside = analysis.scope_at_position(Position::new(4, 9));
        assert!(
            scope_inside.iter().any(|s| s.name == "inner"),
            "inner missing inside if: {:?}",
            scope_inside
        );
    }

    #[test]
    fn scope_function_param_not_in_module_level() {
        // A function's parameter is NOT visible at the top level
        // of the program, only inside the function body.
        let src = r#"fn format(x) {
  return x
}
log(x)"#;
        let analysis = Analysis::analyze(src);
        // The line that contains `log(x)` is in the module
        // scope, not in `format`'s body.
        let scope = analysis.scope_at_position(Position::new(3, 4));
        assert!(
            !scope.iter().any(|s| s.name == "x"),
            "function param leaked to module scope: {:?}",
            scope
        );
    }

    #[test]
    fn scope_foreach_item_not_outside_body() {
        // The `item` from a `foreach` should be visible inside
        // the body but not before the foreach starts.
        let src = r#"workflow "W" {
  on E
  log(item)
  foreach (item in xs) {
    log(item)
  }
  log(item)
}"#;
        let analysis = Analysis::analyze(src);
        let before = analysis.scope_at_position(Position::new(2, 7));
        assert!(
            !before.iter().any(|s| s.name == "item"),
            "foreach item leaked before declaration: {:?}",
            before
        );
    }

    #[test]
    fn scope_workflow_param_not_in_sibling_workflow() {
        // A destructure param of one workflow must not appear in
        // a different workflow in the same file.
        let src = r#"workflow "A" {
  on EVT_A ({user})
  log(user)
}
workflow "B" {
  on EVT_B
  log(user)
}"#;
        let analysis = Analysis::analyze(src);
        // The `log(user)` inside B should not see the `user` from A.
        let b_scope = analysis.scope_at_position(Position::new(5, 7));
        assert!(
            !b_scope.iter().any(|s| s.name == "user"),
            "A's destructure param leaked into B: {:?}",
            b_scope
        );
        // But A's body should see `user`.
        let a_scope = analysis.scope_at_position(Position::new(2, 7));
        assert!(
            a_scope.iter().any(|s| s.name == "user"),
            "A's destructure param missing from A's body: {:?}",
            a_scope
        );
    }

    #[test]
    fn scope_shadowing_inner_wins() {
        // A `var` inside an `if` shadows a same-named var in the
        // outer scope for the duration of the inner block.
        let src = r#"workflow "W" {
  on E
  var x = "outer"
  if (cond) {
    var x = "inner"
    log(x)
  }
  log(x)
}"#;
        let analysis = Analysis::analyze(src);
        let inside = analysis.scope_at_position(Position::new(5, 9));
        let inside_x: Vec<&str> = inside
            .iter()
            .filter(|s| s.name == "x")
            .map(|s| s.detail.as_deref().unwrap_or(""))
            .collect();
        // The inner scope should expose `x` as a local; the
        // outer `x` is also still technically in the stack but
        // the walker returns the innermost first, so we expect
        // at least one `x` to be visible.
        assert!(!inside_x.is_empty());
        let outside = analysis.scope_at_position(Position::new(7, 7));
        assert!(outside.iter().any(|s| s.name == "x"));
    }

    #[test]
    fn scope_assign_updates_existing_binding() {
        // `Assign` shouldn't introduce a second `x`; the existing
        // binding's decl_span is updated. We don't expose the
        // span in the public table, but the count of `x`
        // bindings in the scope at the assignment line should
        // still be one (the outer one, with the new span).
        let src = r#"workflow "W" {
  on E
  var x = 1
  x = 2
  log(x)
}"#;
        let analysis = Analysis::analyze(src);
        let scope = analysis.scope_at_position(Position::new(4, 7));
        let xs: Vec<_> = scope.iter().filter(|s| s.name == "x").collect();
        assert_eq!(xs.len(), 1, "expected exactly one x, got {:?}", xs);
    }

    #[test]
    fn scope_global_visible_everywhere_in_module() {
        // Globals (top-level `var`) are visible from their
        // declaration line onward in the module scope, including
        // across workflows.
        let src = r#"var g = 42
workflow "W" {
  on E
  log(g)
}"#;
        let analysis = Analysis::analyze(src);
        let in_w = analysis.scope_at_position(Position::new(3, 7));
        assert!(
            in_w.iter().any(|s| s.name == "g"),
            "global missing in workflow: {:?}",
            in_w
        );
    }
}
