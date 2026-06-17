//! Completion-item construction: scope-aware symbol completion,
//! member-access completion, and the built-in keyword/function list.
//!
//! The crate-private `build_completions` returns `lsp_types::CompletionItem`
//! so the JSON-RPC handler can use the same builder; the public entry
//! point in `mod.rs` adapts the result into our own `Completion` struct.

use std::path::{Path, PathBuf};

use lsp_types::{CompletionTextEdit as LspCompletionTextEdit, Position, Range, TextEdit};

use crate::inference;
use crate::inference::Type;

use super::{Completion, CompletionKind, CompletionTextEdit};

/// The completion logic, shared with the JSON-RPC handler. We keep a private
/// duplicate here that returns `lsp_types::CompletionItem` and let
/// `into_completion` adapt the result, rather than threading the
/// crate-private type through the wire handlers.
///
/// `inference` is optional. When present, member completions are
/// type-aware: typing `email.` on a `string` shows string methods
/// (`.length`, `.toUpperCase()`, etc.) and typing `items.` on an
/// `array` shows array methods. When absent, the legacy hardcoded
/// member list is used as a fallback.
pub fn build_completions(
    inference: Option<&inference::Inference>,
    source: &str,
    position: Position,
    document_path: Option<&str>,
) -> Vec<lsp_types::CompletionItem> {
    let prefix_line = source.lines().nth(position.line as usize).unwrap_or("");
    let col = (position.character as usize).min(prefix_line.len());
    let before = &prefix_line[..col];

    // 1. Import path completion: cursor is inside a string after `from`.
    if let Some(items) = build_import_path_completions(before, position, document_path) {
        return items;
    }

    // 2. Destructure param completion: cursor is inside ({...}) or {...}
    //    after `on EVENT` or `@import NAME from`.
    if let Some(items) = build_destructure_completions(before, position, inference) {
        return items;
    }

    // 3. Event completion: cursor is after `on ` or `emit ` (or `emit("`)
    if let Some(items) = build_event_completions(before, position, inference) {
        return items;
    }

    // 4. Member completions: "foo.bar" or "foo."
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
            return build_member_completions(inference, source, position, object_name, ident_start);
        }
    }

    // 5. Identifier / keyword completion.
    let prefix = trailing_word(before);
    let prefix_start_col = col - prefix.len();

    // Check if the character before the prefix is `@` — if so, also
    // match labels that start with `@` + prefix (e.g. `@import`).
    let has_at_prefix =
        prefix_start_col > 0 && before.as_bytes().get(prefix_start_col - 1) == Some(&b'@');

    let replace_range = Range {
        start: Position {
            line: position.line,
            character: if has_at_prefix {
                prefix_start_col - 1
            } else {
                prefix_start_col
            } as u32,
        },
        end: Position {
            line: position.line,
            character: col as u32,
        },
    };
    let mut items = Vec::new();

    // Add built-in keywords (these are always available)
    for mut item in keyword_items() {
        let label = item.label.clone();
        let matches = prefix.is_empty()
            || label.starts_with(&prefix)
            || (has_at_prefix && label.starts_with(&format!("@{}", prefix)));
        if matches {
            let new_text = item
                .insert_text
                .clone()
                .unwrap_or_else(|| item.label.clone());
            item.text_edit = Some(LspCompletionTextEdit::Edit(TextEdit {
                range: replace_range,
                new_text,
            }));
            items.push(item);
        }
    }

    // Add functions from the dynamic registry (built-in + user-defined + imported)
    if let Some(inf) = inference {
        for entry in inf
            .registry
            .builtin_functions()
            .iter()
            .chain(inf.registry.user_functions().iter())
        {
            // Skip keywords we already added
            if keyword_items().iter().any(|k| k.label == entry.name) {
                continue;
            }
            let matches = prefix.is_empty() || entry.name.starts_with(&prefix);
            if matches {
                let insert_text = if entry.params.is_empty() {
                    format!("{}()$0", entry.name)
                } else {
                    format!("{}($1)$0", entry.name)
                };
                let detail = if entry.params.is_empty() {
                    workflow_i18n::tf(
                        "lsp.completion_function_detail_no_params",
                        &[("ret", &entry.return_type.label())],
                    )
                } else {
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
                    workflow_i18n::tf(
                        "lsp.completion_function_detail",
                        &[
                            ("params", &params.join(", ")),
                            ("ret", &entry.return_type.label()),
                        ],
                    )
                };
                items.push(lsp_types::CompletionItem {
                    label: entry.name.clone(),
                    kind: Some(lsp_types::CompletionItemKind::FUNCTION),
                    detail: Some(detail),
                    documentation: entry
                        .description
                        .as_ref()
                        .map(|d| lsp_types::Documentation::String(d.clone())),
                    insert_text: Some(insert_text.clone()),
                    text_edit: Some(LspCompletionTextEdit::Edit(TextEdit {
                        range: replace_range,
                        new_text: insert_text,
                    })),
                    ..Default::default()
                });
            }
        }
    }

    items
}

