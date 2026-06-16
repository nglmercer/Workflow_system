//! Rendering of the completion popup and the hover popup.

use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, ScrollArea, Vec2};
use workflow_lsp::features::{Completion, CompletionKind};

pub const COMPLETION_WIDTH: f32 = 280.0;
pub const COMPLETION_MAX_HEIGHT: f32 = 200.0;
pub const COMPLETION_ROW_HEIGHT: f32 = 24.0;

/// Render the completion popup. Returns the index of any item the user
/// clicked, or `None`.
pub fn show_completion(
    ctx: &egui::Context,
    completions: &[Completion],
    current_index: usize,
) -> Option<usize> {
    if completions.is_empty() {
        return None;
    }

    let height = (completions.len() as f32 * COMPLETION_ROW_HEIGHT).min(COMPLETION_MAX_HEIGHT);
    let area = ctx.available_rect();
    let popup_rect = Rect::from_min_size(
        Pos2::new(area.min.x + 16.0, area.max.y - height - 16.0),
        Vec2::new(COMPLETION_WIDTH, height),
    );

    let mut clicked = None;
    egui::Window::new("Completions")
        .fixed_pos(popup_rect.min)
        .fixed_size(Vec2::new(COMPLETION_WIDTH, height))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .show(ctx, |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                for (idx, item) in completions.iter().enumerate() {
                    let selected = idx == current_index;
                    let bg = if selected {
                        Color32::from_rgb(40, 80, 140)
                    } else {
                        Color32::TRANSPARENT
                    };
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(COMPLETION_WIDTH - 8.0, COMPLETION_ROW_HEIGHT),
                        egui::Sense::click(),
                    );
                    if response.hovered() || selected {
                        ui.painter().rect_filled(rect, 2.0, bg);
                    }
                    let color = color_for_kind(item.kind);
                    ui.painter().text(
                        rect.min + Vec2::new(6.0, 4.0),
                        egui::Align2::LEFT_TOP,
                        &item.label,
                        FontId::monospace(13.0),
                        color,
                    );
                    if let Some(detail) = &item.detail {
                        ui.painter().text(
                            rect.min + Vec2::new(160.0, 6.0),
                            egui::Align2::LEFT_TOP,
                            detail,
                            FontId::proportional(11.0),
                            Color32::GRAY,
                        );
                    }
                    if response.clicked() {
                        clicked = Some(idx);
                    }
                }
            });
        });
    clicked
}

/// Render the hover popup at the given screen position.
pub fn show_hover(ctx: &egui::Context, pos: Pos2, text: &str) {
    egui::Window::new("Hover")
        .fixed_pos(pos + Vec2::new(12.0, 12.0))
        .fixed_size(Vec2::new(260.0, 100.0))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(text)
                    .monospace()
                    .color(Color32::from_gray(220)),
            );
        });
}

fn color_for_kind(kind: CompletionKind) -> Color32 {
    match kind {
        CompletionKind::Keyword => Color32::from_rgb(200, 120, 255),
        CompletionKind::Function => Color32::from_rgb(100, 200, 255),
        CompletionKind::Variable => Color32::from_rgb(220, 220, 220),
        CompletionKind::Value => Color32::from_rgb(180, 220, 120),
        CompletionKind::Property => Color32::from_rgb(255, 200, 100),
    }
}
