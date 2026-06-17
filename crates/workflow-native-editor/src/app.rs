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
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use workflow_lsp::features::{self, Diagnostic};
use workflow_lsp::ServerState;

use super::completion::{self, CompletionState};
use super::cursor::{
    self, char_to_line_col, column_at_x, cursor_screen_pos, row_at_y, CursorPosition,
};
use super::diagnostics_panel;
use super::file_browser;
use super::file_io;
use super::find_bar::{self, FindState};
use super::folding;
use super::gutter;
use super::history::{History, Snapshot};
use super::home::{self, HomeAction};
use super::keybindings::{self, Command, Keymap};
use super::layouter::{layout_flow, MatchHighlight, FONT_SIZE, LINE_HEIGHT};
use super::popup;
use super::recent::RecentList;
#[cfg(not(target_arch = "wasm32"))]
use super::search_in_files::{SearchInFilesEvent, SearchInFilesState};
use super::shortcuts_window;
use super::snippet::PendingSnippet;
use super::test_panel;
use workflow_i18n::{t as i18n_t, tf as i18n_tf};

pub struct EditorApp {
    text: String,
    lsp: ServerState,
    uri: String,
    completion: CompletionState,
    status: String,
    cursor: CursorPosition,
    hover_text: Option<popup::HoverContent>,
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
    /// Path of the file backing the buffer, if any. `None` means the
    /// document is "untitled" (or a freshly created example). The
    /// toolbar shows the file name when set; the LSP server is
    /// keyed on the `file://` URI derived from this path.
    file_path: Option<PathBuf>,
    /// True when the in-memory buffer has edits that haven't been
    /// written back to `file_path`. Rendered as a leading dot in
    /// the title bar (e.g. `● main.flow`) so the user can tell at a
    /// glance whether the disk is in sync.
    dirty: bool,
    /// Set by the toolbar Open button and the Ctrl+O command. The
    /// actual `rfd` dialog must run *after* the current `update`
    /// frame returns, because `rfd` blocks on its own event loop
    /// and would deadlock the egui context if invoked mid-frame. The
    /// main loop polls this flag and runs the dialog at a safe
    /// point.
    pending_open_dialog: bool,
    /// Recent-files list, loaded from disk at startup and updated
    /// on every successful file open. The home screen renders this
    /// when no project is open.
    recents: RecentList,
    /// True when the editor should render the home screen instead
    /// of the code editor. Toggled by a "Close Project" toolbar
    /// button: closing the last file takes the user back to the
    /// home screen so they can pick a recent or open a new one.
    home_open: bool,
    /// Latest test runner report, or `None` if no run has
    /// happened yet. Rendered by the bottom test panel.
    test_report: Option<workflow_test_runner::RunReport>,
    /// True while a test run is in flight on a background
    /// thread. Used to render the "Running…" hint and to gate
    /// the Run button.
    tests_running: bool,
    /// Receiver for the background test thread's result. The
    /// main loop polls this each frame and stores the result
    /// in `test_report`.
    test_receiver: Option<std::sync::mpsc::Receiver<workflow_test_runner::RunReport>>,
    /// A `Cancel` flag shared with the test thread. Set to
    /// `true` to ask the runner to stop early. The current
    /// implementation doesn't actually check it (the runner
    /// completes its in-flight tests and reports); it's
    /// included so the panel can render the button without
    /// lying.
    test_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Find bar state. Opened by Ctrl+F, closed by Escape.
    find: FindState,
    /// Currently selected text in the editor (updated each frame).
    selected_text: Option<String>,
    /// Global "find in files" panel. Desktop-only because the
    /// `ignore` walker is not designed for `wasm32-unknown-unknown`.
    #[cfg(not(target_arch = "wasm32"))]
    search_in_files: SearchInFilesState,
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
            status: i18n_t("app.status_ready"),
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
            file_path: None,
            dirty: false,
            pending_open_dialog: false,
            recents: RecentList::load(),
            home_open: true,
            test_report: None,
            tests_running: false,
            test_receiver: None,
            test_cancel: None,
            find: FindState::default(),
            selected_text: None,
            #[cfg(not(target_arch = "wasm32"))]
            search_in_files: SearchInFilesState::default(),
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_global_keys(ctx);

