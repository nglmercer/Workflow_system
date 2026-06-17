//! Editor color palette.
//!
//! Single source of truth for every color the editor paints outside
//! the egui `Visuals` chrome. All popups, panels, gutter accents,
//! and find-match highlights pull from this module so a future
//! theme switch (light mode, per-user color tweaks) only needs to
//! change in one place.
//!
//! Usage:
//!
//! ```
//! use crate::theme::Theme;
//! let accent = Theme::hover_badge(crate::popup::model::HoverKind::Parameter);
//! ```
//!
//! Each method is a pure function — it takes the discriminant (a kind,
//! a severity, a primitive type name) and returns a `Color32`. The
//! hover renderer and the layouter, the diagnostics panel, the test
//! panel, and the completion list all consult `Theme` instead of
//! inlining their own `Color32::from_rgb(...)` calls.

use eframe::egui::Color32;

use crate::popup::{CompletionKind, HoverKind};

// Re-exported so other modules can `use crate::theme::TokenKind`.
// We avoid `pub use` to keep the import surface narrow; downstream
// modules re-import the original.
pub use crate::highlight::TokenKind;

// `DiagnosticSeverity` lives in `workflow_lsp::features`; we
// pattern-match on it via its `Display` representation in
// `diagnostic_severity` to avoid pulling the LSP crate into `theme`.

// ---------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------

impl Theme {
    /// Color for the hover badge background and the matching glyph
    /// and event chip. All `HoverKind` variants get a distinct hue so
    /// the user can tell apart `@param` from `@var` at a glance.
    pub fn hover_badge(kind: HoverKind) -> Color32 {
        match kind {
            HoverKind::Parameter => Color32::from_rgb(70, 130, 200),
            HoverKind::Event => Color32::from_rgb(200, 130, 60),
            HoverKind::Variable => Color32::from_rgb(130, 170, 90),
            HoverKind::Function => Color32::from_rgb(160, 100, 200),
            HoverKind::Type => Color32::from_rgb(80, 170, 170),
            HoverKind::Field => Color32::from_rgb(200, 170, 80),
            HoverKind::Error => Color32::from_rgb(190, 60, 60),
            HoverKind::Warning => Color32::from_rgb(200, 160, 50),
            HoverKind::Doc => Color32::from_rgb(110, 110, 130),
            HoverKind::Test => Color32::from_rgb(80, 170, 110),
        }
    }

    /// Background tint for the small event chip in the hover header.
    /// Same hue as the badge, lower alpha, so it reads as a sub-element.
    #[allow(dead_code)]    pub fn hover_event_chip_bg(badge: Color32) -> Color32 {
        Color32::from_rgba_unmultiplied(badge.r(), badge.g(), badge.b(), 70)
    }

    /// Background tint for a hover type pill (e.g. "Array of", primitive pills).
    #[allow(dead_code)]    pub fn hover_pill_bg(color: Color32) -> Color32 {
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 50)
    }

    /// Title text color in the hover header.
    #[allow(dead_code)]    pub fn hover_title() -> Color32 {
        Color32::from_gray(235)
    }

    /// Default body text color in the hover markdown renderer.
    #[allow(dead_code)]    pub fn hover_base_text() -> Color32 {
        Color32::from_gray(210)
    }

    /// Italic text color (used for `*italic*` markdown spans).
    #[allow(dead_code)]    pub fn hover_italic() -> Color32 {
        Color32::from_gray(180)
    }

    /// Color for the muted italic one-sentence doc line that
    /// appears above the markdown body in the hover popup.
    #[allow(dead_code)]
    pub fn hover_doc_label() -> Color32 {
        Color32::from_gray(160)
    }

    /// Bold text color (used for `**bold**` markdown spans); the renderer
    /// shades the kind's badge color for emphasis.
    #[allow(dead_code)]    pub fn hover_strong_for(kind: HoverKind) -> Color32 {
        Self::hover_badge(kind)
    }

    /// Inline code text color.
    #[allow(dead_code)]    pub fn hover_code_text() -> Color32 {
        Color32::from_rgb(200, 220, 255)
    }

    /// Inline code background tint.
    #[allow(dead_code)]    pub fn hover_code_bg() -> Color32 {
        Color32::from_rgba_unmultiplied(60, 80, 110, 90)
    }

    /// Color for the `HoverSignature::Text` plain-monospace label.
    #[allow(dead_code)]    pub fn hover_signature_text() -> Color32 {
        Color32::from_rgb(180, 200, 220)
    }

    /// Color for the "returns" label that follows a function-typed signature.
    #[allow(dead_code)]    pub fn hover_returns_label() -> Color32 {
        Color32::from_gray(160)
    }

    /// Field-name column color in the function/object field table.
    #[allow(dead_code)]    pub fn hover_field_name() -> Color32 {
        Color32::from_rgb(220, 200, 140)
    }

    /// Color of the `:` separator between a field name and its type.
    #[allow(dead_code)]    pub fn hover_field_colon() -> Color32 {
        Color32::from_gray(120)
    }

    /// Color of the `->` arrow in compact function-type rendering.
    #[allow(dead_code)]    pub fn hover_arrow() -> Color32 {
        Color32::from_gray(160)
    }

    /// "Array of" pill color in `render_type_expr`.
    #[allow(dead_code)]    pub fn hover_array_pill() -> Color32 {
        Color32::from_rgb(160, 100, 200)
    }

    /// Compact `[]` suffix color in `render_type_compact`.
    #[allow(dead_code)]    pub fn hover_compact_array() -> Color32 {
        Color32::from_rgb(160, 100, 200)
    }

    /// Compact `object` / `{ ... }` color.
    #[allow(dead_code)]    pub fn hover_compact_object() -> Color32 {
        Color32::from_rgb(80, 170, 170)
    }

    /// Compact `fn(...) -> T` color.
    #[allow(dead_code)]    pub fn hover_compact_fn() -> Color32 {
        Color32::from_rgb(160, 100, 200)
    }
}

