//! In-process API for the Flow LSP, designed to be called directly from a
//! host editor without going through the JSON-RPC wire protocol.
//!
//! The standalone `flow-lsp` binary still speaks LSP over stdio via
//! `main.rs`, but the editor can import this module and call
//! [`completions_at`] / [`hover_at`] synchronously for zero-overhead
//! integration.

use lsp_types::Position;

use crate::analysis::Analysis;
use crate::inference;
use crate::state::ServerState;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Function,
    Variable,
    Value,
    Property,
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
    let Some(analysis) = state.get_analysis(uri) else {
        return Vec::new();
    };
    let position = Position {
        line: line as u32,
        character: character as u32,
    };
    let inference = state.get_inference(uri);
    build_completions(analysis, source, position)
        .into_iter()
        .map(|item| into_completion_with_type(item, inference, source, position))
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
    let symbol = analysis.lookup(source, position)?;

    // Combine the existing scope/symbol docs with the type/value inference
    // result so the user sees both "what kind of thing is this?" and
    // "what's its type and current value?".
    let mut body = String::new();
    if let Some(detail) = &symbol.detail {
        body.push_str(detail);
        body.push_str("\n\n");
    }

    if let Some(inference) = inference {
        if let Some(binding) = inference.lookup(source, position) {
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
    if body.is_empty() {
        body = symbol.name.clone();
    }
    Some(body)
}

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

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn into_completion(item: lsp_types::CompletionItem) -> Completion {
    let kind = match item.kind {
        Some(lsp_types::CompletionItemKind::KEYWORD) => CompletionKind::Keyword,
        Some(lsp_types::CompletionItemKind::FUNCTION) => CompletionKind::Function,
        Some(lsp_types::CompletionItemKind::VARIABLE) => CompletionKind::Variable,
        Some(lsp_types::CompletionItemKind::VALUE) => CompletionKind::Value,
        Some(lsp_types::CompletionItemKind::PROPERTY) => CompletionKind::Property,
        _ => CompletionKind::Variable,
    };
    Completion {
        label: item.label,
        detail: item.detail,
        insert_text: item.insert_text,
        kind,
    }
}

/// Same as [`into_completion`], but if the completion is a variable we
/// already know about, we replace the LSP wire-format detail with the
/// inferred type (and a folded value, if any).
fn into_completion_with_type(
    item: lsp_types::CompletionItem,
    inference: Option<&inference::Inference>,
    source: &str,
    position: Position,
) -> Completion {
    let mut completion = into_completion(item);
    if let (CompletionKind::Variable, Some(inference)) = (completion.kind, inference) {
        // Look the label up as if it were a word at the current position
        // — for a `Var` completion, the label is the variable name.
        let word_position = Position {
            line: position.line,
            character: position
                .character
                .saturating_sub(completion.label.chars().count() as u32),
        };
        if let Some(binding) = inference.lookup(source, word_position) {
            if binding.name == completion.label {
                let mut detail = binding.ty.label();
                if let Some(value) = &binding.value {
                    detail.push_str(&format!(" = {}", format_value(value)));
                }
                completion.detail = Some(detail);
            }
        }
    }
    completion
}

/// The completion logic, shared with the JSON-RPC handler. We keep a private
/// duplicate here that returns `lsp_types::CompletionItem` and let
/// `into_completion` adapt the result, rather than threading the
/// crate-private type through the wire handlers.
fn build_completions(
    analysis: &Analysis,
    source: &str,
    position: Position,
) -> Vec<lsp_types::CompletionItem> {
    use lsp_types::{CompletionTextEdit, Position as P, Range, TextEdit};

    let prefix_line = source.lines().nth(position.line as usize).unwrap_or("");
    let col = (position.character as usize).min(prefix_line.len());
    let before = &prefix_line[..col];

    // Detect "foo.bar" or "foo." for member completions.
    if let Some(dot_idx) = before.rfind('.') {
        let object_text = &before[..dot_idx];
        let ident_start = object_text
            .as_bytes()
            .iter()
            .rposition(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
            .map(|i| i + 1)
            .unwrap_or(0);
        let object_name = &object_text[ident_start..];
        if !object_name.is_empty() {
            return build_member_completions(object_name);
        }
    }

    let prefix = trailing_word(before);
    let prefix_start_col = col - prefix.len();
    let replace_range = Range {
        start: P {
            line: position.line,
            character: prefix_start_col as u32,
        },
        end: P {
            line: position.line,
            character: col as u32,
        },
    };
    let mut items = Vec::new();

    for sym in analysis.scope_at_position(position) {
        if prefix.is_empty() || sym.name.starts_with(&prefix) {
            items.push(symbol_to_completion(sym, replace_range));
        }
    }

    for mut item in builtin_items() {
        let label = item.label.clone();
        if prefix.is_empty() || label.starts_with(&prefix) {
            item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                range: replace_range,
                new_text: item.label.clone(),
            }));
            items.push(item);
        }
    }

    items
}

