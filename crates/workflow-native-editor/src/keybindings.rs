//! Global key handling for the editor: completion popup navigation and
//! snippet tab-stop navigation.
//!
//! These handlers run once per frame, *before* the central panel, so we can
//! consume the relevant key events with [`egui::InputState::consume_key`]
//! and prevent the embedded `TextEdit` from also seeing them.

use eframe::egui::{self, Key, Modifiers, Ui};

use super::app::CursorPosition;

/// What the user did, as far as global key handling is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Move the completion-popup selection up.
    PopupUp,
    /// Move the completion-popup selection down.
    PopupDown,
    /// Accept the currently-selected completion.
    PopupAccept,
    /// Dismiss the completion popup.
    PopupDismiss,
    /// Advance the active snippet to its next tab stop.
    SnippetAdvance,
    /// Cancel the active snippet.
    SnippetCancel,
    /// Undo the last edit.
    Undo,
    /// Redo the next edit.
    Redo,
    /// No global key was pressed this frame.
    None,
}

/// Inspect the input state and return the action the user took, if any.
/// Consumes the relevant events so the `TextEdit` does not also receive
/// them.
pub fn take_key_action(
    ctx: &egui::Context,
    popup_open: bool,
    has_active_snippet: bool,
) -> KeyAction {
    // Snapshot the keys we care about up front, then mutate state and
    // consume. We can't both iterate `i.events` (immutable) and call
    // `count_and_consume_key` (mutable) on `i` at the same time.
    let mut popup_keys: Vec<(Key, Modifiers)> = Vec::new();
    let mut snippet_keys: Vec<(Key, Modifiers)> = Vec::new();
    let mut undo_keys: Vec<(Key, Modifiers)> = Vec::new();
    let mut redo_keys: Vec<(Key, Modifiers)> = Vec::new();
    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                let is_ctrl = modifiers.ctrl || modifiers.command;
                if popup_open
                    && matches!(
                        key,
                        Key::ArrowDown | Key::ArrowUp | Key::Enter | Key::Tab | Key::Escape
                    )
                {
                    popup_keys.push((*key, *modifiers));
                } else if has_active_snippet && (*key == Key::Tab || *key == Key::Escape) {
                    snippet_keys.push((*key, *modifiers));
                } else if is_ctrl && *key == Key::Z && modifiers.shift {
                    redo_keys.push((*key, *modifiers));
                } else if is_ctrl && *key == Key::Z {
                    undo_keys.push((*key, *modifiers));
                }
            }
        }
    });

    if !popup_keys.is_empty() {
        let mut action = KeyAction::None;
        for (key, _mods) in &popup_keys {
            action = match key {
                Key::ArrowDown => KeyAction::PopupDown,
                Key::ArrowUp => KeyAction::PopupUp,
                Key::Enter | Key::Tab => KeyAction::PopupAccept,
                Key::Escape => KeyAction::PopupDismiss,
                _ => action,
            };
        }
        ctx.input_mut(|i| {
            for (key, mods) in &popup_keys {
                let _ = i.count_and_consume_key(*mods, *key);
            }
        });
        return action;
    }

    if !snippet_keys.is_empty() {
        let mut action = KeyAction::None;
        for (key, _) in &snippet_keys {
            action = match key {
                Key::Tab => KeyAction::SnippetAdvance,
                Key::Escape => KeyAction::SnippetCancel,
                _ => action,
            };
        }
        ctx.input_mut(|i| {
            for (key, mods) in &snippet_keys {
                let _ = i.count_and_consume_key(*mods, *key);
            }
        });
        return action;
    }

    if !undo_keys.is_empty() {
        ctx.input_mut(|i| {
            for (key, mods) in &undo_keys {
                let _ = i.count_and_consume_key(*mods, *key);
            }
        });
        return KeyAction::Undo;
    }

    if !redo_keys.is_empty() {
        ctx.input_mut(|i| {
            for (key, mods) in &redo_keys {
                let _ = i.count_and_consume_key(*mods, *key);
            }
        });
        return KeyAction::Redo;
    }

    KeyAction::None
}

/// Apply a [`KeyAction`] to the editor's completion state, returning the
/// completion index that should be accepted (if any).
pub fn apply_popup_action(
    action: KeyAction,
    completion_visible: &mut bool,
    completion_index: &mut usize,
    max_index: usize,
) -> Option<usize> {
    match action {
        KeyAction::PopupDown => {
            *completion_index = (*completion_index + 1).min(max_index);
            None
        }
        KeyAction::PopupUp => {
            *completion_index = completion_index.saturating_sub(1);
            None
        }
        KeyAction::PopupAccept => {
            *completion_visible = false;
            Some(*completion_index)
        }
        KeyAction::PopupDismiss => {
            *completion_visible = false;
            None
        }
        _ => None,
    }
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
        if prev == '.' {
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