// ---------------------------------------------------------------------------
// Import path completion
// ---------------------------------------------------------------------------

/// When the cursor is inside a string after `from`, suggest `.flow` and
/// `.json` files from the project root directory.
fn build_import_path_completions(
    before: &str,
    position: Position,
    document_path: Option<&str>,
) -> Option<Vec<lsp_types::CompletionItem>> {
    // Detect patterns like `from "` or `from "./` (with optional @import prefix).
    let trimmed = before.trim_end();
    if !trimmed.ends_with('"') {
        return None;
    }
    // Find the `from` keyword before the opening quote.
    let without_quote = &trimmed[..trimmed.len() - 1];
    let from_offset = without_quote.rfind("from ")?;
    let after_from = &without_quote[from_offset + 5..];
    // The part between `from` and `"` must be only whitespace (no identifier).
    if !after_from.trim().is_empty() {
        return None;
    }

    // Determine the directory to scan.
    let doc_path = document_path?;
    let doc_dir = Path::new(doc_path).parent()?;
    let scan_root = resolve_project_root(doc_dir);

    // The string content after the opening quote.
    let after_quote = &before[before.rfind('"').unwrap() + 1..];
    let typed_prefix = after_quote;

    let mut items = Vec::new();
    collect_flow_files(&scan_root, doc_dir, typed_prefix, position, &mut items);
    Some(items)
}

/// Walk up from `start` looking for a project marker (`flow.toml`,
/// `Cargo.toml`, `.git`, `.hg`). Returns the first directory that
/// contains one, or falls back to `start` itself.
fn resolve_project_root(start: &Path) -> PathBuf {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("flow.toml").exists()
            || dir.join("Cargo.toml").exists()
            || dir.join(".git").exists()
            || dir.join(".hg").exists()
        {
            return dir;
        }
        if !dir.pop() {
            return start.to_path_buf();
        }
    }
}