fn build_member_completions(object_name: &str) -> Vec<lsp_types::CompletionItem> {
    if object_name == "data" {
        return vec![make_field("plan", "string"), make_field("items", "array")];
    }
    vec![make_field("length", "number"), make_field("name", "string")]
}

fn trailing_word(before: &str) -> String {
    let bytes = before.as_bytes();
    let mut start = bytes.len();
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    before[start..].to_string()
}

fn symbol_to_completion(
    sym: &crate::analysis::ScopedSymbol,
    replace_range: lsp_types::Range,
) -> lsp_types::CompletionItem {
    let kind = match sym.kind {
        crate::analysis::SymbolKind::Variable => lsp_types::CompletionItemKind::VARIABLE,
        crate::analysis::SymbolKind::Function => lsp_types::CompletionItemKind::FUNCTION,
        crate::analysis::SymbolKind::Parameter => lsp_types::CompletionItemKind::VARIABLE,
        crate::analysis::SymbolKind::Keyword => lsp_types::CompletionItemKind::KEYWORD,
        crate::analysis::SymbolKind::Value => lsp_types::CompletionItemKind::VALUE,
        crate::analysis::SymbolKind::Property => lsp_types::CompletionItemKind::PROPERTY,
    };
    let mut item = lsp_types::CompletionItem {
        label: sym.name.clone(),
        kind: Some(kind),
        detail: sym.detail.clone(),
        documentation: sym
            .documentation
            .clone()
            .map(lsp_types::Documentation::String),
        ..Default::default()
    };
    item.text_edit = Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
        range: replace_range,
        new_text: sym.name.clone(),
    }));
    item
}

fn make_field(name: &str, ty: &str) -> lsp_types::CompletionItem {
    lsp_types::CompletionItem {
        label: name.to_string(),
        kind: Some(lsp_types::CompletionItemKind::PROPERTY),
        detail: Some(format!(": {}", ty)),
        documentation: Some(lsp_types::Documentation::String(format!(
            "Property of type {}",
            ty
        ))),
        ..Default::default()
    }
}

fn builtin_items() -> Vec<lsp_types::CompletionItem> {
    use lsp_types::CompletionItem;
    vec![
        CompletionItem {
            label: "var".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some("Variable declaration".to_string()),
            insert_text: Some("var ${1:name} = ${2:value}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "fn".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some("Function definition".to_string()),
            insert_text: Some("fn ${1:name}(${2:params}) {\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "workflow".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some("Workflow definition".to_string()),
            insert_text: Some("workflow \"${1:Name}\" {\n\ton ${2:EVENT}\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "on".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some("Event trigger".to_string()),
            insert_text: Some("on ${1:EVENT}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "if".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some("Conditional".to_string()),
            insert_text: Some("if (${1:cond}) {\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "else".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some("Else branch".to_string()),
            insert_text: Some("else {\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "foreach".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some("Loop".to_string()),
            insert_text: Some("foreach (${1:item} in ${2:items}) {\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "return".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some("Return statement".to_string()),
            insert_text: Some("return ${1:value}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "log".to_string(),
            kind: Some(lsp_types::CompletionItemKind::FUNCTION),
            detail: Some("Log a message".to_string()),
            insert_text: Some("log(\"${1:message}\")".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "len".to_string(),
            kind: Some(lsp_types::CompletionItemKind::FUNCTION),
            detail: Some("Length".to_string()),
            insert_text: Some("len(${1:value})".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "to_string".to_string(),
            kind: Some(lsp_types::CompletionItemKind::FUNCTION),
            detail: Some("Convert to string".to_string()),
            insert_text: Some("to_string(${1:value})".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "to_number".to_string(),
            kind: Some(lsp_types::CompletionItemKind::FUNCTION),
            detail: Some("Convert to number".to_string()),
            insert_text: Some("to_number(${1:value})".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "true".to_string(),
            kind: Some(lsp_types::CompletionItemKind::VALUE),
            detail: Some("Boolean true".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "false".to_string(),
            kind: Some(lsp_types::CompletionItemKind::VALUE),
            detail: Some("Boolean false".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "null".to_string(),
            kind: Some(lsp_types::CompletionItemKind::VALUE),
            detail: Some("Null value".to_string()),
            ..Default::default()
        },
    ]
}
