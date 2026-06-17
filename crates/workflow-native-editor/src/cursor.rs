//! Cursor position and conversions between line/column and char/byte
//! offsets.
//!
//! The editor speaks two coordinate systems:
//!
//! - **Line/column** (1-based, displayed in the status bar and the
//!   gutter). Columns are in characters (so a tab in column 1 is
//!   column 1, not column 4).
//! - **Char offset** (0-based, the `CCursor` coordinate space used by
//!   `egui::TextEdit`). Offsets count Unicode scalar values.
//! - **Byte offset** (0-based, used for `String` slicing).
//!
//! Conversions live here so the rest of the editor doesn't have to
//! hand-roll the "walk lines, sum char counts" loop every time it
//! needs to go from `(line, col)` to a position in the buffer.

use eframe::egui::{self, Pos2};

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorPosition {
    pub line: usize,
    pub col: usize,
}

impl CursorPosition {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// A selection range expressed as char indices into the buffer.
///
/// `cursor` is the active end (where new typing would land);
/// `anchor` is the fixed end. When `anchor == cursor` the selection
/// is empty. Storing both ends lets undo/redo restore a multi-char
/// selection, not just the caret position.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelectionRange {
    pub anchor: usize,
    pub cursor: usize,
}

impl SelectionRange {
    #[allow(dead_code)]
    pub fn point(at: usize) -> Self {
        Self {
            anchor: at,
            cursor: at,
        }
    }

    #[allow(dead_code)]
    pub fn is_empty(self) -> bool {
        self.anchor == self.cursor
    }

    /// Normalize so `start <= end`.
    pub fn normalized(self) -> (usize, usize) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }
}

/// Convert a 0-based `(line, col)` to a 0-based char offset. If
/// `line` is past the end of the document, returns the total char
/// count. `col` is clamped to the line's char count.
pub fn line_col_to_char(text: &str, line: usize, col: usize) -> usize {
    let total_lines = text.split('\n').count();
    if line >= total_lines {
        return text.chars().count();
    }
    let mut offset = 0usize;
    for (i, current_line) in text.split('\n').enumerate() {
        if i == line {
            return offset + col.min(current_line.chars().count());
        }
        offset += current_line.chars().count() + 1;
    }
    offset
}

/// Convert a 0-based char offset to a 1-based `(line, col)`.
#[allow(dead_code)]
pub fn char_to_line_col(text: &str, char_offset: usize) -> (usize, usize) {
    let mut remaining = char_offset;
    for (i, line) in text.split('\n').enumerate() {
        let len = line.chars().count();
        if remaining <= len {
            return (i + 1, remaining + 1);
        }
        remaining -= len + 1;
    }
    // Past end of document — return the last line and column.
    let last_line = text.split('\n').count();
    (last_line, 1)
}

/// The char offset of the start of the word (or member-access token)
/// immediately before `char_offset`. A "word" is a run of
/// alphanumerics, `_`, or `.` (so `foo.bar.baz` is one word).
pub fn word_start_before(text: &str, char_offset: usize) -> usize {
    let before: String = text.chars().take(char_offset).collect();
    before
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_alphanumeric() && *ch != '_' && *ch != '.')
        .map(|(i, _)| i + 1)
        .unwrap_or(0)
}

/// Convert a `(start, end)` char range into a `(start, end)` byte
/// range. Either end being past the buffer clamps to the buffer
/// length.
pub fn char_range_to_byte_range(text: &str, char_start: usize, char_end: usize) -> (usize, usize) {
    let start_byte = text
        .char_indices()
        .nth(char_start)
        .map(|(b, _)| b)
        .unwrap_or(text.len());
    let end_byte = text
        .char_indices()
        .nth(char_end)
        .map(|(b, _)| b)
        .unwrap_or(text.len());
    (start_byte, end_byte)
}

