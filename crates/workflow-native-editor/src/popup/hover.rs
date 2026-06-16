//! Hover popup renderer.
//!
//! Renders a [`HoverContent`] as a frameless egui `Window` floating
//! near the cursor. The window has a colored badge, a glyph-prefixed
//! title, an optional event chip, a signature slot (rendered either
//! as a parsed type table or as monospace text), and a scrollable
//! mini-markdown body.

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, RichText, Rounding, ScrollArea, Sense, Ui, Vec2,
};

use super::layout::{popup_frame, HOVER_MAX_WIDTH, HOVER_MIN_WIDTH};
use super::markdown::render_mini_markdown;
use super::model::{HoverContent, HoverSignature, TypeExpr};

/// Render the hover popup at the given screen position.
pub fn show_hover(ctx: &egui::Context, pos: Pos2, content: &HoverContent) {
    if content.title.is_empty() && content.docs.is_none() && content.signature.is_none() {
        return;
    }

    let frame = popup_frame(ctx);
    // First, render the popup with a provisional position. egui computes
    // the inner size and gives us back a response whose rect we can then
    // translate / clamp to the screen before the frame is committed.
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

    // egui windows can still drift off-screen on resize; the *next*
    // frame's response is what the user actually sees, so we accept
    // the small one-frame visual jitter rather than fight egui's
    // placement pipeline.
    let _ = ctx.screen_rect(); // kept for parity with the completion clamp path
}

/// Backwards-compatible entry point that takes a raw markdown blob.
#[allow(dead_code)]
pub fn show_hover_markdown(ctx: &egui::Context, pos: Pos2, markdown: &str) {
    show_hover(ctx, pos, &HoverContent::from_markdown(markdown));
}

fn render_hover_body(ui: &mut Ui, content: &HoverContent) {
    // Header row: [badge] [glyph] [title]
    let badge_text = content.kind.badge();
    let badge_color = content.kind.badge_color();
    let glyph = content.kind.glyph();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        // Badge chip
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

        // Glyph + title
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
                .color(Color32::from_gray(235)),
        );

        // Event chip (only when the LSP body contained `(event "FOO")`).
        if let Some(event_name) = &content.event {
            let chip_text = format!("→ {}", event_name);
            let (chip_rect, _) = ui.allocate_exact_size(
                Vec2::new(event_chip_width(&chip_text), 18.0),
                Sense::hover(),
            );
            ui.painter().rect_filled(
                chip_rect,
                Rounding::same(4.0),
                Color32::from_rgba_unmultiplied(
                    badge_color.r(),
                    badge_color.g(),
                    badge_color.b(),
                    70,
                ),
            );
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

// ---------------------------------------------------------------------------
// Signature renderer
// ---------------------------------------------------------------------------
//
// This is what makes the params look like *params* and not like a
// `//@` comment. The dispatcher below turns each `HoverSignature`
// variant into the layout that fits it best:

fn render_signature(ui: &mut Ui, sig: &HoverSignature) {
    match sig {
        HoverSignature::Text(s) => {
            ui.indent("hover-sig", |ui| {
                ui.label(
                    RichText::new(s)
                        .monospace()
                        .size(12.0)
                        .color(Color32::from_rgb(180, 200, 220)),
                );
            });
        }
        HoverSignature::Type(ty) => {
            render_type_expr(ui, ty);
        }
    }
}

/// Render a type expression. Objects become 2-column field tables;
/// arrays show a chip + the element type; functions show a parameter
/// list with the return type; primitives show a small pill.
pub fn render_type_expr(ui: &mut Ui, ty: &TypeExpr) {
    match ty {
        TypeExpr::Name(n) => {
            render_type_pill(ui, n, type_color(n));
        }
        TypeExpr::Array(inner) => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                render_type_pill(ui, "Array of", Color32::from_rgb(160, 100, 200));
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
                        RichText::new("returns")
                            .strong()
                            .size(11.0)
                            .color(Color32::from_gray(160)),
                    );
                    render_type_compact(ui, ret);
                });
            });
        }
    }
}

/// Compact inline form of a type (used inside `Array of` chips, the
/// `returns` line, etc.). Recurses so `Array(Array(number))` is
/// nested correctly.
///
/// `ui.indent` requires a vertical parent, but this helper is
/// invoked from horizontal contexts too (the row in a field table,
/// the `Array of ...` row, the `returns` row). We open a vertical
/// sub-UI at the top so the recursive `ui.indent` calls always
/// land on a vertical parent and egui doesn't panic.
fn render_type_compact(ui: &mut Ui, ty: &TypeExpr) {
    ui.vertical(|ui| match ty {
        TypeExpr::Name(n) => render_type_pill(ui, n, type_color(n)),
        TypeExpr::Array(inner) => {
            render_type_pill(ui, "[]", Color32::from_rgb(160, 100, 200));
            render_type_compact(ui, inner);
        }
        TypeExpr::Object(fields) => {
            render_type_pill(ui, "object", Color32::from_rgb(80, 170, 170));
            if !fields.is_empty() {
                ui.add_space(2.0);
                render_field_table(ui, fields);
            }
        }
        TypeExpr::Func { params, ret } => {
            render_type_pill(ui, "fn", Color32::from_rgb(160, 100, 200));
            ui.indent("hover-fn-compact", |ui| {
                render_field_table(ui, params);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("->")
                            .monospace()
                            .color(Color32::from_gray(160))
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
    // 2-column table: name (accent) | type (monospace muted).
    ui.indent("hover-fields", |ui| {
        let name_w = fields
            .iter()
            .map(|f| text_pixel_width(&f.name, 12.0, true))
            .fold(0.0_f32, f32::max)
            .max(40.0);
        for f in fields {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // Name (fixed-width column for alignment)
                let (name_rect, _) =
                    ui.allocate_exact_size(Vec2::new(name_w, 16.0), Sense::hover());
                ui.painter().text(
                    name_rect.left_center() + Vec2::new(0.0, -1.0),
                    Align2::LEFT_CENTER,
                    &f.name,
                    FontId::monospace(12.0),
                    Color32::from_rgb(220, 200, 140),
                );
                // Colon
                ui.label(
                    RichText::new(":")
                        .monospace()
                        .color(Color32::from_gray(120))
                        .size(12.0),
                );
                // Type
                render_type_compact(ui, &f.ty);
            });
        }
    });
}

fn render_type_pill(ui: &mut Ui, text: &str, color: Color32) {
    let w = text_pixel_width(text, 11.0, true) + 10.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 16.0), Sense::hover());
    ui.painter().rect_filled(
        rect,
        Rounding::same(3.0),
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 50),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(11.0),
        color,
    );
}

/// Approximate pixel width of a monospace string at the given size.
fn text_pixel_width(s: &str, size: f32, _monospace: bool) -> f32 {
    // monospace ≈ 0.6 em per char in egui's default font
    s.chars().count() as f32 * size * 0.6
}

fn event_chip_width(text: &str) -> f32 {
    text.chars().count() as f32 * 6.6 + 12.0
}

/// Map a primitive type name to a per-type color.
fn type_color(name: &str) -> Color32 {
    match name {
        "number" => Color32::from_rgb(120, 200, 255),
        "string" => Color32::from_rgb(180, 220, 120),
        "bool" => Color32::from_rgb(255, 170, 90),
        "null" => Color32::from_gray(140),
        "any" => Color32::from_gray(170),
        _ => Color32::from_rgb(200, 180, 220), // user-defined types
    }
}
