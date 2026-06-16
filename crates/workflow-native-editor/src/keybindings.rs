//! Global key handling for the editor: completion popup navigation,
//! snippet tab-stop navigation, undo/redo, and editor commands.
//!
//! The keybindings system is a small data-driven [`Keymap`] that maps
//! [`Chord`]s to [`Command`]s. It supports **chords** — a key like
//! `Ctrl+K` followed by `Ctrl+L` is treated as a single command —
//! which lets us mirror VS Code-style multi-key shortcuts without
//! exploding the `Command` enum.
//!
//! These handlers run once per frame, *before* the central panel, so
//! they can consume the relevant key events with
//! [`egui::InputState::consume_key`] and prevent the embedded
//! `TextEdit` from also seeing them.

use eframe::egui::{self, Key, Modifiers, Ui};

use super::cursor::CursorPosition;

/// An editor action. The keymap maps chord sequences to commands;
/// the editor's `handle_global_keys` dispatches on the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    // Popup navigation
    PopupUp,
    PopupDown,
    PopupAccept,
    PopupDismiss,
    // Snippet navigation
    SnippetAdvance,
    SnippetCancel,
    // History
    Undo,
    Redo,
    // File
    Open,
    Save,
    // Edit
    ToggleComment,
    DuplicateLine,
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    Indent,
    Outdent,
    // View
    ToggleFoldAtCursor,
    UnfoldAll,
    // Navigation (stubs: just update `self.status` for now)
    Find,
    GotoLine,
    /// Toggle the keyboard-shortcuts help window.
    ShowShortcuts,
    /// Run tests for the current file.
    RunTests,
    /// No command was triggered.
    None,
}

impl Command {
    /// A short human-readable description of what the command does.
    /// Used by the shortcuts help window.
    pub fn description(self) -> &'static str {
        match self {
            Command::PopupUp => "Completion popup: previous item",
            Command::PopupDown => "Completion popup: next item",
            Command::PopupAccept => "Completion popup: accept item",
            Command::PopupDismiss => "Completion popup: dismiss",
            Command::SnippetAdvance => "Snippet: next tab stop",
            Command::SnippetCancel => "Snippet: cancel",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::Open => "Open file",
            Command::Save => "Save",
            Command::ToggleComment => "Toggle line comment",
            Command::DuplicateLine => "Duplicate line",
            Command::DeleteLine => "Delete line",
            Command::MoveLineUp => "Move line up",
            Command::MoveLineDown => "Move line down",
            Command::Indent => "Indent line",
            Command::Outdent => "Outdent line",
            Command::ToggleFoldAtCursor => "Toggle fold at cursor",
            Command::UnfoldAll => "Unfold all",
            Command::Find => "Find",
            Command::GotoLine => "Go to line",
            Command::ShowShortcuts => "Show keyboard shortcuts",
            Command::RunTests => "Run tests",
            Command::None => "(no command)",
        }
    }
}

/// A keyboard chord: a single key with a set of modifiers. The
/// modifiers are platform-aware: `command` is set on macOS, `ctrl`
/// on Linux/Windows. The keymap treats `ctrl || command` as the
/// primary modifier (a single "command key" regardless of OS) and
/// also requires `shift` to match exactly when set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub ctrl_or_cmd: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub fn key(key: Key) -> Self {
        Self {
            key,
            ctrl_or_cmd: false,
            shift: false,
            alt: false,
        }
    }

    pub fn ctrl(key: Key) -> Self {
        Self {
            key,
            ctrl_or_cmd: true,
            shift: false,
            alt: false,
        }
    }

    pub fn ctrl_shift(key: Key) -> Self {
        Self {
            key,
            ctrl_or_cmd: true,
            shift: true,
            alt: false,
        }
    }

    pub fn alt(key: Key) -> Self {
        Self {
            key,
            ctrl_or_cmd: false,
            shift: false,
            alt: true,
        }
    }

    pub fn alt_shift(key: Key) -> Self {
        Self {
            key,
            ctrl_or_cmd: false,
            shift: true,
            alt: true,
        }
    }

    /// Does this chord match a `Key` + `Modifiers` pair from egui?
    fn matches(&self, key: Key, mods: Modifiers) -> bool {
        if self.key != key {
            return false;
        }
        let cmd = mods.ctrl || mods.command;
        if self.ctrl_or_cmd != cmd {
            return false;
        }
        if self.shift != mods.shift {
            return false;
        }
        if self.alt != mods.alt {
            return false;
        }
        true
    }
}

