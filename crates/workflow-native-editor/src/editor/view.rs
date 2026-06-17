//! Editor rendering and view-state plumbing.
//!
//! The big method here is [`EditorApp::render_editor`], which
//! paints the gutter + TextEdit for the current buffer, wires up
//! the completion popup, hovers, find-match highlights, and
//! feeds the undo history with pre/post snapshots. Smaller
//! methods ([`EditorApp::paint_find_highlights`],
//! [`EditorApp::apply_snippet_cursor`],
//! [`EditorApp::maybe_clear_pending_snippet`],
//! [`EditorApp::toggle_fold_at_cursor`],
//! [`EditorApp::jump_to_current_match`],
//! [`EditorApp::open_search_result`]) are extracted to keep
//! `render_editor`'s body scannable.

use std::collections::HashSet;
use std::sync::Arc;

use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
    FontId, Rect, ScrollArea, TextEdit, Ui, Vec2,
};

use super::super::cursor::{
    self, char_to_line_col, cursor_screen_pos, CursorPosition, SelectionRange,
};
use super::super::folding;
use super::super::gutter;
use super::super::keybindings;
use super::super::layouter::{layout_flow, MatchHighlight, FONT_SIZE, LINE_HEIGHT};
use super::super::search_in_files::FileMatch;
use super::super::EditorApp;
use workflow_i18n::{t as i18n_t, tf as i18n_tf};
use workflow_lsp::features;

impl EditorApp {
    /// Move the text cursor to the start of the current find match,
    /// if any, and request a repaint so the editor scrolls there.
    pub(crate) fn jump_to_current_match(&mut self) {
        if let Some((start, _)) = self.find.current_range() {
            let (line, col) = char_to_line_col(&self.text, start);
            self.cursor = CursorPosition::new(line, col);
        }
    }

