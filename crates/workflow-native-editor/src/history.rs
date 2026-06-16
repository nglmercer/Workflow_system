//! Snapshot-based undo/redo history for the editor.
//!
//! The editor owns the entire text state (it does not delegate to
//! `TextEdit`'s built-in undoer), so we keep our own history of
//! `(text, cursor, pending_snippet)` snapshots.
//!
//! Two kinds of edits land here:
//!
//! - **Typing edits**: pushed automatically when the `TextEdit` reports a
//!   change. To avoid one entry per keystroke, we coalesce edits that
//!   happen within a short window of each other.
//! - **Structural edits**: completion insertions, snippet insertions, and
//!   the Clear button. These always push a fresh entry, so undo reverts
//!   the whole operation in one step.
//!
//! The model:
//!
//! ```text
//!     past (undoable snapshots)        head (current)   pre_head
//!     [ state A, state B, state C ]    state D          state C
//! ```
//!
//! - `pre_head` is the snapshot the user would return to on undo. It
//!   matches the top of `past` for typing bursts, and is set explicitly
//!   for structural edits.
//! - `head` is the current state. After undo, the previous head goes on
//!   `future` and `head` becomes whatever was on top of `past`.

use super::cursor::CursorPosition;
use super::snippet::PendingSnippet;

const COALESCE_WINDOW_MS: u128 = 500;
const MAX_HISTORY: usize = 256;

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub text: String,
    pub cursor: CursorPosition,
    pub pending_snippet: Option<PendingSnippet>,
    pub last_edit_at_ms: u128,
}

pub struct History {
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
    head: Option<Snapshot>,
    pre_head: Option<Snapshot>,
}

