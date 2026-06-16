//! Top-level `EditorApp` for the native editor.
//!
//! This module glues together the smaller focused modules:
//! - [`super::snippet`] — LSP-style snippet expansion and tab stops
//! - [`super::keybindings`] — global key handling for the completion popup
//!   and snippet navigation
//! - [`super::layouter`] — the custom syntax-highlighting layouter
//! - [`super::popup`] — completion and hover popup rendering
//! - [`super::highlight`] — per-line tokenization for syntax highlighting

use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
    Align2, Color32, FontId, Pos2, Rect, RichText, ScrollArea, TextEdit, Ui, Vec2,
};
use std::collections::BTreeSet;
use std::sync::Arc;
use workflow_lsp::features::{Completion, Diagnostic};
use workflow_lsp::ServerState;

use super::folding::{self, FoldRegion};
use super::keybindings::{self, KeyAction};
use super::layouter::{layout_flow, FONT_SIZE, LINE_HEIGHT};
use super::popup;
use super::snippet::{self, PendingSnippet};

use super::history::{History, Snapshot};

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorPosition {
    pub line: usize,
    pub col: usize,
}

pub struct EditorApp {
    text: String,
    lsp: ServerState,
    uri: String,
    completions: Vec<Completion>,
    completion_visible: bool,
    completion_index: usize,
    status: String,
    cursor: CursorPosition,
    hover_text: Option<String>,
    hover_pos: Option<Pos2>,
    /// Active LSP snippet, if any. The user accepts a snippet-style completion
    /// and then tabs through the stops. When `stops` is empty or `current`
    /// reaches the end, the snippet is done.
    pending_snippet: Option<PendingSnippet>,
    /// Where the snippet starts in the document, as a `CCursor` (char index
    /// from the start of the document). We re-derive the cursor position
    /// for each stop relative to this anchor.
    snippet_anchor: usize,
    history: History,
    /// Wall-clock time used to coalesce typing edits into a single undo
    /// step. We don't need a real clock — a frame counter is fine.
    edit_clock_ms: u128,
    /// Snapshot of the editor state at the start of the current frame.
    /// If the user typed this frame, we push this to the undo stack.
    frame_start: Option<Snapshot>,
    /// Screen position of the cursor, used to position the completion popup.
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
            completions: Vec::new(),
            completion_visible: false,
            completion_index: 0,
            status: "Ready".to_string(),
            cursor: CursorPosition { line: 1, col: 1 },
            hover_text: None,
            hover_pos: None,
            pending_snippet: None,
            snippet_anchor: 0,
            history: History::new(),
            edit_clock_ms: 0,
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
                        // Structural edit — one undo step, not coalesced.
                        self.history.snapshot(self.snapshot()).commit_structural();
                        self.text.clear();
                        self.lsp.update_document(&self.uri, &self.text);
                        self.frame_start = Some(self.snapshot());
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_editor(ui);
        });

        // Show diagnostics panel at the bottom if there are any
        if !self.diagnostics.is_empty() {
            egui::TopBottomPanel::bottom("diagnostics").show(ctx, |ui| {
                ui.label(RichText::new("Problems").strong());
                ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
                    for diag in &self.diagnostics {
                        let (color, icon) = match diag.severity {
                            workflow_lsp::features::DiagnosticSeverity::Error => {
                                (Color32::from_rgb(255, 80, 80), "✗")
                            }
                            workflow_lsp::features::DiagnosticSeverity::Warning => {
                                (Color32::from_rgb(255, 200, 50), "⚠")
                            }
                            _ => (Color32::GRAY, "ℹ"),
                        };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon).color(color));
                            ui.label(
                                RichText::new(format!(
                                    "Ln {}, Col {}: {}",
                                    diag.start_line + 1,
                                    diag.start_col + 1,
                                    diag.message
                                ))
                                .color(color),
                            );
                        });
                    }
                });
            });
        }

        if self.completion_visible && !self.completions.is_empty() {
            if let Some(idx) = popup::show_completion(
                ctx,
                &self.completions,
                self.completion_index,
                self.cursor_screen_pos,
            ) {
                self.insert_completion(idx);
            }
        }

        if let (Some(text), Some(pos)) = (self.hover_text.clone(), self.hover_pos) {
            popup::show_hover(ctx, pos, &text);
        }
    }
}