// ---------------------------------------------------------------------------
// Primitive type colors
// ---------------------------------------------------------------------------

impl Theme {
    /// Color for a primitive or alias type pill. Unknown / user types
    /// fall back to a soft purple so they read as "library" rather
    /// than as any specific primitive.
    #[allow(dead_code)]    pub fn type_color(name: &str) -> Color32 {
        match name {
            "number" => Color32::from_rgb(120, 200, 255),
            "string" => Color32::from_rgb(180, 220, 120),
            "bool" => Color32::from_rgb(255, 170, 90),
            "null" => Color32::from_gray(140),
            "any" => Color32::from_gray(170),
            _ => Color32::from_rgb(200, 180, 220),
        }
    }
}

// ---------------------------------------------------------------------------
// Completion
// ---------------------------------------------------------------------------

impl Theme {
    /// Accent color for a completion item by its kind. The kind enum
    /// already exposes 7 variants, each of which gets a distinct hue
    /// so the completion list reads as a color-coded palette.
    #[allow(dead_code)]    pub fn completion(kind: CompletionKind) -> Color32 {
        match kind {
            CompletionKind::Keyword => Color32::from_rgb(200, 120, 255),
            CompletionKind::Function => Color32::from_rgb(100, 200, 255),
            CompletionKind::Variable => Color32::from_rgb(220, 220, 220),
            CompletionKind::Value => Color32::from_rgb(180, 220, 120),
            CompletionKind::Property => Color32::from_rgb(255, 200, 100),
            CompletionKind::Field => Color32::from_rgb(150, 220, 200),
            CompletionKind::File => Color32::from_rgb(160, 180, 220),
        }
    }
}

// ---------------------------------------------------------------------------
// Syntax highlighting
// ---------------------------------------------------------------------------

impl Theme {
    /// Color for a syntax-highlighted token by kind. The eight kinds
    /// are kept distinct (with `Operator` and `Punctuation` sharing a
    /// muted gray family) so a `//@T` annotated line reads as a
    /// well-typed chain of tokens.
    #[allow(dead_code)]    pub fn token(kind: TokenKind) -> Color32 {
        match kind {
            TokenKind::Keyword => Color32::from_rgb(180, 130, 220),
            TokenKind::String => Color32::from_rgb(180, 220, 120),
            TokenKind::Number => Color32::from_rgb(220, 200, 120),
            TokenKind::Comment => Color32::from_gray(120),
            TokenKind::Function => Color32::from_rgb(120, 180, 240),
            TokenKind::Operator => Color32::from_gray(180),
            TokenKind::Punctuation => Color32::from_gray(170),
            TokenKind::Variable => Color32::from_rgb(220, 160, 100),
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics, test pass/fail
// ---------------------------------------------------------------------------

impl Theme {
    /// Color for a diagnostic severity row. `Info` and `Hint` share
    /// `Color32::GRAY` deliberately — they read as "informational"
    /// rather than as distinct severities.
    #[allow(dead_code)]    pub fn diagnostic_severity(severity: &str) -> Color32 {
        match severity {
            "error" => Color32::from_rgb(255, 80, 80),
            "warning" => Color32::from_rgb(255, 200, 50),
            "info" | "hint" => Color32::GRAY,
            _ => Color32::GRAY,
        }
    }

    /// Color for a passed / failed test indicator.
    #[allow(dead_code)]    pub fn test_pass(passed: bool) -> Color32 {
        if passed {
            Color32::from_rgb(80, 200, 120)
        } else {
            Color32::from_rgb(255, 80, 80)
        }
    }
}

