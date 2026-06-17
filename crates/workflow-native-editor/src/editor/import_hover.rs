//! Import-line hover fast path.
//!
//! Imports (`@import NAME from "<path>"` / `import NAME from {...}`)
//! get a custom hover that surfaces the resolved schema rather
//! than the generic "imported binding X" text.
//!
//! The free functions here do all the parsing and LSP lookups
//! without ever touching `EditorApp`'s private fields. The thin
//! shim methods on `EditorApp` (in `app.rs`) read the fields and
//! forward the call. That keeps the field privacy tight while
//! still splitting the implementation across files.

use workflow_lsp::ServerState;

use super::super::popup;

/// A lightweight view of an import line, used by the import-hover
/// fast path. Keeps the per-line parser logic out of the editor's
/// hot loop.
#[derive(Debug, Clone)]
pub(crate) struct ImportLine {
    pub(crate) name: String,
    pub(crate) source: ImportSourceLine,
    /// The trimmed line text — kept for diagnostic messages and
    /// future hover variants.
    #[allow(dead_code)]
    pub(crate) line_text: String,
    /// Byte offset (in `text`) of the first non-whitespace
    /// byte on the import line. Used as the lookup position for
    /// the scope index so the import binding is guaranteed to be
    /// in scope (`decl_span.start <= byte_offset`).
    pub(crate) byte_offset: usize,
}

/// What follows `import NAME from` on an import line. Strings are
/// paths or URLs (depending on prefix); inline objects are
/// represented as a unit variant — we only need to distinguish
/// "path-like" from "inline" for the hover copy.
#[derive(Debug, Clone)]
pub(crate) enum ImportSourceLine {
    Path(String),
    Inline,
}

/// Pure helper: if `line_idx` of `text` is an `@import` /
/// `import` line, return the parsed `(name, source)` pair.
/// Returns `None` for every other line.
pub(crate) fn import_at_line(text: &str, line_idx: usize) -> Option<ImportLine> {
    let line = text.split('\n').nth(line_idx)?;
    let trimmed = line.trim_start();
    let body = trimmed
        .strip_prefix("@import ")
        .or_else(|| trimmed.strip_prefix("import "))?;
    // Body shape: `NAME from <source>` where source is either a
    // quoted string or an inline `{...}` object.
    let (name, rest) = body.split_once(' ')?;
    if name.is_empty() || !name.chars().all(is_ident_char) {
        return None;
    }
    let after_name = rest.trim_start();
    let after_from = after_name.strip_prefix("from ")?.trim_start();
    let source = parse_import_source(after_from);
    // The binding becomes visible at its `decl_span.start`, so
    // use the byte offset of the first non-whitespace byte on
    // this line as the lookup position. That guarantees the
    // import binding is in scope no matter where on the line
    // the user is hovering.
    let leading_ws = line.len() - trimmed.len();
    let byte_offset = byte_offset_of_line(text, line_idx, leading_ws);
    Some(ImportLine {
        name: name.to_string(),
        source,
        line_text: trimmed.to_string(),
        byte_offset,
    })
}

/// Build a [`popup::HoverContent`] for an import line. The
/// signature is the resolved schema (rendered as a type table);
/// the docs surface the source path so the user can see where
/// the values come from.
pub(crate) fn build_import_hover(
    lsp: &ServerState,
    uri: &str,
    text: &str,
    import: &ImportLine,
) -> popup::HoverContent {
    let binding = lsp
        .get_inference(uri)
        .and_then(|inf| inf.lookup_at_offset(text, import.byte_offset, &import.name));
    let schema_expr = binding
        .as_ref()
        .map(|b| popup::type_to_type_expr(&b.ty))
        .unwrap_or_else(|| popup::TypeExpr::Name("any".into()));
    let source_path = match &import.source {
        ImportSourceLine::Path(p) => Some(p.clone()),
        ImportSourceLine::Inline => None,
    };
    popup::HoverContent::for_import(&import.name, &schema_expr, source_path.as_deref())
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Byte offset of the byte at column `col` (in characters, not
/// bytes) on the line `line_idx` (0-based). Used by the import
/// hover to query the scope index at a position guaranteed to be
/// inside the import's `decl_span`. The implementation walks
/// the source line-by-line so it stays correct for multi-byte
/// UTF-8 content.
fn byte_offset_of_line(source: &str, line_idx: usize, col: usize) -> usize {
    let mut current_line = 0usize;
    let mut offset_at_line_start = 0usize;
    for (i, ch) in source.char_indices() {
        if current_line == line_idx {
            // Walk to the requested column.
            let chars_before = source[offset_at_line_start..i].chars().count();
            if chars_before >= col {
                return i;
            }
        }
        if ch == '\n' {
            if current_line == line_idx {
                // Past the end of the requested line.
                return i;
            }
            current_line += 1;
            offset_at_line_start = i + 1;
        }
    }
    // Past the end of the source.
    source.len()
}

/// Parse the source half of an import line. The grammar accepts a
/// quoted string (path or URL) or an inline `{...}` JSON object;
/// we return whichever shape we see, with the quotes stripped from
/// the string form.
fn parse_import_source(s: &str) -> ImportSourceLine {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return ImportSourceLine::Path(inner.to_string());
    }
    if let Some(inner) = s.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        return ImportSourceLine::Path(inner.to_string());
    }
    if s.starts_with('{') {
        return ImportSourceLine::Inline;
    }
    // Fallback: treat as a bare path so the user still sees
    // *something* in the hover source line.
    ImportSourceLine::Path(s.to_string())
}

