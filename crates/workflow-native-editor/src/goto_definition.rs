//! Go-to-definition for the symbol under the cursor.
//!
//! The command has two halves:
//!
//! 1. Parse the word at the cursor ([`extract_word_at_position`]).
//! 2. Look up the symbol in the LSP function registry. If it's
//!    a user-defined function, search the in-file `import`
//!    statements for the source path
//!    ([`find_import_source`]) and open it via
//!    [`crate::EditorApp::load_path_into_editor`].
//!
//! [`goto_definition_at_cursor`] is the top-level entry point
//! that the keymap dispatches to. It returns `true` if a file
//! was opened (so callers can decide whether to re-render), and
//! updates `self.status` with a human-readable outcome in every
//! case.

use std::path::PathBuf;

/// Extract the word at the given column position in a line. The
/// word is the maximal `[A-Za-z0-9_]+` run that contains
/// `col`. Returns the empty string if `col` is out of bounds.
pub(crate) fn extract_word_at_position(line: &str, col: usize) -> String {
    let bytes = line.as_bytes();
    if col >= bytes.len() {
        return String::new();
    }

    // Find the start of the word (go backwards until we find a non-alphanumeric, non-underscore)
    let mut start = col;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }

    // Find the end of the word (go forwards until we find a non-alphanumeric, non-underscore)
    let mut end = col;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }

    line[start..end].to_string()
}

/// Find the source file path for an imported function by looking
/// at the import statements in the current file. The
/// `_function_name` argument is reserved for future use (today we
/// just pick the first `import NAME from "*.flow"` whose
/// referenced file exists; once we wire the function name to a
/// specific import the resolution becomes per-binding).
pub(crate) fn find_import_source(
    text: &str,
    file_path: Option<&std::path::Path>,
    _function_name: &str,
) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();

    // Look for import statements that might contain this function
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") && trimmed.contains(" from ") {
            // Parse: import name from "path"
            if let Some(from_idx) = trimmed.find(" from ") {
                let path_part = &trimmed[from_idx + 6..];
                let path = path_part.trim().trim_matches('"').trim_matches('\'');

                // Check if the path is a .flow file
                if path.ends_with(".flow") {
                    // Resolve the path relative to the current file
                    if let Some(current_dir) = file_path.and_then(|p| p.parent()) {
                        let full_path: PathBuf = current_dir.join(path);
                        if full_path.exists() {
                            return Some(full_path.to_string_lossy().into_owned());
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{extract_word_at_position, find_import_source};

    #[test]
    fn word_at_middle_of_identifier() {
        let line = "log(item.name)";
        let word = extract_word_at_position(line, 5);
        assert_eq!(word, "item");
    }

    #[test]
    fn word_at_start_of_identifier() {
        let line = "log(message)";
        let word = extract_word_at_position(line, 0);
        assert_eq!(word, "log");
    }

    #[test]
    fn word_at_end_of_identifier() {
        let line = "log(message)";
        // `m` at index 6 is the last char of "message"
        let word = extract_word_at_position(line, 6);
        assert_eq!(word, "message");
    }

    #[test]
    fn word_out_of_bounds_is_empty() {
        let line = "log";
        let word = extract_word_at_position(line, 100);
        assert_eq!(word, "");
    }

    #[test]
    fn word_underscores_are_part_of_identifier() {
        let line = "fn my_helper() {}";
        let word = extract_word_at_position(line, 4);
        assert_eq!(word, "my_helper");
    }

    #[test]
    fn import_source_resolves_relative_to_file() {
        // The function only checks `full_path.exists()`, so we
        // exercise the no-match branch (the path doesn't exist)
        // and assert the resolver returns `None` rather than
        // panicking.
        let text = "import utils from \"./shared_utils.flow\"\n";
        let path = std::path::Path::new("/nonexistent/main.flow");
        assert!(find_import_source(text, Some(path), "utils").is_none());
    }
}
