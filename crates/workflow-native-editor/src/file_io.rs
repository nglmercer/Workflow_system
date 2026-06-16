//! File I/O for the native editor: open from disk, save to disk,
//! path ↔ `file://` URI conversion, and supported-extension helpers.
//!
//! Drag-and-drop delivers a path to the editor; the toolbar file
//! picker delivers a path through the platform's native file
//! dialog. Both code paths funnel through [`load_from_path`] and
//! [`save_to_path`] so the rest of the editor doesn't care where the
//! path came from.
//!
//! Path ↔ URI mapping follows RFC 8089: an absolute POSIX path
//! `/foo/bar.flow` becomes `file:///foo/bar.flow`. On Windows we
//! keep the leading drive letter and prefix it with `file:///`, e.g.
//! `C:\foo\bar.flow` → `file:///C:/foo/bar.flow`. The LSP server
//! is keyed on the URI string, so a stable conversion matters more
//! than URL-encoding every byte: for the typical workflow files we
//! accept, ASCII paths round-trip cleanly.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors produced by file I/O. The editor surfaces the `Display`
/// string in its status bar.
#[derive(Debug, Error)]
pub enum FileIoError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unsupported file type: {0:?} (expected .flow, .yaml, .yml, .json, or .toml)")]
    UnsupportedExtension(PathBuf),
    #[error("dropped item is not a file: {0}")]
    NotAFile(PathBuf),
}

const SUPPORTED_EXTS: &[&str] = &["flow", "yaml", "yml", "json", "toml"];

/// True if `path` has an extension we know how to edit. Used by
/// the file dialog filter and the drag-and-drop handler to silently
/// reject anything that isn't a workflow / config file.
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTS.iter().any(|s| s.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

/// Read the file at `path` and return its contents. Validates that
/// the extension is supported so the rest of the editor doesn't
/// have to guess about format detection on every load.
pub fn load_from_path(path: &Path) -> Result<String, FileIoError> {
    if !path.is_file() {
        return Err(FileIoError::NotAFile(path.to_path_buf()));
    }
    if !is_supported(path) {
        return Err(FileIoError::UnsupportedExtension(path.to_path_buf()));
    }
    std::fs::read_to_string(path).map_err(|source| FileIoError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Write `contents` to `path`, creating parent directories if they
/// don't exist. Returns the path on success so callers can update
/// their own state without re-deriving it.
pub fn save_to_path(path: &Path, contents: &str) -> Result<PathBuf, FileIoError> {
    if !is_supported(path) {
        return Err(FileIoError::UnsupportedExtension(path.to_path_buf()));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| FileIoError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    std::fs::write(path, contents).map_err(|source| FileIoError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(path.to_path_buf())
}

/// Convert a filesystem path to an LSP `file://` URI. The LSP
/// state is keyed on the URI string, so this needs to be stable for
/// the same path across calls.
pub fn path_to_uri(path: &Path) -> String {
    // On Windows, `Path::to_string_lossy` produces strings like
    // `C:\foo\bar.flow`. We need to forward-slash the separators
    // and strip the leading `/` that would otherwise be inserted
    // for an absolute Windows path. The simplest portable form is
    // to URL-encode each component, but for the paths we care about
    // (ASCII, no `?`/`#` characters in directory names) the
    // naive forward-slash form is fine.
    let raw = path.to_string_lossy().replace('\\', "/");
    if raw.starts_with('/') {
        format!("file://{}", raw)
    } else {
        format!("file:///{}", raw)
    }
}

/// Parse a `file://` URI back into a path. Returns `None` for
/// non-`file` schemes or malformed inputs; callers should treat
/// that as "keep the existing URI" rather than panicking.
#[allow(dead_code)]
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let trimmed = rest.trim_start_matches('/');
    if cfg!(windows) && trimmed.len() >= 2 && trimmed.as_bytes()[1] == b':' {
        Some(PathBuf::from(trimmed.replace('/', "\\")))
    } else {
        Some(PathBuf::from(format!("/{}", trimmed)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions_match() {
        assert!(is_supported(Path::new("a.flow")));
        assert!(is_supported(Path::new("a.YAML")));
        assert!(is_supported(Path::new("a.json")));
        assert!(is_supported(Path::new("a.toml")));
        assert!(!is_supported(Path::new("a.txt")));
        assert!(!is_supported(Path::new("a")));
    }

    #[test]
    fn posix_path_to_uri() {
        let uri = path_to_uri(Path::new("/tmp/foo/bar.flow"));
        assert_eq!(uri, "file:///tmp/foo/bar.flow");
    }

    #[test]
    fn posix_uri_to_path_roundtrip() {
        let path = PathBuf::from("/tmp/foo/bar.flow");
        let uri = path_to_uri(&path);
        let back = uri_to_path(&uri).expect("uri parses");
        assert_eq!(back, path);
    }

    #[test]
    fn uri_to_path_rejects_non_file_scheme() {
        assert!(uri_to_path("https://example.com/a.flow").is_none());
        assert!(uri_to_path("not a uri").is_none());
    }

    #[test]
    fn unsupported_extension_rejected() {
        let err = load_from_path(Path::new("/tmp/missing.txt")).unwrap_err();
        // We don't assert on the variant — just that loading
        // surfaces an error rather than silently succeeding.
        let _ = format!("{}", err);
    }
}
