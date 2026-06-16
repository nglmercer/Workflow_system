//! LSP-style snippet expansion.
//!
//! When a completion is accepted, its `insert_text` may contain LSP snippet
//! placeholders like `$1`, `${2:value}`, or `$0`. This module parses that
//! body into the literal text to insert plus an ordered list of tab stops.

#[derive(Debug, Clone)]
pub struct PendingSnippet {
    /// Tab stops, in the order they should be visited. The first stop is the
    /// initial one (visible right after insertion).
    pub stops: Vec<SnippetStop>,
    /// Index of the stop the user is currently editing.
    pub current: usize,
}

#[derive(Debug, Clone)]
pub struct SnippetStop {
    /// Char offset from the start of the *expanded* snippet body.
    pub start: usize,
    /// Length of the default text in characters.
    pub length: usize,
    /// Default placeholder text. Empty means there's no default and the stop
    /// is just a cursor position.
    #[allow(dead_code)]
    pub default: String,
}

impl PendingSnippet {
    pub fn advance(&mut self) -> bool {
        self.current += 1;
        if self.current >= self.stops.len() {
            true // finished
        } else {
            false
        }
    }

    /// The current stop, expressed as a `(start, length)` char range relative
    /// to the start of the *expanded* snippet body. `None` if there are no
    /// more stops.
    pub fn current_stop_range(&self) -> Option<(usize, usize)> {
        let stop = self.stops.get(self.current)?;
        Some((stop.start, stop.length))
    }
}

/// Expand a LSP-style snippet body into the literal text that should be
/// inserted, plus the list of tab stops.
///
/// Supports the subset of LSP snippet syntax we actually use:
/// - `$0` — final cursor resting place (length 0)
/// - `$N` (1-9) — tab stop at position N
/// - `${N}` — tab stop with empty default
/// - `${N:text}` — tab stop with `text` as the default
/// - `\$` — escaped dollar sign
/// - `\\` — escaped backslash
/// - `\}` — escaped closing brace
pub fn expand(body: &str) -> (String, Vec<SnippetStop>) {
    let mut out = String::new();
    let mut stops: Vec<SnippetStop> = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                let next = chars[i + 1];
                if next == '$' || next == '\\' || next == '}' {
                    out.push(next);
                    i += 2;
                    continue;
                }
                out.push(c);
                i += 1;
            }
            '$' => {
                if let Some((consumed, stop_index, default)) = parse_stop(&chars[i..]) {
                    let start_chars = out.chars().count();
                    // Most snippet bodies are flat, but recurse so a default
                    // that contains nested placeholders still expands.
                    let (expanded_default, _) = expand(&default);
                    out.push_str(&expanded_default);
                    let length_chars = expanded_default.chars().count();
                    if stop_index != 0 {
                        stops.push(SnippetStop {
                            start: start_chars,
                            length: length_chars,
                            default,
                        });
                    }
                    // `$0` marks the final cursor resting place, not a
                    // navigable stop — the snippet is "done" when no more
                    // stops remain.
                    i += consumed;
                    continue;
                }
                out.push(c);
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }

    stops.sort_by_key(|s| s.start);
    (out, stops)
}

/// Try to parse a snippet stop starting at the current position (which
/// must be a `$`). Returns `(chars_consumed, stop_index, default_text)`.
fn parse_stop(chars: &[char]) -> Option<(usize, usize, String)> {
    debug_assert_eq!(chars[0], '$');
    let rest = &chars[1..];

    // `${N:text}` or `${N}`
    if rest.first() == Some(&'{') {
        let after_brace = &rest[1..];
        // Find the colon or closing brace.
        let mut idx = 0;
        while idx < after_brace.len() {
            let c = after_brace[idx];
            if c == ':' || c == '}' {
                break;
            }
            idx += 1;
        }
        if idx >= after_brace.len() {
            return None;
        }
        let num_str: String = after_brace[..idx].iter().collect();
        let n: usize = num_str.parse().ok()?;
        let mut default = String::new();
        let mut pos = idx;
        if after_brace[pos] == ':' {
            pos += 1;
            let mut depth: i32 = 1;
            while pos < after_brace.len() && depth > 0 {
                let c = after_brace[pos];
                if c == '\\' && pos + 1 < after_brace.len() {
                    default.push(after_brace[pos + 1]);
                    pos += 2;
                    continue;
                }
                if c == '{' {
                    depth += 1;
                } else if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                default.push(c);
                pos += 1;
            }
            if pos >= after_brace.len() || after_brace[pos] != '}' {
                return None;
            }
            pos += 1;
        } else if after_brace[pos] == '}' {
            pos += 1;
        } else {
            return None;
        }
        // `pos` is the position in `after_brace` (i.e. relative to the `{`).
        // We also consumed the leading `$` and the `{` itself, so the total
        // characters consumed from the input slice are 1 (for `$`) + 1 (for
        // `{`) + `pos`.
        return Some((2 + pos, n, default));
    }

    // `$N` where N is a single digit 0-9.
    if !rest.is_empty() && rest[0].is_ascii_digit() {
        let n = (rest[0] as u8 - b'0') as usize;
        return Some((2, n, String::new()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_simple_snippet() {
        let (body, stops) = expand("var ${1:name} = ${2:value}");
        assert_eq!(body, "var name = value");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].start, 4);
        assert_eq!(stops[0].length, 4);
        assert_eq!(stops[0].default, "name");
        assert_eq!(stops[1].start, 11);
        assert_eq!(stops[1].length, 5);
        assert_eq!(stops[1].default, "value");
    }

    #[test]
    fn expand_workflow_snippet() {
        let (body, stops) = expand("workflow \"${1:Name}\" {\n\ton ${2:EVENT}\n\t$0\n}");
        assert_eq!(body, "workflow \"Name\" {\n\ton EVENT\n\t\n}");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].start, 10);
        assert_eq!(stops[0].length, 4);
        assert_eq!(stops[1].start, 22);
        assert_eq!(stops[1].length, 5);
    }

    #[test]
    fn expand_no_snippet() {
        let (body, stops) = expand("plain text");
        assert_eq!(body, "plain text");
        assert!(stops.is_empty());
    }

    #[test]
    fn expand_escapes() {
        let (body, _) = expand("\\$ and \\\\");
        assert_eq!(body, "$ and \\");
    }

    #[test]
    fn expand_simple_dollar_n() {
        let (body, stops) = expand("foo $1 bar");
        assert_eq!(body, "foo  bar");
        assert_eq!(stops.len(), 1);
        assert_eq!(stops[0].start, 4);
        assert_eq!(stops[0].length, 0);
    }
}
