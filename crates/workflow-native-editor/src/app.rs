//! Top-level `EditorApp` for the native editor.
//!
//! This module glues together the smaller focused modules:
//! - [`super::cursor`] — line/column ↔ char/byte conversions and
//!   galley hit-testing
//! - [`super::snippet`] — LSP-style snippet expansion and tab stops
//! - [`super::keybindings`] — global key handling for the completion
//!   popup and snippet navigation
//! - [`super::layouter`] — the custom syntax-highlighting layouter
//! - [`super::popup`] — completion and hover popup rendering
//! - [`super::highlight`] — per-line tokenization for syntax
//!   highlighting
//! - [`super::gutter`] — line-number and fold-chevron gutter
//! - [`super::diagnostics_panel`] — bottom panel for LSP diagnostics
//! - [`super::completion`] — completion state and insertion builder
//! - [`super::folding`] — code-fold detection and application
//! - [`super::history`] — snapshot-based undo/redo

use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
    FontId, Pos2, Rect, RichText, ScrollArea, TextEdit, Ui, Vec2,
};
use std::collections::BTreeSet;
use std::sync::Arc;
use workflow_lsp::features::{self, Diagnostic};
use workflow_lsp::ServerState;

use super::completion::{self, CompletionState};
use super::cursor::{self, column_at_x, cursor_screen_pos, row_at_y, CursorPosition};
use super::diagnostics_panel;
use super::folding;
use super::gutter;
use super::history::{History, Snapshot};
use super::keybindings::{self, Command, Keymap};
use super::layouter::{layout_flow, FONT_SIZE, LINE_HEIGHT};
use super::popup;
use super::shortcuts_window;
use super::snippet::PendingSnippet;

pub struct EditorApp {
    text: String,
    lsp: ServerState,
    uri: String,
    completion: CompletionState,
    status: String,
    cursor: CursorPosition,
    hover_text: Option<String>,
    hover_pos: Option<Pos2>,
    /// Active LSP snippet, if any. The user accepts a snippet-style
    /// completion and then tabs through the stops. When `stops` is
    /// empty or `current` reaches the end, the snippet is done.
    pending_snippet: Option<PendingSnippet>,
    /// Char offset of where the snippet starts in the document. We
    /// re-derive the cursor position for each stop relative to this
    /// anchor.
    snippet_anchor: usize,
    history: History,
    /// Snapshot of the editor state at the start of the current
    /// frame. If the user typed this frame, we push this to the undo
    /// stack.
    frame_start: Option<Snapshot>,
    /// Screen position of the cursor, used to position the completion
    /// popup.
    cursor_screen_pos: Option<Pos2>,
    /// Diagnostics (errors, warnings) for the current document.
    diagnostics: Vec<Diagnostic>,
    /// Start-line of every collapsed fold region. Stable across edits
    /// as long as the relative position of the block's opening line
    /// doesn't change.
    collapsed: BTreeSet<usize>,
    /// Key bindings: maps chord sequences to commands.
    keymap: Keymap,
    /// Whether the keyboard-shortcuts help window is visible. The
    /// `F1` key (mapped to `Command::ShowShortcuts`) and the toolbar
    /// button flip this; `Esc` closes it.
    shortcuts_open: bool,
}

const EXAMPLE_PROGRAM: &str = r#"workflow "Native Example" {
  on START
  var message = "Hello from native editor"
  log(message)
  if (data.plan == "premium") {
    log("Premium user")
  } else {
    log("Free user")
  }
  foreach (item in data.items) {
    log(item.name)
  }
}

fn double(x) {
  return x * 2
}"#;

