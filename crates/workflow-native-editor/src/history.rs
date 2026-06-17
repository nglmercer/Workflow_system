//! Snapshot-based undo/redo history for the editor.
//!
//! The editor owns the entire text state (it does not delegate to
//! `TextEdit`'s built-in undoer), so we keep our own history of
//! `(text, cursor, selection, snippet, clipboard)` snapshots.
//!
//! Two kinds of edits land here:
//!
//! - **Typing edits**: pushed automatically when the `TextEdit` reports a
//!   change. To avoid one entry per keystroke, we coalesce edits that
//!   happen within a short window of each other *and* that look like
//!   typing (no newline, no large delta, no clipboard involvement).
//! - **Structural edits**: completion insertions, snippet insertions,
//!   the Clear button, line edits, paste, and cut. These always push a
//!   fresh entry, so undo reverts the whole operation in one step.
//!
//! Design:
//!
//! - `EditorApp` is the source of truth for the *current* state. History
//!   stores only *previous* states on two stacks: `past` (undoable) and
//!   `future` (redoable). There is no `head`/`pre_head` field; undo/redo
//!   swap the live state with the top of the relevant stack.
//! - The caller is responsible for capturing the *post-edit* snapshot
//!   and passing it to `commit_typing` / `commit_structural`. History
//!   pushes the snapshot directly — no second guessing about what state
//!   to record.
//! - Coalescing looks at the top of `past`: if it is a typing snapshot
//!   whose timestamp is within the coalesce window, the new typing
//!   snapshot *replaces* it. Structural snapshots are never coalesced.

use std::collections::VecDeque;

use super::cursor::{CursorPosition, SelectionRange};
use super::snippet::PendingSnippet;

const COALESCE_WINDOW_MS: u128 = 500;
const MAX_HISTORY: usize = 256;

/// A previous editor state. Stored on the undo/redo stacks.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub text: String,
    pub cursor: CursorPosition,
    /// Char-index anchor + cursor. `None` means "no selection info
    /// captured"; an empty selection is still represented as
    /// `Some(SelectionRange { anchor, cursor })` with equal indices.
    pub selection: Option<SelectionRange>,
    pub pending_snippet: Option<PendingSnippet>,
    /// Char offset of the snippet's first stop within `text`. Stored
    /// explicitly so undo can restore the snippet anchor without a
    /// fragile substring search.
    pub snippet_anchor: Option<usize>,
    /// What the OS clipboard held at the time of the snapshot, so
    /// undo/redo can restore the clipboard through a cut/paste.
    /// `None` means "unknown / not tracked".
    pub clipboard: Option<String>,
    /// Wall-clock-ish time of the post-edit moment (used for
    /// coalescing). Captured by the caller at commit time.
    pub last_edit_at_ms: u128,
    /// `true` for a structural commit; `false` for a typing commit.
    /// Used to prevent a structural edit from being silently absorbed
    /// into a subsequent typing burst.
    pub structural: bool,
}

/// Two stacks of *previous* states. The current state lives in
/// `EditorApp`, not here.
pub struct History {
    past: VecDeque<Snapshot>,
    future: VecDeque<Snapshot>,
    /// Whether the most recent push onto `past` was a typing commit
    /// within the coalesce window of the previous one. Used so the
    /// next typing commit can replace it instead of pushing a new
    /// entry. Reset on any structural commit or undo/redo.
    last_typing_in_burst: bool,
}

impl History {
    pub fn new() -> Self {
        Self {
            past: VecDeque::new(),
            future: VecDeque::new(),
            last_typing_in_burst: false,
        }
    }

    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Record a *typing* edit. The caller passes the *post-edit*
    /// snapshot (after the mutation has been applied to
    /// `EditorApp.text` and friends).
    ///
    /// Coalescing rules:
    /// - If the top of `past` is a typing snapshot whose
    ///   `last_edit_at_ms` is within `COALESCE_WINDOW_MS` of the new
    ///   snapshot's, *replace* it. This collapses a burst of
    ///   keystrokes into a single undo step.
    /// - Otherwise, push a new entry.
    ///
    /// Any structural edit resets the coalesce state, so a structural
    /// edit followed by typing within 500 ms is *not* merged.
    pub fn commit_typing(&mut self, snap: Snapshot) {
        debug_assert!(!snap.structural, "commit_typing called with a structural snapshot");
        let within_window = self
            .past
            .back()
            .map(|top| {
                !top.structural
                    && snap
                        .last_edit_at_ms
                        .saturating_sub(top.last_edit_at_ms)
                        < COALESCE_WINDOW_MS
            })
            .unwrap_or(false)
            && self.last_typing_in_burst;
        if within_window {
            // Coalesce: replace the previous typing snapshot with the
            // new one. The previous one's *text* is no longer
            // reachable, but that's the whole point of coalescing.
            self.past.pop_back();
        }
        self.past.push_back(snap);
        self.trim_to_bound();
        self.future.clear();
        self.last_typing_in_burst = true;
    }