    /// Open a file from a global-search result and position the
    /// cursor on the matching line. If the result is already in
    /// the current buffer we just move the cursor.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open_search_result(&mut self, _ctx: &egui::Context, m: FileMatch) {
        let already_open = self
            .file_path
            .as_deref()
            .map(|p| p == m.path.as_path())
            .unwrap_or(false);
        if !already_open {
            if let Err(e) = self.load_path_into_editor(&m.path) {
                self.status = i18n_tf("app.status_open_failed", &[("error", &e.to_string())]);
                return;
            }
        }
        let target = m.line.min(1);
        self.cursor = CursorPosition::new(target, m.col);
        self.find.open(None);
        self.find.query = self.search_in_files.query.clone();
        self.find.update_matches(&self.text);
    }

    /// Paint translucent rectangles over the editor for every visible
    /// find match in the current galley. Returns the rect of the
    /// current match so the caller can scroll the editor there.
    pub(crate) fn paint_find_highlights(
        &self,
        ui: &egui::Ui,
        galley: &Arc<egui::Galley>,
        editor_rect: Rect,
    ) -> Option<Rect> {
        if !self.find.open || self.find.match_offsets.is_empty() {
            return None;
        }
        let source_to_display = folding::source_to_display_map(&self.text, &self.collapsed);
        let mut current_rect: Option<Rect> = None;
        for (idx, (start, end)) in self.find.match_offsets.iter().enumerate() {
            let (line, _col_start) = char_to_line_col(&self.text, *start);
            let (line_end, _col_end) = char_to_line_col(&self.text, *end);
            if line_end != line {
                // Skip multi-line matches for now; they would split
                // the highlight across rows and complicate the
                // visual.
                continue;
            }
            let display_line = match source_to_display.get(line.saturating_sub(1)) {
                Some(&dl) if dl == usize::MAX => continue, // inside a collapsed fold
                Some(&dl) => dl,
                None => continue,
            };
            if display_line >= galley.rows.len() {
                continue;
            }
            let row = &galley.rows[display_line];

            // Use the char index in the display row, computed from the
            // source byte offset. We approximate by computing the
            // column via the row's leading whitespace (line's
            // start) and the byte offset within the line.
            let line_byte_start: usize = self
                .text
                .lines()
                .take(line.saturating_sub(1))
                .map(|l| l.len() + 1)
                .sum();
            let col_in_line_start = start.saturating_sub(line_byte_start);
            let col_in_line_end = end.saturating_sub(line_byte_start);
            let row_start_x = row.rect.min.x;
            // Use cursor_screen_pos to find the left edge of the row,
            // then step per character.
            let left_pos =
                cursor::cursor_screen_pos(galley, editor_rect, display_line, col_in_line_start);
            let right_pos =
                cursor::cursor_screen_pos(galley, editor_rect, display_line, col_in_line_end);
            let mut rect = egui::Rect::from_min_max(left_pos, right_pos);
            // Make sure the rect has a sensible height even for empty
            // matches.
            if rect.height() < LINE_HEIGHT {
                rect = egui::Rect::from_min_size(
                    rect.min,
                    egui::Vec2::new(rect.width().max(2.0), LINE_HEIGHT),
                );
            }
            let _ = row_start_x;
            let color = if idx == self.find.current_match {
                crate::theme::Theme::CURRENT_FIND_MATCH_HIGHLIGHT
            } else {
                crate::theme::Theme::FIND_MATCH_HIGHLIGHT
            };
            let painter = ui.painter_at(editor_rect);
            painter.rect_filled(rect, 0.0, color);
            if idx == self.find.current_match {
                current_rect = Some(rect);
            }
        }
        current_rect
    }

    pub(crate) fn render_editor(&mut self, ctx: &egui::Context, ui: &mut Ui) {
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

                    // Get known function names from the LSP inference for syntax highlighting
                    let known_functions: HashSet<String> = self
                        .lsp
                        .get_inference(&self.uri)
                        .map(|inf| {
                            let mut names: HashSet<String> =
                                inf.registry.function_names().into_iter().collect();
                            // Also include locally-defined functions
                            for name in inf.functions.keys() {
                                names.insert(name.clone());
                            }
                            names
                        })
                        .unwrap_or_default();

                    // Build search match highlights
                    let highlights: Vec<MatchHighlight> =
                        if self.find.open && !self.find.query.is_empty() {
                            self.find
                                .match_offsets
                                .iter()
                                .enumerate()
                                .map(|(i, &(start, end))| MatchHighlight {
                                    start,
                                    end,
                                    is_current: i == self.find.current_match,
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };

                    let output = TextEdit::multiline(&mut display_text)
                        .font(FontId::monospace(FONT_SIZE))
                        .desired_width(f32::INFINITY)
                        .layouter(&mut |ui, t, wrap_width| {
                            layout_flow(ui, t, wrap_width, &known_functions, &highlights)
                        })
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
                        &self.text,
                    );

                    // Capture the *previous* state for the undo history
                    // before the TextEdit mutates it.
                    let pre_edit = self.frame_start.take();
                    let post_edit = self.snapshot(ctx);

                    // Capture the cursor *before* the TextEdit may
                    // have moved it so we can detect cursor-only
                    // movement (arrow keys, click) below and use
                    // that to dismiss the completion popup.
                    let prev_cursor = self.cursor;

                    // Apply the snippet cursor before we move any
                    // fields out of `output`.
                    self.apply_snippet_cursor(&output);
                    // If `apply_snapshot` left a pending cursor
                    // range from a recent undo/redo, push it into
                    // the TextEdit's state so the visible caret
                    // moves. We do this *after* `apply_snippet_cursor`
                    // so a restored snippet cursor wins over a plain
                    // restore-cursor — the snippet is the more
                    // specific intent.
                    if self.pending_snippet.is_none() {
                        if let Some((start, end)) = self.pending_cursor_char_range.take() {
                            let range = CCursorRange::two(CCursor::new(start), CCursor::new(end));
                            let mut new_state = output.state.clone();
                            new_state.cursor.set_char_range(Some(range));
                            new_state.store(&output.response.ctx, output.response.id);
                        }
                    } else {
                        // Snippet wins; discard the pending range.
                        self.pending_cursor_char_range = None;
                    }

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
                        let pre_text_len = self.text.chars().count();
                        self.text = new_source;
                        self.dirty = true;

                        if let Some(prev) = pre_edit {
                            let mut snap = prev;
                            // Refresh the timestamp to *now* so
                            // coalescing against the previous typing
                            // burst uses wall-clock-of-edit, not the
                            // start-of-frame timestamp.
                            snap.last_edit_at_ms = self.now_ms(ctx);
                            // Detect a paste: a single-frame delta
                            // large enough that no human typed it, or
                            // any newlines introduced in one frame.
                            // Both signal a clipboard insertion.
                            let post_text_len = self.text.chars().count();
                            let delta = post_text_len as isize - pre_text_len as isize;
                            let introduced_newline = delta > 0
                                && self.text[pre_text_len.min(self.text.len())..]
                                    .chars()
                                    .any(|c| c == '\n');
                            let is_paste = delta.abs() >= 8 || introduced_newline;
                            if is_paste {
                                snap.structural = true;
                                self.history.commit_structural(snap);
                            } else {
                                self.history.commit_typing(snap);
                            }
                        }
                        self.lsp.update_document(&self.uri, &self.text);
                        self.diagnostics = features::diagnostics_at(&self.lsp, &self.uri);
                        // The snippet's selection is only dropped
                        // when the user actually modified the
                        // current stop's text. A normal keystroke
                        // inside a snippet advances the snippet, not
                        // destroys it.
                        self.maybe_clear_pending_snippet();
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
                        // Capture selected text for Find bar pre-fill
                        if !range.is_empty() {
                            let char_range = range.as_sorted_char_range();
                            self.selected_text = Some(display_text[char_range].to_string());
                        } else {
                            self.selected_text = None;
                        }
                        // Mirror the selection in char coordinates so
                        // history snapshots can record it. `secondary`
                        // is the anchor in egui's cursor model; `primary`
                        // is the caret. Use the embedded `ccursor` char
                        // index.
                        self.selected_range = Some(SelectionRange {
                            anchor: range.secondary.ccursor.index,
                            cursor: range.primary.ccursor.index,
                        });
                    } else {
                        self.selected_range = None;
                    }

                    let cursor_moved = self.cursor != prev_cursor;

                    // Handle Ctrl+Click for go-to-definition
                    if cursor_moved && !response.changed() {
                        // Cursor moved without text change (click)
                        let ctrl_held = ui.input(|i| i.modifiers.ctrl);
                        if ctrl_held {
                            self.goto_definition_at_cursor();
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
                    } else if response.changed() || cursor_moved {
                        // Text changed but the new char isn't a
                        // completion trigger, OR the cursor moved
                        // (arrow keys, click) without any text
                        // change: in both cases the previous
                        // completion no longer applies, so dismiss
                        // it. Without the `cursor_moved` branch the
                        // popup would stay open when the user
                        // simply navigates with the arrow keys.
                        self.completion.dismiss();
                    }

                    self.update_hover(response.rect, &galley, response.hover_pos());

                    if let Some(current_rect) =
                        self.paint_find_highlights(ui, &galley, response.rect)
                    {
                        ui.scroll_to_rect(current_rect, Some(egui::Align::Center));
                    }

                    if !response.changed() {
                        self.frame_start = Some(post_edit);
                    }
                });
            });
    }

    /// Force the TextEdit's cursor to the active snippet stop after
    /// the galley has been laid out for the frame. The `output.state`
    /// is then stored so the change persists into the next frame.
    pub(crate) fn apply_snippet_cursor(&mut self, output: &egui::widgets::text_edit::TextEditOutput) {
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

    /// No-op kept as a hook for future snippet-preservation policy.
    /// Previously the editor dropped `pending_snippet` on every
    /// `response.changed()`; that interacted badly with undo/redo,
    /// which restores the snippet from the snapshot. The snippet
    /// now survives text changes and is only cleared explicitly via
    /// Esc (`Command::SnippetCancel`) or by tabbing past the last
    /// stop.
    #[inline]
    pub(crate) fn maybe_clear_pending_snippet(&mut self) {}

    /// Toggle the fold region whose header line contains the cursor.
    /// If the cursor isn't on a fold header, the status bar is
    /// updated to explain that nothing happened.
    pub(crate) fn toggle_fold_at_cursor(&mut self) {
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
                self.status = i18n_t("app.status_no_fold");
            }
        }
    }
}
