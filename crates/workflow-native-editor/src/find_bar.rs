//! Find/Replace bar at the bottom of the editor.
//!
//! Renders a horizontal bar with search input, match count,
//! next/previous navigation, case-sensitivity toggle, and close
//! button. The bar is toggled by Ctrl+F and closed by Escape.

use eframe::egui::{self, RichText, TextEdit};

/// State for the find bar.
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
}

impl Default for FindState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            case_sensitive: false,
            regex: false,
            whole_word: false,
            current_match: 0,
            total_matches: 0,
            match_offsets: Vec::new(),
        }
    }
}

impl FindState {
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
        self.open = false;
        self.query.clear();
        self.current_match = 0;
        self.total_matches = 0;
        self.match_offsets.clear();
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

        // Case sensitivity toggle (Aa)
        if ui
            .add(
                egui::Button::new(RichText::new("Aa").small())
                    .rounding(4.0)
                    .min_size(egui::vec2(28.0, 20.0))
                    .selected(state.case_sensitive),
            )
            .clicked()
        {
            action = FindAction::ToggleCase;
        }

        // Regex toggle (.*)
        if ui
            .add(
                egui::Button::new(RichText::new(".*").small())
                    .rounding(4.0)
                    .min_size(egui::vec2(28.0, 20.0))
                    .selected(state.regex),
            )
            .clicked()
        {
            action = FindAction::ToggleRegex;
        }

        // Whole word toggle (Ab)
        if ui
            .add(
                egui::Button::new(RichText::new("Ab").small())
                    .rounding(4.0)
                    .min_size(egui::vec2(28.0, 20.0))
                    .selected(state.whole_word),
            )
            .clicked()
        {
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

        // Navigation buttons (up/down arrows)
        if ui
            .add(
                egui::Button::new(RichText::new("▲").small())
                    .rounding(4.0)
                    .min_size(egui::vec2(24.0, 20.0)),
            )
            .clicked()
        {
            action = FindAction::Previous;
        }
        if ui
            .add(
                egui::Button::new(RichText::new("▼").small())
                    .rounding(4.0)
                    .min_size(egui::vec2(24.0, 20.0)),
            )
            .clicked()
        {
            action = FindAction::Next;
        }

        // Close button (× multiplication sign)
        if ui
            .add(
                egui::Button::new(RichText::new("×").small())
                    .rounding(4.0)
                    .min_size(egui::vec2(24.0, 20.0)),
            )
            .clicked()
        {
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
        let mut s = FindState::default();
        s.query = "log".to_string();
        s.update_matches("log(message)\nlog(\"other\")");
        assert_eq!(s.total_matches, 2);
        assert_eq!(s.match_offsets, vec![(0, 3), (13, 16)]);
    }

    #[test]
    fn case_sensitive_toggle_changes_match_count() {
        let mut s = FindState::default();
        s.query = "Foo".to_string();
        s.update_matches("Foo bar foo");
        assert_eq!(s.total_matches, 2); // both Foo and foo match
        s.case_sensitive = true;
        s.update_matches("Foo bar foo");
        assert_eq!(s.total_matches, 1); // only Foo
    }

    #[test]
    fn next_match_wraps_around() {
        let mut s = FindState::default();
        s.query = "a".to_string();
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
        let mut s = FindState::default();
        s.query = "a".to_string();
        s.update_matches("aaa");
        s.current_match = 0;
        s.prev_match();
        assert_eq!(s.current_match, 2); // wrapped
    }

    #[test]
    fn empty_query_matches_nothing() {
        let mut s = FindState::default();
        s.query = String::new();
        s.update_matches("anything");
        assert_eq!(s.total_matches, 0);
        assert_eq!(s.match_offsets.len(), 0);
    }

    #[test]
    fn current_range_returns_match() {
        let mut s = FindState::default();
        s.query = "log".to_string();
        s.update_matches("first log then log");
        assert_eq!(s.current_range(), Some((6, 9)));
    }
}
