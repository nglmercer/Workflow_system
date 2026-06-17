//! Hover popup renderer.
//!
//! Renders a [`HoverContent`] as a frameless egui `Window` floating
//! near the cursor. The window has a colored badge, a glyph-prefixed
//! title, an optional event chip, a signature slot (rendered either
//! as a parsed type table or as monospace text), and a scrollable
//! mini-markdown body.
//!
//! Colors are pulled from the [`crate::theme::Theme`] palette so the
//! hover chrome stays in lockstep with the rest of the editor.

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, RichText, Rounding, ScrollArea, Sense, Ui, Vec2,
};

use super::layout::{popup_frame, HOVER_MAX_WIDTH, HOVER_MIN_WIDTH};
use super::markdown::render_mini_markdown;
use super::model::{HoverContent, HoverSignature, TypeExpr};
use crate::theme::Theme;

/// Render the hover popup at the given screen position.
pub fn show_hover(ctx: &egui::Context, pos: Pos2, content: &HoverContent) {
    if content.title.is_empty() && content.docs.is_none() && content.signature.is_none() {
        return;
    }

    let frame = popup_frame(ctx);
    let provisional = Pos2::new(pos.x + 12.0, pos.y + 14.0);
    egui::Window::new("Hover")
        .fixed_pos(provisional)
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_max_width(HOVER_MAX_WIDTH);
            ui.set_min_width(HOVER_MIN_WIDTH);
            render_hover_body(ui, content);
        });

    let _ = ctx.screen_rect();
}

/// Backwards-compatible entry point that takes a raw markdown blob.
#[allow(dead_code)]
pub fn show_hover_markdown(ctx: &egui::Context, pos: Pos2, markdown: &str) {
    show_hover(ctx, pos, &HoverContent::from_markdown(markdown));
}

fn render_hover_body(ui: &mut Ui, content: &HoverContent) {
    let badge_text = content.kind.badge();
    let badge_color = Theme::hover_badge(content.kind);
    let glyph = content.kind.glyph();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let badge_w = badge_text.chars().count() as f32 * 6.6 + 12.0;
        let (badge_rect, _) = ui.allocate_exact_size(Vec2::new(badge_w, 18.0), Sense::hover());
        ui.painter()
            .rect_filled(badge_rect, Rounding::same(4.0), badge_color);
        ui.painter().text(
            badge_rect.left_center() + Vec2::new(6.0, -1.0),
            Align2::LEFT_CENTER,
            badge_text,
            FontId::monospace(11.0),
            Color32::WHITE,
        );

        ui.label(
            RichText::new(glyph)
                .monospace()
                .color(badge_color)
                .size(13.0),
        );
        ui.label(
            RichText::new(&content.title)
                .strong()
                .size(13.0)
                .color(Theme::hover_title()),
        );

        if let Some(event_name) = &content.event {
            let chip_text = format!("→ {}", event_name);
            let (chip_rect, _) = ui.allocate_exact_size(
                Vec2::new(event_chip_width(&chip_text), 18.0),
                Sense::hover(),
            );
            ui.painter()
                .rect_filled(chip_rect, Rounding::same(4.0), Theme::hover_event_chip_bg(badge_color));
            ui.painter().text(
                chip_rect.center(),
                Align2::CENTER_CENTER,
                chip_text,
                FontId::monospace(11.0),
                badge_color,
            );
        }
    });

    if let Some(sig) = &content.signature {
        ui.add_space(4.0);
        render_signature(ui, sig);
    }

    // One-sentence doc line for the kind. Rendered in a muted
    // italic so the user can read it without it competing with
    // the type table or markdown body below.
    if let Some(doc) = content.kind.doc() {
        ui.add_space(4.0);
        ui.label(
            RichText::new(doc)
                .italics()
                .size(11.0)
                .color(Theme::hover_doc_label()),
        );
    }

    if let Some(docs) = &content.docs {
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        ScrollArea::vertical()
            .max_height(180.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                render_mini_markdown(ui, docs, content.kind);
            });
    }
}

fn render_signature(ui: &mut Ui, sig: &HoverSignature) {
    match sig {
        HoverSignature::Text(s) => {
            ui.indent("hover-sig", |ui| {
                ui.label(
                    RichText::new(s)
                        .monospace()
                        .size(12.0)
                        .color(Theme::hover_signature_text()),
                );
            });
        }
        HoverSignature::Type(ty) => {
            render_type_expr(ui, ty);
        }
    }
}

pub fn render_type_expr(ui: &mut Ui, ty: &TypeExpr) {
    match ty {
        TypeExpr::Name(n) => {
            render_type_pill(ui, n, Theme::type_color(n));
        }
        TypeExpr::Array(inner) => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                render_type_pill(ui, "Array of", Theme::hover_array_pill());
                render_type_compact(ui, inner);
            });
        }
        TypeExpr::Object(fields) => {
            render_field_table(ui, fields);
        }
        TypeExpr::Func { params, ret } => {
            render_field_table(ui, params);
            ui.indent("hover-func-ret", |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(
                        RichText::new(i18n_t_hovers_returns())
                            .strong()
                            .size(11.0)
                            .color(Theme::hover_returns_label()),
                    );
                    render_type_compact(ui, ret);
                });
            });
        }
    }
}

