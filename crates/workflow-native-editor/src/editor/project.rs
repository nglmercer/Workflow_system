//! Project lifecycle: file open/save, new project, close.
//!
//! Owns every method that mutates `EditorApp` based on a user
//! action that interacts with the file system or with the
//! "current project" concept:
//!
//! - [`EditorApp::run_open_dialog`] / [`EditorApp::load_path_into_editor`]
//!   — open an existing file (or pick one via a native dialog).
//! - [`EditorApp::save_current`] / [`EditorApp::save_as_dialog`]
//!   — write the buffer to disk (with a Save As fallback when
//!   the buffer is untitled).
//! - [`EditorApp::new_untitled`] / [`EditorApp::close_project`]
//!   — start a fresh empty buffer or return to the home screen.
//! - [`EditorApp::handle_home_action`] — dispatch on a click in
//!   the home screen (New File / Open File… / Open recent).
//! - [`EditorApp::title_label`] — render the file name (with a
//!   dirty marker) for the toolbar.

use eframe::egui;

use super::super::cursor::CursorPosition;
use super::super::file_io;
use super::super::history::History;
use super::super::home::HomeAction;
use super::super::EditorApp;
use workflow_i18n::{t as i18n_t, tf as i18n_tf};
use workflow_lsp::features;

impl EditorApp {
    /// Build the title-bar label. Shows the file name (or
    /// `i18n_t("editor.title_untitled")`) with a leading dot when the buffer has unsaved
    /// edits. We use a Unicode bullet rather than `*` to match the
    /// common "modified" indicator in cross-platform editors.
    pub(crate) fn title_label(&self) -> String {
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

    /// Run the native "open" file dialog. Called from
    /// [`Self::update`] when `pending_open_dialog` is set, so it
    /// runs *outside* the egui scope-stack that holds `self`
    /// mutably. The dialog itself is modal; we let the user cancel
    /// without touching the buffer.
    pub(crate) fn run_open_dialog(&mut self) {
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
    pub(crate) fn load_path_into_editor(
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

        // Reload plugins from the project directory (next to the
        // opened file) so plugin functions/objects are available
        // for completions, hover, and diagnostics.
        if let Some(parent) = path_buf.parent() {
            let project_plugin_dir = parent.join("plugins");
            if project_plugin_dir.exists() {
                self.plugin_manager =
                    super::super::plugin_manager::EditorPluginManager::new(&project_plugin_dir);
                self.plugin_manager.load_all();
            }
        }

        self.lsp
            .set_plugin_registry(self.plugin_manager.function_registry().clone());
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
    pub(crate) fn save_current(&mut self) {
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
    pub(crate) fn save_as_dialog(&mut self) {
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
    pub(crate) fn handle_home_action(&mut self, ctx: &egui::Context, action: HomeAction) {
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
    pub(crate) fn new_untitled(&mut self, ctx: &egui::Context) {
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
    pub(crate) fn close_project(&mut self, ctx: &egui::Context) {
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

#[cfg(test)]
mod tests {
    use super::super::super::EditorApp;

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
