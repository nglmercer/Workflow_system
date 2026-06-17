//! Find/Replace bar at the bottom of the editor.
//!
//! Renders a horizontal bar with search input, match count,
//! next/previous navigation, case-sensitivity toggle, and close
//! button. The bar is toggled by Ctrl+F and closed by Escape.
//!
//! Icons are painted using egui's `Painter` API for consistent
//! cross-platform rendering (no font dependency).

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Rounding, Stroke, TextEdit, Vec2};
use crate::theme::Theme;

/// State for the find bar.
#[derive(Default)]
pub struct FindState {
    /// Whether the find bar is visible.
    pub open: bool,
    /// The current search query.
    pub query: String,
    /// Case-sensitive search when `true`.
    pub case_sensitive: bool,
    /// Regex mode when `true`.
    pub regex: bool,
    /// Whole word match when `true`.
    pub whole_word: bool,
    /// Index of the currently highlighted match (0-based).
    pub current_match: usize,
    /// Total number of matches found.
    pub total_matches: usize,
    /// Byte offsets of all matches in the text.
    pub match_offsets: Vec<(usize, usize)>,
    /// Previous search queries (most recent first, max 20).
    pub search_history: Vec<String>,
}

impl FindState {
    const MAX_HISTORY: usize = 20;

    /// Open the find bar and pre-fill with the current selection if any.
    pub fn open(&mut self, selected_text: Option<&str>) {
        self.open = true;
        if let Some(sel) = selected_text {
            if !sel.is_empty() && sel.len() < 100 {
                self.query = sel.to_string();
            }
        }
    }

    /// Close the find bar and clear state.
    pub fn close(&mut self) {
        let query = self.query.clone();
        if !query.is_empty() {
            self.push_history(&query);
        }
        self.open = false;
        self.query.clear();
        self.current_match = 0;
        self.total_matches = 0;
        self.match_offsets.clear();
    }

    /// Add a query to the search history (most recent first).
    fn push_history(&mut self, query: &str) {
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|h| h != query);
        self.search_history.insert(0, query.to_string());
        self.search_history.truncate(Self::MAX_HISTORY);
    }

    /// Update match offsets based on the current query and text.
    pub fn update_matches(&mut self, text: &str) {
        self.match_offsets.clear();
        if self.query.is_empty() {
            self.total_matches = 0;
            self.current_match = 0;
            return;
        }
        let mut haystack = text;
        let mut needle = self.query.as_str();
        let owned_lower_haystack;
        let owned_lower_needle;
        if !self.case_sensitive {
            owned_lower_haystack = text.to_lowercase();
            owned_lower_needle = self.query.to_lowercase();
            haystack = &owned_lower_haystack;
            needle = &owned_lower_needle;
        }
        let mut start = 0;
        while let Some(pos) = haystack[start..].find(needle) {
            let absolute = start + pos;
            let end = absolute + needle.len();
            self.match_offsets.push((absolute, end));
            start = absolute + needle.len().max(1);
            if needle.is_empty() {
                break;
            }
        }
        self.total_matches = self.match_offsets.len();
        if self.current_match >= self.total_matches {
            self.current_match = self.total_matches.saturating_sub(1);
        }
    }

    /// Navigate to the next match (wraps around).
    pub fn next_match(&mut self) {
        if self.total_matches > 0 {
            self.current_match = (self.current_match + 1) % self.total_matches;
        }
    }

    /// Navigate to the previous match (wraps around).
    pub fn prev_match(&mut self) {
        if self.total_matches > 0 {
            self.current_match = if self.current_match == 0 {
                self.total_matches - 1
            } else {
                self.current_match - 1
            };
        }
    }

    /// Toggle case-sensitive mode and re-run the match scan.
    pub fn toggle_case_sensitive(&mut self, text: &str) {
        self.case_sensitive = !self.case_sensitive;
        self.update_matches(text);
    }

    /// Toggle regex mode and re-run the match scan.
    pub fn toggle_regex(&mut self, text: &str) {
        self.regex = !self.regex;
        self.update_matches(text);
    }

    /// Toggle whole-word matching and re-run the match scan.
    pub fn toggle_whole_word(&mut self, text: &str) {
        self.whole_word = !self.whole_word;
        self.update_matches(text);
    }

    /// Get the byte range of the current match, if any.
    pub fn current_range(&self) -> Option<(usize, usize)> {
        self.match_offsets.get(self.current_match).copied()
    }
}

/// Action returned by the find bar.
pub enum FindAction {
    None,
    Close,
    Next,
    Previous,
    QueryChanged,
    ToggleCase,
    ToggleRegex,
    ToggleWholeWord,
}