impl History {
    pub fn new() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            head: None,
            pre_head: None,
        }
    }

    /// Take a snapshot of the *current* state, returning a `Current` that
    /// can be committed as either typing or structural.
    pub fn snapshot(&mut self, current: Snapshot) -> Current<'_> {
        Current {
            history: self,
            current,
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

    /// Pop the most recent state off the undo stack.
    pub fn undo(&mut self) -> Option<Snapshot> {
        let snapshot = self.past.pop()?;
        if let Some(head) = self.head.take() {
            self.future.push(head);
        }
        // Don't push `snapshot` onto future — that's the state we're
        // restoring *to*, not the state we're moving away from. The
        // current head (now on top of future) is what redo will bring
        // back.
        self.head = self.pre_head.take();
        Some(snapshot)
    }

    /// Pop the most recently undone state off the redo stack.
    pub fn redo(&mut self) -> Option<Snapshot> {
        let snapshot = self.future.pop()?;
        if let Some(head) = self.head.take() {
            self.past.push(head);
        }
        self.past.push(snapshot.clone());
        self.head = Some(snapshot.clone());
        // The pre_head for the redone edit is whatever the past top is
        // — but that has just changed. Leave pre_head None for now;
        // the next edit will refresh it.
        self.pre_head = None;
        Some(snapshot)
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Current<'a> {
    history: &'a mut History,
    current: Snapshot,
}

impl<'a> Current<'a> {
    /// Record a *typing* edit. The current snapshot becomes the new
    /// `head`. The previous head is pushed to `past` only if the
    /// previous head was a *new* edit group (i.e. its timestamp is at
    /// least `COALESCE_WINDOW_MS` in the past). This way a stream of
    /// typing within 500 ms collapses into a single undo step.
    pub fn commit_typing(&mut self) {
        let now = self.current.last_edit_at_ms;
        if let Some(prev_head) = self.history.head.take() {
            if now.saturating_sub(prev_head.last_edit_at_ms) >= COALESCE_WINDOW_MS {
                // The previous edit group is "done". Promote it to past.
                let pre = self
                    .history
                    .pre_head
                    .take()
                    .unwrap_or_else(|| prev_head.clone());
                push_bounded(&mut self.history.past, pre);
            }
            // else: drop prev_head, the new head supersedes it as a
            // continuation of the same burst.
        }
        // The new head's "pre" is the previous head's pre (or the
        // previous head itself on the first commit).
        if self.history.pre_head.is_none() {
            // No pre_head yet — this is the first commit, so the
            // pre_head is the initial state which we don't have. We
            // leave it None; undo will gracefully return None.
        }
        self.history.head = Some(self.current.clone());
        self.history.future.clear();
    }

    /// Record a *structural* edit (completion, snippet, Clear). Always
    /// pushes a fresh entry, even if it would otherwise coalesce.
    pub fn commit_structural(&mut self) {
        if let Some(prev_head) = self.history.head.take() {
            let pre = self
                .history
                .pre_head
                .take()
                .unwrap_or_else(|| prev_head.clone());
            push_bounded(&mut self.history.past, pre);
        }
        let pre = self.current.clone();
        self.history.pre_head = Some(pre);
        self.history.head = Some(self.current.clone());
        self.history.future.clear();
    }

    /// Drop the current snapshot without recording it.
    #[allow(dead_code)]
    pub fn discard(self) {}
}

fn push_bounded(stack: &mut Vec<Snapshot>, snap: Snapshot) {
    stack.push(snap);
    while stack.len() > MAX_HISTORY {
        stack.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text: &str, line: usize, col: usize, ms: u128) -> Snapshot {
        Snapshot {
            text: text.to_string(),
            cursor: CursorPosition { line, col },
            pending_snippet: None,
            last_edit_at_ms: ms,
        }
    }

    #[test]
    fn typing_coalesces_within_window() {
        let mut h = History::new();
        h.snapshot(snap("a", 1, 1, 0)).commit_typing();
        h.snapshot(snap("ab", 1, 2, 100)).commit_typing();
        h.snapshot(snap("abc", 1, 3, 200)).commit_typing();
        // All three should be coalesced into one head. Undo should
        // restore us to the state *before* the burst — which we
        // never recorded, so undo returns None. But the head should
        // hold "abc" and future should be empty (nothing undone).
        assert!(h.undo().is_none());
        let head = h.head_for_test().expect("head");
        assert_eq!(head.text, "abc");
    }

    #[test]
    fn typing_after_pause_pushes_undo_entry() {
        let mut h = History::new();
        h.snapshot(snap("a", 1, 1, 0)).commit_typing();
        h.snapshot(snap("ab", 1, 2, 100)).commit_typing();
        h.snapshot(snap("abc", 1, 3, 200)).commit_typing();
        // Pause > 500ms, then type again.
        h.snapshot(snap("abcd", 1, 4, 2000)).commit_typing();
        // Undo should now return "abc" (the head before this edit).
        let s = h.undo().expect("undo available");
        assert_eq!(s.text, "abc");
    }

    #[test]
    fn structural_edit_always_pushes() {
        let mut h = History::new();
        h.snapshot(snap("a", 1, 1, 0)).commit_typing();
        h.snapshot(snap("b", 1, 1, 100)).commit_structural();
        // After structural, undo should return to the pre-structural
        // state ("a").
        let s = h.undo().expect("undo available");
        assert_eq!(s.text, "a");
    }

    #[test]
    fn redo_round_trip() {
        let mut h = History::new();
        h.snapshot(snap("a", 1, 1, 0)).commit_typing();
        h.snapshot(snap("ab", 1, 2, 100)).commit_typing();
        h.snapshot(snap("abc", 1, 3, 2000)).commit_typing();
        let _ = h.undo().unwrap();
        let s = h.redo().unwrap();
        assert_eq!(s.text, "abc");
    }

    #[test]
    fn new_typing_clears_redo() {
        let mut h = History::new();
        h.snapshot(snap("a", 1, 1, 0)).commit_typing();
        h.snapshot(snap("ab", 1, 2, 100)).commit_typing();
        h.snapshot(snap("abc", 1, 3, 2000)).commit_typing();
        let _ = h.undo().unwrap();
        // New typing breaks the redo chain.
        h.snapshot(snap("abcd", 1, 4, 3000)).commit_typing();
        assert!(!h.can_redo());
    }

    impl History {
        #[cfg(test)]
        fn head_for_test(&self) -> Option<&Snapshot> {
            self.head.as_ref()
        }
    }
}