        // Run any deferred native file dialog now that we're at
        // the top of the frame, outside the egui scope-stack that
        // holds mutable borrows. `rfd::FileDialog::pick_file`
        // spins its own event loop; calling it from inside a
        // `show()` closure would deadlock.
        if std::mem::take(&mut self.pending_open_dialog) {
            self.run_open_dialog();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let title = self.title_label();
                ui.label(RichText::new(title).strong());
                ui.separator();
                ui.label(i18n_tf(
                    "app.status_position",
                    &[
                        ("line", &self.cursor.line.to_string()),
                        ("col", &self.cursor.col.to_string()),
                    ],
                ));
                ui.separator();
                // Language selector. Lists every bundled locale
                // with its localized display name; selecting one
                // flips the global i18n catalog and requests a
                // repaint so the next frame reflects the change.
                let current = workflow_i18n::current_locale();
                egui::ComboBox::from_label(i18n_t("toolbar.locale_label"))
                    .selected_text(workflow_i18n::display_name(&current))
                    .show_ui(ui, |ui| {
                        for &code in workflow_i18n::available_locales() {
                            let label = workflow_i18n::display_name(code);
                            if ui.selectable_label(code == current, label).clicked() {
                                workflow_i18n::init_with(code);
                                ctx.request_repaint();
                            }
                        }
                    });
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    if ui
                        .button(RichText::new(i18n_t("toolbar.clear")).small())
                        .clicked()
                    {
                        self.history
                            .snapshot(self.snapshot(ctx))
                            .commit_structural();
                        self.text.clear();
                        self.lsp.update_document(&self.uri, &self.text);
                        self.dirty = true;
                        self.frame_start = Some(self.snapshot(ctx));
                    }
                    if ui
                        .button(RichText::new(i18n_t("toolbar.shortcuts")).small())
                        .clicked()
                    {
                        self.shortcuts_open = !self.shortcuts_open;
                    }
                    if ui
                        .button(RichText::new(i18n_t("toolbar.save")).small())
                        .clicked()
                    {
                        self.save_current();
                    }
                    if ui
                        .button(RichText::new(i18n_t("toolbar.open")).small())
                        .clicked()
                    {
                        // Defer the dialog so it runs at the top of
                        // the next frame, outside the egui borrow.
                        self.pending_open_dialog = true;
                    }
                    if ui
                        .button(RichText::new(i18n_t("toolbar.close_project")).small())
                        .clicked()
                    {
                        self.close_project(ctx);
                    }
                });
            });
        });

        if self.home_open {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(action) = home::show(ui, &self.recents) {
                    self.handle_home_action(ctx, action);
                }
                // Drag-and-drop also works from the home screen:
                // dropping a file jumps straight into the editor.
                let dropped: Vec<PathBuf> = ctx.input(|i| {
                    i.raw
                        .dropped_files
                        .iter()
                        .filter_map(|f| f.path.clone())
                        .collect()
                });
                for path in dropped {
                    if let Err(e) = self.load_path_into_editor(&path) {
                        self.status =
                            i18n_tf("app.status_open_failed", &[("error", &e.to_string())]);
                    }
                }
            });
        } else {
            // When a file is open, render a left-side file browser
            // and the editor in the remaining central area. The
            // browser returns a path if the user clicked a sibling
            // file in the current directory.
            if let Some(picked) = file_browser::show(ctx, self.file_path.as_deref()) {
                if let Err(e) = self.load_path_into_editor(&picked) {
                    self.status = i18n_tf("app.status_open_failed", &[("error", &e.to_string())]);
                }
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                self.render_editor(ctx, ui);
                let dropped: Vec<PathBuf> = ctx.input(|i| {
                    i.raw
                        .dropped_files
                        .iter()
                        .filter_map(|f| f.path.clone())
                        .collect()
                });
                for path in dropped {
                    if let Err(e) = self.load_path_into_editor(&path) {
                        self.status =
                            i18n_tf("app.status_open_failed", &[("error", &e.to_string())]);
                    }
                }
            });
        }

        if let Some(msg) = diagnostics_panel::show(ctx, &self.diagnostics) {
            self.status = msg;
        }

        // Show the find bar if open.
        if self.find.open {
            egui::TopBottomPanel::bottom("find_bar").show(ctx, |ui| {
                let action = find_bar::show(ui, &mut self.find);
                match action {
                    find_bar::FindAction::Close => self.find.close(),
                    find_bar::FindAction::Next => {
                        self.find.next_match();
                        self.jump_to_current_match();
                    }
                    find_bar::FindAction::Previous => {
                        self.find.prev_match();
                        self.jump_to_current_match();
                    }
                    find_bar::FindAction::QueryChanged => {
                        self.find.update_matches(&self.text);
                        self.jump_to_current_match();
                    }
                    find_bar::FindAction::ToggleCase => {
                        self.find.toggle_case_sensitive(&self.text);
                        self.jump_to_current_match();
                    }
                    find_bar::FindAction::ToggleRegex => {
                        self.find.toggle_regex(&self.text);
                        self.jump_to_current_match();
                    }
                    find_bar::FindAction::ToggleWholeWord => {
                        self.find.toggle_whole_word(&self.text);
                        self.jump_to_current_match();
                    }
                    find_bar::FindAction::None => {}
                }
            });
        }

        // Drain any test result that arrived from the background
        // thread. This is cheap: `try_recv` returns immediately
        // when the channel is empty.
        self.poll_test_result();

        // Show the test panel. The panel renders both the
        // "Run" button (which calls `on_run`) and the result
        // list (read from `self.test_report`).
        let report = self.test_report.clone();
        let running = self.tests_running;
        let action = std::cell::Cell::new(None::<&str>);
        let msg = test_panel::show(
            ctx,
            &report,
            running,
            || action.set(Some("run")),
            || action.set(Some("cancel")),
        );
        match action.get() {
            Some("run") => self.run_tests(),
            Some("cancel") => self.cancel_tests(),
            _ => {}
        }
        if let Some(m) = msg {
            self.status = m;
        }

        // Drain any pending results from the global search
        // worker. The poll is cheap (a non-blocking mpsc
        // `try_recv`) so we do it every frame.
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.search_in_files.poll() {
                ctx.request_repaint();
            }
            if let Some(event) = super::search_in_files::show(ctx, &mut self.search_in_files) {
                match event {
                    SearchInFilesEvent::OpenMatch(idx) => {
                        if let Some(m) = self.search_in_files.results.get(idx).cloned() {
                            self.open_search_result(ctx, m.clone());
                        }
                    }
                }
            }
        }

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

        if let (Some(content), Some(pos)) = (self.hover_text.clone(), self.hover_pos) {
            popup::show_hover(ctx, pos, &content);
        }

        shortcuts_window::show(ctx, &mut self.shortcuts_open, &self.keymap);
    }
}