// ---------------------------------------------------------------------------
// Find-match highlights
// ---------------------------------------------------------------------------

impl Theme {
    /// Background tint for a non-current find match. Used by the
    /// `egui::Painter` overlay drawn in `paint_find_highlights`.
    pub const FIND_MATCH_HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(255, 220, 0, 40);

    /// Background tint for the *current* find match (the one the
    /// cursor is sitting on). Slightly stronger so it pops out of
    /// the surrounding matches.
    pub const CURRENT_FIND_MATCH_HIGHLIGHT: Color32 =
        Color32::from_rgba_premultiplied(255, 220, 0, 90);

    /// Background tint used by the *layouter* for find-match
    /// highlights. Kept as a separate constant from
    /// `FIND_MATCH_HIGHLIGHT` so the layouter's color choice can
    /// evolve independently of the editor-painter's. Pinned by a
    /// regression test in `app.rs::tests`.
    pub const LAYOUT_FIND_MATCH_HIGHLIGHT: Color32 =
        Color32::from_rgba_premultiplied(255, 200, 0, 60);

    /// Current-match highlight used by the layouter.
    pub const LAYOUT_CURRENT_FIND_MATCH_HIGHLIGHT: Color32 =
        Color32::from_rgba_premultiplied(255, 140, 0, 100);
}

// ---------------------------------------------------------------------------
// Gutter
// ---------------------------------------------------------------------------

impl Theme {
    /// Right-border vertical rule between the gutter and the editor area.
    #[allow(dead_code)]    pub fn gutter_border() -> Color32 {
        Color32::from_gray(60)
    }

    /// Line-number text in the gutter.
    #[allow(dead_code)]    pub fn gutter_text() -> Color32 {
        Color32::from_gray(140)
    }

    /// Hovered line-number text in the gutter.
    #[allow(dead_code)]    pub fn gutter_text_hover() -> Color32 {
        Color32::from_gray(240)
    }
}

// ---------------------------------------------------------------------------
// Fold chevron (gutter)
// ---------------------------------------------------------------------------

/// Fold chevron color by fold kind. Distinct hues so a workflow-level
/// fold is visually different from a function-level fold.
pub fn fold_chevron(kind: &str) -> Color32 {
    match kind {
        "function" => Color32::from_rgb(120, 180, 255),
        "workflow" => Color32::from_rgb(255, 180, 120),
        _ => Color32::from_gray(160),
    }
}

/// Map a [`crate::folding::FoldKind`] to its string label so the
/// theme function above can match on a `&str` without importing the
/// folding module. The two are kept in sync by the editor's own
/// `fold_kind_label` callsite.
pub fn fold_kind_label(kind: crate::folding::FoldKind) -> &'static str {
    match kind {
        crate::folding::FoldKind::Function => "function",
        crate::folding::FoldKind::Workflow => "workflow",
    }
}

// ---------------------------------------------------------------------------
// Shortcuts window
// ---------------------------------------------------------------------------

impl Theme {
    /// Color for the chord-label column in the shortcuts help window.
    #[allow(dead_code)]    pub fn chord_label() -> Color32 {
        Color32::from_rgb(220, 220, 255)
    }
}

// ---------------------------------------------------------------------------
// Find-bar icons
// ---------------------------------------------------------------------------

impl Theme {
    /// Default icon color in the find bar (↑, ↓, ✕, etc.).
    #[allow(dead_code)]    pub fn find_icon() -> Color32 {
        Color32::from_gray(180)
    }

    /// Hovered icon color in the find bar.
    #[allow(dead_code)]    pub fn find_icon_hover() -> Color32 {
        Color32::from_gray(240)
    }

    /// Active (pressed) icon color in the find bar.
    #[allow(dead_code)]    pub fn find_icon_active() -> Color32 {
        Color32::from_rgb(100, 200, 255)
    }
}

// ---------------------------------------------------------------------------
// Shadows / chrome
// ---------------------------------------------------------------------------

impl Theme {
    /// Shadow alpha for the popup frame.
    pub const POPUP_SHADOW: Color32 = Color32::from_black_alpha(140);
}

// ---------------------------------------------------------------------------
// Search-in-files results
// ---------------------------------------------------------------------------

impl Theme {
    /// Accent color for a search-in-files hit. Used for the matched
    /// substring inside each result row.
    #[allow(dead_code)]    pub fn search_hit() -> Color32 {
        Color32::from_rgb(255, 220, 0)
    }
}

// ---------------------------------------------------------------------------
// Module shape
// ---------------------------------------------------------------------------

