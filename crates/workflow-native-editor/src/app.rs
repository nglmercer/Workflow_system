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
use super::keybindings::{self, KeyAction};
use super::layouter::{layout_flow, FONT_SIZE, LINE_HEIGHT};
use super::popup;
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
                ui.horizontal(|ui| {
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

                    let galley = output.galley.clone();
                    gutter::paint(
                        ui,
                        gutter_rect,
                        &output.galley,
                        &regions_for_gutter,
                        &display_text,
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
        let popup_open = self.completion.visible && !self.completion.items.is_empty();
        let action = keybindings::take_key_action(ctx, popup_open, self.pending_snippet.is_some());
        match action {
            KeyAction::None => {}
            KeyAction::PopupUp
            | KeyAction::PopupDown
            | KeyAction::PopupAccept
            | KeyAction::PopupDismiss => {
                if let Some(idx) = self.completion.handle_key(action) {
                    self.insert_completion(ctx, idx);
                }
            }
            KeyAction::SnippetAdvance => {
                if let Some(snippet) = self.pending_snippet.as_mut() {
                    if snippet.advance() {
                        self.pending_snippet = None;
                    }
                }
            }
            KeyAction::SnippetCancel => {
                self.pending_snippet = None;
            }
            KeyAction::Undo => self.undo(ctx),
            KeyAction::Redo => self.redo(ctx),
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
}