impl EditorApp {
    fn render_editor(&mut self, ui: &mut Ui) {
        // Save the *pre-edit* state once per frame so we can push it to
        // the undo stack if the user typed.
        if self.frame_start.is_none() {
            self.frame_start = Some(self.snapshot());
        }
        // Detect current fold regions in the source. Prune any
        // collapsed-fold id that no longer refers to a real region
        // (e.g. the user deleted the header).
        let regions = folding::detect_folds(&self.text);
        let live_ids: BTreeSet<usize> = regions.iter().map(|r| r.start_line).collect();
        self.collapsed.retain(|id| live_ids.contains(id));
        let regions_for_gutter = regions.clone();

        // Build the text that the TextEdit will actually display:
        // collapsed folds have their body replaced with a placeholder.
        let mut display_text = folding::apply_folds(&self.text, &self.collapsed);
        let pre_display = display_text.clone();

        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Gutter: line numbers + fold chevrons. Sized to fit
                    // the larger of (digit count for line numbers,
                    // 2-char chevron).
                    let line_count = display_text.lines().count().max(1);
                    let digits = ((line_count as f64).log10().floor() as usize + 1).max(2);
                    let gutter_width = (digits as f32) * 9.0 + 24.0; // digits * px + chevron area

                    let (gutter_rect, _gutter_response) = ui.allocate_exact_size(
                        Vec2::new(gutter_width, ui.available_height()),
                        egui::Sense::hover(),
                    );

                    // Editor
                    let output = TextEdit::multiline(&mut display_text)
                        .font(FontId::monospace(FONT_SIZE))
                        .desired_width(f32::INFINITY)
                        .layouter(&mut |ui, t, wrap_width| layout_flow(ui, t, wrap_width))
                        .show(ui);

                    // Now paint the gutter using the text editor's response rect
                    // to ensure proper alignment
                    let editor_rect = output.response.rect;
                    self.paint_gutter(ui, gutter_rect, &regions_for_gutter, &display_text, editor_rect.min.y);

                    // Capture the *previous* state for the undo history
                    // before the TextEdit mutates it.
                    let pre_edit = self.frame_start.take();
                    let post_edit = self.snapshot();

                    // Apply the snippet cursor before we move any fields
                    // out of `output`.
                    self.apply_snippet_cursor(&output);

                    let response = output.response;
                    let galley = output.galley;

                    if response.changed() {
                        // Splice the visible edits back into the source.
                        let new_source = folding::sync_edits(
                            &self.text,
                            &pre_display,
                            &display_text,
                            &self.collapsed,
                        );
                        self.text = new_source;

                        if let Some(prev) = pre_edit {
                            let mut snap = prev;
                            snap.last_edit_at_ms = self.now_ms();
                            self.history.snapshot(snap).commit_typing();
                        }
                        self.lsp.update_document(&self.uri, &self.text);
                        self.diagnostics =
                            workflow_lsp::features::diagnostics_at(&self.lsp, &self.uri);
                        // The user typed something: the snippet's
                        // selection is no longer meaningful, so drop it.
                        self.pending_snippet = None;
                    }

                    if let Some(range) = &output.cursor_range {
                        let primary = range.primary;
                        let line = primary.rcursor.row + 1;
                        let col = primary.rcursor.column + 1;
                        if line != self.cursor.line || col != self.cursor.col {
                            self.cursor = CursorPosition { line, col };
                        }
                        let row_idx = primary.rcursor.row;
                        if row_idx < galley.rows.len() {
                            let row = &galley.rows[row_idx];
                            let row_min_x = row.rect.min.x;
                            let mut cursor_x = row_min_x;
                            let mut glyph_count = 0;
                            for glyph in &row.glyphs {
                                if glyph_count >= primary.rcursor.column {
                                    break;
                                }
                                cursor_x = glyph.pos.x + glyph.size.x;
                                glyph_count += 1;
                            }
                            let cursor_y = row.rect.min.y;
                            self.cursor_screen_pos = Some(
                                response.rect.min
                                    + Vec2::new(
                                        cursor_x - response.rect.min.x,
                                        cursor_y - response.rect.min.y,
                                    ),
                            );
                        }
                    }

                    if response.changed()
                        && keybindings::should_request_completion(ui, &self.text, self.cursor)
                    {
                        self.request_completion();
                    } else if response.changed() {
                        self.completion_visible = false;
                    }

                    self.update_hover(response.rect, &galley, response.hover_pos());

                    if !response.changed() {
                        self.frame_start = Some(post_edit);
                    }
                });
            });
    }

    /// Draw line numbers and fold chevrons in the gutter. Clicking a
    /// chevron toggles the corresponding fold. The TextEdit and the
    /// gutter scroll together because they're in the same `ScrollArea`.
    fn paint_gutter(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        regions: &[FoldRegion],
        display_text: &str,
        editor_start_y: f32,
    ) {
        let painter = ui.painter_at(rect);
        // Subtle separator between gutter and editor.
        painter.line_segment(
            [rect.right_top(), rect.right_bottom()],
            (1.0, Color32::from_gray(60)),
        );

        let font = FontId::monospace(FONT_SIZE);
        let line_count = display_text.lines().count().max(1);
        let text_color = Color32::from_gray(140);

        for line_idx in 0..line_count {
            // Use the editor's start y position to align line numbers
            // with the actual text lines.
            let y = editor_start_y + (line_idx as f32) * LINE_HEIGHT;
            if y > rect.max.y {
                break;
            }
            let num = format!("{}", line_idx + 1);
            // Right-align the number within the chevron area + digit
            // area. We place it at `rect.max.x - 6.0`.
            let anchor = Pos2::new(rect.max.x - 6.0, y);
            painter.text(anchor, Align2::RIGHT_TOP, num, font.clone(), text_color);

            // If this line is the start of a foldable region, draw a
            // clickable chevron. We allocate a click region the size
            // of `LINE_HEIGHT` square at the left edge of the gutter.
            if let Some(region) = regions.iter().find(|r| r.start_line == line_idx) {
                let chevron_rect = Rect::from_min_size(
                    Pos2::new(rect.min.x + 2.0, y),
                    Vec2::new(16.0, LINE_HEIGHT),
                );
                let id = ui.id().with(("fold", region.start_line));
                let response = ui.interact(chevron_rect, id, egui::Sense::click());
                let collapsed = self.collapsed.contains(&region.start_line);
                let glyph = if collapsed { "▶" } else { "▼" };
                let base_color = match region.kind {
                    folding::FoldKind::Function => Color32::from_rgb(120, 180, 255),
                    folding::FoldKind::Workflow => Color32::from_rgb(255, 180, 120),
                };
                let color = if response.hovered() {
                    Color32::from_gray(240)
                } else {
                    base_color
                };
                if response.clicked() {
                    if collapsed {
                        self.collapsed.remove(&region.start_line);
                    } else {
                        self.collapsed.insert(region.start_line);
                    }
                }
                painter.text(
                    chevron_rect.center(),
                    Align2::CENTER_CENTER,
                    glyph,
                    font.clone(),
                    color,
                );
            }
        }
    }

    /// Run the global key handlers, then apply the result to editor state.
    fn handle_global_keys(&mut self, ctx: &egui::Context) {
        let popup_open = self.completion_visible && !self.completions.is_empty();
        let action = keybindings::take_key_action(ctx, popup_open, self.pending_snippet.is_some());
        match action {
            KeyAction::None => {}
            KeyAction::PopupUp
            | KeyAction::PopupDown
            | KeyAction::PopupAccept
            | KeyAction::PopupDismiss => {
                let max_index = self.completions.len().saturating_sub(1);
                if let Some(idx) = keybindings::apply_popup_action(
                    action,
                    &mut self.completion_visible,
                    &mut self.completion_index,
                    max_index,
                ) {
                    self.insert_completion(idx);
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
            KeyAction::Undo => self.undo(),
            KeyAction::Redo => self.redo(),
        }
    }

    fn undo(&mut self) {
        if let Some(snap) = self.history.undo() {
            self.apply_snapshot(snap);
        }
    }

    fn redo(&mut self) {
        if let Some(snap) = self.history.redo() {
            self.apply_snapshot(snap);
        }
    }

    fn apply_snapshot(&mut self, snap: Snapshot) {
        self.text = snap.text;
        self.cursor = snap.cursor;
        self.pending_snippet = snap.pending_snippet;
        self.snippet_anchor = 0;
        self.lsp.update_document(&self.uri, &self.text);
        // The frame_start was captured before this undo/redo; invalidate
        // it so the next render captures a fresh pre-edit snapshot.
        self.frame_start = None;
        self.completion_visible = false;
    }

    fn now_ms(&mut self) -> u128 {
        self.edit_clock_ms = self.edit_clock_ms.wrapping_add(16);
        self.edit_clock_ms
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            pending_snippet: self.pending_snippet.clone(),
            last_edit_at_ms: self.edit_clock_ms,
        }
    }

    /// Force the TextEdit's cursor to the active snippet stop after the
    /// galley has been laid out for the frame. The `output.state` is then
    /// stored so the change persists into the next frame.
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

    fn request_completion(&mut self) {
        let items = workflow_lsp::features::completions_at(
            &self.lsp,
            &self.uri,
            self.cursor.line - 1,
            self.cursor.col - 1,
        );
        self.completions = items;
        self.completion_visible = !self.completions.is_empty();
        if self.completion_visible && self.completion_index >= self.completions.len() {
            self.completion_index = 0;
        }
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
        let mut line_idx = 0usize;
        let mut best_y = f32::NEG_INFINITY;
        for (idx, row) in galley.rows.iter().enumerate() {
            let y = row.rect.min.y;
            if y <= local.y && y >= best_y {
                best_y = y;
                line_idx = idx;
            }
        }
        if line_idx >= galley.rows.len() {
            self.hover_text = None;
            self.hover_pos = None;
            return;
        }
        let row = &galley.rows[line_idx];
        let row_min_x = row.rect.min.x;
        let col = row
            .glyphs
            .iter()
            .take_while(|g| g.pos.x + g.size.x - row_min_x <= local.x)
            .count();

        if let Some(text) = workflow_lsp::features::hover_at(&self.lsp, &self.uri, line_idx, col) {
            self.hover_text = Some(text);
            self.hover_pos = Some(pos);
        } else {
            self.hover_text = None;
            self.hover_pos = None;
        }
    }

    fn insert_completion(&mut self, idx: usize) {
        if idx >= self.completions.len() {
            return;
        }
        // Structural edit — push the current state so the user can undo
        // the whole completion in one step.
        let pre_snap = self.snapshot();
        let item = self.completions[idx].clone();

        // Use the LSP-provided text edit if available, otherwise fall back
        // to computing the replacement range ourselves.
        if let Some(text_edit) = &item.text_edit {
            let (start_line, start_col, end_line, end_col) = text_edit.range;
            let start_line = start_line as usize;
            let start_col = start_col as usize;
            let end_line = end_line as usize;
            let end_col = end_col as usize;

            // Convert line/col to char offsets
            let mut start_chars = 0usize;
            let mut end_chars = 0usize;
            for (i, line) in self.text.split('\n').enumerate() {
                if i == start_line {
                    start_chars += start_col.min(line.chars().count());
                }
                if i == end_line {
                    end_chars += end_col.min(line.chars().count());
                    break;
                } else {
                    end_chars += line.chars().count() + 1;
                }
            }

            // Use the insert_text if available, otherwise use the new_text from text_edit
            let replacement = item.insert_text.as_deref().unwrap_or(&text_edit.new_text);
            self.apply_replacement(start_chars, end_chars, replacement);
        } else {
            // Fallback: compute the range ourselves
            let raw = item.insert_text.unwrap_or(item.label);

            // Compute the current cursor offset in *characters* (CCursor space).
            let line_idx = self.cursor.line.saturating_sub(1);
            let col_idx = self.cursor.col.saturating_sub(1);
            let mut char_offset = 0usize;
            for (i, line) in self.text.split('\n').enumerate() {
                if i == line_idx {
                    char_offset += col_idx.min(line.chars().count());
                    break;
                } else {
                    char_offset += line.chars().count() + 1;
                }
            }

            // The "word start" is the position from which we'll replace
            let before_chars: String = self.text.chars().take(char_offset).collect();
            let word_start_chars = before_chars
                .char_indices()
                .rev()
                .find(|(_, ch)| !ch.is_alphanumeric() && *ch != '_' && *ch != '.')
                .map(|(i, _)| i + 1)
                .unwrap_or(0);

            if raw.contains('$') {
                // Snippet path: expand the body, replace the prefix range, and
                // record the tab stops.
                let (expanded, stops) = snippet::expand(&raw);
                self.apply_replacement(word_start_chars, char_offset, &expanded);

                self.snippet_anchor = word_start_chars;
                self.pending_snippet = if stops.is_empty() {
                    None
                } else {
                    Some(PendingSnippet { stops, current: 0 })
                };
            } else {
                self.apply_replacement(word_start_chars, char_offset, &raw);
                self.pending_snippet = None;
            }
        }

        self.completion_visible = false;
        self.lsp.update_document(&self.uri, &self.text);
        // Commit *after* the mutation: the snapshot we took is the
        // pre-edit state, which is what we want on the undo stack.
        self.history.snapshot(pre_snap).commit_structural();
        self.frame_start = Some(self.snapshot());
    }

    fn apply_replacement(&mut self, start_chars: usize, end_chars: usize, replacement: &str) {
        // Convert char offsets to byte offsets.
        let start_byte = self
            .text
            .char_indices()
            .nth(start_chars)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len());
        let end_byte = self
            .text
            .char_indices()
            .nth(end_chars)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len());
        let mut buf = String::with_capacity(self.text.len() + replacement.len());
        buf.push_str(&self.text[..start_byte]);
        buf.push_str(replacement);
        buf.push_str(&self.text[end_byte..]);
        self.text = buf;
    }
}
