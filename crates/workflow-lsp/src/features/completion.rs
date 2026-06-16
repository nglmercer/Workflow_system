//! Completion-item construction: scope-aware symbol completion,
//! member-access completion, and the built-in keyword/function list.
//!
//! The crate-private `build_completions` returns `lsp_types::CompletionItem`
//! so the JSON-RPC handler can use the same builder; the public entry
//! point in `mod.rs` adapts the result into our own `Completion` struct.

use lsp_types::{CompletionTextEdit as LspCompletionTextEdit, Position, Range, TextEdit};

use crate::inference;

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
) -> Vec<lsp_types::CompletionItem> {
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
            return build_member_completions(inference, source, position, object_name, ident_start);
        }
    }

    let prefix = trailing_word(before);
    let prefix_start_col = col - prefix.len();
    let replace_range = Range {
        start: Position {
            line: position.line,
            character: prefix_start_col as u32,
        },
        end: Position {
            line: position.line,
            character: col as u32,
        },
    };
    let mut items = Vec::new();

    // Identifier completion only shows built-in language constructs
    // (keywords, built-in functions, and constant values). We
    // deliberately do NOT enumerate user-defined variables from
    // `analysis.scope_at_position` here: variables are shown via
    // hover and via member access (`foo.|`), but they don't belong
    // in the top-level completion popup because they would
    // dominate the list (every `var`, every foreach item, every
    // destructure binding — e.g. `users` and `meta` from
    // `on NESTED_DATA ({users, meta})`) and would suggest names
    // the user has just typed at a moment when they are usually
    // reaching for a built-in.

    for mut item in builtin_items() {
        let label = item.label.clone();
        if prefix.is_empty() || label.starts_with(&prefix) {
            // Prefer the snippet body (insert_text) over the bare label so
            // accepting "if" inside an existing block expands to a full
            // `if (...) { ... }` template with $1/$2/$0 tab stops, not just
            // the word "if". Fall back to the label for items that have no
            // snippet body.
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

    items
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
    let replace_range = Range {
        start: Position {
            line: position.line,
            character: position.character,
        },
        end: Position {
            line: position.line,
            character: position.character,
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
        items.push(make_field("length", "number"));
        items.push(make_field("name", "string"));
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

pub fn into_completion(item: lsp_types::CompletionItem) -> Completion {
    let kind = match item.kind {
        Some(lsp_types::CompletionItemKind::KEYWORD) => CompletionKind::Keyword,
        Some(lsp_types::CompletionItemKind::FUNCTION) => CompletionKind::Function,
        Some(lsp_types::CompletionItemKind::VARIABLE) => CompletionKind::Variable,
        Some(lsp_types::CompletionItemKind::VALUE) => CompletionKind::Value,
        Some(lsp_types::CompletionItemKind::PROPERTY) => CompletionKind::Property,
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
        // No builtin in the current list starts with `u`, so the
        // list is expected to be empty. The key invariant is that
        // the user variable is not surfaced.
        assert!(
            !l.iter().any(|s| s.starts_with("u") && *s != "u"),
            "unexpected 'u*' user variable in completion: {:?}",
            l
        );
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
}