/// Marker type that namespaces the palette functions. Every `Theme::…`
/// call is a pure function; we never construct an instance.
pub struct Theme;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::HoverKind;

    #[test]
    fn hover_badge_is_distinct_per_kind() {
        let kinds = [
            HoverKind::Parameter,
            HoverKind::Event,
            HoverKind::Variable,
            HoverKind::Function,
            HoverKind::Type,
            HoverKind::Field,
            HoverKind::Error,
            HoverKind::Warning,
            HoverKind::Doc,
        ];
        let mut seen = std::collections::HashSet::new();
        for k in kinds {
            let c = Theme::hover_badge(k);
            assert!(seen.insert(c), "duplicate color for {:?}", k);
        }
    }

    #[test]
    fn type_color_is_distinct_for_primitives() {
        let names = ["number", "string", "bool", "null", "any", "user_type"];
        let mut seen = std::collections::HashSet::new();
        for n in names {
            let c = Theme::type_color(n);
            assert!(seen.insert(c), "duplicate color for {}", n);
        }
    }

    #[test]
    fn completion_is_distinct_per_kind() {
        use workflow_lsp::features::CompletionKind;
        let kinds = [
            CompletionKind::Keyword,
            CompletionKind::Function,
            CompletionKind::Variable,
            CompletionKind::Value,
            CompletionKind::Property,
            CompletionKind::Field,
            CompletionKind::File,
        ];
        let mut seen = std::collections::HashSet::new();
        for k in kinds {
            let c = Theme::completion(k);
            assert!(seen.insert(c), "duplicate color for {:?}", k);
        }
    }

    #[test]
    fn token_is_distinct_per_kind() {
        let kinds = [
            TokenKind::Keyword,
            TokenKind::String,
            TokenKind::Number,
            TokenKind::Comment,
            TokenKind::Function,
            TokenKind::Operator,
            TokenKind::Punctuation,
            TokenKind::Variable,
        ];
        let mut seen = std::collections::HashSet::new();
        for k in kinds {
            let c = Theme::token(k);
            assert!(seen.insert(c), "duplicate color for {:?}", token_name(k));
        }
    }

    fn token_name(k: crate::highlight::TokenKind) -> &'static str {
        match k {
            crate::highlight::TokenKind::Keyword => "Keyword",
            crate::highlight::TokenKind::String => "String",
            crate::highlight::TokenKind::Number => "Number",
            crate::highlight::TokenKind::Comment => "Comment",
            crate::highlight::TokenKind::Function => "Function",
            crate::highlight::TokenKind::Operator => "Operator",
            crate::highlight::TokenKind::Punctuation => "Punctuation",
            crate::highlight::TokenKind::Variable => "Variable",
        }
    }

    #[test]
    fn diagnostic_severity_error_and_warning_are_distinct() {
        assert_ne!(
            Theme::diagnostic_severity("error"),
            Theme::diagnostic_severity("warning"),
        );
    }

    #[test]
    fn test_pass_distinguishes_pass_and_fail() {
        assert_ne!(Theme::test_pass(true), Theme::test_pass(false));
    }

    #[test]
    fn find_highlight_constants_have_nonzero_alpha() {
        assert!(Theme::FIND_MATCH_HIGHLIGHT.a() > 0);
        assert!(Theme::CURRENT_FIND_MATCH_HIGHLIGHT.a() > 0);
        assert!(Theme::LAYOUT_FIND_MATCH_HIGHLIGHT.a() > 0);
        assert!(Theme::LAYOUT_CURRENT_FIND_MATCH_HIGHLIGHT.a() > 0);
    }

    #[test]
    fn find_highlight_painter_and_layouter_paired() {
        // Regression: the painter in `app.rs` and the layouter
        // previously diverged (different RGB, different alpha).
        // The two pairs are now distinct surfaces but each
        // should be > 0 alpha, and the current-match pair should
        // be stronger than the non-current pair on both sides.
        assert!(Theme::CURRENT_FIND_MATCH_HIGHLIGHT.a() > Theme::FIND_MATCH_HIGHLIGHT.a());
        assert!(
            Theme::LAYOUT_CURRENT_FIND_MATCH_HIGHLIGHT.a()
                > Theme::LAYOUT_FIND_MATCH_HIGHLIGHT.a()
        );
    }

    #[test]
    fn hover_event_chip_alpha_is_70() {
        let badge = Theme::hover_badge(HoverKind::Parameter);
        let chip = Theme::hover_event_chip_bg(badge);
        assert_eq!(chip.a(), 70);
    }

    #[test]
    fn hover_pill_alpha_is_50() {
        let color = Theme::type_color("number");
        let pill = Theme::hover_pill_bg(color);
        assert_eq!(pill.a(), 50);
    }
}
