//! Line-level text-edit operations.
//!
//! These are the editor commands that manipulate the buffer at
//! the granularity of "the cursor's line" or "the current
//! selection":
//!
//! - [`EditorApp::cut_selection`] / [`EditorApp::copy_selection`] /
//!   [`EditorApp::paste_clipboard`] — clipboard integration.
//! - [`EditorApp::toggle_comment_at_cursor`] — toggle `// `
//!   comment markers.
//! - [`EditorApp::duplicate_line`] / [`EditorApp::delete_line`] /
//!   [`EditorApp::move_line`] / [`EditorApp::indent_at_cursor`] —
//!   line-level structural edits.
//!
//! All operations thread through [`EditorApp::apply_text_edit`]
//! (defined in `history_ops`) so the LSP, diagnostics, and undo
//! stack stay in sync.

use eframe::egui;

use super::super::cursor::{self, CursorPosition};
use super::super::EditorApp;
use workflow_lsp::features;

impl EditorApp {
    /// Compute the byte range of the line containing the cursor
    /// (0-indexed). Returns `(start, end_inclusive_of_newline)` so
    /// callers can pick exactly the line text or the line +
    /// trailing newline.
    pub(crate) fn cursor_line_range(&self) -> (usize, usize) {
        let line_idx = self.cursor.line.saturating_sub(1);
        let lines: Vec<&str> = self.text.split('\n').collect();
        if line_idx >= lines.len() {
            return (self.text.len(), self.text.len());
        }
        let mut start = 0usize;
        for (i, l) in lines.iter().enumerate() {
            if i == line_idx {
                // The "end" of the line is the start of the next
                // line, or end of text. The line's text plus the
                // trailing newline is the standard unit.
                let end = start + l.len() + 1;
                return (start, end.min(self.text.len()));
            }
            start += l.len() + 1;
        }
        (self.text.len(), self.text.len())
    }

    /// Replace the line at the cursor with `new_line` (no trailing
    /// newline). Re-runs the LSP and updates diagnostics.
    pub(crate) fn replace_cursor_line(&mut self, ctx: &egui::Context, new_line: String) {
        let (start, end) = self.cursor_line_range();
        // The line's text is `text[start..end-1]` (minus the trailing
        // newline). If `end` is the very end of the buffer (no
        // trailing newline), we just take `text[start..end]`.
        let line_end = if end > start && self.text.as_bytes().get(end - 1) == Some(&b'\n') {
            end - 1
        } else {
            end.min(self.text.len())
        };
        let before = &self.text[..start];
        let after = &self.text[line_end..];
        let mut new_text = String::with_capacity(before.len() + new_line.len() + after.len());
        new_text.push_str(before);
        new_text.push_str(&new_line);
        new_text.push_str(after);
        self.apply_text_edit(ctx, new_text);
    }

    /// Insert `new_text` at the cursor line and re-run the LSP.
    /// Used for duplicate-line and toggle-comment.
    pub(crate) fn insert_text(&mut self, ctx: &egui::Context, new_text: String) {
        let (start, end) = self.cursor_line_range();
        let line_end = if end > start && self.text.as_bytes().get(end - 1) == Some(&b'\n') {
            end - 1
        } else {
            end.min(self.text.len())
        };
        let before = &self.text[..line_end];
        let after = &self.text[line_end..];
        let mut combined = String::with_capacity(before.len() + new_text.len() + after.len());
        combined.push_str(before);
        combined.push_str(&new_text);
        combined.push_str(after);
        self.apply_text_edit(ctx, combined);
    }

    /// Apply a whole-text edit and refresh everything that depends
    /// on the buffer (LSP, diagnostics, history, frame snapshot).
    /// Captures the *pre-edit* state and pushes it onto the undo
    /// stack as a structural edit; the new live state lives in
    /// `EditorApp` (not on the history stack).
    pub(crate) fn apply_text_edit(&mut self, ctx: &egui::Context, new_text: String) {
        let mut pre = self.snapshot(ctx);
        self.text = new_text;
        self.dirty = true;
        self.lsp.update_document(&self.uri, &self.text);
        self.diagnostics = features::diagnostics_at(&self.lsp, &self.uri);
        pre.structural = true;
        pre.last_edit_at_ms = self.now_ms(ctx);
        self.history.commit_structural(pre);
        self.frame_start = Some(self.snapshot(ctx));
    }

