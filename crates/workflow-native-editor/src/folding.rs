//! Code-folding regions: detect `fn` and `workflow` blocks and apply
//! collapse/expand to a source string.
//!
//! Folding is detected by scanning the source line-by-line and matching
//! braces — Flow blocks always have a `{` on the header line and a
//! matching `}` at the end. We don't reuse the parser for this because
//! the editor needs to keep folding working on partially-broken source
//! (mid-typing, syntax errors, etc.).

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldKind {
    Function,
    Workflow,
}

#[derive(Debug, Clone)]
pub struct FoldRegion {
    pub kind: FoldKind,
    /// 0-based line of the block's opening (`fn ...` or `workflow ...`).
    pub start_line: usize,
    /// 0-based line of the matching closing `}`.
    pub end_line: usize,
    /// 1-based lines folded (for the placeholder text).
    pub body_lines: usize,
    /// Header text for the placeholder, e.g. `fn double` or `workflow "X"`.
    pub header: String,
}

impl FoldRegion {
    /// A stable id for the region: its starting line. This stays valid
    /// across edits that don't change the relative position of the
    /// block's opening line; if the user adds/removes lines above the
    /// block, the id is invalidated and the fold is dropped.
    pub fn id(&self) -> usize {
        self.start_line
    }
}

/// Walk the source and return every foldable region. A region is a line
/// that starts with `fn ` or `workflow "..."` and contains an opening
/// `{` whose matching `}` we can find.
pub fn detect_folds(source: &str) -> Vec<FoldRegion> {
    let mut regions = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let (kind, header) = if let Some(rest) = trimmed.strip_prefix("fn ") {
            let name = rest
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("")
                .to_string();
            if name.is_empty() || !line.contains('{') {
                continue;
            }
            (FoldKind::Function, format!("fn {}", name))
        } else if trimmed.starts_with("workflow ") {
            if !line.contains('{') {
                continue;
            }
            // Pull the workflow name out of the header — the source is
            // `workflow "Name" {`, so we grab the quoted string.
            let name = trimmed
                .trim_start_matches("workflow ")
                .split('{')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            (FoldKind::Workflow, format!("workflow {}", name))
        } else {
            continue;
        };
        if let Some(end) = find_matching_brace(&lines, i) {
            // A 1-line block (open and close on the same line) isn't
            // worth folding.
            if end <= i {
                continue;
            }
            regions.push(FoldRegion {
                kind,
                start_line: i,
                end_line: end,
                body_lines: end - i - 1,
                header,
            });
        }
    }
    regions
}