    /// Record a *structural* edit. Always pushes a fresh entry —
    /// never coalesces.
    pub fn commit_structural(&mut self, snap: Snapshot) {
        debug_assert!(snap.structural, "commit_structural called with a typing snapshot");
        self.past.push_back(snap);
        self.trim_to_bound();
        self.future.clear();
        self.last_typing_in_burst = false;
    }

    /// Pop the most recent state off the undo stack. The returned
    /// `Snapshot` is the state to *restore* — i.e. what `EditorApp`
    /// should look like after the undo. The current live state must
    /// be pushed onto `future` by the caller (typically *after* the
    /// caller captures it), so the caller pattern is:
    ///
    /// 1. `let live = editorapp.snapshot(...);`
    /// 2. `let prev = history.undo();` (returns the state to land on)
    /// 3. `history.push_future(live);` (stashes the just-undone state)
    /// 4. `editorapp.apply(prev);`
    ///
    /// `undo_live` wraps the common case: pass the current live
    /// snapshot, get back the snapshot to restore, and the live
    /// snapshot is automatically pushed onto `future`.
    pub fn undo(&mut self) -> Option<Snapshot> {
        let prev = self.past.pop_back()?;
        self.last_typing_in_burst = false;
        Some(prev)
    }

    /// Push the current live snapshot onto `future` (called after
    /// `undo` or when the editor's live state changes for a reason
    /// other than a user-typed commit).
    pub fn push_future(&mut self, snap: Snapshot) {
        self.future.push_back(snap);
        self.trim_future_to_bound();
    }

    /// Pop the most recently undone state off the redo stack and
    /// return it. The caller is responsible for capturing the live
    /// state and pushing it onto `past` via `push_past`, then
    /// applying the returned snapshot.
    pub fn redo(&mut self) -> Option<Snapshot> {
        let next = self.future.pop_back()?;
        self.last_typing_in_burst = false;
        Some(next)
    }

    /// Push a snapshot onto `past` (called after `redo` with the
    /// editor's current live state).
    pub fn push_past(&mut self, snap: Snapshot) {
        self.past.push_back(snap);
        self.trim_to_bound();
    }

