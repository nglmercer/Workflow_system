//! File browser side panel.
//!
//! Renders a one-level listing of the parent directory of the
//! currently-open file, with the open file highlighted. We
//! intentionally do *not* walk subdirectories: a full tree view
//! would balloon the module's complexity (cache invalidation on
//! filesystem changes, lazy expansion, symlink loops) for a
//! feature the user didn't ask for. A flat list of the working
//! directory's `.flow`/`.yaml`/`.yml`/`.json`/`.toml` files is
//! enough to give "the open file is in this directory" context
//! and one-click switching to a sibling file.
//!
//! The panel is `Option<()>`-returning in the same vein as the
//! diagnostics panel: a `Some` payload carries a one-shot
//! action ("user clicked this path — open it").

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use super::file_io;

/// Filter a directory listing down to the editor's supported
/// workflow extensions. Hidden files (those starting with `.`)
/// are excluded. The result is sorted alphabetically so the
/// panel is stable across frames.
pub fn list_workflow_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !n.starts_with('.'))
                .unwrap_or(false)
        })
        .filter(|p| file_io::is_supported(p))
        .collect();
    out.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    out
}

/// Render the side panel when a file is open. `current` is the
/// path of the open file (used to highlight it and to derive the
/// parent directory). Returns `Some(path)` if the user clicked a
/// file in the list, signalling the editor to switch to that file.
pub fn show(ctx: &egui::Context, current: Option<&Path>) -> Option<PathBuf> {
    let path = current?;
    let parent = path.parent()?;
    if !parent.is_dir() {
        return None;
    }
    let mut chosen: Option<PathBuf> = None;
    egui::SidePanel::left("file_browser")
        .resizable(true)
        .default_width(220.0)
        .min_width(140.0)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new(parent.display().to_string())
                    .strong()
                    .small(),
            );
            ui.separator();
            let files = list_workflow_files(parent);
            if files.is_empty() {
                ui.label(
                    RichText::new("(no workflow files in this directory)")
                        .italics()
                        .small()
                        .weak(),
                );
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for f in &files {
                    let is_current = f == path;
                    let label = f
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("(invalid)");
                    let response = ui.selectable_label(is_current, label);
                    if response.clicked() && !is_current {
                        chosen = Some(f.clone());
                    }
                }
            });
        });
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn filters_by_extension_and_ignores_dotfiles() {
        let dir = std::env::temp_dir().join(format!(
            "flow_editor_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.flow"), "").unwrap();
        fs::write(dir.join("b.yaml"), "").unwrap();
        fs::write(dir.join("c.txt"), "").unwrap();
        fs::write(dir.join(".hidden.flow"), "").unwrap();

        let mut got = list_workflow_files(&dir);
        got.sort();
        let names: Vec<String> = got
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.flow", "b.yaml"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_directory_returns_empty() {
        let dir = std::env::temp_dir().join("definitely_not_here_xyz");
        assert!(list_workflow_files(&dir).is_empty());
    }
}
