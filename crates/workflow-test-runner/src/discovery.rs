//! Sidecar test-file discovery.
//!
//! The convention is the same as Rust's `tests/` directory and Go's
//! `_test.go` files: a test file named `foo.test.flow` lives next
//! to its host `foo.flow`. The discovery module walks a path
//! (file or directory) and produces a [`DiscoverEntry`] per test
//! file it finds, each paired with the host file (when present)
//! that contains the workflows it exercises.
//!
//! Discovery is intentionally cheap: it only lists files. Parsing
//! and execution happen later, in [`execute`](crate::execute).

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoverError {
    #[error("path is not a file or directory: {0}")]
    NotFound(PathBuf),
    #[error("io error walking {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// One test file and (optionally) the host workflow file it
/// exercises. The runner executes the union of `test_file` and
/// `host_file` — the test file contributes the `TestDef`s and the
/// host file contributes the `WorkflowDef`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverEntry {
    /// Path to the file containing `test "..." { ... }` blocks.
    pub test_file: PathBuf,
    /// Path to the host `.flow`/`.yaml`/`.json`/`.toml` file with
    /// the workflows under test, or `None` if no host was found
    /// (e.g. a user pointed the runner at a single test file).
    pub host_file: Option<PathBuf>,
}

/// Walk `path` and return one [`DiscoverEntry`] per test file.
/// `path` may be:
/// - a single `*.test.flow` file (one entry, no host),
/// - a single `*.flow` file (zero or one test file alongside it),
/// - a directory (recursive walk).
pub fn discover(path: &Path) -> Result<Vec<DiscoverEntry>, DiscoverError> {
    if !path.exists() {
        return Err(DiscoverError::NotFound(path.to_path_buf()));
    }
    if path.is_file() {
        return Ok(vec![entry_for_file(path)?]);
    }
    let mut out = Vec::new();
    walk_dir(path, &mut out)?;
    // Stable order so the CLI prints the same report every run.
    out.sort_by(|a, b| a.test_file.cmp(&b.test_file));
    Ok(out)
}

fn entry_for_file(path: &Path) -> Result<DiscoverEntry, DiscoverError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let is_test_flow = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".test.flow"));
    if ext == "flow" && is_test_flow {
        Ok(DiscoverEntry {
            test_file: path.to_path_buf(),
            host_file: find_host_for(path),
        })
    } else {
        // The user pointed us at something other than a *.test.flow
        // — return it as a host file with no associated test. The
        // runner will simply find no tests in it.
        Ok(DiscoverEntry {
            test_file: path.to_path_buf(),
            host_file: None,
        })
    }
}

fn walk_dir(dir: &Path, out: &mut Vec<DiscoverEntry>) -> Result<(), DiscoverError> {
    let read = fs::read_dir(dir).map_err(|e| DiscoverError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;
    for entry in read {
        let entry = entry.map_err(|e| DiscoverError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let p = entry.path();
        if p.is_dir() {
            walk_dir(&p, out)?;
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            let is_test_flow = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".test.flow"));
            if ext == "flow" && is_test_flow {
                out.push(DiscoverEntry {
                    test_file: p.clone(),
                    host_file: find_host_for(&p),
                });
            }
        }
    }
    Ok(())
}

/// Given `foo.test.flow`, look for `foo.flow` in the same
/// directory. Returns `None` if not found — the runner is
/// forgiving: a missing host file is a runtime error, not a
/// discovery error.
fn find_host_for(test_path: &Path) -> Option<PathBuf> {
    let parent = test_path.parent()?;
    let name = test_path.file_name()?.to_str()?;
    let host_name = name.strip_suffix(".test.flow")?;
    let candidate = parent.join(format!("{}.flow", host_name));
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "wf_test_runner_discover_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn discover_finds_sidecar() {
        let dir = tmp_dir();
        fs::write(dir.join("hello.flow"), "workflow \"X\" { on E }\n").unwrap();
        fs::write(
            dir.join("hello.test.flow"),
            "test \"t\" { on E expect logs [] }\n",
        )
        .unwrap();
        let entries = discover(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].host_file.as_deref(),
            Some(dir.join("hello.flow").as_path())
        );
    }

    #[test]
    fn discover_returns_test_file_alone() {
        let dir = tmp_dir();
        fs::write(dir.join("orphan.test.flow"), "test \"t\" { on E }\n").unwrap();
        let entries = discover(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].host_file.is_none());
    }

    #[test]
    fn discover_single_file() {
        let dir = tmp_dir();
        let path = dir.join("solo.test.flow");
        fs::write(&path, "test \"t\" { on E }\n").unwrap();
        let entries = discover(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].test_file, path);
    }
}