/// Find the line index of the `}` that closes the `{` on `start_line`.
/// Returns `None` if we can't find a matching close before EOF.
fn find_matching_brace(lines: &[&str], start_line: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut found_open = false;
    for (i, line) in lines.iter().enumerate().skip(start_line) {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    found_open = true;
                }
                '}' => {
                    depth -= 1;
                    if found_open && depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Take the source text and a set of collapsed region ids (start lines),
/// and return a new string where the body of each collapsed region has
/// been replaced with a single placeholder line.
pub fn apply_folds(source: &str, collapsed: &BTreeSet<usize>) -> String {
    if collapsed.is_empty() {
        return source.to_string();
    }
    let regions = detect_folds(source);
    let active: Vec<&FoldRegion> = regions
        .iter()
        .filter(|r| collapsed.contains(&r.id()))
        .collect();
    if active.is_empty() {
        return source.to_string();
    }
    let lines: Vec<&str> = source.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(region) = active.iter().find(|r| r.start_line == i) {
            // Emit the header line verbatim.
            out.push_str(lines[i]);
            out.push('\n');
            // Replace the body with a single placeholder.
            out.push_str(&format!(
                "  // ... {} line{} folded ({}) ...\n",
                region.body_lines,
                if region.body_lines == 1 { "" } else { "s" },
                region.header,
            ));
            // Skip the body and the closing brace.
            i = region.end_line + 1;
        } else {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
        }
    }
    // `lines()` strips a trailing empty line; if the source ended with
    // `\n`, `out` should too.
    if !source.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Given the pre-edit and post-edit display text, plus the regions we
/// had collapsed, return the corrected source text. The TextEdit mutated
/// `display_text`; we need to splice the visible (non-folded) edits
/// back into the original `source` while preserving the folded bodies.
///
/// Walk `display` and `source` in lockstep, skipping over the folded
/// bodies in `source` (which are replaced by a single placeholder in
/// `display`). For each visible source line, copy the corresponding
/// display line — using `post` if it differs from `pre`, otherwise
/// `pre` — and that becomes the new source line.
pub fn sync_edits(source: &str, pre: &str, post: &str, collapsed: &BTreeSet<usize>) -> String {
    if collapsed.is_empty() {
        return post.to_string();
    }
    let regions = detect_folds(source);
    let active: Vec<&FoldRegion> = regions
        .iter()
        .filter(|r| collapsed.contains(&r.id()))
        .collect();

    let source_lines: Vec<&str> = source.lines().collect();
    let pre_lines: Vec<&str> = pre.lines().collect();
    let post_lines: Vec<&str> = post.lines().collect();

    let mut out_lines: Vec<String> = Vec::with_capacity(source_lines.len());
    let mut src_idx = 0usize;
    let mut display_idx = 0usize;
    while src_idx < source_lines.len() {
        // Is the current source line the *header* of a collapsed
        // region? If so, the display text replaces the body with a
        // single placeholder line. We emit the source header (with
        // any user edits applied) and then jump source past the body
        // while advancing display by 2 (header + placeholder).
        if let Some(region) = active.iter().find(|r| r.start_line == src_idx) {
            let r = *region;
            // Header line: visible, so apply edits.
            out_lines.push(pick_line(
                &pre_lines,
                &post_lines,
                display_idx,
                source_lines[src_idx],
            ));
            src_idx += 1;
            display_idx += 1;
            // Body lines: not visible. Copy them through from source
            // unchanged.
            for body_line in &source_lines[src_idx..r.end_line] {
                out_lines.push((*body_line).to_string());
            }
            src_idx = r.end_line;
            // Closing brace line: not visible in the display (the
            // placeholder took its place). Advance display past the
            // placeholder.
            display_idx += 1;
            // Emit the closing brace from source.
            if src_idx < source_lines.len() {
                out_lines.push(source_lines[src_idx].to_string());
                src_idx += 1;
            }
        } else {
            out_lines.push(pick_line(
                &pre_lines,
                &post_lines,
                display_idx,
                source_lines[src_idx],
            ));
            src_idx += 1;
            display_idx += 1;
        }
    }
    let mut out = out_lines.join("\n");
    if source.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn pick_line(pre: &[&str], post: &[&str], idx: usize, fallback: &str) -> String {
    let pre_line = pre.get(idx).copied();
    let post_line = post.get(idx).copied();
    if pre_line != post_line {
        post_line.or(pre_line).unwrap_or(fallback).to_string()
    } else {
        fallback.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_function_block() {
        let src = "fn add(a, b) {\n  return a + b\n}\n";
        let folds = detect_folds(src);
        assert_eq!(folds.len(), 1);
        assert_eq!(folds[0].kind, FoldKind::Function);
        assert_eq!(folds[0].start_line, 0);
        assert_eq!(folds[0].end_line, 2);
        assert_eq!(folds[0].body_lines, 1);
        assert_eq!(folds[0].header, "fn add");
    }

    #[test]
    fn detects_workflow_block() {
        let src = "workflow \"W\" {\n  on E\n  log(x)\n}\n";
        let folds = detect_folds(src);
        assert_eq!(folds.len(), 1);
        assert_eq!(folds[0].kind, FoldKind::Workflow);
        assert_eq!(folds[0].header, "workflow \"W\"");
    }

    #[test]
    fn nested_braces_tracked() {
        let src = "fn outer() {\n  if (x) {\n    log(1)\n  }\n  log(2)\n}\n";
        let folds = detect_folds(src);
        assert_eq!(folds.len(), 1);
        assert_eq!(folds[0].start_line, 0);
        assert_eq!(folds[0].end_line, 5);
    }

    #[test]
    fn single_line_block_not_folded() {
        let src = "fn noop() { return 0 }\n";
        let folds = detect_folds(src);
        assert!(folds.is_empty());
    }

    #[test]
    fn apply_collapses_body() {
        let src = "fn add(a, b) {\n  return a + b\n}\nfn sub(a, b) {\n  return a - b\n}\n";
        let mut collapsed = BTreeSet::new();
        collapsed.insert(0);
        let out = apply_folds(src, &collapsed);
        // First function folded, second untouched.
        assert!(out.contains("// ... 1 line folded (fn add)"));
        assert!(out.contains("return a - b"));
        // The folded function's body line shouldn't appear.
        assert!(!out.contains("return a + b"));
    }

    #[test]
    fn sync_edits_preserves_folded_body() {
        let src = "fn add(a, b) {\n  return a + b\n}\nfn sub(a, b) {\n  return a - b\n}\n";
        let mut collapsed = BTreeSet::new();
        collapsed.insert(0);
        let pre = apply_folds(src, &collapsed);
        // Simulate the user editing the second function's body.
        let post = pre.replace("return a - b", "return a - b * 2");
        let new_src = sync_edits(src, &pre, &post, &collapsed);
        // The folded body of `add` is preserved.
        assert!(new_src.contains("return a + b"));
        // The edit to `sub` is applied.
        assert!(new_src.contains("return a - b * 2"));
    }
}
