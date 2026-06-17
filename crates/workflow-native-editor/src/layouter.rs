//! Custom layouter used by `TextEdit::multiline` to render the editor with
//! per-token syntax highlighting and search match highlighting.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};
use std::collections::HashSet;
use std::sync::Arc;

use super::highlight::{token_color, tokenize_line};

pub const FONT_SIZE: f32 = 14.0;
pub const LINE_HEIGHT: f32 = 20.0;

/// Colors for search match highlighting.
pub use crate::theme::Theme;

// Backward-compat aliases: the layouter has historically exposed
// `MATCH_HIGHLIGHT` / `CURRENT_MATCH_HIGHLIGHT` as `pub const`s. They
// now point at the centralized `Theme` constants so the editor
// painter and the layouter can never drift.
pub const MATCH_HIGHLIGHT: Color32 = Theme::LAYOUT_FIND_MATCH_HIGHLIGHT;
pub const CURRENT_MATCH_HIGHLIGHT: Color32 = Theme::LAYOUT_CURRENT_FIND_MATCH_HIGHLIGHT;

/// A range of bytes to highlight, with whether it's the current match.
#[derive(Clone, Debug)]
pub struct MatchHighlight {
    pub start: usize,
    pub end: usize,
    pub is_current: bool,
}

/// Build a syntax-highlighted `Galley` for the given text, suitable for
/// `TextEdit::layouter`. Each line is tokenized and laid out as a series of
/// `TextFormat`-colored sections.
///
/// `known_functions` is a set of function names that should be highlighted
/// as functions (e.g., from the dynamic FunctionRegistry).
///
/// `highlights` is a list of byte ranges to highlight (search matches).
pub fn layout_flow(
    ui: &egui::Ui,
    text: &str,
    wrap_width: f32,
    known_functions: &HashSet<String>,
    highlights: &[MatchHighlight],
) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.first_row_min_height = LINE_HEIGHT;

    let default_format = TextFormat {
        font_id: FontId::monospace(FONT_SIZE),
        color: Color32::from_gray(220),
        ..Default::default()
    };

    // Build a set of byte positions where highlights start/end
    // for quick lookup during layout.
    let mut highlight_at_byte: Vec<Option<bool>> = vec![None; text.len() + 1];
    for h in highlights {
        let end = h.end.min(text.len());
        for slot in highlight_at_byte[h.start..end].iter_mut() {
            *slot = Some(h.is_current);
        }
    }

    let mut byte_offset = 0usize;
    let lines: Vec<&str> = text.split('\n').collect();
    for (line_idx, raw_line) in lines.iter().enumerate() {
        let tokens = tokenize_line(raw_line, known_functions);
        if tokens.is_empty() {
            job.append("", 0.0, default_format.clone());
        } else {
            for token in tokens {
                let token_start = byte_offset;
                let token_end = byte_offset + token.text.len();

                // Check if this token overlaps with any highlight
                let mut i = token_start;
                while i < token_end {
                    if let Some(is_current) = highlight_at_byte[i] {
                        // Find the end of this highlight run within the token
                        let mut run_end = i + 1;
                        while run_end < token_end && highlight_at_byte[run_end] == Some(is_current)
                        {
                            run_end += 1;
                        }
                        // Append the highlighted segment
                        let seg_start = i - token_start;
                        let seg_end = run_end - token_start;
                        if seg_start > 0 {
                            let mut fmt = default_format.clone();
                            fmt.color = token_color(token.kind);
                            job.append(&token.text[seg_start..seg_end], 0.0, fmt);
                        }
                        let bg = if is_current {
                            CURRENT_MATCH_HIGHLIGHT
                        } else {
                            MATCH_HIGHLIGHT
                        };
                        let mut fmt = default_format.clone();
                        fmt.color = token_color(token.kind);
                        fmt.background = bg;
                        job.append(&token.text[seg_start..seg_end], 0.0, fmt);
                        i = run_end;
                    } else {
                        // Find end of non-highlighted run
                        let mut run_end = i + 1;
                        while run_end < token_end && highlight_at_byte[run_end].is_none() {
                            run_end += 1;
                        }
                        let seg_start = i - token_start;
                        let seg_end = run_end - token_start;
                        let mut fmt = default_format.clone();
                        fmt.color = token_color(token.kind);
                        job.append(&token.text[seg_start..seg_end], 0.0, fmt);
                        i = run_end;
                    }
                }

                byte_offset = token_end;
            }
        }
        // Newline is 1 byte
        byte_offset += 1;
        if line_idx + 1 < lines.len() {
            job.append("\n", 0.0, default_format.clone());
        }
    }

    ui.fonts(|f| f.layout_job(job))
}

#[cfg(test)]
mod theme_tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn match_highlight_aliases_match_theme() {
        assert_eq!(
            crate::layouter::MATCH_HIGHLIGHT,
            Theme::LAYOUT_FIND_MATCH_HIGHLIGHT,
        );
        assert_eq!(
            crate::layouter::CURRENT_MATCH_HIGHLIGHT,
            Theme::LAYOUT_CURRENT_FIND_MATCH_HIGHLIGHT,
        );
    }
}