/// A keymap: an ordered list of `(ChordMatcher, Command)` pairs.
/// The first matching entry wins. The matcher supports both *exact*
/// (single-key) chords and *prefix* chords (the first key of a
/// two-key sequence like `Ctrl+K Ctrl+L`).
#[derive(Default)]
pub struct Keymap {
    entries: Vec<(ChordMatcher, Command)>,
    /// A pending chord prefix: when the user types the first key of a
    /// two-key sequence, we remember it and wait for the next key.
    /// Cleared if a non-matching key arrives.
    pending: Option<Chord>,
    /// True if a prefix match consumed a key event this frame. The
    /// caller should redraw the editor to show the pending state in
    /// the status bar.
    pending_consumed: bool,
}

/// A keymap entry's chord matcher. Most entries are `Exact` (a single
/// chord maps to a command). `Chord` entries represent a *complete*
/// two-key sequence — when the user types the prefix, the first key
/// event is consumed and stashed as a pending prefix; the next key
/// event is then matched against the suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordMatcher {
    Exact(Chord),
    /// A two-key chord: the prefix has already been matched (and its
    /// event consumed); this matcher is checked against the next key
    /// event. The `prefix` field is for sanity-checking that we're
    /// resolving the right prefix; in practice the resolver only
    /// looks at `suffix`.
    Chord {
        prefix: Chord,
        suffix: Chord,
    },
}

impl Keymap {
    /// Iterate the keymap as `(binding_label, command)` pairs,
    /// suitable for rendering in a help window. Entries that share
    /// a chord prefix (e.g. `Ctrl+K Ctrl+L` and `Ctrl+K Ctrl+J`)
    /// are flattened into a single `Ctrl+K Ctrl+L`-style label. The
    /// order matches the keymap's insertion order.
    pub fn bindings(&self) -> Vec<(String, Command)> {
        let mut out: Vec<(String, Command)> = Vec::new();
        for (matcher, cmd) in &self.entries {
            if matches!(cmd, Command::None) {
                continue;
            }
            let label = match matcher {
                ChordMatcher::Exact(c) => chord_label(*c),
                ChordMatcher::Chord { prefix, suffix } => {
                    format!("{} {}", chord_label(*prefix), chord_label(*suffix))
                }
            };
            out.push((label, *cmd));
        }
        out
    }

