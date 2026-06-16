//! Custom layouter used by `TextEdit::multiline` to render the editor with
//! per-token syntax highlighting.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};
use std::collections::HashSet;
use std::sync::Arc;

use super::highlight::{token_color, tokenize_line};

pub const FONT_SIZE: f32 = 14.0;
pub const LINE_HEIGHT: f32 = 20.0;

/// Build a syntax-highlighted `Galley` for the given text, suitable for
/// `TextEdit::layouter`. Each line is tokenized and laid out as a series of
/// `TextFormat`-colored sections.
///
/// `known_functions` is a set of function names that should be highlighted
/// as functions (e.g., from the dynamic FunctionRegistry).
pub fn layout_flow(
    ui: &egui::Ui,
    text: &str,
    wrap_width: f32,
    known_functions: &HashSet<String>,
) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.first_row_min_height = LINE_HEIGHT;

    let default_format = TextFormat {
        font_id: FontId::monospace(FONT_SIZE),
        color: Color32::from_gray(220),
        ..Default::default()
    };

    let lines: Vec<&str> = text.split('\n').collect();
    for (line_idx, raw_line) in lines.iter().enumerate() {
        let tokens = tokenize_line(raw_line, known_functions);
        if tokens.is_empty() {
            job.append("", 0.0, default_format.clone());
        } else {
            for token in tokens {
                let mut format = default_format.clone();
                format.color = token_color(token.kind);
                job.append(&token.text, 0.0, format);
            }
        }
        if line_idx + 1 < lines.len() {
            job.append("\n", 0.0, default_format.clone());
        }
    }

    ui.fonts(|f| f.layout_job(job))
}
