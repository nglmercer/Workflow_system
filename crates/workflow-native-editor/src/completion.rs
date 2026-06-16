//! Completion state and insertion logic.
//!
//! This module owns the completion popup's state and the algorithm for
//! turning a chosen `Completion` into a `(replace_range, replacement,
//! snippet)` triple. The editor's `insert_completion` only needs to
//! apply that triple to the buffer and record the snippet.
//!
//! The insertion builder unifies the two paths the editor used to
//! take — one for LSP-provided `text_edit` and one for the fallback
//! range — into a single function that returns the same shape.

use workflow_lsp::features::{completions_at, Completion};
use workflow_lsp::ServerState;

use super::cursor::{line_col_to_char, word_start_before, CursorPosition};
use super::snippet::{self, PendingSnippet};

/// State for the completion popup: the current candidates, whether
/// the popup is visible, and the highlighted index.
#[derive(Default)]
pub struct CompletionState {
    pub items: Vec<Completion>,
    pub visible: bool,
    pub index: usize,
}

impl CompletionState {
    /// Request completions for `(line, col)` (0-based) from `lsp`.
    /// Hides the popup if there are no candidates and clamps the
    /// highlighted index to the new candidate list.
    pub fn request(&mut self, lsp: &ServerState, uri: &str, line: usize, col: usize) {
        self.items = completions_at(lsp, uri, line, col);
        self.visible = !self.items.is_empty();
        if self.visible && self.index >= self.items.len() {
            self.index = 0;
        }
    }

    /// Mark the popup as dismissed without clearing the candidates.
    pub fn dismiss(&mut self) {
        self.visible = false;
    }

    /// Move the selection down by one, clamped to the last item.
    pub fn select_next(&mut self) {
        let max = self.items.len().saturating_sub(1);
        self.index = (self.index + 1).min(max);
    }