// `update_hover` lives in `app.rs` because it threads together
// the cursor position, the galley, and the LSP lookup. The
// import-fast-path branch reads its data from `import_at_line` /
// `build_import_hover` (above) and writes the result back into
// `self.hover_text` / `self.hover_pos`. Keeping the dispatcher
// in `app.rs` keeps the egui types and the `self` mut borrow
// in the file that already has them.

#[cfg(test)]
mod tests {
    use super::{parse_import_source, ImportSourceLine};
    use crate::editor::import_hover::import_at_line;

    #[test]
    fn import_at_line_recognizes_at_import_keyword() {
        let text = "@import USER_REGISTERED from \"./user_registered.json\"\n";
        let line = import_at_line(text, 0).expect("expected an import line to be detected");
        assert_eq!(line.name, "USER_REGISTERED");
        assert!(matches!(
            line.source,
            ImportSourceLine::Path(ref p) if p == "./user_registered.json"
        ));
    }

    #[test]
    fn import_at_line_recognizes_plain_import_keyword() {
        let text = "import utils from \"./shared_utils.flow\"\n";
        let line = import_at_line(text, 0).expect("expected an import line to be detected");
        assert_eq!(line.name, "utils");
        assert!(matches!(
            line.source,
            ImportSourceLine::Path(ref p) if p == "./shared_utils.flow"
        ));
    }

    #[test]
    fn import_at_line_returns_none_for_non_import_lines() {
        let text = "workflow \"W\" { on E\n  log(1)\n}\n";
        for idx in 0..3 {
            assert!(
                import_at_line(text, idx).is_none(),
                "line {idx} should not look like an import"
            );
        }
    }

    #[test]
    fn import_at_line_handles_inline_object_source() {
        let text = "@import EVT from { id: 1, name: \"x\" }\n";
        let line = import_at_line(text, 0).expect("import line");
        assert_eq!(line.name, "EVT");
        assert!(matches!(line.source, ImportSourceLine::Inline));
    }

    #[test]
    fn parse_import_source_strips_double_quotes() {
        match parse_import_source("  \"./schema.json\"  ") {
            ImportSourceLine::Path(p) => assert_eq!(p, "./schema.json"),
            other => panic!("expected Path, got {:?}", other),
        }
    }

    #[test]
    fn parse_import_source_strips_single_quotes() {
        match parse_import_source("'./schema.json'") {
            ImportSourceLine::Path(p) => assert_eq!(p, "./schema.json"),
            other => panic!("expected Path, got {:?}", other),
        }
    }

    #[test]
    fn parse_import_source_recognises_inline_object() {
        assert!(matches!(
            parse_import_source("{ a: 1 }"),
            ImportSourceLine::Inline
        ));
    }

    #[test]
    fn parse_import_source_treats_bare_text_as_path() {
        match parse_import_source("./bare.json") {
            ImportSourceLine::Path(p) => assert_eq!(p, "./bare.json"),
            other => panic!("expected Path, got {:?}", other),
        }
    }

    // The two `build_import_hover_*` tests below need direct
    // access to `EditorApp`'s private fields (`text`, `lsp`,
    // `uri`, `file_path`) to seed the LSP state, so they live
    // in `app.rs` next to the `super::*` shim methods that read
    // those fields. Keeping them here would require either
    // leaking `pub(crate)` accessors or duplicating the
    // shim-then-test pattern. Centralising the LSP-seeded
    // tests in `app.rs` keeps the field-touching surface in
    // one place.
}