    /// Build the default keymap. New bindings are added here and
    /// nowhere else.
    pub fn new() -> Self {
        let entries: Vec<(ChordMatcher, Command)> = vec![
            // --- Popup navigation (only active when popup is open) ---
            // These are single-key commands. They were previously
            // declared as `Prefix` chords, which caused the keymap
            // to stash the key as a pending chord prefix and return
            // `Command::None` — the popup could not be navigated or
            // dismissed. `Exact` makes them fire immediately while
            // still being gated by `popup_open` in `gated()`.
            (
                ChordMatcher::Exact(Chord::key(Key::ArrowDown)),
                Command::PopupDown,
            ),
            (
                ChordMatcher::Exact(Chord::key(Key::ArrowUp)),
                Command::PopupUp,
            ),
            (
                ChordMatcher::Exact(Chord::key(Key::Enter)),
                Command::PopupAccept,
            ),
            (
                ChordMatcher::Exact(Chord::key(Key::Tab)),
                Command::PopupAccept,
            ),
            (
                ChordMatcher::Exact(Chord::key(Key::Escape)),
                Command::PopupDismiss,
            ),
            // --- Undo / redo ---
            // --- Snippet navigation (gated by `snippet_active`) ---
            (
                ChordMatcher::Exact(Chord::key(Key::Tab)),
                Command::SnippetAdvance,
            ),
            (
                ChordMatcher::Exact(Chord::key(Key::Escape)),
                Command::SnippetCancel,
            ),
            (ChordMatcher::Exact(Chord::ctrl(Key::Z)), Command::Undo),
            (
                ChordMatcher::Exact(Chord::ctrl_shift(Key::Z)),
                Command::Redo,
            ),
            (ChordMatcher::Exact(Chord::ctrl(Key::Y)), Command::Redo),
            // --- File ---
            (ChordMatcher::Exact(Chord::ctrl(Key::O)), Command::Open),
            (ChordMatcher::Exact(Chord::ctrl(Key::S)), Command::Save),
            // --- Edit ---
            (
                ChordMatcher::Exact(Chord::ctrl(Key::Slash)),
                Command::ToggleComment,
            ),
            (
                ChordMatcher::Exact(Chord::alt_shift(Key::ArrowDown)),
                Command::DuplicateLine,
            ),
            (
                ChordMatcher::Exact(Chord::alt_shift(Key::ArrowUp)),
                Command::DuplicateLine,
            ),
            (
                ChordMatcher::Exact(Chord::ctrl_shift(Key::K)),
                Command::DeleteLine,
            ),
            (
                ChordMatcher::Exact(Chord::alt(Key::ArrowDown)),
                Command::MoveLineDown,
            ),
            (
                ChordMatcher::Exact(Chord::alt(Key::ArrowUp)),
                Command::MoveLineUp,
            ),
            (
                ChordMatcher::Exact(Chord::ctrl(Key::CloseBracket)),
                Command::Indent,
            ),
            (
                ChordMatcher::Exact(Chord::ctrl(Key::OpenBracket)),
                Command::Outdent,
            ),
            // --- View (chord examples) ---
            // `Ctrl+K Ctrl+L` toggles the fold at the cursor.
            // `Ctrl+K Ctrl+J` unfolds all regions.
            (
                ChordMatcher::Chord {
                    prefix: Chord::ctrl(Key::K),
                    suffix: Chord::ctrl(Key::L),
                },
                Command::ToggleFoldAtCursor,
            ),
            (
                ChordMatcher::Chord {
                    prefix: Chord::ctrl(Key::K),
                    suffix: Chord::ctrl(Key::J),
                },
                Command::UnfoldAll,
            ),
            // --- Navigation stubs ---
            (ChordMatcher::Exact(Chord::ctrl(Key::F)), Command::Find),
            (ChordMatcher::Exact(Chord::ctrl(Key::G)), Command::GotoLine),
            // --- Help ---
            (
                ChordMatcher::Exact(Chord::key(Key::F1)),
                Command::ShowShortcuts,
            ),
            // --- Tests ---
            (ChordMatcher::Exact(Chord::ctrl(Key::T)), Command::RunTests),
        ];
        Self {
            entries,
            pending: Option::None,
            pending_consumed: false,
        }
    }

    /// The currently-pending chord prefix, if any. The editor can
    /// display this in the status bar so the user knows the keymap
    /// is waiting for the second key of a chord.
    pub fn pending(&self) -> Option<Chord> {
        self.pending
    }

    /// True if this frame consumed a key event to set a chord
    /// prefix. The editor uses this to request a repaint so the
    /// status-bar indicator appears immediately.
    pub fn took_prefix(&self) -> bool {
        self.pending_consumed
    }

    /// Inspect the input state and return the command the user
    /// triggered this frame, if any. Consumes the relevant events so
    /// the `TextEdit` does not also receive them. `popup_open` and
    /// `snippet_active` gate popup/snippet commands: those commands
    /// are silently ignored (and do not consume keys) when their
    /// gate is closed, so a bare `Tab` still inserts a tab.
    pub fn take_command(
        &mut self,
        ctx: &egui::Context,
        popup_open: bool,
        snippet_active: bool,
    ) -> Command {
        self.pending_consumed = false;

        // If we have a pending chord, the next key press should
        // resolve it. Look only at the entries that match the
        // pending chord.
        if let Some(pending) = self.pending.take() {
            let resolved = self.resolve_chord(ctx, &pending);
            if let Some(cmd) = resolved {
                return cmd;
            }
            // No match — the pending key falls through. Don't
            // consume; the user just hit an unrelated key.
        }

        // No pending prefix (or it just expired). Look for a new
        // prefix or exact match.
        self.collect_new(ctx, popup_open, snippet_active)
    }

    fn resolve_chord(&mut self, ctx: &egui::Context, pending: &Chord) -> Option<Command> {
        let events: Vec<(Key, Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });

