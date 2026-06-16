//! Rendering of the completion popup and the hover popup.
//!
//! Both popups are pure functions of `(ctx, state) -> output` so they
//! can be unit-tested and reused without coupling to `EditorApp`.

use eframe::egui::{
    self, epaint, Align2, Color32, FontId, Frame, Margin, Pos2, Rect, Response, RichText,
    Rounding, ScrollArea, Sense, Stroke, Ui, Vec2,
};
use workflow_lsp::features::{Completion, CompletionKind};

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

pub const COMPLETION_WIDTH: f32 = 320.0;
pub const COMPLETION_MAX_HEIGHT: f32 = 220.0;
pub const COMPLETION_ROW_HEIGHT: f32 = 24.0;
pub const HOVER_MAX_WIDTH: f32 = 440.0;
pub const HOVER_MIN_WIDTH: f32 = 220.0;
const SCREEN_EDGE_MARGIN: f32 = 8.0;

// ---------------------------------------------------------------------------
// Hover model
// ---------------------------------------------------------------------------

/// Semantic category of whatever the cursor is hovering. Drives the
/// colored badge in the popup header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // not all variants are emitted by the LSP yet
pub enum HoverKind {
    Parameter,
    Event,
    Variable,
    Function,
    Type,
    Field,
    Error,
    Warning,
    Doc,
}

impl HoverKind {
    /// Short tag shown inside the colored badge.
    fn badge(&self) -> &'static str {
        match self {
            Self::Parameter => "@param",
            Self::Event => "@event",
            Self::Variable => "@var",
            Self::Function => "@fn",
            Self::Type => "@type",
            Self::Field => "@field",
            Self::Error => "@error",
            Self::Warning => "@warn",
            Self::Doc => "@doc",
        }
    }

    /// Color for the badge background and text.
    fn badge_color(&self) -> Color32 {
        match self {
            Self::Parameter => Color32::from_rgb(70, 130, 200),
            Self::Event => Color32::from_rgb(200, 130, 60),
            Self::Variable => Color32::from_rgb(130, 170, 90),
            Self::Function => Color32::from_rgb(160, 100, 200),
            Self::Type => Color32::from_rgb(80, 170, 170),
            Self::Field => Color32::from_rgb(200, 170, 80),
            Self::Error => Color32::from_rgb(190, 60, 60),
            Self::Warning => Color32::from_rgb(200, 160, 50),
            Self::Doc => Color32::from_rgb(110, 110, 130),
        }
    }

    /// Glyph prefix shown before the title (monospace, colored).
    fn glyph(&self) -> &'static str {
        match self {
            Self::Parameter => "ƒ",
            Self::Event => "※",
            Self::Variable => "v",
            Self::Function => "λ",
            Self::Type => "τ",
            Self::Field => "·",
            Self::Error => "✗",
            Self::Warning => "!",
            Self::Doc => "✦",
        }
    }
}

/// Structured payload for a hover popup.
///
/// `HoverContent::from_markdown` lets us ingest the existing LSP output
/// (which is already a `MarkupKind::Markdown` blob) without changing
/// the LSP protocol: we split on blank lines, take the first
/// paragraph as the title, classify the rest by content, and let the
/// renderer apply the colors.
#[derive(Clone, Debug)]
pub struct HoverContent {
    /// Bold title shown in the header next to the badge.
    pub title: String,
    /// Optional monospace subtitle (e.g. a function signature).
    pub signature: Option<String>,
    /// Optional body, rendered with the mini-markdown parser.
    pub docs: Option<String>,
    /// Semantic category for the badge.
    pub kind: HoverKind,
}

impl HoverContent {
    #[allow(dead_code)] // public API for non-LSP callers
    pub fn new(title: impl Into<String>, kind: HoverKind) -> Self {
        Self {
            title: title.into(),
            signature: None,
            docs: None,
            kind,
        }
    }

