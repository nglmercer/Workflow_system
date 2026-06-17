//! Completion popup renderer.
//!
//! Renders the list of LSP [`Completion`] items as a vertical scrolling
//! window near the cursor, with per-item glyph + detail row. Returns
//! the index of any item the user clicked (or `None`).

use crate::theme::Theme;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Rounding, ScrollArea, Sense, Stroke, Ui,
    Vec2,
};
use workflow_lsp::features::{Completion, CompletionKind};

use super::layout::{
    clamp_to_screen, popup_frame, COMPLETION_MAX_HEIGHT, COMPLETION_ROW_HEIGHT, COMPLETION_WIDTH,
    SCREEN_EDGE_MARGIN,
};

/// Render the completion popup. Returns the index of any item the user
/// clicked, or `None`.
pub fn show_completion(
    ctx: &egui::Context,
    completions: &[Completion],
    current_index: usize,
    cursor_pos: Option<Pos2>,
) -> Option<usize> {
    if completions.is_empty() {
        return None;
    }

    let row_h = COMPLETION_ROW_HEIGHT;
    let height = (completions.len() as f32 * row_h + 8.0).min(COMPLETION_MAX_HEIGHT);
    let anchor = cursor_pos.unwrap_or_else(|| {
        let area = ctx.available_rect();
        Pos2::new(area.min.x + 16.0, area.max.y - height - 16.0)
    });

    let screen = ctx.screen_rect();
    let desired = Rect::from_min_size(
        anchor + Vec2::new(0.0, 20.0),
        Vec2::new(COMPLETION_WIDTH, height),
    );
    let popup_pos = clamp_to_screen(desired, screen, SCREEN_EDGE_MARGIN).min;

    let frame = popup_frame(ctx);
    let mut clicked = None;

    egui::Window::new("Completions")
        .fixed_pos(popup_pos)
        .fixed_size(Vec2::new(COMPLETION_WIDTH, height))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .frame(frame)
        .show(ctx, |ui| {
            ScrollArea::vertical()
                .max_height(height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_width(COMPLETION_WIDTH);
                    for (idx, item) in completions.iter().enumerate() {
                        if render_completion_row(ui, item, idx == current_index, row_h).clicked() {
                            clicked = Some(idx);
                        }
                    }
                });
        });

    clicked
}

fn render_completion_row(ui: &mut Ui, item: &Completion, selected: bool, height: f32) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(COMPLETION_WIDTH, height), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, &item.label));

    let visuals = ui.visuals();
    let bg = if selected {
        visuals.widgets.active.bg_fill
    } else if response.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, Rounding::same(3.0), bg);
    }
    if selected {
        let stroke = Stroke::new(1.0, visuals.widgets.active.bg_stroke.color);
        ui.painter()
            .rect_stroke(rect.shrink(0.5), Rounding::same(3.0), stroke);
    }

    let accent = Theme::completion(item.kind);
    let glyph = kind_glyph(item.kind);
    let indent = 8.0;
    let has_detail = item.detail.is_some();

    // Left: kind glyph + label
    let label_color = if selected {
        visuals.widgets.active.text_color()
    } else {
        accent
    };
    ui.painter().text(
        rect.min + Vec2::new(indent, height * 0.5),
        Align2::LEFT_CENTER,
        format!("{} {}", glyph, item.label),
        FontId::monospace(13.0),
        label_color,
    );

    // Right: detail
    if has_detail {
        if let Some(detail) = &item.detail {
            ui.painter().text(
                rect.max - Vec2::new(indent, 0.0),
                Align2::RIGHT_CENTER,
                detail,
                FontId::proportional(11.0),
                visuals.weak_text_color(),
            );
        }
    }

    // Selected marker
    if selected {
        ui.painter().text(
            rect.min + Vec2::new(1.0, height * 0.5),
            Align2::LEFT_CENTER,
            ">",
            FontId::monospace(11.0),
            visuals.widgets.active.bg_stroke.color,
        );
    }

    response
}

fn kind_glyph(kind: CompletionKind) -> &'static str {
    match kind {
        CompletionKind::Keyword => ">",
        CompletionKind::Function => "f",
        CompletionKind::Variable => "v",
        CompletionKind::Value => "=",
        CompletionKind::Property => ".",
        CompletionKind::Field => ".",
        CompletionKind::File => "[]",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use workflow_lsp::features::CompletionKind;

    #[test]
    fn completion_colors_match_theme() {
        assert_eq!(
            Theme::completion(CompletionKind::Keyword),
            Color32::from_rgb(200, 120, 255)
        );
        assert_eq!(
            Theme::completion(CompletionKind::Function),
            Color32::from_rgb(100, 200, 255)
        );
    }

    #[test]
    fn completion_kind_glyphs_are_short() {
        let k = CompletionKind::Keyword;
        let g = kind_glyph(k);
        assert!(!g.is_empty());
    }
}