impl Default for EditorApp {
    fn default() -> Self {
        let text = EXAMPLE_PROGRAM.to_string();
        let mut lsp = ServerState::new();
        let uri = "file:///example.flow".to_string();
        lsp.update_document(&uri, &text);

        Self {
            text,
            lsp,
            uri,
            completion: CompletionState::default(),
            status: "Ready".to_string(),
            cursor: CursorPosition::new(1, 1),
            hover_text: None,
            hover_pos: None,
            pending_snippet: None,
            snippet_anchor: 0,
            history: History::new(),
            frame_start: None,
            cursor_screen_pos: None,
            diagnostics: Vec::new(),
            collapsed: BTreeSet::new(),
            keymap: Keymap::new(),
            shortcuts_open: false,
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_global_keys(ctx);

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Flow Native Editor").font(FontId::proportional(16.0)));
                ui.separator();
                ui.label(format!("Ln {}, Col {}", self.cursor.line, self.cursor.col));
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    if ui.button("Clear").clicked() {
                        self.history
                            .snapshot(self.snapshot(ctx))
                            .commit_structural();
                        self.text.clear();
                        self.lsp.update_document(&self.uri, &self.text);
                        self.frame_start = Some(self.snapshot(ctx));
                    }
                    if ui.button("Shortcuts (F1)").clicked() {
                        self.shortcuts_open = !self.shortcuts_open;
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_editor(ctx, ui);
        });

        diagnostics_panel::show(ctx, &self.diagnostics);

        if self.completion.visible && !self.completion.items.is_empty() {
            if let Some(idx) = popup::show_completion(
                ctx,
                &self.completion.items,
                self.completion.index,
                self.cursor_screen_pos,
            ) {
                self.insert_completion(ctx, idx);
            }
        }

        if let (Some(text), Some(pos)) = (self.hover_text.clone(), self.hover_pos) {
            popup::show_hover(ctx, pos, &text);
        }

        shortcuts_window::show(ctx, &mut self.shortcuts_open, &self.keymap);
    }
}