    /// Cut the current selection to the OS clipboard, committing
    /// the change as a structural history entry. If there is no
    /// selection, this is a no-op (a bare cut on an empty selection
    /// wouldn't have anything to cut).
    pub(crate) fn cut_selection(&mut self, ctx: &egui::Context) {
        let Some(sel) = self.selected_range else {
            return;
        };
        let (start, end) = sel.normalized();
        if start == end {
            return;
        }
        let (b_start, b_end) = cursor::char_range_to_byte_range(&self.text, start, end);
        let cut = self.text[b_start..b_end].to_string();
        let mut pre = self.snapshot(ctx);
        // The pre-state's clipboard field already records the
        // clipboard that was on the OS before this cut, captured
        // by `snapshot()`. We don't need to carry it separately.
        self.text = format!("{}{}", &self.text[..b_start], &self.text[b_end..]);
        self.dirty = true;
        self.last_clipboard = Some(cut.clone());
        self.selected_text = None;
        self.selected_range = None;
        ctx.output_mut(|o| o.copied_text = cut);
        self.lsp.update_document(&self.uri, &self.text);
        self.diagnostics = features::diagnostics_at(&self.lsp, &self.uri);
        pre.structural = true;
        pre.last_edit_at_ms = self.now_ms(ctx);
        self.history.commit_structural(pre);
        self.frame_start = Some(self.snapshot(ctx));
    }

    /// Copy the current selection to the OS clipboard. Does not
    /// touch history (copy is not an edit).
    pub(crate) fn copy_selection(&mut self, ctx: &egui::Context) {
        let Some(text) = self.selected_text.clone() else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.last_clipboard = Some(text.clone());
        ctx.output_mut(|o| o.copied_text = text);
    }

