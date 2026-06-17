//! Global key handling.
//!
//! [`EditorApp::handle_global_keys`] is called once per frame at
//! the top of `update`. It:
//!
//! 1. Closes transient overlays (find bar, search-in-files,
//!    shortcuts window) on `Esc`.
//! 2. Asks the [`super::super::keybindings::Keymap`] for the
//!    [`super::super::keybindings::Command`] matching the
//!    current key + modifier state.
//! 3. Dispatches that command to the corresponding editor method.
//!
//! The dispatch is a `match` on every [`Command`] variant. Adding
//! a new command is a three-step change: add a variant to
//! `Command`, add a keymap binding, add a dispatch arm here.

use eframe::egui;

use super::super::keybindings::Command;
use super::super::shortcuts_window;
use super::super::EditorApp;
use workflow_i18n::{t as i18n_t, tf as i18n_tf};

impl EditorApp {
    /// Run the global key handlers, then apply the result to editor
    /// state.
    pub(crate) fn handle_global_keys(&mut self, ctx: &egui::Context) {
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
                &[("label", &pending.label())],
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
            Command::Cut => self.cut_selection(ctx),
            Command::Copy => self.copy_selection(ctx),
            Command::Paste => self.paste_clipboard(ctx),
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
}
