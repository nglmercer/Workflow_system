//! Recent-files tracking for the native editor.
//!
//! Stores the last [`MAX_ENTRIES`] paths the user opened, most
//! recent first, deduplicated by canonicalized path. The list is
//! persisted as JSON in the user's config directory:
//!
//! - Linux:   `$XDG_CONFIG_HOME/flow-editor/recent.json`
//!   (falling back to `$HOME/.config/flow-editor/recent.json`)
//! - macOS:   `$HOME/Library/Application Support/flow-editor/recent.json`
//! - Windows: `%APPDATA%\flow-editor\recent.json`
//!
//! The list survives across launches so the home screen can offer
//! "recently used" quick-picks. Missing or malformed files are
//! treated as "no recents" — we never refuse to start the editor
//! because the recents file is broken.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const APP_DIR: &str = "flow-editor";
const FILE_NAME: &str = "recent.json";
/// Cap the recents list so the home screen stays scannable and
/// the file doesn't grow unbounded over years of edits.
pub const MAX_ENTRIES: usize = 10;

#[derive(Debug, Error)]
pub enum RecentError {
    #[error("could not determine config directory")]
    NoConfigDir,
    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse recents file: {0}")]
    Parse(#[from] serde_json::Error),
}

/// A recent-files list. Owned by `EditorApp` and updated on every
/// successful file open. Serialized as a plain JSON array of
/// strings so the on-disk format is human-readable and stable
/// across versions.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RecentList {
    paths: Vec<PathBuf>,
}

impl RecentList {
    /// Load the recents list from disk. A missing or empty file
    /// returns an empty list; a malformed file also returns an
    /// empty list (we don't want a corrupt recents.json to block
    /// the editor from starting).
    pub fn load() -> Self {
        read_from_disk().unwrap_or_default()
    }

    /// Add `path` to the front of the list. If `path` (or its
    /// canonical form) is already present, it's moved to the front
    /// rather than duplicated. The list is then truncated to
    /// [`MAX_ENTRIES`].
    ///
    /// The mutation is *in-memory only* — the caller is responsible
    /// for calling [`Self::save`] when the editor wants the
    /// change to persist. We don't save on every keystroke; we
    /// save on a successful file open and on graceful shutdown.
    pub fn touch(&mut self, path: &Path) {
        // Canonicalize to dedupe symlinks and relative-vs-absolute
        // duplicates. If canonicalization fails (e.g. the file no
        // longer exists on disk), fall back to the literal path.
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.paths.retain(|p| p != &key);
        self.paths.insert(0, key);
        if self.paths.len() > MAX_ENTRIES {
            self.paths.truncate(MAX_ENTRIES);
        }
    }

    /// Snapshot of the current entries, most recent first. Used by
    /// the home screen to render the clickable list.
    pub fn entries(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Persist the current list to disk. Creates the config
    /// directory if it doesn't exist.
    pub fn save(&self) -> Result<(), RecentError> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| RecentError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json).map_err(|source| RecentError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(())
    }
}

/// Path of the on-disk recents file.
pub fn config_path() -> Result<PathBuf, RecentError> {
    let base = config_dir().ok_or(RecentError::NoConfigDir)?;
    Ok(base.join(APP_DIR).join(FILE_NAME))
}

fn read_from_disk() -> Result<RecentList, RecentError> {
    let path = config_path()?;
    let raw = std::fs::read_to_string(&path).map_err(|source| RecentError::Io {
        path: path.clone(),
        source,
    })?;
    let list: RecentList = serde_json::from_str(&raw)?;
    Ok(list)
}

fn config_dir() -> Option<PathBuf> {
    // Honor XDG_CONFIG_HOME on Linux when set, matching the
    // behavior of most freedesktop apps. Fall back to $HOME on
    // Linux/macOS. On Windows, %APPDATA% is the canonical config
    // location.
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg));
            }
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_pushes_to_front() {
        let mut r = RecentList::default();
        r.touch(Path::new("/a.flow"));
        r.touch(Path::new("/b.flow"));
        assert_eq!(r.entries()[0], PathBuf::from("/b.flow"));
        assert_eq!(r.entries()[1], PathBuf::from("/a.flow"));
    }

    #[test]
    fn touch_dedups_existing_entry() {
        let mut r = RecentList::default();
        r.touch(Path::new("/a.flow"));
        r.touch(Path::new("/b.flow"));
        r.touch(Path::new("/a.flow"));
        assert_eq!(r.entries().len(), 2);
        assert_eq!(r.entries()[0], PathBuf::from("/a.flow"));
        assert_eq!(r.entries()[1], PathBuf::from("/b.flow"));
    }

    #[test]
    fn touch_caps_at_max_entries() {
        let mut r = RecentList::default();
        for i in 0..(MAX_ENTRIES + 5) {
            r.touch(Path::new(format!("/file{}.flow", i).as_str()));
        }
        assert_eq!(r.entries().len(), MAX_ENTRIES);
        // The most recent insertion should be at the front.
        assert!(r.entries()[0].to_string_lossy().contains(&format!(
            "file{}.flow",
            MAX_ENTRIES + 4
        )));
    }

    #[test]
    fn empty_default_serializes_to_empty_array() {
        let r = RecentList::default();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "{\"paths\":[]}");
    }
}