    /// Paste the OS clipboard at the cursor (replacing the current
    /// selection if any). Reads the clipboard from egui's input
    /// state — egui handles both internal and platform clipboards.
    /// Commits the change as a structural history entry.
    pub(crate) fn paste_clipboard(&mut self, ctx: &egui::Context) {
        let clipboard_text = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        });
        let clipboard_text = match clipboard_text {
            Some(t) if !t.is_empty() => t,
            // Fall back to whatever we last saw on the OS clipboard
            // (egui doesn't always emit a Paste event for
            // platform-clipboard pastes; this is best-effort).
            _ => match self.last_clipboard.clone() {
                Some(t) if !t.is_empty() => t,
                _ => return,
            },
        };
        let (start, end) = match self.selected_range {
            Some(sel) => sel.normalized(),
            None => {
                let pos =
                    cursor::line_col_to_char(&self.text, self.cursor.line - 1, self.cursor.col - 1);
                (pos, pos)
            }
        };
        let (b_start, b_end) = cursor::char_range_to_byte_range(&self.text, start, end);
        let mut pre = self.snapshot(ctx);
        self.text = format!(
            "{}{}{}",
            &self.text[..b_start],
            &clipboard_text,
            &self.text[b_end..]
        );
        self.dirty = true;
        self.selected_text = None;
        self.selected_range = None;
        self.lsp.update_document(&self.uri, &self.text);
        self.diagnostics = features::diagnostics_at(&self.lsp, &self.uri);
        pre.structural = true;
        pre.last_edit_at_ms = self.now_ms(ctx);
        self.history.commit_structural(pre);
        self.frame_start = Some(self.snapshot(ctx));
    }

    /// Toggle `// ` at the start of every line in the current
    /// selection (or the cursor's line if no selection). Indents are
    /// preserved: the comment prefix is added after leading spaces.
    pub(crate) fn toggle_comment_at_cursor(&mut self, ctx: &egui::Context) {
        let line_idx = self.cursor.line.saturating_sub(1);
        let lines: Vec<&str> = self.text.split('\n').collect();
        if line_idx >= lines.len() {
            return;
        }
        let mut all_commented = true;
        for l in &lines {
            let trimmed = l.trim_start();
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                all_commented = false;
                break;
            }
        }
        let mut new_lines: Vec<String> = Vec::with_capacity(lines.len());
        for l in &lines {
            let leading: String = l.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            let body = &l[leading.len()..];
            if all_commented {
                // Strip `// ` (with optional single trailing space).
                if let Some(rest) = body.strip_prefix("//") {
                    let stripped = rest.strip_prefix(' ').unwrap_or(rest);
                    new_lines.push(format!("{}{}", leading, stripped));
                } else {
                    new_lines.push(l.to_string());
                }
            } else {
                new_lines.push(format!("{}// {}", leading, body));
            }
        }
        let new_text = new_lines.join("\n");
        self.apply_text_edit(ctx, new_text);
    }

    /// Duplicate the cursor's line. The new line is inserted *after*
    /// the current one.
    pub(crate) fn duplicate_line(&mut self, ctx: &egui::Context) {
        let line_idx = self.cursor.line.saturating_sub(1);
        let lines: Vec<&str> = self.text.split('\n').collect();
        if line_idx >= lines.len() {
            return;
        }
        let current = lines[line_idx];
        self.insert_text(ctx, format!("\n{}", current));
        self.status = workflow_i18n::tf(
            "app.status_duplicated_line",
            &[("line", &self.cursor.line.to_string())],
        );
    }

    /// Delete the cursor's line. The line and its trailing newline
    /// are removed.
    pub(crate) fn delete_line(&mut self, ctx: &egui::Context) {
        let line_idx = self.cursor.line.saturating_sub(1);
        let lines: Vec<&str> = self.text.split('\n').collect();
        if line_idx >= lines.len() {
            return;
        }
        if line_idx == lines.len() - 1 {
            // Last line: clear it but keep the trailing newline so
            // the line count is preserved.
            self.replace_cursor_line(ctx, String::new());
        } else {
            // Non-last line: remove the line *and* its trailing
            // newline, so the next line slides up.
            let (start, end) = self.cursor_line_range();
            let new_text = format!("{}{}", &self.text[..start], &self.text[end..]);
            self.apply_text_edit(ctx, new_text);
        }
    }

    /// Move the cursor's line up or down by `delta` rows. The
    /// surrounding line is replaced with the cursor line (and vice
    /// versa). On a successful move the cursor stays on the same
    /// logical line.
    pub(crate) fn move_line(&mut self, ctx: &egui::Context, delta: i32) {
        let line_idx = self.cursor.line.saturating_sub(1) as i32;
        let lines: Vec<&str> = self.text.split('\n').collect();
        let total = lines.len() as i32;
        if total == 0 {
            return;
        }
        let target = line_idx + delta;
        if target < 0 || target >= total {
            return;
        }
        let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        let a = line_idx as usize;
        let b = target as usize;
        new_lines.swap(a, b);
        let new_text = new_lines.join("\n");
        self.apply_text_edit(ctx, new_text);
        // Move the cursor to the new line number. The column stays
        // the same.
        self.cursor = CursorPosition::new(b + 1, self.cursor.col);
    }

    /// Indent or outdent the cursor's line. A positive `delta` adds
    /// two spaces of indent; a negative `delta` strips up to two
    /// leading spaces. Outdenting only affects lines that have at
    /// least one leading space.
    pub(crate) fn indent_at_cursor(&mut self, ctx: &egui::Context, delta: i32) {
        let line_idx = self.cursor.line.saturating_sub(1);
        let lines: Vec<&str> = self.text.split('\n').collect();
        if line_idx >= lines.len() {
            return;
        }
        let line = lines[line_idx];
        let new_line = if delta > 0 {
            format!("  {}", line)
        } else {
            // Strip up to two leading spaces.
            let stripped = line
                .strip_prefix("  ")
                .or_else(|| line.strip_prefix(' '))
                .unwrap_or(line);
            stripped.to_string()
        };
        self.replace_cursor_line(ctx, new_line);
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::cursor::CursorPosition;
    use super::super::super::EditorApp;

    #[test]
    fn cursor_line_range_first_line() {
        let app = EditorApp {
            text: "a\nb\nc".to_string(),
            cursor: CursorPosition::new(1, 1),
            ..Default::default()
        };
        let (start, end) = app.cursor_line_range();
        assert_eq!(start, 0);
        // 'a' is 1 byte, plus the trailing '\n' = 2.
        assert_eq!(end, 2);
    }

    #[test]
    fn cursor_line_range_middle_line() {
        let app = EditorApp {
            text: "a\nbb\nc".to_string(),
            cursor: CursorPosition::new(2, 1),
            ..Default::default()
        };
        let (start, end) = app.cursor_line_range();
        assert_eq!(start, 2);
        assert_eq!(end, 5);
    }

    #[test]
    fn cursor_line_range_last_line_no_trailing_newline() {
        let app = EditorApp {
            text: "a\nb".to_string(),
            cursor: CursorPosition::new(2, 1),
            ..Default::default()
        };
        let (start, end) = app.cursor_line_range();
        assert_eq!((start, end), (2, 3));
    }
}