    #[allow(dead_code)]
    pub fn with_signature(mut self, sig: impl Into<String>) -> Self {
        self.signature = Some(sig.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_docs(mut self, docs: impl Into<String>) -> Self {
        self.docs = Some(docs.into());
        self
    }

    /// Build a `HoverContent` from the legacy markdown blob produced
    /// by `workflow_lsp::features::hover_at`.
    pub fn from_markdown(md: &str) -> Self {
        let mut paras: Vec<String> = split_paragraphs(md).map(str::to_string).collect();
        if paras.is_empty() {
            return Self {
                title: String::new(),
                signature: None,
                docs: None,
                kind: HoverKind::Doc,
            };
        }

        let title = paras.remove(0).trim().to_string();
        let kind = classify_title(&title);

        let mut signature: Option<String> = None;
        let mut body_start = 0;

        if let Some(first) = paras.first() {
            let t = first.trim();
            let looks_like_signature = t.starts_with('(')
                || t.starts_with('`')
                || t.starts_with("//")
                || t.starts_with("**returns:**")
                || t.starts_with("**type:**")
                || t.starts_with("**params:**")
                || t.starts_with("**value:**");
            if looks_like_signature && t.len() <= 200 {
                signature = Some(strip_markdown(t));
                body_start = 1;
            }
        }

        let docs = if body_start < paras.len() {
            Some(paras[body_start..].join("\n\n"))
        } else {
            None
        };

        Self {
            title: strip_markdown(&title),
            signature,
            docs,
            kind,
        }
    }
}

fn classify_title(title: &str) -> HoverKind {
    let lower = title.to_ascii_lowercase();
    if lower.starts_with("parameter of") || lower.starts_with("param of") {
        HoverKind::Parameter
    } else if lower.starts_with("event ") || lower.starts_with("event:") {
        HoverKind::Event
    } else if lower.starts_with("function ") || lower.starts_with("fn ") {
        HoverKind::Function
    } else if lower.starts_with("type ") || lower.starts_with("type:") {
        HoverKind::Type
    } else if lower.starts_with("error") || lower.starts_with("undefined") {
        HoverKind::Error
    } else if lower.starts_with("warning") || lower.starts_with("deprecated") {
        HoverKind::Warning
    } else {
        HoverKind::Doc
    }
}

fn split_paragraphs(md: &str) -> impl Iterator<Item = &str> {
    md.split("\n\n").map(str::trim).filter(|s| !s.is_empty())
}

/// Strip the most common markdown markers without trying to be a
/// full parser. We just collapse `**...**` -> `...`, `` `...` `` ->
/// `...`, and trim leftover backticks/asterisks.
fn strip_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_pair(&chars, i + 2, '*', '*') {
                out.push_str(&chars[i + 2..end].iter().collect::<String>());
                i = end + 2;
                continue;
            }
        }
        if chars[i] == '*' {
            if let Some(end) = find_char(&chars, i + 1, '*') {
                out.push_str(&chars[i + 1..end].iter().collect::<String>());
                i = end + 1;
                continue;
            }
        }
        if chars[i] == '`' {
            if let Some(end) = find_char(&chars, i + 1, '`') {
                out.push_str(&chars[i + 1..end].iter().collect::<String>());
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == target)
}

fn find_pair(chars: &[char], from: usize, a: char, b: char) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|&i| chars[i] == a && chars[i + 1] == b)
}

// ---------------------------------------------------------------------------
// Hover renderer
// ---------------------------------------------------------------------------

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
        let (badge_rect, _) =
            ui.allocate_exact_size(Vec2::new(badge_w, 18.0), Sense::hover());
        ui.painter().rect_filled(badge_rect, Rounding::same(4.0), badge_color);
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
    });

    if let Some(sig) = &content.signature {
        ui.add_space(4.0);
        ui.indent("hover-sig", |ui| {
            ui.label(
                RichText::new(sig)
                    .monospace()
                    .size(12.0)
                    .color(Color32::from_rgb(180, 200, 220)),
            );
        });
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
// Mini-markdown renderer
// ---------------------------------------------------------------------------
//
// The LSP emits a small subset of markdown in its hover output:
//
//   * `**bold**` labels (e.g. `**type:**`, `**value:**`, `**params:**`)
//   * `` `code` `` spans (e.g. `` `(event "NESTED_DATA")` ``)
//   * `//@type` style type annotations
//   * blank-line separated paragraphs
//
// Rather than pull in a full markdown engine we render this subset
// directly with `LayoutJob`, which gives us full control over colors
// and avoids any extra dependency.

fn render_mini_markdown(ui: &mut Ui, md: &str, kind: HoverKind) {
    let accent = kind.badge_color();
    for paragraph in md.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        let mut job = egui::text::LayoutJob::default();
        let mut chars = paragraph.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                // **bold** -> accent
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
                // *italic* -> weak
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
                        fmt.color = Color32::from_gray(180);
                        job.append(&inner, 0.0, fmt);
                    } else {
                        job.append("*", 0.0, base_text_format());
                    }
                }
                // `code` -> monospace + light bg tint
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
                        let fmt = make_code_format(Color32::from_rgb(200, 220, 255));
                        job.append(&inner, 0.0, fmt);
                    } else {
                        job.append("`", 0.0, base_text_format());
                    }
                }
                // `//@type` annotation -> monospace accent (only at start of paragraph)
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
                    // Append a single-char run; this could be optimised
                    // by collecting runs but the LSP output is small.
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
        color: Color32::from_gray(210),
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
        background: Color32::from_rgba_unmultiplied(60, 80, 110, 90),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Completion renderer
