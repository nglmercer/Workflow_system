//! Shared layout helpers and constants for the hover and completion popups.
//!
//! `popup_frame` produces a consistent `Frame` for both popups, and
//! `clamp_to_screen` keeps a proposed rect from drifting off the edge
//! of the window when the editor is resized.

use eframe::egui::{self, epaint, Color32, Frame, Margin, Rect, Rounding, Vec2};

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

pub const COMPLETION_WIDTH: f32 = 320.0;
pub const COMPLETION_MAX_HEIGHT: f32 = 220.0;
pub const COMPLETION_ROW_HEIGHT: f32 = 24.0;
pub const HOVER_MAX_WIDTH: f32 = 440.0;
pub const HOVER_MIN_WIDTH: f32 = 220.0;
pub(crate) const SCREEN_EDGE_MARGIN: f32 = 8.0;

// ---------------------------------------------------------------------------
// Frame helper
// ---------------------------------------------------------------------------

/// Build a consistent popup frame. The shadow and rounding are tuned
/// for dark-mode readability and to keep the popup visually separate
/// from the editor gutter.
pub(crate) fn popup_frame(ctx: &egui::Context) -> Frame {
    let style = ctx.style();
    Frame::popup(&style)
        .inner_margin(Margin::symmetric(10.0, 8.0))
        .rounding(Rounding::same(6.0))
        .shadow(epaint::Shadow {
            offset: Vec2::new(0.0, 3.0),
            blur: 14.0,
            spread: 0.0,
            color: Color32::from_black_alpha(140),
        })
}

// ---------------------------------------------------------------------------
// Screen clamping
// ---------------------------------------------------------------------------

/// Move `rect` so that it stays inside `screen` (with `margin` of
/// padding on every edge).
pub(crate) fn clamp_to_screen(rect: Rect, screen: Rect, margin: f32) -> Rect {
    let mut out = rect;
    let inner_min = screen.min + Vec2::splat(margin);
    let inner_max = screen.max - Vec2::splat(margin);
    if out.max.x > inner_max.x {
        out = out.translate(Vec2::new(inner_max.x - out.max.x, 0.0));
    }
    if out.max.y > inner_max.y {
        out = out.translate(Vec2::new(0.0, inner_max.y - out.max.y));
    }
    if out.min.x < inner_min.x {
        out = out.translate(Vec2::new(inner_min.x - out.min.x, 0.0));
    }
    if out.min.y < inner_min.y {
        out = out.translate(Vec2::new(inner_min.y - out.min.y, 0.0));
    }
    if out.height() > screen.height() - margin * 2.0 || out.width() > screen.width() - margin * 2.0
    {
        out = Rect::from_min_size(inner_min, screen.size() - Vec2::splat(margin * 2.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Pos2;

    #[test]
    fn clamp_to_screen_pushes_popup_inside() {
        let screen = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(800.0, 600.0));
        let bad = Rect::from_min_size(Pos2::new(700.0, 500.0), Vec2::new(200.0, 200.0));
        let good = clamp_to_screen(bad, screen, 8.0);
        let inner = screen.shrink(8.0);
        assert!(
            good.max.x <= inner.max.x + 0.5,
            "max.x={} > {}",
            good.max.x,
            inner.max.x
        );
        assert!(
            good.max.y <= inner.max.y + 0.5,
            "max.y={} > {}",
            good.max.y,
            inner.max.y
        );
        assert!(good.min.x >= inner.min.x - 0.5);
        assert!(good.min.y >= inner.min.y - 0.5);
    }
}