fn render_type_compact(ui: &mut Ui, ty: &TypeExpr) {
    ui.vertical(|ui| match ty {
        TypeExpr::Name(n) => render_type_pill(ui, n, Theme::type_color(n)),
        TypeExpr::Array(inner) => {
            render_type_pill(ui, "[]", Theme::hover_compact_array());
            render_type_compact(ui, inner);
        }
        TypeExpr::Object(fields) => {
            render_type_pill(ui, "object", Theme::hover_compact_object());
            if !fields.is_empty() {
                ui.add_space(2.0);
                render_field_table(ui, fields);
            }
        }
        TypeExpr::Func { params, ret } => {
            render_type_pill(ui, "fn", Theme::hover_compact_fn());
            ui.indent("hover-fn-compact", |ui| {
                render_field_table(ui, params);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("->")
                            .monospace()
                            .color(Theme::hover_arrow())
                            .size(11.0),
                    );
                    render_type_compact(ui, ret);
                });
            });
        }
    });
}

fn render_field_table(ui: &mut Ui, fields: &[super::model::TypeField]) {
    if fields.is_empty() {
        return;
    }
    ui.indent("hover-fields", |ui| {
        let name_w = fields
            .iter()
            .map(|f| text_pixel_width(&f.name, 12.0, true))
            .fold(0.0_f32, f32::max)
            .max(40.0);
        for f in fields {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (name_rect, _) =
                    ui.allocate_exact_size(Vec2::new(name_w, 16.0), Sense::hover());
                ui.painter().text(
                    name_rect.left_center() + Vec2::new(0.0, -1.0),
                    Align2::LEFT_CENTER,
                    &f.name,
                    FontId::monospace(12.0),
                    Theme::hover_field_name(),
                );
                ui.label(
                    RichText::new(":")
                        .monospace()
                        .color(Theme::hover_field_colon())
                        .size(12.0),
                );
                render_type_compact(ui, &f.ty);
            });
        }
    });
}

fn render_type_pill(ui: &mut Ui, text: &str, color: Color32) {
    let w = text_pixel_width(text, 11.0, true) + 10.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 16.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, Rounding::same(3.0), Theme::hover_pill_bg(color));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(11.0),
        color,
    );
}

fn text_pixel_width(s: &str, size: f32, _monospace: bool) -> f32 {
    s.chars().count() as f32 * size * 0.6
}

fn event_chip_width(text: &str) -> f32 {
    text.chars().count() as f32 * 6.6 + 12.0
}

/// Localized "returns" label for the function-type renderer.
fn i18n_t_hovers_returns() -> String {
    workflow_i18n::t("popup.hover_returns")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use crate::popup::HoverKind;

    /// `hover_event_chip_bg` applies alpha 70 of the badge color.
    /// The RGB channels are gamma-corrected premultiplied values, so
    /// we assert only the alpha here.
    #[test]
    fn event_chip_alpha_matches_documented_value() {
        let badge = Theme::hover_badge(HoverKind::Parameter);
        let chip = Theme::hover_event_chip_bg(badge);
        assert_eq!(chip.a(), 70);
    }

    /// `hover_pill_bg` applies alpha 50 of the supplied color.
    #[test]
    fn pill_alpha_matches_documented_value() {
        let color = Theme::type_color("number");
        let pill = Theme::hover_pill_bg(color);
        assert_eq!(pill.a(), 50);
    }

    /// Every primitive type color in the theme is non-default.
    #[test]
    fn type_colors_are_non_default() {
        for name in ["number", "string", "bool", "null", "any"] {
            assert_ne!(Theme::type_color(name), Color32::default());
        }
    }

    /// `hover_title` / `hover_base_text` / `hover_doc_label` are
    /// distinct grays that the renderer uses in different contexts.
    #[test]
    fn hover_text_grays_are_distinct() {
        let title = Theme::hover_title();
        let base = Theme::hover_base_text();
        let doc = Theme::hover_doc_label();
        assert_ne!(title, base);
        assert_ne!(base, doc);
        assert_ne!(title, doc);
    }

    /// `hover_strong_for` returns the kind's badge color so
    /// `**bold**` markdown spans match the badge in the header.
    #[test]
    fn hover_strong_for_matches_kind_badge() {
        for k in [
            HoverKind::Parameter,
            HoverKind::Event,
            HoverKind::Function,
            HoverKind::Type,
            HoverKind::Field,
        ] {
            assert_eq!(Theme::hover_strong_for(k), Theme::hover_badge(k));
        }
    }
}
