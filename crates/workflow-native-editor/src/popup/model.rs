//! Data model for the hover popup.
//!
//! The hover popup is fed a [`HoverContent`] describing what to show
//! (title, badge, signature, body, optional event chip). The renderer
//! in `hover.rs` is a pure function of that struct, so callers can
//! build one from a non-LSP source and reuse the same UI.
//!
//! `from_markdown` is the adapter that ingests the existing LSP
//! `MarkupKind::Markdown` blob without changing the LSP protocol:
//! we split on blank lines, take the first paragraph as the title,
//! classify the rest by content, and let the renderer apply the
//! colors.

use super::type_parser::TypeParser;
use eframe::egui::Color32;
use crate::theme::Theme;

// ---------------------------------------------------------------------------
// HoverKind
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
    pub(crate) fn badge(&self) -> &'static str {
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

    /// One-sentence doc for the kind, surfaced as a muted italic
    /// line in the hover body.
    #[allow(dead_code)]
    pub(crate) fn doc(&self) -> Option<&'static str> {
        Some(match self {
            Self::Parameter => "A workflow parameter bound to the event trigger.",
            Self::Event => "An event this workflow listens for or emits.",
            Self::Variable => "A local or imported variable in scope.",
            Self::Function => "A callable function (built-in or user-defined).",
            Self::Type => "A type expression from the workflow type DSL.",
            Self::Field => "A field on a record/object type.",
            Self::Error => "An error diagnostic surfaced by the LSP or the engine.",
            Self::Warning => "A warning diagnostic (deprecated usage, likely bug, etc.).",
            Self::Doc => "A documentation entry — for symbols with no better kind.",
        })
    }

    /// Glyph prefix shown before the title (monospace, colored).
    pub(crate) fn glyph(&self) -> &'static str {
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

// ---------------------------------------------------------------------------
// Type expressions
// ---------------------------------------------------------------------------

/// A parsed type expression from the workflow type DSL. See
/// `type_parser.rs` for the grammar.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeExpr {
    /// A named primitive or alias: `number`, `string`, `bool`,
    /// `null`, `any`, or a user-defined type name.
    Name(String),
    /// `T[]` — a homogeneous array of `T`.
    Array(Box<TypeExpr>),
    /// `{ name: T, name: T, ... }` — a record type.
    Object(Vec<TypeField>),
    /// `(name: T, ...) -> T` — a function signature.
    Func {
        params: Vec<TypeField>,
        ret: Box<TypeExpr>,
    },
}

/// A single `(name, type)` field in an object or function signature.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeField {
    pub name: String,
    pub ty: TypeExpr,
}

// ---------------------------------------------------------------------------
// HoverSignature
// ---------------------------------------------------------------------------

/// The "signature" slot of a hover popup. The renderer dispatches on
/// the variant so a `//@T` type annotation becomes a *field table*,
/// not a code-formatted comment.
#[derive(Clone, Debug)]
pub enum HoverSignature {
    /// Plain monospace text — function names, short call sigs, etc.
    Text(String),
    /// A parsed type expression — usually an event schema, a binding
    /// type, or a function return type. Rendered as a field table
    /// when it's an object, with a header chip for arrays and
    /// primitive type pills.
    Type(TypeExpr),
}

// ---------------------------------------------------------------------------
// HoverContent
// ---------------------------------------------------------------------------

