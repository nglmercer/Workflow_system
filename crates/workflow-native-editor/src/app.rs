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

use eframe::egui::{self, Pos2, Rect, RichText};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use workflow_lsp::features::{self, Diagnostic};
use workflow_lsp::ServerState;

use super::completion::{self, CompletionState};
use super::cursor::{self, column_at_x, row_at_y, CursorPosition, SelectionRange};
use super::diagnostics_panel;
use super::file_browser;
use super::find_bar::{self, FindState};
use super::history::{History, Snapshot};
use super::home;
use super::keybindings::Keymap;
use super::plugin_manager::EditorPluginManager;
use super::plugin_panel;
use super::popup;
use super::recent::RecentList;
#[cfg(not(target_arch = "wasm32"))]
use super::search_in_files::{SearchInFilesEvent, SearchInFilesState};
use super::shortcuts_window;
use super::snippet::PendingSnippet;
use super::test_panel;
use workflow_i18n::{t as i18n_t, tf as i18n_tf};

pub struct EditorApp {
    pub(crate) text: String,
    pub(crate) lsp: ServerState,
    pub(crate) uri: String,
    pub(crate) completion: CompletionState,
    pub(crate) status: String,
    pub(crate) cursor: CursorPosition,
    pub(crate) hover_text: Option<popup::HoverContent>,
    pub(crate) hover_pos: Option<Pos2>,
    /// Active LSP snippet, if any. The user accepts a snippet-style
    /// completion and then tabs through the stops. When `stops` is
    /// empty or `current` reaches the end, the snippet is done.
    pub(crate) pending_snippet: Option<PendingSnippet>,
    /// Char offset of where the snippet starts in the document. We
    /// re-derive the cursor position for each stop relative to this
    /// anchor.
    pub(crate) snippet_anchor: usize,
    pub(crate) history: History,
    /// Snapshot of the editor state at the start of the current
    /// frame. If the user typed this frame, we push this to the undo
    /// stack.
    pub(crate) frame_start: Option<Snapshot>,
    /// Screen position of the cursor, used to position the completion
    /// popup.
    pub(crate) cursor_screen_pos: Option<Pos2>,
    /// Diagnostics (errors, warnings) for the current document.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Start-line of every collapsed fold region. Stable across edits
    /// as long as the relative position of the block's opening line
    /// doesn't change.
    pub(crate) collapsed: BTreeSet<usize>,
    /// Key bindings: maps chord sequences to commands.
    pub(crate) keymap: Keymap,
    /// Whether the keyboard-shortcuts help window is visible. The
    /// `F1` key (mapped to `Command::ShowShortcuts`) and the toolbar
    /// button flip this; `Esc` closes it.
    pub(crate) shortcuts_open: bool,
    /// Path of the file backing the buffer, if any. `None` means the
    /// document is "untitled" (or a freshly created example). The
    /// toolbar shows the file name when set; the LSP server is
    /// keyed on the `file://` URI derived from this path.
    pub(crate) file_path: Option<PathBuf>,
    /// True when the in-memory buffer has edits that haven't been
    /// written back to `file_path`. Rendered as a leading dot in
    /// the title bar (e.g. `● main.flow`) so the user can tell at a
    /// glance whether the disk is in sync.
    pub(crate) dirty: bool,
    /// Set by the toolbar Open button and the Ctrl+O command. The
    /// actual `rfd` dialog must run *after* the current `update`
    /// frame returns, because `rfd` blocks on its own event loop
    /// and would deadlock the egui context if invoked mid-frame. The
    /// main loop polls this flag and runs the dialog at a safe
    /// point.
    pub(crate) pending_open_dialog: bool,
    /// Recent-files list, loaded from disk at startup and updated
    /// on every successful file open. The home screen renders this
    /// when no project is open.
    pub(crate) recents: RecentList,
    /// True when the editor should render the home screen instead
    /// of the code editor. Toggled by a "Close Project" toolbar
    /// button: closing the last file takes the user back to the
    /// home screen so they can pick a recent or open a new one.
    pub(crate) home_open: bool,
    /// Latest test runner report, or `None` if no run has
    /// happened yet. Rendered by the bottom test panel.
    pub(crate) test_report: Option<workflow_test_runner::RunReport>,
    /// True while a test run is in flight on a background
    /// thread. Used to render the "Running…" hint and to gate
    /// the Run button.
    pub(crate) tests_running: bool,
    /// Receiver for the background test thread's result. The
    /// main loop polls this each frame and stores the result
    /// in `test_report`.
    pub(crate) test_receiver: Option<std::sync::mpsc::Receiver<workflow_test_runner::RunReport>>,
    /// A `Cancel` flag shared with the test thread. Set to
    /// `true` to ask the runner to stop early. The current
    /// implementation doesn't actually check it (the runner
    /// completes its in-flight tests and reports); it's
    /// included so the panel can render the button without
    /// lying.
    pub(crate) test_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Find bar state. Opened by Ctrl+F, closed by Escape.
    pub(crate) find: FindState,
    /// Currently selected text in the editor (updated each frame).
    pub(crate) selected_text: Option<String>,
    /// The current selection as char indices, mirroring `selected_text`
    /// but in buffer coordinates. Captured into history snapshots so
    /// undo/redo can restore multi-char selections.
    pub(crate) selected_range: Option<SelectionRange>,
    /// Last-known OS clipboard text, captured before any cut/paste
    /// so undo can restore it.
    pub(crate) last_clipboard: Option<String>,
    /// Set by `apply_snapshot` after an undo/redo. `render_editor`
    /// reads this and pushes the cursor into the TextEdit's state
    /// so the visible caret moves to the restored position.
    pub(crate) pending_cursor_char_range: Option<(usize, usize)>,
    /// Set by `handle_global_keys` when Ctrl+F is pressed. The actual
    /// `find.open(...)` call is deferred until after `render_editor`
    /// has run, so the just-captured `selected_text` reflects the
    /// current selection rather than the previous frame's stale value.
    pub(crate) pending_find_open: bool,
    /// Global "find in files" panel. Desktop-only because the
    /// `ignore` walker is not designed for `wasm32-unknown-unknown`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) search_in_files: SearchInFilesState,
    /// Plugin manager with hot-reload support.
    pub(crate) plugin_manager: EditorPluginManager,
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