impl EditorApp {
    /// Move the text cursor to the start of the current find match,
    /// if any, and request a repaint so the editor scrolls there.
    fn jump_to_current_match(&mut self) {
        if let Some((start, _)) = self.find.current_range() {
            let (line, col) = char_to_line_col(&self.text, start);
            self.cursor = CursorPosition::new(line, col);
        }
    }

    /// Open a file from a global-search result and position the
    /// cursor on the matching line. If the result is already in
    /// the current buffer we just move the cursor.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_search_result(&mut self, _ctx: &egui::Context, m: super::search_in_files::FileMatch) {
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
    fn paint_find_highlights(
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
                        self.dirty = true;

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
                        // Capture selected text for Find bar pre-fill
                        if !range.is_empty() {
                            let char_range = range.as_sorted_char_range();
                            self.selected_text = Some(display_text[char_range].to_string());
                        } else {
                            self.selected_text = None;
                        }
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

    /// Run the global key handlers, then apply the result to editor
    /// state.
    fn handle_global_keys(&mut self, ctx: &egui::Context) {
        // When the find bar is open, Escape closes it.
        if self.find.open && shortcuts_window::esc_pressed(ctx) {
            self.find.close();
            ctx.input_mut(|i| {
                let _ = i.count_and_consume_key(egui::Modifiers::default(), egui::Key::Escape);
            });
            return;
        }
        // When the search-in-files panel is open, Escape closes it.
        #[cfg(not(target_arch = "wasm32"))]
        if self.search_in_files.open && shortcuts_window::esc_pressed(ctx) {
            self.search_in_files.close();
            ctx.input_mut(|i| {
                let _ = i.count_and_consume_key(egui::Modifiers::default(), egui::Key::Escape);
            });
            return;
        }
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
            self.status = i18n_tf(
                "app.status_chord_pending",
                &[("label", &pending_chord_label(pending))],
            );
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
                self.save_current();
            }
            Command::Open => {
                // Defer to next frame so the dialog runs outside
                // the egui scope-stack that holds `self` mutably.
                self.pending_open_dialog = true;
            }
            Command::Find => {
                self.find.open(self.selected_text.as_deref());
                self.find.update_matches(&self.text);
            }
            Command::GotoLine => {
                self.status = i18n_t("goto.not_implemented");
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
            Command::RunTests => self.run_tests(),
            Command::GotoDefinition => self.goto_definition_at_cursor(),
            #[cfg(not(target_arch = "wasm32"))]
            Command::SearchInFiles => {
                let default_root = self
                    .file_path
                    .as_deref()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf());
                self.search_in_files.open(default_root);
            }
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
            self.hover_text = Some(popup::HoverContent::from_markdown(&text));
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
        self.dirty = true;
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
        self.dirty = true;
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
        self.status = i18n_tf(
            "app.status_duplicated_line",
            &[("line", &self.cursor.line.to_string())],
        );
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

    /// Go to the definition of the symbol under the cursor.
    /// For imported functions, this opens the source file.
    fn goto_definition_at_cursor(&mut self) {
        let line_idx = self.cursor.line.saturating_sub(1);
        let lines: Vec<&str> = self.text.split('\n').collect();
        if line_idx >= lines.len() {
            return;
        }

        let line = lines[line_idx];
        let col = self.cursor.col.saturating_sub(1);
        if col >= line.len() {
            return;
        }

        // Extract the word at the cursor position
        let word = self.extract_word_at_position(line, col);
        if word.is_empty() {
            return;
        }

        // Check if this is an imported function
        if let Some(inference) = self.lsp.get_inference(&self.uri) {
            // Check if the word is a function in the registry
            if let Some(entry) = inference.registry.get(&word) {
                if entry.is_user_defined {
                    // Try to find the source file from the import statements
                    if let Some(source_path) = self.find_import_source(&word) {
                        self.status = i18n_tf("app.status_opening", &[("path", &source_path)]);
                        // Open the source file
                        if let Ok(path) = std::path::Path::new(&source_path).canonicalize() {
                            if let Err(e) = self.load_path_into_editor(&path) {
                                self.status = i18n_tf(
                                    "app.status_failed_to_open",
                                    &[("error", &e.to_string())],
                                );
                            }
                        } else {
                            self.status =
                                i18n_tf("app.status_file_not_found", &[("path", &source_path)]);
                        }
                        return;
                    }
                }
                self.status = i18n_tf("app.status_function_builtin", &[("name", &word)]);
                return;
            }

            // Check if it's a local function
            if inference.functions.contains_key(&word) {
                // For local functions, we could jump to the function definition
                // For now, just show a status message
                self.status = i18n_tf("app.status_function_local", &[("name", &word)]);
                return;
            }
        }

        self.status = i18n_tf("app.status_no_definition", &[("name", &word)]);
    }

    /// Extract the word at the given column position in a line.
    fn extract_word_at_position(&self, line: &str, col: usize) -> String {
        let bytes = line.as_bytes();
        if col >= bytes.len() {
            return String::new();
        }

        // Find the start of the word (go backwards until we find a non-alphanumeric, non-underscore)
        let mut start = col;
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            start -= 1;
        }

        // Find the end of the word (go forwards until we find a non-alphanumeric, non-underscore)
        let mut end = col;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }

        line[start..end].to_string()
    }

    /// Find the source file path for an imported function by looking
    /// at the import statements in the current file.
    fn find_import_source(&self, _function_name: &str) -> Option<String> {
        let lines: Vec<&str> = self.text.split('\n').collect();

        // Look for import statements that might contain this function
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") && trimmed.contains(" from ") {
                // Parse: import name from "path"
                if let Some(from_idx) = trimmed.find(" from ") {
                    let path_part = &trimmed[from_idx + 6..];
                    let path = path_part.trim().trim_matches('"').trim_matches('\'');

                    // Check if the path is a .flow file
                    if path.ends_with(".flow") {
                        // Resolve the path relative to the current file
                        if let Some(current_dir) = self.file_path.as_ref().and_then(|p| p.parent())
                        {
                            let full_path = current_dir.join(path);
                            if full_path.exists() {
                                return Some(full_path.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }

        None
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
                self.status = i18n_t("app.status_no_fold");
            }
        }
    }

    /// Build the title-bar label. Shows the file name (or
    /// i18n_t("editor.title_untitled")) with a leading dot when the buffer has unsaved
    /// edits. We use a Unicode bullet rather than `*` to match the
    /// common "modified" indicator in cross-platform editors.
    fn title_label(&self) -> String {
        let name = match &self.file_path {
            Some(p) => p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("(invalid path)")
                .to_string(),
            None => "Untitled".to_string(),
        };
        if self.dirty {
            format!("● {}", name)
        } else {
            name
        }
    }

    /// Kick off a test run on the in-memory buffer. Spawns a
    /// background thread that parses the buffer, runs every
    /// `test` block, and sends the result back via a channel.
    /// The main loop polls the channel and stores the result in
    /// `self.test_report`. If a run is already in flight this is
    /// a no-op (the panel disables the button while running).
    fn run_tests(&mut self) {
        if self.tests_running {
            return;
        }
        let source = self.text.clone();
        let virtual_path = self
            .file_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<buffer>".to_string());

        // If the open file is a sidecar `*.test.flow`, look for
        // its sibling `*.flow` on disk and feed both to the
        // runner. The test buffer is the source of truth for
        // the `TestDef`s, but the `WorkflowDef`s live in the
        // host file — without it, every test would report
        // "no workflow handles event '<X>'".
        let sidecar = self.file_path.as_ref().and_then(|p| {
            let name = p.file_name()?.to_str()?;
            let stem = name.strip_suffix(".test.flow")?;
            let host = p.with_file_name(format!("{stem}.flow"));
            if host.exists() {
                Some(host)
            } else {
                None
            }
        });
        let (host_source, host_path): (Option<String>, Option<String>) = match &sidecar {
            Some(p) => match std::fs::read_to_string(p) {
                Ok(s) => (Some(s), Some(p.to_string_lossy().into_owned())),
                Err(_) => (None, None),
            },
            None => (None, None),
        };

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runner = workflow_test_runner::TestRunner::with_default_config();
            let report = runner
                .run_source_with_host(
                    &source,
                    &virtual_path,
                    host_source.as_deref(),
                    host_path.as_deref(),
                )
                .unwrap_or_else(|e| {
                    workflow_test_runner::RunReport::from_tests(
                        &virtual_path,
                        vec![workflow_test_runner::TestReport {
                            name: "<runner>".to_string(),
                            source_path: virtual_path.clone(),
                            event: String::new(),
                            asserts: vec![workflow_test_runner::AssertResult::fail(
                                workflow_test_runner::AssertKind::Logs,
                                "",
                                String::new(),
                                format!("runner error: {}", e),
                            )],
                            matched_workflow_count: 0,
                            passed: false,
                        }],
                    )
                });
            let _ = tx.send(report);
        });
        self.test_receiver = Some(rx);
        self.tests_running = true;
        self.status = i18n_t("app.status_running_tests");
    }

    /// Called by the test panel's Cancel button. We don't
    /// actually cancel the in-flight run (the runner completes
    /// its current test and reports), but we flip the cancel
    /// flag for future use and surface a status message.
    fn cancel_tests(&mut self) {
        if let Some(flag) = &self.test_cancel {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.status = i18n_t("app.status_cancel_requested");
    }

    /// Drain the test result channel. Called once per frame.
    fn poll_test_result(&mut self) {
        if let Some(rx) = &self.test_receiver {
            match rx.try_recv() {
                Ok(report) => {
                    self.test_report = Some(report);
                    self.tests_running = false;
                    self.test_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.tests_running = false;
                    self.test_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    /// Run the native "open" file dialog. Called from
    /// [`Self::update`] when `pending_open_dialog` is set, so it
    /// runs *outside* the egui scope-stack that holds `self`
    /// mutably. The dialog itself is modal; we let the user cancel
    /// without touching the buffer.
    fn run_open_dialog(&mut self) {
        let dialog = rfd::FileDialog::new()
            .set_title(i18n_t("dialog.open_title"))
            .add_filter(
                i18n_t("dialog.filter_workflow"),
                &["flow", "yaml", "yml", "json", "toml"],
            )
            .add_filter(i18n_t("dialog.filter_all"), &["*"]);
        match dialog.pick_file() {
            Some(path) => {
                if let Err(e) = self.load_path_into_editor(&path) {
                    self.status = i18n_tf("app.status_open_failed", &[("error", &e.to_string())]);
                }
            }
            None => {
                self.status = i18n_t("app.status_open_cancelled");
            }
        }
    }

    /// Read `path` from disk and replace the editor buffer with
    /// the result. Resets the undo history, the LSP state, and the
    /// dirty flag. Used by both the toolbar Open button and the
    /// drag-and-drop handler.
    fn load_path_into_editor(
        &mut self,
        path: &std::path::Path,
    ) -> Result<(), file_io::FileIoError> {
        let contents = file_io::load_from_path(path)?;
        let path_buf = path.to_path_buf();
        let uri = file_io::path_to_uri(&path_buf);
        self.text = contents;
        self.file_path = Some(path_buf.clone());
        self.uri = uri;
        self.dirty = false;
        // Fresh document — the previous undo history no longer
        // applies to this file.
        self.history = History::new();
        self.completion.dismiss();
        self.pending_snippet = None;
        self.snippet_anchor = 0;
        self.collapsed.clear();
        self.lsp.update_document(&self.uri, &self.text);
        self.diagnostics = features::diagnostics_at(&self.lsp, &self.uri);
        self.cursor = CursorPosition::new(1, 1);
        self.home_open = false;
        self.status = i18n_tf(
            "app.status_opened",
            &[("path", &path.display().to_string())],
        );
        // Record this open in the recents list. We do the file
        // I/O here rather than on every keystroke; the home screen
        // is the only consumer and the list is capped at 10.
        self.recents.touch(&path_buf);
        if let Err(e) = self.recents.save() {
            // Recents persistence is best-effort: log to status
            // but don't undo the file open.
            self.status = i18n_tf(
                "app.status_opened_recents_failed",
                &[
                    ("path", &path.display().to_string()),
                    ("error", &e.to_string()),
                ],
            );
        }
        Ok(())
    }

    /// Save the current buffer to `file_path`. If there is no path
    /// yet (e.g. the editor was started without a file), fall back
    /// to a Save As dialog so the user can pick a destination.
    fn save_current(&mut self) {
        match self.file_path.clone() {
            Some(path) => match file_io::save_to_path(&path, &self.text) {
                Ok(saved) => {
                    self.dirty = false;
                    self.status = i18n_tf(
                        "app.status_saved",
                        &[("path", &saved.display().to_string())],
                    );
                }
                Err(e) => {
                    self.status = i18n_tf("app.status_save_failed", &[("error", &e.to_string())]);
                }
            },
            None => self.save_as_dialog(),
        }
    }

    /// Show a native "Save As" dialog and persist the buffer to
    /// the chosen path. We also push the path back into
    /// [`Self::file_path`] so subsequent Ctrl+S saves overwrite
    /// the same file.
    fn save_as_dialog(&mut self) {
        let dialog = rfd::FileDialog::new()
            .set_title(i18n_t("dialog.save_title"))
            .add_filter(
                i18n_t("dialog.filter_workflow"),
                &["flow", "yaml", "yml", "json", "toml"],
            )
            .set_file_name(i18n_t("dialog.default_name"));
        let chosen = match dialog.save_file() {
            Some(p) => p,
            None => {
                self.status = i18n_t("app.status_save_cancelled");
                return;
            }
        };
        match file_io::save_to_path(&chosen, &self.text) {
            Ok(saved) => {
                self.file_path = Some(saved.clone());
                self.uri = file_io::path_to_uri(&saved);
                self.dirty = false;
                self.status = i18n_tf(
                    "app.status_saved",
                    &[("path", &saved.display().to_string())],
                );
            }
            Err(e) => {
                self.status = i18n_tf("app.status_save_failed", &[("error", &e.to_string())]);
            }
        }
    }

    /// Handle a click on the home screen. "New File" resets the
    /// buffer and switches the editor out of the home view.
    /// "Open File…" reuses the deferred-dialog mechanism so the
    /// `rfd` event loop runs outside the egui borrow. "OpenPath"
    /// loads a recent file directly.
    fn handle_home_action(&mut self, ctx: &egui::Context, action: HomeAction) {
        match action {
            HomeAction::NewFile => {
                self.new_untitled(ctx);
            }
            HomeAction::OpenDialog => {
                self.pending_open_dialog = true;
            }
            HomeAction::OpenPath(path) => {
                if let Err(e) = self.load_path_into_editor(&path) {
                    self.status = i18n_tf("app.status_open_failed", &[("error", &e.to_string())]);
                }
            }
        }
    }

    /// Reset the editor to an empty untitled buffer, leaving the
    /// home screen closed so the user starts editing immediately.
    /// Used by the home screen's "New File" button and by the
    /// toolbar "Close Project" flow.
    fn new_untitled(&mut self, ctx: &egui::Context) {
        self.text.clear();
        self.file_path = None;
        self.uri = "file:///untitled".to_string();
        self.dirty = false;
        self.history = History::new();
        self.completion.dismiss();
        self.pending_snippet = None;
        self.snippet_anchor = 0;
        self.collapsed.clear();
        self.diagnostics.clear();
        self.lsp.update_document(&self.uri, &self.text);
        self.cursor = CursorPosition::new(1, 1);
        self.home_open = false;
        self.status = i18n_t("app.status_new_file");
        self.frame_start = Some(self.snapshot(ctx));
    }

    /// Close the current project and return to the home screen.
    /// The in-memory buffer is discarded; recent files are kept
    /// (they're persisted separately on every load).
    fn close_project(&mut self, ctx: &egui::Context) {
        self.text.clear();
        self.file_path = None;
        self.uri = "file:///untitled".to_string();
        self.dirty = false;
        self.history = History::new();
        self.completion.dismiss();
        self.pending_snippet = None;
        self.snippet_anchor = 0;
        self.collapsed.clear();
        self.diagnostics.clear();
        self.home_open = true;
        self.status = i18n_t("app.status_closed_project");
        let _ = ctx;
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
    use super::EditorApp;
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

    #[test]
    fn title_label_untitled_clean() {
        let app = EditorApp {
            file_path: None,
            dirty: false,
            ..Default::default()
        };
        assert_eq!(app.title_label(), "Untitled");
    }

    #[test]
    fn title_label_untitled_dirty() {
        let app = EditorApp {
            file_path: None,
            dirty: true,
            ..Default::default()
        };
        assert!(app.title_label().starts_with("● "));
    }

    #[test]
    fn title_label_uses_file_name() {
        let app = EditorApp {
            file_path: Some(std::path::PathBuf::from("/tmp/main.flow")),
            dirty: false,
            ..Default::default()
        };
        assert_eq!(app.title_label(), "main.flow");
    }

    #[test]
    fn title_label_dirty_marks_dot() {
        let app = EditorApp {
            file_path: Some(std::path::PathBuf::from("/tmp/main.flow")),
            dirty: true,
            ..Default::default()
        };
        assert_eq!(app.title_label(), "● main.flow");
    }
}