/// Structured payload for a hover popup.
#[derive(Clone, Debug)]
pub struct HoverContent {
    /// Bold title shown in the header next to the badge.
    pub title: String,
    /// Optional structured signature (type table / monospace text).
    pub signature: Option<HoverSignature>,
    /// Optional body, rendered with the mini-markdown parser.
    pub docs: Option<String>,
    /// Optional event reference, surfaced as a small chip in the header.
    /// Parsed out of the LSP body when it shows the literal pattern
    /// `(event "FOO")`.
    pub event: Option<String>,
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
            event: None,
            kind,
        }
    }

    #[allow(dead_code)]
    pub fn with_signature(mut self, sig: impl Into<String>) -> Self {
        self.signature = Some(HoverSignature::Text(sig.into()));
        self
    }

    #[allow(dead_code)]
    pub fn with_typed_signature(mut self, ty: TypeExpr) -> Self {
        self.signature = Some(HoverSignature::Type(ty));
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
                event: None,
                kind: HoverKind::Doc,
            };
        }

        let title = paras.remove(0).trim().to_string();
        let kind = classify_title(&title);

        // Scan every remaining paragraph once, looking for the
        // (event "X") reference, a signature-shaped paragraph, and
        // de-duplicated docs. This avoids losing the event when it
        // happens to live in the same paragraph as the signature.
        let mut event: Option<String> = None;
        let mut signature: Option<HoverSignature> = None;
        let mut body_start: Option<usize> = None;

        for (i, p) in paras.iter().enumerate() {
            let stripped = strip_markdown(p);
            if event.is_none() {
                if let Some(name) = extract_event_ref(&stripped) {
                    event = Some(name);
                }
            }
            if signature.is_none() && body_start.is_none() {
                let t = p.trim();
                let looks_like_signature = t.starts_with('(')
                    || t.starts_with('`')
                    || t.starts_with("//")
                    || t.starts_with("**returns:**")
                    || t.starts_with("**type:**")
                    || t.starts_with("**params:**")
                    || t.starts_with("**value:**");
                if looks_like_signature && t.len() <= 200 {
                    let cleaned = stripped.trim().trim_start_matches("//@").trim().to_string();
                    signature = Some(classify_signature(&cleaned));
                    // The signature paragraph still counts as the
                    // start of the body for docs purposes (so the
                    // signature is *also* visible in the body, in
                    // case the renderer is asked to display it
                    // there).
                    body_start = Some(i);
                }
            }
        }

        let body_start = body_start.map(|i| i + 1).unwrap_or(0);
        let (_, docs) = extract_event_and_docs(&paras[body_start..], &title);

        Self {
            title: strip_markdown(&title),
            signature,
            docs,
            event,
            kind,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Try to parse the trimmed signature string as a `TypeExpr`. Falls
/// back to `HoverSignature::Text` when parsing fails.
fn classify_signature(cleaned: &str) -> HoverSignature {
    let mut p = TypeParser::new(cleaned);
    match p.parse_type_expr() {
        Ok(ty) if p.at_end() => HoverSignature::Type(ty),
        _ => HoverSignature::Text(cleaned.to_string()),
    }
}

/// Extract an `(event "FOO")` reference from the body if present,
/// returning the de-duplicated body. The body is considered a
/// duplicate-of-title if the title phrase appears in the first
/// non-empty paragraph.
fn extract_event_and_docs(paras: &[String], title: &str) -> (Option<String>, Option<String>) {
    if paras.is_empty() {
        return (None, None);
    }
    let mut event: Option<String> = None;
    let title_key = extract_title_key(title);
    let mut kept: Vec<String> = Vec::with_capacity(paras.len());

    for p in paras {
        let stripped = strip_markdown(p);
        // Try to find `(event "FOO")` anywhere in the paragraph.
        if event.is_none() {
            if let Some(name) = extract_event_ref(&stripped) {
                event = Some(name);
            }
        }
        // Drop paragraphs that just re-state the title.
        if !title_key.is_empty() && starts_with_case_insensitive(&stripped, &title_key) {
            continue;
        }
        // Drop paragraphs that are now just an event reference (we
        // already lifted it out).
        if event.is_some()
            && stripped.trim() == format!("(event \"{}\")", event.as_deref().unwrap_or(""))
        {
            continue;
        }
        if stripped.trim().is_empty() {
            continue;
        }
        kept.push(p.clone());
    }

    let docs = if kept.is_empty() {
        None
    } else {
        Some(kept.join("\n\n"))
    };
    (event, docs)
}

fn extract_title_key(title: &str) -> String {
    // e.g. `Parameter of workflow "Nested Loops"` -> `parameter of workflow`
    let lower = title.to_ascii_lowercase();
    if lower.strip_prefix("parameter of workflow ").is_some() {
        return "parameter of workflow".to_string();
    }
    if lower.strip_prefix("parameter of ").is_some() {
        return "parameter of".to_string();
    }
    String::new()
}

fn starts_with_case_insensitive(s: &str, prefix: &str) -> bool {
    if prefix.is_empty() || s.len() < prefix.len() {
        return false;
    }
    s.to_ascii_lowercase().starts_with(prefix)
}

pub(crate) fn extract_event_ref(s: &str) -> Option<String> {
    let needle = "(event \"";
    let idx = s.find(needle)?;
    let after = &s[idx + needle.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
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
pub(crate) fn strip_markdown(s: &str) -> String {
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
        assert_eq!(h.event.as_deref(), Some("NESTED_DATA"));
        assert_eq!(h.kind, HoverKind::Parameter);
    }

    #[test]
    fn from_markdown_promotes_code_to_signature() {
        let md = "my_func\n\n`//@number`\n\n**params:** `(x: number)`\n\ndoes the thing";
        let h = HoverContent::from_markdown(md);
        assert_eq!(h.title, "my_func");
        match &h.signature {
            Some(HoverSignature::Type(TypeExpr::Name(n))) if n == "number" => {}
            other => panic!("expected Type(Name(\"number\")), got {:?}", other),
        }
        assert!(h.docs.as_deref().unwrap().contains("params"));
        assert!(h.docs.as_deref().unwrap().contains("does the thing"));
    }

    #[test]
    fn from_markdown_event_schema_becomes_field_table() {
        let md = "Parameter of workflow \"Nested Loops\"\n\n\
                  //@{ id: number, name: string, orders: { id: number, total: number }[] }[]\n\n\
                  Parameter of workflow \"Nested Loops\" (event \"NESTED_DATA\")";
        let h = HoverContent::from_markdown(md);
        assert_eq!(h.title, "Parameter of workflow \"Nested Loops\"");
        assert_eq!(h.event.as_deref(), Some("NESTED_DATA"));
        match &h.signature {
            Some(HoverSignature::Type(TypeExpr::Array(inner))) => match inner.as_ref() {
                TypeExpr::Object(fields) => {
                    assert_eq!(fields.len(), 3);
                    assert_eq!(fields[0].name, "id");
                    assert_eq!(fields[0].ty, TypeExpr::Name("number".into()));
                    assert_eq!(fields[1].name, "name");
                    assert_eq!(fields[1].ty, TypeExpr::Name("string".into()));
                    assert_eq!(fields[2].name, "orders");
                    match &fields[2].ty {
                        TypeExpr::Array(inner2) => match inner2.as_ref() {
                            TypeExpr::Object(of) => assert_eq!(of.len(), 2),
                            _ => panic!("expected object inside orders[]"),
                        },
                        _ => panic!("expected Array for orders"),
                    }
                }
                _ => panic!("expected Object inside Array"),
            },
            other => panic!("expected Array(Object) signature, got {:?}", other),
        }
        assert!(
            h.docs.is_none(),
            "expected docs to be de-duplicated, got {:?}",
            h.docs
        );
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
        let mut colors: Vec<Color32> = kinds.iter().map(|k| Theme::hover_badge(*k)).collect();
        colors.dedup();
        assert!(
            colors.len() >= kinds.len() - 1,
            "badges should be visually distinct"
        );
    }

    #[test]
    fn classify_signature_promotes_typed_input() {
        match classify_signature("{ id: number, name: string }") {
            HoverSignature::Type(TypeExpr::Object(fields)) => assert_eq!(fields.len(), 2),
            other => panic!("expected Type(Object), got {:?}", other),
        }
        match classify_signature("(event \"FOO\")") {
            HoverSignature::Text(s) => assert_eq!(s, "(event \"FOO\")"),
            other => panic!("expected Text, got {:?}", other),
        }
        match classify_signature("a regular call signature") {
            HoverSignature::Text(s) => assert_eq!(s, "a regular call signature"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn extract_event_ref_picks_name_out_of_paragraph() {
        let s = "Parameter of workflow \"X\" (event \"NESTED_DATA\")";
        assert_eq!(extract_event_ref(s).as_deref(), Some("NESTED_DATA"));
        assert_eq!(extract_event_ref("no event here"), None);
    }
}