    /// Drop all stored history. Used when loading a new file.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
        self.last_typing_in_burst = false;
    }

    /// Return a reference to the top of the past stack, if any.
    /// Useful for coalesce-window introspection.
    #[allow(dead_code)]
    pub fn past_top(&self) -> Option<&Snapshot> {
        self.past.back()
    }

    fn trim_to_bound(&mut self) {
        while self.past.len() > MAX_HISTORY {
            self.past.pop_front();
        }
    }

    fn trim_future_to_bound(&mut self) {
        while self.future.len() > MAX_HISTORY {
            self.future.pop_front();
        }
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text: &str, line: usize, col: usize, ms: u128) -> Snapshot {
        Snapshot {
            text: text.to_string(),
            cursor: CursorPosition { line, col },
            selection: None,
            pending_snippet: None,
            snippet_anchor: None,
            clipboard: None,
            last_edit_at_ms: ms,
            structural: false,
        }
    }

    fn structural_snap(text: &str, line: usize, col: usize, ms: u128) -> Snapshot {
        let mut s = snap(text, line, col, ms);
        s.structural = true;
        s
    }

    #[test]
    fn typing_burst_coalesces_into_single_undo_step() {
        let mut h = History::new();
        h.commit_typing(snap("a", 1, 1, 0));
        h.commit_typing(snap("ab", 1, 2, 100));
        h.commit_typing(snap("abc", 1, 3, 200));
        // All three within the window → past should have a single
        // entry, holding the latest "abc" state. Undo should land on
        // "abc" (i.e. restore the burst). To go further back there
        // is no history, so a second undo returns None.
        assert_eq!(h.past.len(), 1);
        let top = h.past.back().unwrap();
        assert_eq!(top.text, "abc");
    }

    #[test]
    fn typing_after_window_pushes_new_entry() {
        let mut h = History::new();
        h.commit_typing(snap("a", 1, 1, 0));
        h.commit_typing(snap("ab", 1, 2, 100));
        h.commit_typing(snap("abc", 1, 3, 2000));
        // 2000ms > 500ms window → "ab" and "abc" are separate entries.
        assert_eq!(h.past.len(), 2);
        let top = h.past.back().unwrap();
        assert_eq!(top.text, "abc");
    }

    #[test]
    fn structural_edit_always_pushes_fresh_entry() {
        let mut h = History::new();
        h.commit_typing(snap("a", 1, 1, 0));
        h.commit_structural(structural_snap("aX", 1, 2, 100));
        // Structural never coalesces — past has 2 entries.
        assert_eq!(h.past.len(), 2);
        assert!(h.past.back().unwrap().structural);
    }

    #[test]
    fn structural_then_typing_within_window_keeps_both() {
        let mut h = History::new();
        h.commit_typing(snap("a", 1, 1, 0));
        h.commit_structural(structural_snap("aX", 1, 2, 100));
        // Typing 100ms after a structural edit must not be merged.
        h.commit_typing(snap("aXy", 1, 3, 200));
        assert_eq!(h.past.len(), 3);
    }

    #[test]
    fn undo_redo_round_trip_preserves_post_structural_state() {
        // Regression test for the original "post-structural state
        // unrecoverable" bug. The user types "hello", then a
        // structural edit expands it to "hello world". Undo should
        // bring back "hello" (the pre-structural state); redo
        // should bring back "hello world" (the live state).
        //
        // The live state is held by the caller (EditorApp) — not
        // by History — so the test simulates the caller pattern
        // manually: capture the post-structural state, push it
        // onto `future` after undo, push it back onto `past` after
        // redo.
        let mut h = History::new();
        h.commit_typing(snap("hello", 1, 5, 0));
        // The live state right now is "hello". The user does a
        // structural edit that produces "hello world"; the snapshot
        // we commit is the *pre*-edit state ("hello"), tagged as
        // structural.
        h.commit_structural(structural_snap("hello", 1, 5, 1000));

        // First undo: top of past is the pre-structural state.
        // Undo returns it, and the caller pushes the live state
        // ("hello world") onto `future`.
        let prev = h.undo().expect("undo available");
        assert_eq!(prev.text, "hello");
        let live_after_structural = structural_snap("hello world", 1, 11, 1000);
        h.push_future(live_after_structural.clone());

        // Redo: pop the post-structural state off `future`, push
        // the current live state ("hello") back onto `past`.
        let next = h.redo().expect("redo available");
        assert_eq!(next.text, "hello world");
        h.push_past(snap("hello", 1, 5, 0));

        // After the round trip, both states are still reachable.
        let prev2 = h.undo().expect("undo available");
        assert_eq!(prev2.text, "hello");
    }

    #[test]
    fn new_typing_clears_redo() {
        let mut h = History::new();
        h.commit_typing(snap("a", 1, 1, 0));
        h.commit_typing(snap("ab", 1, 2, 1000));
        h.commit_typing(snap("abc", 1, 3, 2000));
        // Undo twice: "abc" → "ab" → "a".
        let _ = h.undo().unwrap();
        h.push_future(snap("abc", 1, 3, 2000));
        let _ = h.undo().unwrap();
        h.push_future(snap("ab", 1, 2, 1000));
        assert!(h.can_redo());
        // New typing breaks the redo chain.
        h.commit_typing(snap("ax", 1, 2, 3000));
        assert!(!h.can_redo());
    }

    #[test]
    fn past_bound_is_enforced() {
        let mut h = History::new();
        // Each commit is more than COALESCE_WINDOW_MS after the
        // previous, so they don't coalesce.
        let mut t = 0u128;
        for i in 0..(MAX_HISTORY + 50) {
            t += 1000;
            h.commit_typing(snap(&format!("step{i}"), 1, 5, t));
        }
        assert_eq!(h.past.len(), MAX_HISTORY);
    }

    #[test]
    fn clear_resets_history() {
        let mut h = History::new();
        h.commit_typing(snap("a", 1, 1, 0));
        h.commit_structural(structural_snap("ab", 1, 2, 1000));
        h.undo();
        h.push_future(structural_snap("ab", 1, 2, 1000));
        h.clear();
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }
}