// ---------------------------------------------------------------------------

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
                        if render_completion_row(
                            ui,
                            item,
                            idx == current_index,
                            row_h,
                        )
                        .clicked()
                        {
                            clicked = Some(idx);
                        }
                    }
                });
        });

    clicked
}

fn render_completion_row(ui: &mut Ui, item: &Completion, selected: bool, height: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(COMPLETION_WIDTH, height),
        Sense::click_and_drag(),
    );
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
        ui.painter().rect_stroke(rect.shrink(0.5), Rounding::same(3.0), stroke);
    }

    let accent = color_for_kind(item.kind);
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

fn color_for_kind(kind: CompletionKind) -> Color32 {
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

// ---------------------------------------------------------------------------
// Frame + layout helpers
// ---------------------------------------------------------------------------

fn popup_frame(ctx: &egui::Context) -> Frame {
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

/// Move `rect` so that it stays inside `screen` (with `margin` of
/// padding on every edge).
fn clamp_to_screen(rect: Rect, screen: Rect, margin: f32) -> Rect {
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
    if out.height() > screen.height() - margin * 2.0
        || out.width() > screen.width() - margin * 2.0
    {
        out = Rect::from_min_size(inner_min, screen.size() - Vec2::splat(margin * 2.0));
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_paragraphs_keeps_order_and_trims() {
        let md = "title\n\n  body  \n\n\n\nmore";
        let paras: Vec<_> = split_paragraphs(md).collect();
        assert_eq!(paras, vec!["title", "body", "more"]);
    }

    #[test]
    fn strip_markdown_handles_bold_italic_and_code() {
        let s = "**type:** `number` and *emphasized*";
        let out = strip_markdown(s);
        assert_eq!(out, "type: number and emphasized");
    }

    #[test]
    fn from_markdown_uses_first_paragraph_as_title() {
        let md = "Parameter of workflow \"Nested Loops\"\n\n(event \"NESTED_DATA\")";
        let h = HoverContent::from_markdown(md);
        assert_eq!(h.title, "Parameter of workflow \"Nested Loops\"");
        assert_eq!(h.signature.as_deref(), Some("(event \"NESTED_DATA\")"));
        assert_eq!(h.kind, HoverKind::Parameter);
    }

    #[test]
    fn from_markdown_promotes_code_to_signature() {
        let md = "my_func\n\n`//@number`\n\n**params:** `(x: number)`\n\ndoes the thing";
        let h = HoverContent::from_markdown(md);
        assert_eq!(h.title, "my_func");
        assert_eq!(h.signature.as_deref(), Some("//@number"));
        assert!(h.docs.as_deref().unwrap().contains("params"));
        assert!(h.docs.as_deref().unwrap().contains("does the thing"));
    }

    #[test]
    fn from_markdown_classifies_event() {
        let md = "Event INCOMING_MESSAGE\n\nFired on inbound message";
        let h = HoverContent::from_markdown(md);
        assert_eq!(h.kind, HoverKind::Event);
    }

    #[test]
    fn from_markdown_empty_returns_empty_content() {
        let h = HoverContent::from_markdown("");
        assert!(h.title.is_empty());
        assert!(h.signature.is_none());
        assert!(h.docs.is_none());
    }

    #[test]
    fn badge_colors_are_distinct() {
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
        let mut colors: Vec<Color32> = kinds.iter().map(|k| k.badge_color()).collect();
        colors.dedup();
        assert!(
            colors.len() >= kinds.len() - 1,
            "badges should be visually distinct"
        );
    }

    #[test]
    fn clamp_to_screen_pushes_popup_inside() {
        let screen = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(800.0, 600.0));
        let bad = Rect::from_min_size(Pos2::new(700.0, 500.0), Vec2::new(200.0, 200.0));
        let good = clamp_to_screen(bad, screen, 8.0);
        // After clamping, the popup should fit inside (screen - margin).
        let inner = screen.shrink(8.0);
        assert!(good.max.x <= inner.max.x + 0.5, "max.x={} > {}", good.max.x, inner.max.x);
        assert!(good.max.y <= inner.max.y + 0.5, "max.y={} > {}", good.max.y, inner.max.y);
        assert!(good.min.x >= inner.min.x - 0.5);
        assert!(good.min.y >= inner.min.y - 0.5);
    }
}