impl EditorApp {
    fn render_editor(&mut self, ctx: &egui::Context, ui: &mut Ui) {
        // Save the *pre-edit* state once per frame so we can push it
        // to the undo stack if the user typed.
        if self.frame_start.is_none() {
            self.frame_start = Some(self.snapshot(ctx));
        }
        // Prune any collapsed-fold id that no longer refers to a
        // real region.
        gutter::prune_stale(&mut self.collapsed, &self.text);
        let regions = folding::detect_folds(&self.text);
        let regions_for_gutter = regions.clone();

        // Build the text the TextEdit will actually display: collapsed
        // folds have their body replaced with a placeholder.
        let pre_display = folding::apply_folds(&self.text, &self.collapsed);
        let mut display_text = pre_display.clone();

        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                // Top-align the gutter and the text edit so their first
                // text rows share the same y coordinate and line numbers
                // stay perfectly aligned with the code.
                ui.horizontal_top(|ui| {
                    let line_count = display_text.split('\n').count().max(1);
                    let gutter_width = gutter::width_for_line_count(line_count);
                    let content_height = line_count as f32 * LINE_HEIGHT;

                    let (gutter_rect, _gutter_response) = ui.allocate_exact_size(
                        Vec2::new(gutter_width, content_height),
                        egui::Sense::hover(),
                    );

                    let output = TextEdit::multiline(&mut display_text)
                        .font(FontId::monospace(FONT_SIZE))
                        .desired_width(f32::INFINITY)
                        .layouter(&mut |ui, t, wrap_width| layout_flow(ui, t, wrap_width))
                        .show(ui);

                    // The TextEdit content starts below its inner margin;
                    // offset gutter numbers by the same amount so they stay
                    // on the same baseline as the text rows.
                    let text_top_offset = output.response.rect.min.y - gutter_rect.min.y;
                    let galley = output.galley.clone();
                    gutter::paint(
                        ui,
                        gutter_rect,
                        &output.galley,
                        &regions_for_gutter,
                        text_top_offset,
                        &mut self.collapsed,
                    );

                    // Capture the *previous* state for the undo history
                    // before the TextEdit mutates it.
                    let pre_edit = self.frame_start.take();
                    let post_edit = self.snapshot(ctx);

                    // Apply the snippet cursor before we move any
                    // fields out of `output`.
                    self.apply_snippet_cursor(&output);

                    let response = output.response;

                    if response.changed() {
                        // Splice the visible edits back into the
                        // source.
                        let new_source = folding::sync_edits(
                            &self.text,
                            &pre_display,
                            &display_text,
                            &self.collapsed,
                        );
                        self.text = new_source;

                        if let Some(prev) = pre_edit {
                            let mut snap = prev;
                            snap.last_edit_at_ms = self.now_ms(ctx);
                            self.history.snapshot(snap).commit_typing();
                        }
                        self.lsp.update_document(&self.uri, &self.text);
                        self.diagnostics = features::diagnostics_at(&self.lsp, &self.uri);
                        // The user typed something: the snippet's
                        // selection is no longer meaningful, so drop
                        // it.
                        self.pending_snippet = None;
                    }

                    if let Some(range) = &output.cursor_range {
                        let primary = range.primary;
                        let line = primary.rcursor.row + 1;
                        let col = primary.rcursor.column + 1;
                        if line != self.cursor.line || col != self.cursor.col {
                            self.cursor = CursorPosition::new(line, col);
                        }
                        if primary.rcursor.row < galley.rows.len() {
                            self.cursor_screen_pos = Some(cursor_screen_pos(
                                &galley,
                                response.rect,
                                primary.rcursor.row,
                                primary.rcursor.column,
                            ));
                        }
                    }

                    if response.changed()
                        && keybindings::should_request_completion(ui, &self.text, self.cursor)
                    {
                        self.completion.request(
                            &self.lsp,
                            &self.uri,
                            self.cursor.line - 1,
                            self.cursor.col - 1,
                        );
                    } else if response.changed() {
                        self.completion.dismiss();
                    }

                    self.update_hover(response.rect, &galley, response.hover_pos());

                    if !response.changed() {
                        self.frame_start = Some(post_edit);
                    }
                });
            });
    }

    /// Run the global key handlers, then apply the result to editor
    /// state.
    fn handle_global_keys(&mut self, ctx: &egui::Context) {
        // When the shortcuts window is open, swallow `Esc` so it
        // closes the window instead of cancelling a snippet or
        // dismissing the completion popup.
        if self.shortcuts_open && shortcuts_window::esc_pressed(ctx) {
            self.shortcuts_open = false;
            // Drain the Esc event so the keymap below doesn't see it.
            ctx.input_mut(|i| {
                let _ = i.count_and_consume_key(egui::Modifiers::default(), egui::Key::Escape);
            });
            return;
        }
        let popup_open = self.completion.visible && !self.completion.items.is_empty();
        let snippet_active = self.pending_snippet.is_some();
        let command = self.keymap.take_command(ctx, popup_open, snippet_active);
        if self.keymap.took_prefix() {
            ctx.request_repaint();
        }
        // Surface the pending chord in the status bar so the user
        // knows the keymap is waiting for a second key.
        if let Some(pending) = self.keymap.pending() {
            self.status = format!("…{}", pending_chord_label(pending));
        } else if matches!(command, Command::None) {
            // Don't clobber an existing status message just because
            // the user hit an unrelated key.
        }
        // Special-case the popup/snippet commands that only fire
        // when their gate is open. The keymap only emits these
        // commands when the right gate is set (popup_open /
        // snippet_active) thanks to the `take_command` semantics, so
        // we don't double-check here.
        match command {
            Command::None => {}
            Command::PopupUp
            | Command::PopupDown
            | Command::PopupAccept
            | Command::PopupDismiss => {
                if let Some(idx) = self.completion.handle_key(command) {
                    self.insert_completion(ctx, idx);
                }
            }
            Command::SnippetAdvance => {
                if let Some(snippet) = self.pending_snippet.as_mut() {
                    if snippet.advance() {
                        self.pending_snippet = None;
                    }
                }
            }
            Command::SnippetCancel => {
                self.pending_snippet = None;
            }
            Command::Undo => self.undo(ctx),
            Command::Redo => self.redo(ctx),
            Command::Save => {
                // No file system integration yet; just announce it
                // in the status bar so the user sees the binding
                // worked. A future phase can wire this to disk I/O.
                self.status = "Saved (no file backing yet)".to_string();
            }
            Command::Find => {
                self.status = "Find: not implemented yet (Ctrl+F)".to_string();
            }
            Command::GotoLine => {
                self.status = "Go to line: not implemented yet (Ctrl+G)".to_string();
            }
            Command::ShowShortcuts => {
                self.shortcuts_open = !self.shortcuts_open;
            }
            Command::ToggleComment => self.toggle_comment_at_cursor(ctx),
            Command::DuplicateLine => self.duplicate_line(ctx),
            Command::DeleteLine => self.delete_line(ctx),
            Command::MoveLineUp => self.move_line(ctx, -1),
            Command::MoveLineDown => self.move_line(ctx, 1),
            Command::Indent => self.indent_at_cursor(ctx, 1),
            Command::Outdent => self.indent_at_cursor(ctx, -1),
            Command::ToggleFoldAtCursor => self.toggle_fold_at_cursor(),
            Command::UnfoldAll => self.collapsed.clear(),
        }
    }

    fn undo(&mut self, ctx: &egui::Context) {
        if let Some(snap) = self.history.undo() {
            self.apply_snapshot(ctx, snap);
        }
    }

    fn redo(&mut self, ctx: &egui::Context) {
        if let Some(snap) = self.history.redo() {
            self.apply_snapshot(ctx, snap);
        }
    }

    fn apply_snapshot(&mut self, ctx: &egui::Context, snap: Snapshot) {
        self.text = snap.text;
        self.cursor = snap.cursor;
        self.pending_snippet = snap.pending_snippet;
        // Re-derive the snippet anchor from the *current* text: the
        // snippet's stop ranges are relative to its start, so we find
        // the first stop's text in the buffer and pin the anchor
        // there. If the snippet has no stops or we can't find the
        // anchor text, fall back to 0.
        self.snippet_anchor = self.locate_snippet_anchor();
        self.lsp.update_document(&self.uri, &self.text);
        // The frame_start was captured before this undo/redo;
        // invalidate it so the next render captures a fresh
        // pre-edit snapshot.
        self.frame_start = None;
        self.completion.dismiss();
        let _ = ctx;
    }

    /// Find the char offset of the active snippet's anchor. We do a
    /// best-effort search: take the first non-empty stop's default
    /// text and locate it in the buffer starting from offset 0.
    fn locate_snippet_anchor(&self) -> usize {
        let Some(snippet) = self.pending_snippet.as_ref() else {
            return 0;
        };
        let needle = match snippet.stops.first() {
            Some(stop) if !stop.default.is_empty() => &stop.default,
            _ => return 0,
        };
        self.text
            .find(needle)
            .map(|byte_off| self.text[..byte_off].chars().count())
            .unwrap_or(0)
    }

    /// Wall-clock time in milliseconds since startup. Used as the
    /// undo-coalescing key.
    fn now_ms(&self, ctx: &egui::Context) -> u128 {
        ctx.input(|i| (i.time * 1000.0) as u128)
    }

    /// Build a snapshot using the current real time.
    fn snapshot(&self, ctx: &egui::Context) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            pending_snippet: self.pending_snippet.clone(),
            last_edit_at_ms: self.now_ms(ctx),
        }
    }

    /// Force the TextEdit's cursor to the active snippet stop after
    /// the galley has been laid out for the frame. The `output.state`
    /// is then stored so the change persists into the next frame.
    fn apply_snippet_cursor(&mut self, output: &egui::widgets::text_edit::TextEditOutput) {
        let Some(snippet) = self.pending_snippet.as_ref() else {
            return;
        };
        let Some((local_start, length)) = snippet.current_stop_range() else {
            return;
        };
        let start = self.snippet_anchor + local_start;
        let end = start + length;
        let range = CCursorRange::two(CCursor::new(start), CCursor::new(end));
        let mut new_state = output.state.clone();
        new_state.cursor.set_char_range(Some(range));
        new_state.store(&output.response.ctx, output.response.id);
    }

    fn update_hover(&mut self, rect: Rect, galley: &Arc<egui::Galley>, hover_pos: Option<Pos2>) {
        let pos = match hover_pos {
            Some(p) => p,
            None => {
                self.hover_text = None;
                self.hover_pos = None;
                return;
            }
        };
        if !rect.contains(pos) {
            self.hover_text = None;
            self.hover_pos = None;
            return;
        }
        let local = pos - rect.min;
        let line_idx = row_at_y(galley, local.y);
        if line_idx >= galley.rows.len() {
            self.hover_text = None;
            self.hover_pos = None;
            return;
        }
        let col = column_at_x(galley, line_idx, local.x);
        if let Some(text) = features::hover_at(&self.lsp, &self.uri, line_idx, col) {
            self.hover_text = Some(text);
            self.hover_pos = Some(pos);
        } else {
            self.hover_text = None;
            self.hover_pos = None;
        }
    }

    fn insert_completion(&mut self, ctx: &egui::Context, idx: usize) {
        let item = match self.completion.items.get(idx) {
            Some(it) => it.clone(),
            None => return,
        };
        // Structural edit — push the current state so the user can
        // undo the whole completion in one step.
        let pre_snap = self.snapshot(ctx);
        let insertion = completion::build_insertion(&item, &self.text, self.cursor);
        self.text = cursor::splice(&self.text, insertion.start, insertion.end, &insertion.text);
        self.snippet_anchor = insertion.start;
        self.pending_snippet = insertion.snippet;
        self.completion.dismiss();
        self.lsp.update_document(&self.uri, &self.text);
        // Commit *after* the mutation: the snapshot we took is the
        // pre-edit state, which is what we want on the undo stack.
        self.history.snapshot(pre_snap).commit_structural();
        self.frame_start = Some(self.snapshot(ctx));
    }

    /// Compute the byte range of the line containing the cursor
    /// (0-indexed). Returns `(start, end_inclusive_of_newline)` so
    /// callers can pick exactly the line text or the line +
    /// trailing newline.
    fn cursor_line_range(&self) -> (usize, usize) {
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
    fn replace_cursor_line(&mut self, ctx: &egui::Context, new_line: String) {
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
    fn insert_text(&mut self, ctx: &egui::Context, new_text: String) {
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
    fn apply_text_edit(&mut self, ctx: &egui::Context, new_text: String) {
        let pre = self.snapshot(ctx);
        self.text = new_text;
        self.lsp.update_document(&self.uri, &self.text);
        self.diagnostics = features::diagnostics_at(&self.lsp, &self.uri);
        self.history.snapshot(pre).commit_structural();
        self.frame_start = Some(self.snapshot(ctx));
    }

    /// Toggle `// ` at the start of every line in the current
    /// selection (or the cursor's line if no selection). Indents are
    /// preserved: the comment prefix is added after leading spaces.
    fn toggle_comment_at_cursor(&mut self, ctx: &egui::Context) {
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
    fn duplicate_line(&mut self, ctx: &egui::Context) {
        let line_idx = self.cursor.line.saturating_sub(1);
        let lines: Vec<&str> = self.text.split('\n').collect();
        if line_idx >= lines.len() {
            return;
        }
        let current = lines[line_idx];
        self.insert_text(ctx, format!("\n{}", current));
        self.status = format!("Duplicated line {}", self.cursor.line);
    }

    /// Delete the cursor's line. The line and its trailing newline
    /// are removed.
    fn delete_line(&mut self, ctx: &egui::Context) {
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
    fn move_line(&mut self, ctx: &egui::Context, delta: i32) {
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
    fn indent_at_cursor(&mut self, ctx: &egui::Context, delta: i32) {
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

    /// Toggle the fold region whose header line contains the cursor.
    /// If the cursor isn't on a fold header, the status bar is
    /// updated to explain that nothing happened.
    fn toggle_fold_at_cursor(&mut self) {
        let line_idx = self.cursor.line.saturating_sub(1);
        let regions = folding::detect_folds(&self.text);
        let hit = regions
            .iter()
            .find(|r| r.start_line == line_idx)
            .map(|r| r.id());
        match hit {
            Some(id) => {
                if !self.collapsed.remove(&id) {
                    self.collapsed.insert(id);
                }
            }
            None => {
                self.status = "No fold region on this line".to_string();
            }
        }
    }
}

/// Render a chord as a short human-readable label, e.g. "Ctrl+K"
/// for a pending prefix. Used by the status bar.
fn pending_chord_label(c: super::keybindings::Chord) -> String {
    let mut s = String::new();
    if c.ctrl_or_cmd {
        s.push_str("Ctrl+");
    }
    if c.alt {
        s.push_str("Alt+");
    }
    if c.shift {
        s.push_str("Shift+");
    }
    s.push_str(&format!("{:?}", c.key));
    s
}

#[cfg(test)]
mod tests {
    use super::super::keybindings::Chord;
    use super::pending_chord_label;
    use eframe::egui::Key;

    #[test]
    fn pending_label_basic() {
        let c = Chord::ctrl(Key::K);
        assert_eq!(pending_chord_label(c), "Ctrl+K");
    }

    #[test]
    fn pending_label_with_shift() {
        let c = Chord::ctrl_shift(Key::Z);
        assert_eq!(pending_chord_label(c), "Ctrl+Shift+Z");
    }
}