/// Replace the text in `source` between `char_start` and `char_end`
/// with `replacement`. Returns the new string.
pub fn splice(text: &str, char_start: usize, char_end: usize, replacement: &str) -> String {
    let (start_byte, end_byte) = char_range_to_byte_range(text, char_start, char_end);
    let mut buf = String::with_capacity(text.len() + replacement.len());
    buf.push_str(&text[..start_byte]);
    buf.push_str(replacement);
    buf.push_str(&text[end_byte..]);
    buf
}

/// Compute the screen position of a cursor at `(row, col)` (both
/// 0-based) inside `galley`.
///
/// `galley_pos` is the galley's top-left in screen coordinates (the
/// value reported by `TextEditOutput::galley_pos`); it is added to the
/// row's local offset so the result lands on screen, not in the
/// galley's own `(0, 0)`-rooted coordinate space.
///
/// The galley must have been laid out for the current frame.
pub fn cursor_screen_pos(galley: &egui::Galley, galley_pos: Pos2, row: usize, col: usize) -> Pos2 {
    let local = cursor_galley_pos(galley, row, col);
    galley_pos + local.to_vec2()
}

/// The cursor's position in the galley's own coordinate system
/// (galley `(0, 0)` is its top-left). Returns `Pos2::ZERO` if `row` is
/// past the end of the galley. Used by [`cursor_screen_pos`]; exposed
/// for testing.
pub fn cursor_galley_pos(galley: &egui::Galley, row: usize, col: usize) -> Pos2 {
    if row >= galley.rows.len() {
        return Pos2::ZERO;
    }
    let galley_row = &galley.rows[row];
    let row_min_x = galley_row.rect.min.x;
    let mut cursor_x = row_min_x;
    for (glyph_count, glyph) in galley_row.glyphs.iter().enumerate() {
        if glyph_count >= col {
            break;
        }
        cursor_x = glyph.pos.x + glyph.size.x;
    }
    let cursor_y = galley_row.rect.min.y;
    Pos2::new(cursor_x, cursor_y)
}

/// Find the row in `galley` whose `rect.min.y` is the largest value
/// `<= local_y` (i.e. the row the cursor would be on at the given
/// local Y coordinate).
pub fn row_at_y(galley: &egui::Galley, local_y: f32) -> usize {
    let mut best_y = f32::NEG_INFINITY;
    let mut line_idx = 0;
    for (idx, row) in galley.rows.iter().enumerate() {
        let y = row.rect.min.y;
        if y <= local_y && y >= best_y {
            best_y = y;
            line_idx = idx;
        }
    }
    line_idx
}

