//! The line-number + fold-chevron gutter.
//!
//! The gutter is drawn as a sibling of the `TextEdit` in the same
//! horizontal `ui.horizontal`, so the two scroll together naturally.
//! It owns the digit-counting (for width sizing), the per-line
//! chevron hit-testing, and the click → toggle dispatch.

use std::collections::BTreeSet;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Vec2};

use super::folding::{self, FoldKind, FoldRegion};
use super::layouter::LINE_HEIGHT;

/// Paint line numbers and fold chevrons into the given `rect`.
///
/// `galley` is the laid-out text from the `TextEdit`; its row
/// positions already account for the widget's inner margin, so the
/// line numbers stay perfectly aligned with the code.
///
/// `collapsed` is mutated in place when the user clicks a chevron.
pub fn paint(
    ui: &mut egui::Ui,
    rect: Rect,
    galley: &egui::Galley,
    regions: &[FoldRegion],
    display_text: &str,
    collapsed: &mut BTreeSet<usize>,
) {
    let painter = ui.painter_at(rect);
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        (1.0, Color32::from_gray(60)),
    );

    let font = FontId::monospace(super::layouter::FONT_SIZE);
    let line_count = display_text.split('\n').count().max(1);
    let text_color = Color32::from_gray(140);

    for line_idx in 0..line_count {
        let y = if line_idx < galley.rows.len() {
            galley.rows[line_idx].rect.min.y
        } else {
            galley
                .rows
                .last()
                .map_or(rect.min.y, |r| r.rect.min.y + LINE_HEIGHT)
        };
        if y > rect.max.y {
            break;
        }
        let num = format!("{}", line_idx + 1);
        let anchor = Pos2::new(rect.max.x - 6.0, y);
        painter.text(anchor, Align2::RIGHT_TOP, num, font.clone(), text_color);

        if let Some(region) = regions.iter().find(|r| r.start_line == line_idx) {
            draw_chevron(ui, &painter, rect, y, region, font.clone(), collapsed);
        }
    }
}

/// Compute the gutter width that fits both the line-number digits and
/// the 16-pixel chevron column. Always reserves at least 2 digits of
/// space so a single-line document still has room.
pub fn width_for_line_count(line_count: usize) -> f32 {
    let digits = ((line_count as f64).log10().floor() as usize + 1).max(2);
    (digits as f32) * 9.0 + 24.0
}

fn draw_chevron(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    gutter_rect: Rect,
    y: f32,
    region: &FoldRegion,
    font: FontId,
    collapsed: &mut BTreeSet<usize>,
) {
    let chevron_rect = Rect::from_min_size(
        Pos2::new(gutter_rect.min.x + 2.0, y),
        Vec2::new(16.0, LINE_HEIGHT),
    );
    let id = ui.id().with(("fold", region.start_line));
    let response = ui.interact(chevron_rect, id, egui::Sense::click());
    let is_collapsed = collapsed.contains(&region.start_line);
    let glyph = if is_collapsed { "▶" } else { "▼" };
    let base_color = match region.kind {
        FoldKind::Function => Color32::from_rgb(120, 180, 255),
        FoldKind::Workflow => Color32::from_rgb(255, 180, 120),
    };
    let color = if response.hovered() {
        Color32::from_gray(240)
    } else {
        base_color
    };
    if response.clicked() {
        toggle_collapse(collapsed, region, is_collapsed);
    }
    painter.text(
        chevron_rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        font,
        color,
    );
}

fn toggle_collapse(collapsed: &mut BTreeSet<usize>, region: &FoldRegion, is_collapsed: bool) {
    if is_collapsed {
        collapsed.remove(&region.start_line);
    } else {
        collapsed.insert(region.start_line);
    }
}

/// Prune any collapsed-fold id that no longer refers to a real
/// region in `regions` (e.g. the user deleted the header).
pub fn prune_stale(collapsed: &mut BTreeSet<usize>, source: &str) {
    let live: BTreeSet<usize> = folding::detect_folds(source)
        .iter()
        .map(|r| r.start_line)
        .collect();
    collapsed.retain(|id| live.contains(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_scales_with_line_count() {
        // 1 line, 100 lines, 1000 lines.
        assert!(width_for_line_count(1) >= 2.0 * 9.0 + 24.0);
        let w100 = width_for_line_count(100);
        let w1000 = width_for_line_count(1000);
        assert!(w1000 > w100);
    }

    #[test]
    fn toggle_collapse_inserts_and_removes() {
        let mut collapsed = BTreeSet::new();
        let region = FoldRegion {
            kind: FoldKind::Function,
            start_line: 0,
            end_line: 3,
            body_lines: 2,
            header: "fn foo".to_string(),
        };
        toggle_collapse(&mut collapsed, &region, false);
        assert!(collapsed.contains(&0));
        toggle_collapse(&mut collapsed, &region, true);
        assert!(collapsed.is_empty());
    }

    #[test]
    fn prune_stale_drops_missing_folds() {
        let mut collapsed = BTreeSet::new();
        collapsed.insert(0);
        collapsed.insert(42);
        prune_stale(&mut collapsed, "fn real() {\n  body\n}\n");
        // The fold at line 0 still exists; line 42 does not.
        assert!(collapsed.contains(&0));
        assert!(!collapsed.contains(&42));
    }
}