        for (key, mods) in &events {
            for (matcher, cmd) in &self.entries {
                if let ChordMatcher::Chord { prefix, suffix } = matcher {
                    if *prefix == *pending && suffix.matches(*key, *mods) {
                        consume(ctx, *key, *mods);
                        return Some(*cmd);
                    }
                }
            }
        }
        // No chord match: clear the pending prefix so the next event
        // is processed normally.
        None
    }

    fn collect_new(
        &mut self,
        ctx: &egui::Context,
        popup_open: bool,
        snippet_active: bool,
    ) -> Command {
        let events: Vec<(Key, Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });

        for (key, mods) in &events {
            for (matcher, cmd) in &self.entries {
                if !gated(*cmd, popup_open, snippet_active) {
                    continue;
                }
                match matcher {
                    ChordMatcher::Exact(c) => {
                        if c.matches(*key, *mods) {
                            consume(ctx, *key, *mods);
                            return *cmd;
                        }
                    }
                    ChordMatcher::Chord { prefix, .. } => {
                        // The first key of a `Chord` entry is itself
                        // a prefix. We set it as pending so the
                        // next frame's `resolve_chord` can match
                        // against the suffix.
                        if prefix.matches(*key, *mods) {
                            consume(ctx, *key, *mods);
                            self.pending = Some(*prefix);
                            self.pending_consumed = true;
                            return Command::None;
                        }
                    }
                }
            }
        }
        Command::None
    }
}

/// True if `cmd` is allowed to fire given the current gates.
/// Popup commands only fire when the popup is open; snippet
/// commands only fire when a snippet is active. All other
/// commands are unconditional.
fn gated(cmd: Command, popup_open: bool, snippet_active: bool) -> bool {
    match cmd {
        Command::PopupUp | Command::PopupDown | Command::PopupAccept | Command::PopupDismiss => {
            popup_open
        }
        Command::SnippetAdvance | Command::SnippetCancel => snippet_active,
        _ => true,
    }
}

fn consume(ctx: &egui::Context, key: Key, mods: Modifiers) {
    ctx.input_mut(|i| {
        let _ = i.count_and_consume_key(mods, key);
    });
}