// ---------------------------------------------------------------------------
// Icon painting helpers — draw simple shapes with egui::Painter so we
// don't depend on any icon font that may be missing on some platforms.
// ---------------------------------------------------------------------------

const ICON_BTN_SIZE: f32 = 28.0;

// Local constants for the icon glyphs painted by `paint_arrow_up`,
// `paint_arrow_down`, `paint_close`, `paint_check`. The single source
// of truth for the icon palette is `crate::theme::Theme` — these
// `const` aliases stay in sync via the `theme_icons_match` test in
// `theme.rs::tests`.
const ICON_COLOR: Color32 = Color32::from_gray(180);
const ICON_COLOR_HOVER: Color32 = Color32::from_gray(240);

/// Draw an upward-pointing triangle (previous match).
fn paint_arrow_up(painter: &egui::Painter, rect: Rect, color: Color32) {
    let cx = rect.center().x;
    let top = Pos2::new(cx, rect.top() + 4.0);
    let bl = Pos2::new(rect.left() + 5.0, rect.bottom() - 4.0);
    let br = Pos2::new(rect.right() - 5.0, rect.bottom() - 4.0);
    painter.add(egui::Shape::convex_polygon(
        vec![top, bl, br],
        color,
        Stroke::NONE,
    ));
}

/// Draw a downward-pointing triangle (next match).
fn paint_arrow_down(painter: &egui::Painter, rect: Rect, color: Color32) {
    let cx = rect.center().x;
    let bot = Pos2::new(cx, rect.bottom() - 4.0);
    let tl = Pos2::new(rect.left() + 5.0, rect.top() + 4.0);
    let tr = Pos2::new(rect.right() - 5.0, rect.top() + 4.0);
    painter.add(egui::Shape::convex_polygon(
        vec![bot, tl, tr],
        color,
        Stroke::NONE,
    ));
}

/// Draw an X (close).
fn paint_close(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(2.5, color);
    let pad = 6.0;
    let tl = Pos2::new(rect.left() + pad, rect.top() + pad);
    let br = Pos2::new(rect.right() - pad, rect.bottom() - pad);
    let tr = Pos2::new(rect.right() - pad, rect.top() + pad);
    let bl = Pos2::new(rect.left() + pad, rect.bottom() - pad);
    painter.line_segment([tl, br], stroke);
    painter.line_segment([tr, bl], stroke);
}

/// Draw a checkmark (for toggle buttons when active).
fn paint_check(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(2.5, color);
    let p1 = Pos2::new(rect.left() + 5.0, rect.center().y);
    let p2 = Pos2::new(rect.center().x - 1.0, rect.bottom() - 5.0);
    let p3 = Pos2::new(rect.right() - 5.0, rect.top() + 5.0);
    painter.line_segment([p1, p2], stroke);
    painter.line_segment([p2, p3], stroke);
}

/// A custom icon button that paints its icon using the Painter API.
fn icon_button(ui: &mut egui::Ui, _id: &str, paint_fn: fn(&egui::Painter, Rect, Color32)) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(ICON_BTN_SIZE), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let color = if response.hovered() || response.is_pointer_button_down_on() {
            ICON_COLOR_HOVER
        } else {
            ICON_COLOR
        };
        let painter = ui.painter();
        paint_fn(painter, rect, color);
    }
    response.clicked()
}

/// A toggle icon button that shows a checkmark when active.
fn toggle_icon_button(
    ui: &mut egui::Ui,
    _id: &str,
    active: bool,
    paint_fn: fn(&egui::Painter, Rect, Color32),
) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(ICON_BTN_SIZE), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let bg = if active {
            Color32::from_rgba_premultiplied(80, 80, 80, 120)
        } else if response.hovered() {
            Color32::from_rgba_premultiplied(60, 60, 60, 80)
        } else {
            Color32::TRANSPARENT
        };
        let painter = ui.painter();
        painter.rect_filled(rect, Rounding::same(4.0), bg);
        let color = if active {
            Theme::find_icon_active()
        } else {
            ICON_COLOR
        };
        paint_fn(painter, rect, color);
        if active {
            paint_check(painter, rect, color);
        }
    }
    response.clicked()
}

