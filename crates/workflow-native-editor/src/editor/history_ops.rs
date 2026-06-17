//! Undo / redo / snapshot machinery for the editor.
//!
//! The undo stack itself lives in [`super::super::history::History`]
//! and is exposed through it. What this module owns is the
//! glue between the live [`EditorApp`] state and the
//! `Snapshot` representation:
//!
//! - [`EditorApp::snapshot`] / [`EditorApp::now_ms`] — capture
//!   the current state as a `Snapshot` (or its timestamp key).
//! - [`EditorApp::apply_snapshot`] — restore the live state from
//!   a `Snapshot` and notify the LSP.
//! - [`EditorApp::undo`] / [`EditorApp::redo`] — pop from one
//!   end of the history, push the current state onto the other
//!   end, and apply the result.
//!
//! [`EditorApp::apply_text_edit`] (in `edit_ops`) is the
//! structural-edit counterpart: it calls `snapshot` to capture the
//! pre-edit state, mutates the buffer, and commits the snapshot
//! via `commit_structural`. Every keystroke from the keymap
//! funnels through one of these two paths.

use eframe::egui;

use super::super::cursor;
use super::super::history::Snapshot;
use super::super::EditorApp;
use workflow_lsp::features;

impl EditorApp {
    pub(crate) fn undo(&mut self, ctx: &egui::Context) {
        if let Some(snap) = self.history.undo() {
            // Stash the current live state on `future` so redo can
            // bring it back. The snapshot reflects the state *before*
            // we apply the undo.
            self.history.push_future(self.snapshot(ctx));
            self.apply_snapshot(ctx, snap);
        }
    }

    pub(crate) fn redo(&mut self, ctx: &egui::Context) {
        if let Some(snap) = self.history.redo() {
            // Stash the current live state on `past` so a subsequent
            // undo can return to it. This is the symmetric counterpart
            // of `undo`.
            self.history.push_past(self.snapshot(ctx));
            self.apply_snapshot(ctx, snap);
        }
    }

    pub(crate) fn apply_snapshot(&mut self, ctx: &egui::Context, snap: Snapshot) {
        self.text = snap.text;
        self.cursor = snap.cursor;
        self.selected_text = None;
        self.selected_range = snap.selection;
        self.pending_snippet = snap.pending_snippet;
        // Use the stored snippet anchor directly. Falls back to 0
        // for snapshots recorded by the legacy code (which never
        // stored an anchor).
        self.snippet_anchor = snap.snippet_anchor.unwrap_or(0);
        // Restore the clipboard to whatever was on it at snapshot
        // time, so undo of a cut/paste round-trips the clipboard
        // content too.
        if let Some(cb) = snap.clipboard {
            self.last_clipboard = Some(cb.clone());
            ctx.output_mut(|o| o.copied_text = cb);
        }
        // Compute the char range to push into the TextEdit's
        // internal state. If the snapshot has a selection, use it;
        // otherwise the cursor is a single point.
        self.pending_cursor_char_range = Some(match snap.selection {
            Some(sel) => sel.normalized(),
            None => {
                let char_pos =
                    cursor::line_col_to_char(&self.text, snap.cursor.line - 1, snap.cursor.col - 1);
                (char_pos, char_pos)
            }
        });
        self.lsp.update_document(&self.uri, &self.text);
        self.diagnostics = features::diagnostics_at(&self.lsp, &self.uri);
        // The frame_start was captured before this undo/redo;
        // invalidate it so the next render captures a fresh
        // pre-edit snapshot.
        self.frame_start = None;
        self.completion.dismiss();
    }

    /// Wall-clock time in milliseconds since startup. Used as the
    /// undo-coalescing key.
    pub(crate) fn now_ms(&self, ctx: &egui::Context) -> u128 {
        ctx.input(|i| (i.time * 1000.0) as u128)
    }

    /// Build a snapshot of the *current* live state. Used both to
    /// stash the current state on `future` (before undo) and to seed
    /// the post-edit snapshot for a commit. The `structural` flag is
    /// always `false` here — callers that need a structural snapshot
    /// flip it before passing to `commit_structural`.
    pub(crate) fn snapshot(&self, ctx: &egui::Context) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            selection: self.selected_range,
            pending_snippet: self.pending_snippet.clone(),
            snippet_anchor: Some(self.snippet_anchor),
            clipboard: self.last_clipboard.clone(),
            last_edit_at_ms: self.now_ms(ctx),
            structural: false,
        }
    }
}
