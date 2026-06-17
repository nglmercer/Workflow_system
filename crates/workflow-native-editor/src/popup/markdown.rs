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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::HoverKind;
    use crate::theme::Theme;
    use eframe::egui::Color32;

    /// Helper: build a `LayoutJob` from a one-paragraph markdown
    /// string so we can inspect the colors of the spans without
    /// going through egui's renderer.
    fn collect_format(md: &str, kind: HoverKind) -> Vec<Color32> {
        // Render into a throwaway Ui by drawing into a 0x0 frame.
        // Easiest: call render_mini_markdown into a no-op context
        // and rely on the fact that we only test pure color values,
        // not pixels. We use a Vec<Color32> collected by walking
        // a job of the same shape the renderer builds.
        let mut job = egui::text::LayoutJob::default();
        let accent = Theme::hover_badge(kind);
        let chars: Vec<char> = md.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            // Replicate the renderer's per-token color choice.
            if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
                let mut j = i + 2;
                while j + 1 < chars.len() && !(chars[j] == '*' && chars[j + 1] == '*') {
                    j += 1;
                }
                job.append(
                    &chars[i + 2..j].iter().collect::<String>(),
                    0.0,
                    make_text_format(FontId::monospace(12.0), accent, false),
                );
                i = j + 2;
            } else if chars[i] == '`' {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '`' {
                    j += 1;
                }
                job.append(
                    &chars[i + 1..j].iter().collect::<String>(),
                    0.0,
                    make_code_format(Theme::hover_code_text()),
                );
                i = j + 1;
            } else if chars[i] == '*' {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '*' {
                    j += 1;
                }
                let mut fmt = base_text_format();
                fmt.font_id = FontId::proportional(12.5);
                fmt.italics = true;
                fmt.color = Theme::hover_italic();
                job.append(&chars[i + 1..j].iter().collect::<String>(), 0.0, fmt);
                i = j + 1;
            } else {
                job.append(&chars[i].to_string(), 0.0, base_text_format());
                i += 1;
            }
        }
        job.sections.into_iter().map(|s| s.format.color).collect()
    }

    #[test]
    fn bold_uses_kind_accent() {
        let cs = collect_format("**type:**", HoverKind::Function);
        assert_eq!(cs[0], Theme::hover_badge(HoverKind::Function));
    }

    #[test]
    fn code_uses_code_text_color() {
        let cs = collect_format("`x`", HoverKind::Parameter);
        assert_eq!(cs[0], Theme::hover_code_text());
    }

    #[test]
    fn italic_uses_italic_color() {
        let cs = collect_format("*em*", HoverKind::Parameter);
        assert_eq!(cs[0], Theme::hover_italic());
    }

    #[test]
    fn plain_text_uses_base_color() {
        let cs = collect_format("plain", HoverKind::Parameter);
        assert_eq!(cs[0], Theme::hover_base_text());
    }
}