        // Initialize plugin manager with default plugin directory
        let plugin_dir = std::env::current_dir()
            .unwrap_or_default()
            .join("plugins");
        let mut plugin_manager = EditorPluginManager::new(&plugin_dir);
        plugin_manager.load_all();

        // Bridge plugin functions/objects into the LSP so completions,
        // hover, and diagnostics recognize plugin-provided symbols.
        lsp.set_plugin_registry(plugin_manager.function_registry().clone());
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
            selected_range: None,
            last_clipboard: None,
            pending_cursor_char_range: None,
            pending_find_open: false,
            #[cfg(not(target_arch = "wasm32"))]
            search_in_files: SearchInFilesState::default(),
            plugin_manager,
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_global_keys(ctx);

        // Poll plugin file watcher events
        if self.plugin_manager.poll_events() {
            ctx.request_repaint();
        }

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
                        let mut pre = self.snapshot(ctx);
                        self.text.clear();
                        self.pending_snippet = None;
                        self.snippet_anchor = 0;
                        self.lsp.update_document(&self.uri, &self.text);
                        self.dirty = true;
                        pre.structural = true;
                        pre.last_edit_at_ms = self.now_ms(ctx);
                        self.history.commit_structural(pre);
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

        // Flush any deferred `Find` request now that the central
        // panel has rendered. The Ctrl+F handler runs at the top of
        // the frame, so reading `selected_text` there would yield
        // the previous frame's value (or `None` if the user just
        // changed the selection this frame). When the home screen
        // is showing there is no editor to pre-fill from, so the
        // find bar opens empty.
        if self.pending_find_open {
            self.pending_find_open = false;
            self.find.open(self.selected_text.as_deref());
            self.find.update_matches(&self.text);
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

        // Show plugin panel
        if let Some(action) = plugin_panel::show(ctx, &self.plugin_manager) {
            match action {
                plugin_panel::PluginAction::ReloadAll => {
                    let loaded = self.plugin_manager.reload_all();
                    self.status = if loaded.is_empty() {
                        "No plugins loaded".to_string()
                    } else {
                        format!("Reloaded {} plugin(s): {}", loaded.len(), loaded.join(", "))
                    };
                    // Re-register plugin functions in the LSP inference
                    // so completions/hover/diagnostics reflect the reloaded plugins.
                    self.lsp.register_plugin_functions(&self.uri);
                }
                plugin_panel::PluginAction::TogglePanel => {
                    self.plugin_manager.toggle_panel();
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
    // The methods that used to live here have been moved into
    // the `editor::` submodules (`input`, `view`, `project`,
    // `tests_runner`, `edit_ops`, `history_ops`, `import_hover`).
    // Rust's `impl` blocks can be split across files, so call
    // sites like `self.handle_global_keys(ctx)` and
    // `self.render_editor(ctx, ui)` resolve to the methods in
    // those modules without any shim here.

    pub(crate) fn update_hover(
        &mut self,
        rect: Rect,
        galley: &Arc<egui::Galley>,
        hover_pos: Option<Pos2>,
    ) {
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

        // Imports get a custom hover that surfaces the imported
        // shape (e.g. `{ email: string, plan: string }`) rather than
        // the generic "imported binding X" text. The fast path
        // lives in `editor::import_hover`; here we just call into
        // it and forward the result.
        if let Some(import_line) = crate::editor::import_hover::import_at_line(&self.text, line_idx)
        {
            self.hover_text = Some(crate::editor::import_hover::build_import_hover(
                &self.lsp,
                &self.uri,
                &self.text,
                &import_line,
            ));
            self.hover_pos = Some(pos);
            return;
        }

        if let Some(text) = features::hover_at(&self.lsp, &self.uri, line_idx, col) {
            self.hover_text = Some(popup::HoverContent::from_markdown(&text));
            self.hover_pos = Some(pos);
        } else {
            self.hover_text = None;
            self.hover_pos = None;
        }
    }

    pub(crate) fn insert_completion(&mut self, ctx: &egui::Context, idx: usize) {
        let item = match self.completion.items.get(idx) {
            Some(it) => it.clone(),
            None => return,
        };
        // Capture the pre-edit state. The completion is a structural
        // edit, so we want undo to revert the *whole* insertion in
        // one step.
        let mut pre_snap = self.snapshot(ctx);
        let insertion = completion::build_insertion(&item, &self.text, self.cursor);
        self.text = cursor::splice(&self.text, insertion.start, insertion.end, &insertion.text);
        self.snippet_anchor = insertion.start;
        self.pending_snippet = insertion.snippet;
        self.completion.dismiss();
        self.dirty = true;
        self.lsp.update_document(&self.uri, &self.text);
        self.diagnostics = features::diagnostics_at(&self.lsp, &self.uri);
        pre_snap.structural = true;
        pre_snap.last_edit_at_ms = self.now_ms(ctx);
        self.history.commit_structural(pre_snap);
        self.frame_start = Some(self.snapshot(ctx));
    }

    /// Go to the definition of the symbol under the cursor.
    /// For imported functions, this opens the source file. The
    /// word extraction and import-source lookup live in
    /// `crate::goto_definition`; this method handles the
    /// `&mut self` side (status updates, file open, mutating
    /// `self.cursor` via `load_path_into_editor`).
    pub(crate) fn goto_definition_at_cursor(&mut self) {
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
        let word = crate::goto_definition::extract_word_at_position(line, col);
        if word.is_empty() {
            return;
        }

        // Check if this is an imported function
        if let Some(inference) = self.lsp.get_inference(&self.uri) {
            // Check if the word is a function in the registry
            if let Some(entry) = inference.registry.get(&word) {
                if entry.is_user_defined {
                    // Try to find the source file from the import statements
                    if let Some(source_path) = crate::goto_definition::find_import_source(
                        &self.text,
                        self.file_path.as_deref(),
                        &word,
                    ) {
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

    // The methods that used to live here have been moved into
    // `editor::view` (rendering + fold toggling),
    // `editor::project` (file I/O + project lifecycle), and
    // `editor::tests_runner` (test runner). Rust's `impl` blocks
    // can be split across files, so `self.title_label()`,
    // `self.save_current()`, `self.render_editor()` etc. all
    // resolve to the methods in those modules without any
    // shim here.
}

#[cfg(test)]
mod tests {
    use super::super::file_io;
    use super::EditorApp;

    #[test]
    fn build_import_hover_uses_resolved_schema() {
        use crate::popup::HoverKind;
        let text = r#"@import USER_REGISTERED from "./user_registered.json"

workflow "W" {
  on USER_REGISTERED
  log(USER_REGISTERED.email)
}
"#;
        let mut app = EditorApp {
            text: text.to_string(),
            ..Default::default()
        };
        app.lsp.update_document(&app.uri, &app.text);
        let import =
            crate::editor::import_hover::import_at_line(&app.text, 0).expect("import line");
        let hover =
            crate::editor::import_hover::build_import_hover(&app.lsp, &app.uri, &app.text, &import);
        assert_eq!(hover.title, "USER_REGISTERED");
        assert_eq!(hover.kind, HoverKind::Import);
        assert!(
            hover
                .docs
                .as_deref()
                .unwrap()
                .contains("user_registered.json"),
            "expected source path in docs, got {:?}",
            hover.docs
        );
    }

    /// Regression: the editor's default URI is
    /// `file:///example.flow`, whose parent is `/`. The import
    /// `./user_registered.json` does not exist there, so the schema
    /// resolver used to skip the binding. The previous test only
    /// checked that the source path made it into the hover docs
    /// (which it does regardless of the schema). This one opens the
    /// real `examples/advanced.flow` and asserts the
    /// `NESTED_DATA` binding's hover signature carries the resolved
    /// schema, not the `any` fallback the underlying
    /// `infer_imports` bug produced.
    #[test]
    fn build_import_hover_for_real_file_resolves_schema() {
        use crate::popup::{HoverSignature, TypeExpr};
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/advanced.flow");
        let path = path.canonicalize().unwrap_or(path);
        let source = std::fs::read_to_string(&path).expect("read advanced.flow");
        let mut app = EditorApp::default();
        let uri = file_io::path_to_uri(&path);
        app.uri = uri.clone();
        app.text = source.clone();
        app.file_path = Some(path.clone());
        app.lsp.update_document(&app.uri, &app.text);

        let program = workflow_parser::FlowParser::parse_flow_program(&source).expect("parse");
        let nested = program
            .imports
            .iter()
            .find(|imp| imp.name == "NESTED_DATA")
            .expect("NESTED_DATA import");
        let line_idx = source[..nested.span.start]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let import = crate::editor::import_hover::import_at_line(&app.text, line_idx)
            .expect("import line for NESTED_DATA");
        let hover =
            crate::editor::import_hover::build_import_hover(&app.lsp, &app.uri, &app.text, &import);
        assert_eq!(hover.title, "NESTED_DATA");
        match hover.signature {
            Some(HoverSignature::Type(TypeExpr::Object(ref fields))) => {
                assert!(
                    fields.iter().any(|f| f.name == "users"),
                    "expected `users` field in resolved schema, got {:?}",
                    fields
                );
            }
            other => panic!(
                "expected the resolved schema, got {:?} (any-fallback means infer_imports is still broken)",
                other
            ),
        }
    }
}

#[cfg(test)]
mod theme_regression_tests {
    use super::*;
    use crate::theme::Theme;

    /// Regression: the find-match highlight constants in `app.rs` (the
    /// painter overlay) and the layouter (the
    /// `MATCH_HIGHLIGHT`/`CURRENT_MATCH_HIGHLIGHT` consts) were
    /// previously two different yellows at two different alphas.
    /// The theme module now pins the two pairs; this test asserts
    /// they are not silently re-introduced.
    #[test]
    fn find_match_painter_uses_theme_constants() {
        assert!(Theme::CURRENT_FIND_MATCH_HIGHLIGHT.a() >= Theme::FIND_MATCH_HIGHLIGHT.a());
        assert!(
            Theme::LAYOUT_CURRENT_FIND_MATCH_HIGHLIGHT.a()
                >= Theme::LAYOUT_FIND_MATCH_HIGHLIGHT.a()
        );
    }
}
