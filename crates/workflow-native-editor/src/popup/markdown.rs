//! Mini-markdown renderer for the hover popup body.
//!
//! The LSP emits a small subset of markdown in its hover output:
//!
//! - `**bold**` labels (e.g. `**type:**`, `**value:**`, `**params:**`)
//! - `` `code` `` spans (e.g. `` `(event "NESTED_DATA")` ``)
//! - `//@type` style type annotations
//! - blank-line separated paragraphs
//!
//! Rather than pull in a full markdown engine we render this subset
//! directly with `LayoutJob`, which gives us full control over colors
//! and avoids any extra dependency.

use eframe::egui::{self, Color32, FontId, Ui};

use super::model::HoverKind;

pub(super) fn render_mini_markdown(ui: &mut Ui, md: &str, kind: HoverKind) {
    let accent = kind.badge_color();
    for paragraph in md.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        let mut job = egui::text::LayoutJob::default();
        let mut chars = paragraph.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                // **bold** -> accent
                '*' if chars.peek() == Some(&'*') => {
                    chars.next();
                    let mut inner = String::new();
                    let mut closed = false;
                    while let Some(&nc) = chars.peek() {
                        chars.next();
                        if nc == '*' && chars.peek() == Some(&'*') {
                            chars.next();
                            closed = true;
                            break;
                        }
                        inner.push(nc);
                    }
                    if closed {
                        let fmt = make_text_format(FontId::monospace(12.0), accent, false);
                        job.append(&inner, 0.0, fmt);
                    } else {
                        job.append(&format!("**{}", inner), 0.0, base_text_format());
                    }
                }
                // *italic* -> weak
                '*' => {
                    let mut inner = String::new();
                    let mut closed = false;
                    while let Some(&nc) = chars.peek() {
                        chars.next();
                        if nc == '*' {
                            closed = true;
                            break;
                        }
                        inner.push(nc);
                    }
                    if closed {
                        let mut fmt = base_text_format();
                        fmt.font_id = FontId::proportional(12.5);
                        fmt.italics = true;
                        fmt.color = Color32::from_gray(180);
                        job.append(&inner, 0.0, fmt);
                    } else {
                        job.append("*", 0.0, base_text_format());
                    }
                }
                // `code` -> monospace + light bg tint
                '`' => {
                    let mut inner = String::new();
                    let mut closed = false;
                    while let Some(&nc) = chars.peek() {
                        chars.next();
                        if nc == '`' {
                            closed = true;
                            break;
                        }
                        inner.push(nc);
                    }
                    if closed {
                        let fmt = make_code_format(Color32::from_rgb(200, 220, 255));
                        job.append(&inner, 0.0, fmt);
                    } else {
                        job.append("`", 0.0, base_text_format());
                    }
                }
                // `//@type` annotation -> monospace accent (only at start of paragraph)
                '/' if job.is_empty() && chars.peek() == Some(&'/') => {
                    let mut annot = String::new();
                    annot.push('/');
                    annot.push('/');
                    chars.next();
                    while let Some(&nc) = chars.peek() {
                        if nc == '\n' {
                            break;
                        }
                        chars.next();
                        annot.push(nc);
                    }
                    let fmt = make_text_format(FontId::monospace(12.0), accent, false);
                    job.append(&annot, 0.0, fmt);
                }
                other => {
                    // Append a single-char run; this could be optimised
                    // by collecting runs but the LSP output is small.
                    job.append(&other.to_string(), 0.0, base_text_format());
                }
            }
        }
        ui.label(job);
        ui.add_space(2.0);
    }
}

fn base_text_format() -> egui::text::TextFormat {
    egui::text::TextFormat {
        font_id: FontId::proportional(12.5),
        color: Color32::from_gray(210),
        ..Default::default()
    }
}

fn make_text_format(font: FontId, color: Color32, italics: bool) -> egui::text::TextFormat {
    egui::text::TextFormat {
        font_id: font,
        color,
        italics,
        ..Default::default()
    }
}

fn make_code_format(color: Color32) -> egui::text::TextFormat {
    egui::text::TextFormat {
        font_id: FontId::monospace(12.0),
        color,
        background: Color32::from_rgba_unmultiplied(60, 80, 110, 90),
        ..Default::default()
    }
}