/// Recursively collect `.flow` and `.json` files under `root`,
/// computing paths relative to `doc_dir`. Only files whose
/// relative path starts with `prefix` are included.
fn collect_flow_files(
    root: &Path,
    doc_dir: &Path,
    prefix: &str,
    position: Position,
    out: &mut Vec<lsp_types::CompletionItem>,
) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            // Skip hidden dirs and target/node_modules.
            if name.to_string_lossy().starts_with('.') || name == "target" || name == "node_modules"
            {
                continue;
            }
            collect_flow_files(&path, doc_dir, prefix, position, out);
        } else {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "flow" && ext != "json" {
                continue;
            }
            let rel = path
                .strip_prefix(doc_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            // Always use `./` prefix for relative paths.
            let display = if rel.starts_with('/') || rel.starts_with('.') {
                rel.clone()
            } else {
                format!("./{}", rel)
            };
            if !prefix.is_empty() && !display.starts_with(prefix) {
                continue;
            }
            let new_text = format!("\"{}\"", display);
            out.push(lsp_types::CompletionItem {
                label: display,
                kind: Some(lsp_types::CompletionItemKind::FILE),
                detail: Some(format!("({})", ext)),
                text_edit: Some(lsp_types::CompletionTextEdit::Edit(TextEdit {
                    range: Range {
                        start: Position {
                            line: position.line,
                            character: (position.character as usize).saturating_sub(prefix.len())
                                as u32,
                        },
                        end: Position {
                            line: position.line,
                            character: position.character,
                        },
                    },
                    new_text,
                })),
                ..Default::default()
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Destructure param completion
// ---------------------------------------------------------------------------

/// When the cursor is inside `({` or `{` after `on EVENT` or
/// `@import NAME from`, suggest the field names of the event/schema type.
fn build_destructure_completions(
    before: &str,
    position: Position,
    inference: Option<&inference::Inference>,
) -> Option<Vec<lsp_types::CompletionItem>> {
    let trimmed = before.trim_end();

    // We need to be inside a `{` that is part of a destructure pattern.
    // Find the innermost unmatched `{`.
    let open_brace_idx = find_unmatched_open_brace(trimmed)?;
    let before_brace = &trimmed[..open_brace_idx];
    let after_brace = &trimmed[open_brace_idx + 1..];

    // The text inside the braces so far — we use it to compute the
    // prefix for filtering.
    let inner_prefix = after_brace.trim_start();

    // Determine the event/schema name from the context before `{`.
    let event_name = extract_event_name(before_brace)?;
    let inf = inference?;

    // Search the scope at the current line for the event name directly,
    // rather than using lookup() which may fail if cursor is past the
    // end of the line.
    let line_idx = position.line as usize;
    let scope = inf.scope_at.get(line_idx)?;
    let binding = scope.iter().find(|b| b.name == event_name)?;

    let fields = match &binding.ty {
        Type::Object(fields) => fields,
        _ => return None,
    };

    // Only suggest fields that haven't been typed yet.
    let already: Vec<&str> = inner_prefix
        .split([',', '}', ')'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let prefix_start_col =
        (open_brace_idx + 1 + (after_brace.len() - after_brace.trim_start().len())) as u32;

    // Extract the prefix the user has typed inside the braces.
    let field_prefix = after_brace.trim_start();

    let mut items = Vec::new();
    for (name, ty) in fields {
        if already.contains(&name.as_str()) {
            continue;
        }
        // Filter by prefix: only show fields that start with what
        // the user has typed so far.
        if !field_prefix.is_empty() && !name.starts_with(field_prefix) {
            continue;
        }
        let replace_range = Range {
            start: Position {
                line: position.line,
                character: prefix_start_col,
            },
            end: Position {
                line: position.line,
                character: position.character,
            },
        };
        items.push(lsp_types::CompletionItem {
            label: name.clone(),
            kind: Some(lsp_types::CompletionItemKind::FIELD),
            detail: Some(format!(": {}", ty.label())),
            text_edit: Some(LspCompletionTextEdit::Edit(TextEdit {
                range: replace_range,
                new_text: name.clone(),
            })),
            ..Default::default()
        });
    }
    Some(items)
}

/// Find the index of the rightmost `{` in `before` that has no
/// matching `}` to its right. Returns `None` if there is no such
/// open brace (i.e. braces are balanced or none exist).
fn find_unmatched_open_brace(before: &str) -> Option<usize> {
    let bytes = before.as_bytes();
    let mut depth: i32 = 0;
    let mut last_open: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'{' => {
                depth += 1;
                last_open = Some(i);
            }
            b'}' => {
                depth -= 1;
                if depth <= 0 {
                    last_open = None;
                    depth = 0;
                }
            }
            _ => {}
        }
    }
    last_open
}

/// Given text before an open `{`, extract the event name from the
/// preceding `on EVENT` or `@import NAME from` pattern.
fn extract_event_name(before_brace: &str) -> Option<String> {
    let trimmed = before_brace.trim_end();
    // Pattern 1: `on EVENT_NAME {`
    if let Some(idx) = trimmed.rfind("on ") {
        let after_on = &trimmed[idx + 3..];
        let name: String = after_on
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        if !name.is_empty() {
            return Some(name);
        }
    }
    // Pattern 2: `@import NAME from ... {`  or  `import NAME from ... {`
    if let Some(idx) = trimmed.rfind("import ") {
        let after_import = &trimmed[idx + 7..];
        let name: String = after_import
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Event completion
// ---------------------------------------------------------------------------

/// When the cursor is after `on ` or `emit `, suggest known events.
/// Also works for `emit("` to suggest events inside the string.
fn build_event_completions(
    before: &str,
    position: Position,
    inference: Option<&inference::Inference>,
) -> Option<Vec<lsp_types::CompletionItem>> {
    let trimmed = before.trim_end();
    let inf = inference?;

    // Check for `on ` followed by a partial event name
    let after_on = if let Some(idx) = trimmed.rfind("on ") {
        let after = &trimmed[idx + 3..];
        // Make sure there's no `{` after `on` (that's destructure)
        if after.contains('{') {
            return None;
        }
        Some(after)
    } else {
        None
    };

    // Check for `emit("` or `emit("` followed by a partial event name
    let after_emit = if let Some(idx) = trimmed.rfind("emit(\"") {
        Some(&trimmed[idx + 6..])
    } else {
        trimmed.rfind("emit('").map(|idx| &trimmed[idx + 6..])
    };

    let (prefix, prefix_start_col) = if let Some(after) = after_on {
        (after.to_string(), trimmed.len() - after.len())
    } else if let Some(after) = after_emit {
        // The prefix is inside the string, so we need to adjust for the quote
        (after.to_string(), trimmed.len() - after.len())
    } else {
        return None;
    };

    let replace_range = Range {
        start: Position {
            line: position.line,
            character: prefix_start_col as u32,
        },
        end: Position {
            line: position.line,
            character: position.character,
        },
    };

    let mut items = Vec::new();

    // Add all known events from the inference
    for (name, info) in &inf.events {
        let matches = prefix.is_empty() || name.starts_with(&prefix);
        if matches {
            let detail = if info.is_external {
                format!("external event (line {})", info.line + 1)
            } else {
                format!("event (line {})", info.line + 1)
            };
            let documentation = match info.usage {
                inference::EventUsage::On => "Listened to by a workflow",
                inference::EventUsage::Emit => "Emitted by code",
                inference::EventUsage::Import => "Imported from external schema",
            };
            items.push(lsp_types::CompletionItem {
                label: name.clone(),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                detail: Some(detail),
                documentation: Some(lsp_types::Documentation::String(documentation.to_string())),
                text_edit: Some(LspCompletionTextEdit::Edit(TextEdit {
                    range: replace_range,
                    new_text: name.clone(),
                })),
                ..Default::default()
            });
        }
    }

    // Also suggest common event naming patterns as snippets
    if prefix.is_empty() || "USER_".starts_with(&prefix) {
        items.push(lsp_types::CompletionItem {
            label: "USER_REGISTERED".to_string(),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            detail: Some(workflow_i18n::t("lsp.completion_example_event")),
            text_edit: Some(LspCompletionTextEdit::Edit(TextEdit {
                range: replace_range,
                new_text: "USER_REGISTERED".to_string(),
            })),
            ..Default::default()
        });
    }
    if prefix.is_empty() || "PAYMENT_".starts_with(&prefix) {
        items.push(lsp_types::CompletionItem {
            label: "PAYMENT_RECEIVED".to_string(),
            kind: Some(lsp_types::CompletionItemKind::ENUM),
            detail: Some(workflow_i18n::t("lsp.completion_example_event")),
            text_edit: Some(LspCompletionTextEdit::Edit(TextEdit {
                range: replace_range,
                new_text: "PAYMENT_RECEIVED".to_string(),
            })),
            ..Default::default()
        });
    }

    Some(items)
}

/// Build the list of completions for a member access expression
/// (`foo.bar`, where the cursor is on or after the `.`). The result
/// is type-aware: when we can resolve the type of `object_name` from
/// the inference scope, we show only the methods and properties that
/// actually exist on that type. `object_name` is kept for
/// documentation and for future heuristics; the type-driven path
/// already does the work.
#[allow(unused_variables)]
fn build_member_completions(
    inference: Option<&inference::Inference>,
    source: &str,
    position: Position,
    object_name: &str,
    object_col: usize,
) -> Vec<lsp_types::CompletionItem> {
    // The replacement range spans the partial prefix the user has
    // already typed after the `.`. `object_col` is the column of the
    // object's first character, so the `.` sits at
    // `object_col + object_name.len()` and the prefix begins at
    // `object_col + object_name.len() + 1`. When the cursor is right
    // after the `.`, the range collapses to a zero-width insert at
    // the cursor, which is the correct shape for "no prefix yet".
    let prefix_start_col = (object_col + object_name.len() + 1) as u32;
    let cursor_col = position.character.max(prefix_start_col);
    let replace_range = Range {
        start: Position {
            line: position.line,
            character: prefix_start_col,
        },
        end: Position {
            line: position.line,
            character: cursor_col,
        },
    };

    // Resolve the type of `object_name` at the current line. The
    // position passed to `inference::lookup` is on the identifier
    // itself (any column within it works), so we use `object_col`.
    let object_type: Option<inference::Type> = inference.and_then(|inf| {
        let lookup_pos = Position {
            line: position.line,
            character: object_col as u32,
        };
        inf.lookup(source, lookup_pos).map(|b| b.ty)
    });

    // No more hardcoded `data` fast path: completions for
    // `data.<x>` come from the inference, which types the `data`
    // binding from whatever the user imported with
    // `@import data from ...` (or from `data.<x>` usages in the
    // body, when no schema is present).

    let mut items: Vec<lsp_types::CompletionItem> = Vec::new();

    // Type-aware: properties + methods from the primitive table.
    if let Some(ty) = &object_type {
        for p in inference::methods::properties_for(ty) {
            items.push(make_property_completion(&p, replace_range));
        }
        for m in inference::methods::methods_for(ty) {
            items.push(make_method_completion(&m, replace_range));
        }
    }

    // Fallback for when inference is missing or the type is `Any`:
    // expose the most common members so `foo.|` isn't empty.
    if items.is_empty() {
        items.push(make_field("length", "number", replace_range));
        items.push(make_field("name", "string", replace_range));
    }

    items
}

fn make_property_completion(
    p: &inference::methods::Property,
    replace_range: Range,
) -> lsp_types::CompletionItem {
    let mut item = lsp_types::CompletionItem {
        label: p.name.to_string(),
        kind: Some(lsp_types::CompletionItemKind::PROPERTY),
        detail: Some(format!(": {}", p.ty.label())),
        documentation: Some(lsp_types::Documentation::String(p.doc.to_string())),
        ..Default::default()
    };
    item.text_edit = Some(LspCompletionTextEdit::Edit(TextEdit {
        range: replace_range,
        new_text: p.name.to_string(),
    }));
    item
}

fn make_method_completion(
    m: &inference::methods::Method,
    replace_range: Range,
) -> lsp_types::CompletionItem {
    // Build a method-call snippet with tab stops so the user gets
    // `name($1)$0` instead of bare `name` — the LSP client fills in
    // the arguments.
    let label = m.name.to_string();
    let insert = format!("{}($1)$0", label);
    let mut item = lsp_types::CompletionItem {
        label,
        kind: Some(lsp_types::CompletionItemKind::METHOD),
        detail: Some(format!("(): {}", m.ret.label())),
        documentation: Some(lsp_types::Documentation::String(m.doc.to_string())),
        ..Default::default()
    };
    item.text_edit = Some(LspCompletionTextEdit::Edit(TextEdit {
        range: replace_range,
        new_text: insert,
    }));
    item
}

fn trailing_word(before: &str) -> String {
    let bytes = before.as_bytes();
    let mut start = bytes.len();
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    before[start..].to_string()
}

fn make_field(name: &str, ty: &str, replace_range: Range) -> lsp_types::CompletionItem {
    let mut item = lsp_types::CompletionItem {
        label: name.to_string(),
        kind: Some(lsp_types::CompletionItemKind::PROPERTY),
        detail: Some(format!(": {}", ty)),
        documentation: Some(lsp_types::Documentation::String(format!(
            "Property of type {}",
            ty
        ))),
        ..Default::default()
    };
    item.text_edit = Some(LspCompletionTextEdit::Edit(TextEdit {
        range: replace_range,
        new_text: name.to_string(),
    }));
    item
}

pub fn into_completion(item: lsp_types::CompletionItem) -> Completion {
    let kind = match item.kind {
        Some(lsp_types::CompletionItemKind::KEYWORD) => CompletionKind::Keyword,
        Some(lsp_types::CompletionItemKind::FUNCTION) => CompletionKind::Function,
        Some(lsp_types::CompletionItemKind::VARIABLE) => CompletionKind::Variable,
        Some(lsp_types::CompletionItemKind::VALUE) => CompletionKind::Value,
        Some(lsp_types::CompletionItemKind::PROPERTY) => CompletionKind::Property,
        Some(lsp_types::CompletionItemKind::FIELD) => CompletionKind::Field,
        Some(lsp_types::CompletionItemKind::FILE) => CompletionKind::File,
        _ => CompletionKind::Variable,
    };

    let text_edit = item.text_edit.map(|te| match te {
        lsp_types::CompletionTextEdit::Edit(edit) => CompletionTextEdit {
            range: (
                edit.range.start.line,
                edit.range.start.character,
                edit.range.end.line,
                edit.range.end.character,
            ),
            new_text: edit.new_text,
        },
        lsp_types::CompletionTextEdit::InsertAndReplace(ir) => CompletionTextEdit {
            range: (
                ir.insert.start.line,
                ir.insert.start.character,
                ir.insert.end.line,
                ir.insert.end.character,
            ),
            new_text: ir.new_text,
        },
    });

    Completion {
        label: item.label,
        detail: item.detail,
        insert_text: item.insert_text,
        kind,
        text_edit,
    }
}

/// Same as [`into_completion`], but if the completion is a variable we
/// already know about, we replace the LSP wire-format detail with the
/// inferred type (and a folded value, if any).
pub fn into_completion_with_type(
    item: lsp_types::CompletionItem,
    inference: Option<&inference::Inference>,
    source: &str,
    position: Position,
    format_value: impl Fn(&inference::Value) -> String,
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

/// Returns only keyword completions. Function completions are now
/// provided dynamically by the FunctionRegistry.
fn keyword_items() -> Vec<lsp_types::CompletionItem> {
    use lsp_types::CompletionItem;
    vec![
        CompletionItem {
            label: "var".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_var")),
            insert_text: Some("var ${1:name} = ${2:value}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "fn".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_fn")),
            insert_text: Some("fn ${1:name}(${2:params}) {\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "workflow".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_workflow")),
            insert_text: Some("workflow \"${1:Name}\" {\n\ton ${2:EVENT}\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "on".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_on")),
            insert_text: Some("on ${1:EVENT}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "if".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_if")),
            insert_text: Some("if (${1:cond}) {\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "else".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_else")),
            insert_text: Some("else {\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "foreach".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_foreach")),
            insert_text: Some("foreach (${1:item} in ${2:items}) {\n\t$0\n}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "return".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_return")),
            insert_text: Some("return ${1:value}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "import".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_import")),
            insert_text: Some("import ${1:name} from \"${2:path}\"".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "@import".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_at_import")),
            insert_text: Some("@import ${1:name} from \"${2:path}\"".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "from".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_from")),
            ..Default::default()
        },
        CompletionItem {
            label: "emit".to_string(),
            kind: Some(lsp_types::CompletionItemKind::KEYWORD),
            detail: Some(workflow_i18n::t("lsp.completion_detail_emit")),
            insert_text: Some("emit ${1:EVENT}".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "true".to_string(),
            kind: Some(lsp_types::CompletionItemKind::VALUE),
            detail: Some(workflow_i18n::t("lsp.completion_detail_true")),
            ..Default::default()
        },
        CompletionItem {
            label: "false".to_string(),
            kind: Some(lsp_types::CompletionItemKind::VALUE),
            detail: Some(workflow_i18n::t("lsp.completion_detail_false")),
            ..Default::default()
        },
        CompletionItem {
            label: "null".to_string(),
            kind: Some(lsp_types::CompletionItemKind::VALUE),
            detail: Some(workflow_i18n::t("lsp.completion_detail_null")),
            ..Default::default()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ServerState;

    fn completions_at(source: &str, line: u32, character: u32) -> Vec<Completion> {
        let mut state = ServerState::new();
        let uri = "file:///test.flow";
        state.update_document(uri, source);
        crate::features::completions_at(&state, uri, line as usize, character as usize)
    }

    fn labels(items: &[Completion]) -> Vec<&str> {
        let mut v: Vec<&str> = items.iter().map(|c| c.label.as_str()).collect();
        v.sort();
        v
    }

    #[test]
    fn member_completions_for_string_show_string_methods() {
        // `//@string` annotates `email` as a string. Typing `email.|`
        // should produce string methods (`.toUpperCase`, `.contains`,
        // ...) and the `.length` property — never number methods or
        // array methods. We use `email.length` (a valid member
        // access) and put the cursor on `.` between `email` and
        // `length` so completion kicks in for the member expression.
        let source = "//@string\nfn f(email) {\n  log(email.length)\n}\n";
        // 0: //@string
        // 1: fn f(email) {
        // 2:   log(email.length)
        // 3: }
        // Cursor right after the `.` at column 13 (2 spaces + `log(`
        // (4) + `email` (5) + `.` (1) = 12; column 13 is between
        // the dot and `length`).
        let items = completions_at(source, 2, 13);
        let l = labels(&items);
        // Property
        assert!(l.contains(&"length"), "missing length: {:?}", l);
        // String methods
        for m in [
            "toUpperCase",
            "toLowerCase",
            "trim",
            "contains",
            "startsWith",
            "endsWith",
        ] {
            assert!(l.contains(&m), "missing string method {}: {:?}", m, l);
        }
        // Not array methods, not number methods
        for m in ["first", "last", "toFixed"] {
            assert!(!l.contains(&m), "unexpected {} for string: {:?}", m, l);
        }
        // Property items are marked PROPERTY.
        let length = items.iter().find(|c| c.label == "length").unwrap();
        assert!(matches!(length.kind, CompletionKind::Property));
    }

    #[test]
    fn member_completions_for_array_show_array_methods() {
        // A `var items = [...]` makes `items` an Array. Typing
        // `items.|` should show array methods.
        let source = "workflow \"W\" {\n  on E\n  var items = [1, 2, 3]\n  log(items.length)\n}\n";
        // 0: workflow "W" {
        // 1:   on E
        // 2:   var items = [1, 2, 3]
        // 3:   log(items.length)
        // 4: }
        let items = completions_at(source, 3, 14);
        let l = labels(&items);
        assert!(l.contains(&"length"), "missing length: {:?}", l);
        for m in ["first", "last", "join", "contains", "reverse"] {
            assert!(l.contains(&m), "missing array method {}: {:?}", m, l);
        }
        for m in ["toUpperCase", "toFixed"] {
            assert!(!l.contains(&m), "unexpected {} for array: {:?}", m, l);
        }
    }

    #[test]
    fn member_completions_fallback_when_type_unknown() {
        // No annotation, no inference ⇒ the type is `Any`. The
        // fallback list is the legacy `length` + `name` set so
        // `foo.|` is never empty.
        let source = "workflow \"W\" {\n  on E\n  var x = data\n  log(x.length)\n}\n";
        // 0: workflow "W" {
        // 1:   on E
        // 2:   var x = data
        // 3:   log(x.length)
        // 4: }
        let items = completions_at(source, 3, 9);
        let l = labels(&items);
        assert!(l.contains(&"length"), "missing length: {:?}", l);
        assert!(l.contains(&"name"), "missing name: {:?}", l);
    }

    /// Regression: accepting a member completion must replace the
    /// partial prefix the user has already typed. Previously
    /// `build_member_completions` emitted a zero-width range at the
    /// cursor, so accepting "email" after typing `emai` inserted
    /// `email` after `emai`, yielding `emaiemail` instead of
    /// `email`. The replacement range must span from the column
    /// right after the `.` to the cursor.
    #[test]
    fn member_completion_replace_range_covers_partial_prefix() {
        let source = "//@string\nfn f(email) {\n  log(email.emai)\n}\n";
        // 0: //@string
        // 1: fn f(email) {
        // 2:   log(email.emai)
        // 3: }
        // The cursor sits right after `emai`. Counting columns on
        // line 2 (0-based): two leading spaces (0..2), `log(` (2..6),
        // `email` (6..11), `.` (11), `emai` (12..16). Cursor at 16.
        let items = completions_at(source, 2, 16);
        let length = items
            .iter()
            .find(|c| c.label == "length")
            .expect("length completion");
        let text_edit = length.text_edit.as_ref().expect("text_edit present");
        assert_eq!(
            text_edit.range,
            (2, 12, 2, 16),
            "expected replace range to cover the partial prefix 'emai'"
        );
    }

    /// When the cursor sits right after the `.` with no prefix yet,
    /// the replacement range collapses to a zero-width insert at the
    /// cursor. That's the correct shape — the completion is *inserted*
    /// rather than *replaced*, but the editor's `splice` does the
    /// right thing in both cases.
    #[test]
    fn member_completion_replace_range_empty_when_no_prefix() {
        let source = "//@string\nfn f(email) {\n  log(email.)\n}\n";
        // Line 2: `  log(email.)`. Column 12 is the position right
        // after the `.` (two spaces + `log(` + `email` + `.` = 12).
        let items = completions_at(source, 2, 12);
        let length = items
            .iter()
            .find(|c| c.label == "length")
            .expect("length completion");
        let text_edit = length.text_edit.as_ref().expect("text_edit present");
        assert_eq!(
            text_edit.range,
            (2, 12, 2, 12),
            "expected zero-width insert right after the dot"
        );
    }

    /// User-defined variables (locals, foreach items, and workflow
    /// destructure params) must never appear in the top-level
    /// identifier completion popup. The completion should only
    /// suggest built-in keywords, built-in functions, and constant
    /// values. This is the regression test for the screenshot
    /// scenario in `examples/advanced.flow`-style code: pressing
    /// `u` or `m` inside the body of a workflow that destructured
    /// `({users, meta})` was offering `users` and `meta` as
    /// completions, polluting the popup with the user's own
    /// variables.
    #[test]
    fn identifier_completion_never_suggests_user_variables() {
        // The previous form of this test used `//@external` to mark
        // `NESTED_DATA` as external. With the new import-based
        // mechanism, the event is external simply because the
        // workflow event is in `SCREAMING_SNAKE_CASE`, so the
        // annotation is no longer needed.
        let source = "workflow \"Nested Loops\" {\n  on NESTED_DATA ({users, meta})\n  l\n}\n";
        // Line 3 (0-indexed 2) is `  l`. Cursor right after `l`,
        // column 3.
        let items = completions_at(source, 2, 3);
        let l = labels(&items);
        // User variables must not appear.
        assert!(
            !l.contains(&"meta"),
            "meta (workflow destructure param) leaked into identifier completion: {:?}",
            l
        );
        assert!(
            !l.contains(&"users"),
            "users (workflow destructure param) leaked into identifier completion: {:?}",
            l
        );
        // Builtins that match the prefix `l` should still be present.
        assert!(l.contains(&"log"), "missing log builtin: {:?}", l);
        assert!(l.contains(&"len"), "missing len builtin: {:?}", l);
    }

    /// Same rule, but the prefix is a single character that
    /// *matches* a user variable. Pressing `u` should not suggest
    /// `users`; pressing `m` should not suggest `meta`. We type
    /// `us` so the prefix actually matches a builtin too — the
    /// completion list must contain `users`-related stuff only
    /// if the user variable mechanism is broken.
    #[test]
    fn single_char_prefix_does_not_match_user_variables() {
        let source = "workflow \"W\" {\n  on E\n  on NESTED_DATA ({users, meta})\n  u\n}\n";
        // Line 5 (0-indexed 4) is `  u`. Cursor right after `u`,
        // column 3.
        let items = completions_at(source, 4, 3);
        let l = labels(&items);
        assert!(
            !l.contains(&"users"),
            "users leaked with prefix 'u': {:?}",
            l
        );
        // The only `u*` items allowed are built-in functions like `upper`.
        // User variables like `users` must not appear.
        let u_items: Vec<&str> = l
            .iter()
            .filter(|s| s.starts_with("u") && **s != "u")
            .copied()
            .collect();
        let allowed_u_builtins = ["upper"];
        for item in &u_items {
            assert!(
                allowed_u_builtins.contains(item),
                "unexpected 'u*' item in completion: {:?}",
                item
            );
        }
    }

    /// An empty prefix (cursor at the start of an empty token)
    /// must still show builtins, never variables.
    #[test]
    fn empty_prefix_shows_only_builtins() {
        let source = "workflow \"W\" {\n  on E\n  var total = 1\n  \n}\n";
        // Line 4 (0-indexed 3) is `  ` (two spaces, empty token).
        // Cursor at column 3 (after the two spaces).
        let items = completions_at(source, 3, 3);
        let l = labels(&items);
        // `total` is a `var` declared earlier and is in scope — it
        // must not appear.
        assert!(
            !l.contains(&"total"),
            "user variable 'total' leaked into completion: {:?}",
            l
        );
        // The full builtin set should be present.
        for builtin in ["var", "if", "foreach", "log", "len", "true", "false"] {
            assert!(l.contains(&builtin), "missing builtin {}: {:?}", builtin, l);
        }
    }

    /// Helper: register a test file so inference can resolve @import paths.
    fn completions_at_with_file(
        source: &str,
        line: u32,
        character: u32,
        path: &str,
    ) -> Vec<Completion> {
        let mut state = ServerState::new();
        let uri = format!("file://{}", path);
        state.update_document(&uri, source);
        crate::features::completions_at(&state, &uri, line as usize, character as usize)
    }

    // -- Import keyword completion tests ---------------------------------

    #[test]
    fn import_keyword_suggested_when_typing_imp() {
        let source = "imp\n";
        let items = completions_at(source, 0, 3);
        let l = labels(&items);
        assert!(l.contains(&"import"), "missing import: {:?}", l);
    }

    #[test]
    fn at_import_keyword_suggested_when_typing_at_i() {
        // Typing `@i` should suggest `@import`.
        let source = "@i\n";
        let items = completions_at(source, 0, 2);
        let l = labels(&items);
        assert!(l.contains(&"@import"), "missing @import: {:?}", l);
    }

    #[test]
    fn emit_keyword_suggested_when_typing_em() {
        let source = "workflow \"W\" {\n  on E\n  em\n}\n";
        let items = completions_at(source, 2, 4);
        let l = labels(&items);
        assert!(l.contains(&"emit"), "missing emit: {:?}", l);
    }

    #[test]
    fn from_keyword_suggested_when_typing_fro() {
        let source = "fro\n";
        let items = completions_at(source, 0, 3);
        let l = labels(&items);
        assert!(l.contains(&"from"), "missing from: {:?}", l);
    }

    #[test]
    fn import_snippet_expands_correctly() {
        let source = "imp";
        let items = completions_at(source, 0, 3);
        let import_item = items.iter().find(|c| c.label == "import").unwrap();
        assert_eq!(
            import_item.insert_text.as_deref(),
            Some("import ${1:name} from \"${2:path}\"")
        );
    }

    #[test]
    fn at_import_snippet_expands_correctly() {
        // When typing `@im`, `trailing_word` returns `im` (since `@`
        // is not alphanumeric), so the prefix is `im` and `@import`
        // doesn't start with `im`. We can't assert on the snippet
        // body in that case — the user typed `@` separately and
        // the prefix no longer matches. So we type just `@` and
        // verify the snippet body directly.
        let source = "@";
        let items = completions_at(source, 0, 1);
        let l = labels(&items);
        assert!(l.contains(&"@import"), "missing @import for '@': {:?}", l);
        // And verify the snippet body is correct.
        let item = items.iter().find(|c| c.label == "@import").unwrap();
        assert_eq!(
            item.insert_text.as_deref(),
            Some("@import ${1:name} from \"${2:path}\"")
        );
    }

    // -- Destructure param completion tests ------------------------------

    #[test]
    fn destructure_params_suggested_for_workflow_event() {
        // Register a real file so @import resolution works.
        let path = "/tmp/test_destructure.flow";
        let source = "@import NESTED_DATA from { users: [], meta: {} }\nworkflow \"W\" {\n  on NESTED_DATA ({u\n}\n";
        let items = completions_at_with_file(source, 2, 20, path);
        let l = labels(&items);
        assert!(l.contains(&"users"), "missing users: {:?}", l);
        // `meta` should not appear because prefix `u` doesn't match
        assert!(
            !l.contains(&"meta"),
            "meta should not appear with prefix 'u': {:?}",
            l
        );
    }

    #[test]
    fn destructure_params_suggested_for_at_import() {
        let path = "/tmp/test_destructure2.flow";
        let source = "@import NESTED_DATA from { users: [], meta: {} }\nworkflow \"W\" {\n  on NESTED_DATA ({m\n}\n";
        let items = completions_at_with_file(source, 2, 20, path);
        let l = labels(&items);
        assert!(l.contains(&"meta"), "missing meta: {:?}", l);
    }

    #[test]
    fn destructure_params_empty_prefix_shows_all() {
        let path = "/tmp/test_destructure3.flow";
        let source =
            "@import DATA from { name: \"\", count: 0 }\nworkflow \"W\" {\n  on DATA ({\n}\n";
        let items = completions_at_with_file(source, 2, 15, path);
        let l = labels(&items);
        assert!(l.contains(&"name"), "missing name: {:?}", l);
        assert!(l.contains(&"count"), "missing count: {:?}", l);
    }

    #[test]
    fn destructure_params_no_completion_for_non_object() {
        // If the event type is not an object, no destructure completions.
        let source = "workflow \"W\" {\n  on E\n  log(x)\n}\n";
        let items = completions_at(source, 1, 6);
        // Should get keyword completions, not destructure params
        let l = labels(&items);
        assert!(
            !l.contains(&"name"),
            "should not suggest fields for non-object: {:?}",
            l
        );
    }
}
