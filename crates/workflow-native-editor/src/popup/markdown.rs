//! Mini-markdown renderer for the hover popup body.
//!
//! The LSP emits a small subset of markdown in its hover output:
//!
//! - `**bold**` labels (e.g. `**type:**`, `**value:**`, `**params:**`)
//! - `` `code` `` spans (e.g. `` `(event "NESTED_DATA")` ``)
//! - `//@type` style type annotations
//! - blank-line separated paragraphs
//!
//! Colors are pulled from the [`crate::theme::Theme`] palette so the
//! body chrome stays consistent with the rest of the editor.

use eframe::egui::{self, Color32, FontId, Ui};

use super::model::HoverKind;
use crate::theme::Theme;

pub(super) fn render_mini_markdown(ui: &mut Ui, md: &str, kind: HoverKind) {
    let accent = Theme::hover_badge(kind);
    for paragraph in md.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        let mut job = egui::text::LayoutJob::default();
        let mut chars = paragraph.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
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
                        fmt.color = Theme::hover_italic();
                        job.append(&inner, 0.0, fmt);
                    } else {
                        job.append("*", 0.0, base_text_format());
                    }
                }
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
                        let fmt = make_code_format(Theme::hover_code_text());
                        job.append(&inner, 0.0, fmt);
                    } else {
                        job.append("`", 0.0, base_text_format());
                    }
                }
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
        color: Theme::hover_base_text(),
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
        background: Theme::hover_code_bg(),
        ..Default::default()
    }
}