    /// Move the selection up by one, clamped to the first item.
    pub fn select_prev(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    /// Accept the currently selected completion, returning a copy of
    /// it. The caller is responsible for actually applying it to the
    /// buffer.
    pub fn accept(&mut self) -> Option<Completion> {
        let item = self.items.get(self.index)?.clone();
        self.visible = false;
        Some(item)
    }

    /// Apply a `KeyAction` from `super::keybindings` and return the
    /// index the user just accepted (if any).
    pub fn handle_key(&mut self, action: crate::keybindings::KeyAction) -> Option<usize> {
        use crate::keybindings::KeyAction;
        match action {
            KeyAction::PopupDown => {
                self.select_next();
                Option::None
            }
            KeyAction::PopupUp => {
                self.select_prev();
                Option::None
            }
            KeyAction::PopupAccept => self.accept().map(|_| self.index),
            KeyAction::PopupDismiss => {
                self.dismiss();
                Option::None
            }
            _ => Option::None,
        }
    }
}

/// What the editor should do with the buffer when a completion is
/// accepted.
#[derive(Debug, Clone)]
pub struct Insertion {
    /// Start of the range to replace, in char offsets.
    pub start: usize,
    /// End of the range to replace, in char offsets.
    pub end: usize,
    /// The text to insert.
    pub text: String,
    /// If the inserted text contains snippet placeholders, the parsed
    /// tab stops. `None` means a plain replacement.
    pub snippet: Option<PendingSnippet>,
}

/// Build an `Insertion` from a chosen `Completion`, given the current
/// `text` and `cursor`. The completion may provide an explicit
/// `text_edit` (LSP-supplied) or we compute the replacement range
/// ourselves.
pub fn build_insertion(item: &Completion, text: &str, cursor: CursorPosition) -> Insertion {
    // Prefer the LSP-supplied text edit when available.
    if let Some(text_edit) = &item.text_edit {
        let (sl, sc, el, ec) = text_edit.range;
        let start = line_col_to_char(text, sl as usize, sc as usize);
        let end = line_col_to_char(text, el as usize, ec as usize);
        let raw = item.insert_text.as_deref().unwrap_or(&text_edit.new_text);
        return finalize(raw, start, end);
    }

    // Fallback: replace the word (or member-access token) ending at
    // the cursor with `insert_text` (or `label` if no `insert_text`).
    let raw = item
        .insert_text
        .clone()
        .unwrap_or_else(|| item.label.clone());
    let cursor_offset = line_col_to_char(
        text,
        cursor.line.saturating_sub(1),
        cursor.col.saturating_sub(1),
    );
    let word_start = word_start_before(text, cursor_offset);
    finalize(&raw, word_start, cursor_offset)
}

/// Run snippet expansion on `raw`, producing an `Insertion` with the
/// expanded text and (if the body had stops) a `PendingSnippet`.
fn finalize(raw: &str, start: usize, end: usize) -> Insertion {
    if raw.contains(')') {
        let (expanded, stops) = snippet::expand(raw);
        let snippet = if stops.is_empty() {
            None
        } else {
            Some(PendingSnippet { stops, current: 0 })
        };
        Insertion {
            start,
            end,
            text: expanded,
            snippet,
        }
    } else {
        Insertion {
            start,
            end,
            text: raw.to_string(),
            snippet: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workflow_lsp::features::{CompletionKind, CompletionTextEdit};

    fn completion_with_text_edit(
        label: &str,
        new_text: &str,
        range: (u32, u32, u32, u32),
    ) -> Completion {
        Completion {
            label: label.to_string(),
            detail: None,
            insert_text: None,
            kind: CompletionKind::Function,
            text_edit: Some(CompletionTextEdit {
                range,
                new_text: new_text.to_string(),
            }),
        }
    }

    fn completion_with_insert_text(label: &str, insert_text: &str) -> Completion {
        Completion {
            label: label.to_string(),
            detail: None,
            insert_text: Some(insert_text.to_string()),
            kind: CompletionKind::Function,
            text_edit: None,
        }
    }

    #[test]
    fn build_insertion_uses_text_edit_when_present() {
        let text = "log()\n";
        let item = completion_with_text_edit("log", "log(${1:msg})", (0, 0, 0, 3));
        let cursor = CursorPosition::new(1, 4);
        let ins = build_insertion(&item, text, cursor);
        // Range covers "log".
        assert_eq!(ins.start, 0);
        assert_eq!(ins.end, 3);
        // Snippet was expanded.
        assert_eq!(ins.text, "log(msg)");
        assert!(ins.snippet.is_some());
    }

    #[test]
    fn build_insertion_falls_back_to_word_range() {
        let text = "lo";
        let item = completion_with_insert_text("log", "log()");
        let cursor = CursorPosition::new(1, 3);
        let ins = build_insertion(&item, text, cursor);
        assert_eq!(ins.start, 0);
        assert_eq!(ins.end, 2);
        assert_eq!(ins.text, "log()");
        // "log()" has no snippet placeholders (no `)` is the heuristic
        // for "snippet-like"; "log()" matches and expands with no
        // stops, so the snippet is None).
        assert!(ins.snippet.is_none());
    }

    #[test]
    fn build_insertion_plain_replacement_has_no_snippet() {
        let text = "lo";
        let item = completion_with_insert_text("log", "log");
        let cursor = CursorPosition::new(1, 3);
        let ins = build_insertion(&item, text, cursor);
        assert_eq!(ins.text, "log");
        assert!(ins.snippet.is_none());
    }

    #[test]
    fn build_insertion_with_member_access() {
        let text = "data.pla";
        let item = completion_with_insert_text("data.plan", "data.plan");
        // Cursor at end of `data.pla` (8 chars, col 9 = past end,
        // clamps to col 8 → char offset 7).
        let cursor = CursorPosition::new(1, 9);
        let ins = build_insertion(&item, text, cursor);
        // `.` is part of the word, so `word_start_before(8)` returns
        // 0: the whole `data.pla` is the completion's replace range.
        // This matches the original editor behavior.
        assert_eq!(ins.start, 0);
        assert_eq!(ins.end, 8);
        assert_eq!(ins.text, "data.plan");
    }

    #[test]
    fn state_request_then_navigate() {
        let mut state = CompletionState {
            items: vec![
                completion_with_insert_text("a", "a"),
                completion_with_insert_text("b", "b"),
                completion_with_insert_text("c", "c"),
            ],
            visible: true,
            ..CompletionState::default()
        };
        assert_eq!(state.index, 0);
        state.select_next();
        assert_eq!(state.index, 1);
        state.select_next();
        state.select_next();
        // Clamped to last.
        assert_eq!(state.index, 2);
        state.select_prev();
        assert_eq!(state.index, 1);
    }
}
