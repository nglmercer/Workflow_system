use std::path::{Path, PathBuf};

use plugin_system::{FileLoader, PluginLoader};

/// Loads workflow plugins from a directory on the filesystem.
pub struct WorkflowPluginLoader {
    dir: PathBuf,
}

impl WorkflowPluginLoader {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    /// Returns the plugin directory path.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Scan the plugin directory and return `(name, loader)` pairs for
    /// every shared library found.
    pub fn discover(&self) -> Vec<(String, Box<dyn PluginLoader>)> {
        let mut plugins = Vec::new();

        let expected_ext = if cfg!(target_os = "linux") {
            "so"
        } else if cfg!(target_os = "macos") {
            "dylib"
        } else if cfg!(target_os = "windows") {
            "dll"
        } else {
            "so"
        };

        if !self.dir.exists() {
            return plugins;
        }

        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == expected_ext {
                            let name = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let loader = Box::new(FileLoader::new(&path));
                            plugins.push((name, loader));
                        }
                    }
                }
            }
        }

        plugins
    }
}