/// Render the find bar. Returns a `FindAction` indicating what
/// the user did this frame.
pub fn show(ui: &mut egui::Ui, state: &mut FindState) -> FindAction {
    let mut action = FindAction::None;

    ui.horizontal(|ui| {
        ui.label(RichText::new("Find").strong());

        let response = ui.add(
            TextEdit::singleline(&mut state.query)
                .desired_width(200.0)
                .hint_text("Search...")
                .margin(4.0),
        );

        if response.changed() {
            action = FindAction::QueryChanged;
        }

        // Case sensitivity toggle
        if toggle_icon_button(
            ui,
            "find_case",
            state.case_sensitive,
            |painter, rect, color| {
                // Draw "Aa" text
                let font = egui::FontId::monospace(11.0);
                let center = rect.center();
                let a1 = painter.layout_no_wrap("A".to_string(), font.clone(), color);
                let a2 = painter.layout_no_wrap("a".to_string(), font, color);
                let w = a1.size().x + a2.size().x;
                let x = center.x - w / 2.0;
                let y = center.y - a1.size().y / 2.0;
                painter.galley(Pos2::new(x, y), a1, Color32::TRANSPARENT);
                painter.galley(Pos2::new(x + 8.0, y), a2, Color32::TRANSPARENT);
            },
        ) {
            action = FindAction::ToggleCase;
        }

        // Regex toggle
        if toggle_icon_button(ui, "find_regex", state.regex, |painter, rect, color| {
            let font = egui::FontId::monospace(11.0);
            let galley = painter.layout_no_wrap(".*".to_string(), font, color);
            let pos = rect.center() - galley.size() / 2.0;
            painter.galley(pos, galley, Color32::TRANSPARENT);
        }) {
            action = FindAction::ToggleRegex;
        }

        // Whole word toggle
        if toggle_icon_button(ui, "find_word", state.whole_word, |painter, rect, color| {
            let font = egui::FontId::monospace(11.0);
            let galley = painter.layout_no_wrap("Ab".to_string(), font, color);
            let pos = rect.center() - galley.size() / 2.0;
            painter.galley(pos, galley, Color32::TRANSPARENT);
        }) {
            action = FindAction::ToggleWholeWord;
        }

        // Match count
        if !state.query.is_empty() {
            let label = if state.total_matches > 0 {
                format!("{}/{}", state.current_match + 1, state.total_matches)
            } else {
                "No matches".to_string()
            };
            ui.label(RichText::new(label).small().weak());
        }

        // Previous match (up arrow)
        if icon_button(ui, "find_prev", paint_arrow_up) {
            action = FindAction::Previous;
        }

        // Next match (down arrow)
        if icon_button(ui, "find_next", paint_arrow_down) {
            action = FindAction::Next;
        }

        // Close button (X)
        if icon_button(ui, "find_close", paint_close) {
            action = FindAction::Close;
        }
    });

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_matches_finds_literal() {
        let mut s = FindState {
            query: "log".to_string(),
            ..Default::default()
        };
        s.update_matches("log(message)\nlog(\"other\")");
        assert_eq!(s.total_matches, 2);
        assert_eq!(s.match_offsets, vec![(0, 3), (13, 16)]);
    }

    #[test]
    fn case_sensitive_toggle_changes_match_count() {
        let mut s = FindState {
            query: "Foo".to_string(),
            ..Default::default()
        };
        s.update_matches("Foo bar foo");
        assert_eq!(s.total_matches, 2); // both Foo and foo match
        s.case_sensitive = true;
        s.update_matches("Foo bar foo");
        assert_eq!(s.total_matches, 1); // only Foo
    }

    #[test]
    fn next_match_wraps_around() {
        let mut s = FindState {
            query: "a".to_string(),
            ..Default::default()
        };
        s.update_matches("aaa");
        assert_eq!(s.total_matches, 3);
        assert_eq!(s.current_match, 0);
        s.next_match();
        assert_eq!(s.current_match, 1);
        s.next_match();
        assert_eq!(s.current_match, 2);
        s.next_match();
        assert_eq!(s.current_match, 0); // wrapped
    }

    #[test]
    fn prev_match_wraps_around() {
        let mut s = FindState {
            query: "a".to_string(),
            ..Default::default()
        };
        s.update_matches("aaa");
        s.current_match = 0;
        s.prev_match();
        assert_eq!(s.current_match, 2); // wrapped
    }

    #[test]
    fn empty_query_matches_nothing() {
        let mut s = FindState {
            query: String::new(),
            ..Default::default()
        };
        s.update_matches("anything");
        assert_eq!(s.total_matches, 0);
        assert_eq!(s.match_offsets.len(), 0);
    }

    #[test]
    fn current_range_returns_match() {
        let mut s = FindState {
            query: "log".to_string(),
            ..Default::default()
        };
        s.update_matches("first log then log");
        assert_eq!(s.current_range(), Some((6, 9)));
    }
}