/// Render a chord as a short human-readable label, e.g. "Ctrl+Shift+Z".
/// Used by the shortcuts help window.
pub fn chord_label(c: Chord) -> String {
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

/// Decide whether a freshly-typed character should trigger a completion
/// request. We trigger on word characters, on `.` for member access, and
/// on Ctrl/Cmd+Space for an explicit request.
pub fn should_request_completion(ui: &Ui, text: &str, cursor: CursorPosition) -> bool {
    let line = match text.lines().nth(cursor.line - 1) {
        Some(l) => l,
        None => return false,
    };
    let col = cursor.col.saturating_sub(1).min(line.len());
    let bytes = line.as_bytes();

    if col > 0 {
        let prev = bytes[col - 1] as char;
        if prev.is_ascii_alphanumeric() || prev == '_' {
            return true;
        }
        if prev == '.' || prev == '@' {
            return true;
        }
    }

    ui.input(|i| {
        i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::Space,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.ctrl || modifiers.command
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Context;

    #[test]
    fn chord_matches_single_key() {
        let c = Chord::ctrl(Key::Z);
        let mods = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert!(c.matches(Key::Z, mods));
        assert!(!c.matches(Key::Y, mods));
    }

    #[test]
    fn chord_matches_command_modifier() {
        // On macOS, modifiers.command is true instead of ctrl.
        let c = Chord::ctrl(Key::S);
        let mods = Modifiers {
            command: true,
            ..Default::default()
        };
        assert!(c.matches(Key::S, mods));
    }

    #[test]
    fn chord_requires_exact_shift() {
        let c = Chord::ctrl_shift(Key::Z);
        let ctrl_only = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        // shift is required
        assert!(!c.matches(Key::Z, ctrl_only));
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert!(c.matches(Key::Z, ctrl_shift));
    }

    #[test]
    fn default_keymap_has_undo_redo() {
        let km = Keymap::new();
        let has = |cmd: Command| km.entries.iter().any(|(_, c)| *c == cmd);
        assert!(has(Command::Undo));
        assert!(has(Command::Redo));
        assert!(has(Command::Save));
        assert!(has(Command::Open));
        assert!(has(Command::ToggleComment));
        assert!(has(Command::MoveLineUp));
        assert!(has(Command::DuplicateLine));
        assert!(has(Command::DeleteLine));
    }

    #[test]
    fn default_keymap_has_fold_chord() {
        // `Ctrl+K Ctrl+L` is implemented as a `Chord` entry whose
        // prefix is `Ctrl+K` and whose suffix is `Ctrl+L`.
        let km = Keymap::new();
        let chord_entry = km
            .entries
            .iter()
            .find(|(_, c)| *c == Command::ToggleFoldAtCursor);
        let is_chord_k_l = matches!(
            chord_entry,
            Some((
                ChordMatcher::Chord {
                    prefix: Chord {
                        ctrl_or_cmd: true,
                        key: Key::K,
                        ..
                    },
                    suffix: Chord {
                        ctrl_or_cmd: true,
                        key: Key::L,
                        ..
                    }
                },
                _
            ))
        );
        assert!(
            is_chord_k_l,
            "expected Ctrl+K Ctrl+L → ToggleFoldAtCursor, got {:?}",
            chord_entry
        );
    }

    // `Context` is hard to construct in a unit test, so we only
    // exercise the pure data parts of the keymap above. The
    // integration with the editor is covered by manual testing and
    // the egui harness.
    fn _ctx_compiles(_: &Context) {}

    /// Regression test: popup-navigation commands are single-key
    /// commands and must be declared as `Exact` chords. They were
    /// previously declared as `Prefix`, which caused the keymap
    /// to stash the key as a pending chord prefix and return
    /// `Command::None` — `Esc` did not dismiss the popup, arrow
    /// keys did not navigate it, and `Enter`/`Tab` did not accept
    /// it. This test pins the binding shape so the bug does not
    /// regress.
    #[test]
    fn popup_commands_are_exact_chords() {
        let km = Keymap::new();
        let popup_keys = [
            (Key::ArrowDown, Command::PopupDown),
            (Key::ArrowUp, Command::PopupUp),
            (Key::Enter, Command::PopupAccept),
            (Key::Tab, Command::PopupAccept),
            (Key::Escape, Command::PopupDismiss),
        ];
        for (key, cmd) in popup_keys {
            let entry = km
                .entries
                .iter()
                .find(|(m, c)| *c == cmd && matcher_key(*m) == Some(key));
            let exact = matches!(entry, Some((ChordMatcher::Exact(_), _)));
            assert!(
                exact,
                "expected {:?} → {:?} to be an Exact chord, got {:?}",
                key, cmd, entry
            );
        }
    }

    fn matcher_key(m: ChordMatcher) -> Option<Key> {
        match m {
            ChordMatcher::Exact(c) => Some(c.key),
            ChordMatcher::Chord { suffix, .. } => Some(suffix.key),
        }
    }

    #[test]
    fn chord_label_basic() {
        assert_eq!(chord_label(Chord::ctrl(Key::S)), "Ctrl+S");
    }

    #[test]
    fn chord_label_with_shift_and_alt() {
        assert_eq!(chord_label(Chord::alt_shift(Key::Z)), "Alt+Shift+Z");
    }

    #[test]
    fn chord_label_plain_key() {
        assert_eq!(chord_label(Chord::key(Key::F1)), "F1");
    }

    #[test]
    fn bindings_includes_chord_expansion() {
        let km = Keymap::new();
        let labels: Vec<String> = km.bindings().into_iter().map(|(l, _)| l).collect();
        // The Ctrl+K Ctrl+L fold toggle must surface as a single
        // combined label, not as a bare `Ctrl+K` prefix entry.
        assert!(
            labels.iter().any(|l| l == "Ctrl+K Ctrl+L"),
            "expected 'Ctrl+K Ctrl+L' in bindings, got {:?}",
            labels
        );
    }

    #[test]
    fn bindings_includes_f1() {
        let km = Keymap::new();
        assert!(
            km.bindings()
                .iter()
                .any(|(l, c)| l == "F1" && *c == Command::ShowShortcuts),
            "F1 should map to ShowShortcuts"
        );
    }

    #[test]
    fn bindings_have_descriptions() {
        let km = Keymap::new();
        for (label, cmd) in km.bindings() {
            assert!(!label.is_empty(), "empty label for {:?}", cmd);
            assert!(
                !cmd.description().is_empty(),
                "empty description for {}",
                label
            );
        }
    }
}