/// Find the column whose right edge is the largest value `<=
/// local_x` (relative to the row's left edge). Takes the galley and
/// a row index so the caller doesn't need to name the (non-reexported)
/// `Row` type.
pub fn column_at_x(galley: &egui::Galley, row_idx: usize, local_x: f32) -> usize {
    if row_idx >= galley.rows.len() {
        return 0;
    }
    let row = &galley.rows[row_idx];
    let row_min_x = row.rect.min.x;
    row.glyphs
        .iter()
        .take_while(|g| g.pos.x + g.size.x - row_min_x <= local_x)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_to_char_basic() {
        let text = "abc\ndef\nghi";
        assert_eq!(line_col_to_char(text, 0, 0), 0);
        assert_eq!(line_col_to_char(text, 0, 1), 1);
        assert_eq!(line_col_to_char(text, 1, 0), 4);
        assert_eq!(line_col_to_char(text, 2, 2), 10);
    }

    #[test]
    fn line_col_to_char_clamps_col() {
        let text = "abc\nde";
        // Col past end of line clamps to last char of the line.
        assert_eq!(line_col_to_char(text, 0, 99), 3);
    }

    #[test]
    fn line_col_to_char_past_eof() {
        let text = "abc\ndef";
        // Line past end returns total char count.
        assert_eq!(line_col_to_char(text, 99, 1), text.chars().count());
    }

    #[test]
    fn char_to_line_col_basic() {
        let text = "abc\ndef\nghi";
        assert_eq!(char_to_line_col(text, 0), (1, 1));
        assert_eq!(char_to_line_col(text, 3), (1, 4));
        assert_eq!(char_to_line_col(text, 4), (2, 1));
        assert_eq!(char_to_line_col(text, 7), (2, 4));
        assert_eq!(char_to_line_col(text, 8), (3, 1));
    }

    #[test]
    fn round_trip_line_col() {
        let text = "alpha\nbeta\ngamma";
        let max_cols = [5, 4, 5];
        for (line, &line_cols) in max_cols.iter().enumerate() {
            for col in 0..line_cols {
                let offset = line_col_to_char(text, line, col);
                let (l2, c2) = char_to_line_col(text, offset);
                let l2_0 = l2 - 1;
                let c2_0 = c2 - 1;
                assert_eq!(
                    (l2_0, c2_0),
                    (line, col),
                    "({line},{col}) -> {offset} -> ({l2_0},{c2_0})"
                );
            }
        }
    }

    #[test]
    fn word_start_before_treats_dot_as_word_char() {
        // `.` is treated as part of a word so `data.plan` and
        // `data.pla` form one word. This matches the original editor
        // behavior: accepting a completion after `data.pla` replaces
        // the whole `data.pla`, not just `pla`.
        let text = "foo.bar.baz";
        assert_eq!(word_start_before(text, 7), 0);
        assert_eq!(word_start_before(text, 4), 0);
        assert_eq!(word_start_before(text, 3), 0);
        // A non-word char (space) is a boundary. "foo bar" → word
        // before offset 5 (the `b`) starts at 4 (after the space).
        assert_eq!(word_start_before("foo bar", 5), 4);
    }

    #[test]
    fn word_start_before_at_start() {
        assert_eq!(word_start_before("hello", 0), 0);
    }

    #[test]
    fn splice_replaces_range() {
        let text = "hello world";
        // Replace the space (char 5) with nothing → "helloworld".
        let out = splice(text, 5, 6, "");
        assert_eq!(out, "helloworld");
        // Replace "world" (chars 6..11) with "Rust" → "hello Rust".
        let out = splice(text, 6, 11, "Rust");
        assert_eq!(out, "hello Rust");
    }

    #[test]
    fn selection_range_point_is_empty() {
        let s = SelectionRange::point(5);
        assert!(s.is_empty());
        assert_eq!(s.normalized(), (5, 5));
    }

    #[test]
    fn selection_range_normalized_orders_endpoints() {
        let s = SelectionRange {
            anchor: 10,
            cursor: 4,
        };
        assert_eq!(s.normalized(), (4, 10));
        let s = SelectionRange {
            anchor: 2,
            cursor: 8,
        };
        assert_eq!(s.normalized(), (2, 8));
    }

    /// Lay out a tiny two-line galley with the default font context.
    /// Kept as a helper so the position tests below stay readable.
    fn two_row_galley() -> std::sync::Arc<egui::Galley> {
        let ctx = egui::Context::default();
        // `begin_frame` (called internally by `run`) sets up the font
        // atlas. Without it, `ctx.fonts` panics with "No fonts
        // available until first call to Context::run()".
        ctx.begin_frame(egui::RawInput::default());

        let mut job = egui::text::LayoutJob::default();
        job.append(
            "hi\nworld",
            0.0,
            egui::text::TextFormat {
                font_id: egui::FontId::monospace(13.0),
                ..Default::default()
            },
        );
        ctx.fonts(|f| f.layout_job(job))
    }

    #[test]
    fn cursor_galley_pos_is_local_to_galley_origin() {
        let galley = two_row_galley();
        assert!(
            galley.rows.len() >= 2,
            "expected at least 2 rows, got {}",
            galley.rows.len()
        );

        let row0 = &galley.rows[0];
        let p0 = cursor_galley_pos(&galley, 0, 0);
        assert!(
            (p0.x - row0.rect.min.x).abs() < 0.5,
            "p0.x={}, row0.rect.min.x={}",
            p0.x,
            row0.rect.min.x
        );
        assert!((p0.y - row0.rect.min.y).abs() < 0.5);

        let row1 = &galley.rows[1];
        let p1 = cursor_galley_pos(&galley, 1, 0);
        assert!((p1.x - row1.rect.min.x).abs() < 0.5);
        assert!((p1.y - row1.rect.min.y).abs() < 0.5);
        assert!(p1.y > p0.y, "row 1 must be below row 0");

        // Row past the end falls back to the galley origin.
        assert_eq!(cursor_galley_pos(&galley, 999, 0), Pos2::ZERO);
    }

    /// Regression: the old `cursor_screen_pos` did
    ///     editor_rect.min + Vec2::new(cursor_x - editor_rect.min.x, cursor_y - editor_rect.min.y)
    /// which is a tautology — the `+` and `-` cancel, so the function
    /// returned the galley-local position as if it were a screen
    /// position. The completion popup therefore landed near the galley
    /// origin (the editor's top-left) instead of at the caret.
    #[test]
    fn cursor_screen_pos_adds_galley_pos_not_tautology() {
        let galley = two_row_galley();

        let galley_pos = Pos2::new(500.0, 200.0);
        let screen_pos = cursor_screen_pos(&galley, galley_pos, 1, 0);

        let local = cursor_galley_pos(&galley, 1, 0);
        let expected = galley_pos + local.to_vec2();
        assert!(
            (screen_pos.x - expected.x).abs() < 0.5,
            "screen_pos.x={}, expected.x={}",
            screen_pos.x,
            expected.x
        );
        assert!(
            (screen_pos.y - expected.y).abs() < 0.5,
            "screen_pos.y={}, expected.y={}",
            screen_pos.y,
            expected.y
        );

        // The crucial guarantee: the screen x is at *least* the
        // galley x. Under the old bug it could be as small as the row's
        // local origin (~0), which is what put the popup on top of the
        // gutter.
        assert!(
            screen_pos.x >= galley_pos.x,
            "screen x ({}) must be >= galley x ({})",
            screen_pos.x,
            galley_pos.x
        );
    }

    #[test]
    fn char_range_to_byte_range_handles_ascii() {
        let text = "hello world";
        assert_eq!(char_range_to_byte_range(text, 0, 5), (0, 5));
        assert_eq!(char_range_to_byte_range(text, 6, 11), (6, 11));
        // Empty range at end of text.
        assert_eq!(char_range_to_byte_range(text, 11, 11), (11, 11));
    }

    /// Regression: the find bar pre-fill used to slice
    /// `display_text[char_range]` directly, which is a byte slice
    /// in Rust. With multi-byte UTF-8 (e.g. accented characters in
    /// identifiers or comments) that panics or produces a
    /// corrupted substring. Converting to a byte range first keeps
    /// the selection intact.
    #[test]
    fn char_range_to_byte_range_handles_multibyte() {
        // "café" — 'é' is 2 bytes (0xC3 0xA9).
        let text = "café";
        assert_eq!(text.len(), 5); // 4 chars, 5 bytes
        assert_eq!(text.chars().count(), 4);
        // Selecting the whole word: chars [0, 4) → bytes [0, 5).
        assert_eq!(char_range_to_byte_range(text, 0, 4), (0, 5));
        // Selecting just "fé": chars [2, 4) → bytes [2, 5).
        assert_eq!(char_range_to_byte_range(text, 2, 4), (2, 5));
    }

    /// Selecting past the end of the text clamps to `text.len()`,
    /// which is what `String` indexing would do anyway.
    #[test]
    fn char_range_to_byte_range_clamps_past_end() {
        let text = "abc";
        assert_eq!(char_range_to_byte_range(text, 0, 99), (0, 3));
        assert_eq!(char_range_to_byte_range(text, 5, 10), (3, 3));
    }
}
