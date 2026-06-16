//! Home screen shown when no project is open.
//!
//! Replaces the central editor panel with a centered card that
//! surfaces the most useful "what now?" affordances:
//!
//! - A short app title and tagline.
//! - "New File" — clears the buffer, treating the editor as a
//!   fresh untitled document.
//! - "Open File…" — runs the same native file dialog as the
//!   toolbar button.
//! - A scrollable list of recent files (most recent first).
//!   Clicking an entry re-opens it. If the file no longer exists
//!   on disk the click surfaces an error in the status bar.
//!
//! The screen is intentionally minimal: it's the destination the
//! user sees when they launch the app cold or close their last
//! file, and it should feel calm rather than busy.

use std::path::PathBuf;

use eframe::egui::{self, RichText, ScrollArea};

use super::recent::RecentList;

/// Action the user took on the home screen. The editor handles
/// each variant in its central `update` loop.
pub enum HomeAction {
    NewFile,
    OpenDialog,
    OpenPath(PathBuf),
}

/// Render the home screen inside the supplied `ui` (the central
/// panel). Returns `Some(action)` if the user did something;
/// `None` if they just looked.
pub fn show(ui: &mut egui::Ui, recents: &RecentList) -> Option<HomeAction> {
    let mut action: Option<HomeAction> = None;

    egui::Frame::none()
        .fill(ui.ctx().style().visuals.panel_fill)
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.heading(RichText::new("Flow Native Editor").size(28.0));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Open a workflow file to start editing")
                        .italics()
                        .weak(),
                );
                ui.add_space(24.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Open File…").size(16.0)).clicked() {
                        action = Some(HomeAction::OpenDialog);
                    }
                    ui.add_space(8.0);
                    if ui.button(RichText::new("New File").size(16.0)).clicked() {
                        action = Some(HomeAction::NewFile);
                    }
                });
                ui.add_space(32.0);
                if recents.entries().is_empty() {
                    ui.label(RichText::new("No recent files").small().weak());
                } else {
                    ui.label(RichText::new("Recent").strong());
                    ui.add_space(6.0);
                    let max_height = 240.0;
                    ScrollArea::vertical()
                        .max_height(max_height)
                        .show(ui, |ui| {
                            for path in recents.entries() {
                                let display = format_recent(path);
                                let response = ui.add_sized(
                                    [360.0, 22.0],
                                    egui::Button::new(RichText::new(display).monospace().small())
                                        .frame(false),
                                );
                                if response.clicked() {
                                    action = Some(HomeAction::OpenPath(path.clone()));
                                }
                            }
                        });
                }
            });
        });
    action
}

/// Render a recent file as `dir/sub.flow  ›  /full/path` so the
/// user can scan the list at a glance. The directory is in a
/// slightly dimmer color via a two-line label, but egui's
/// `RichText` doesn't compose inline; we keep it as a single
/// line with the short name first, full path in parens.
fn format_recent(path: &std::path::Path) -> String {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("(invalid)");
    let parent = path.parent().and_then(|p| p.to_str()).unwrap_or("");
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}   —   {}", name, parent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn format_recent_with_parent() {
        let s = format_recent(Path::new("/home/me/projects/main.flow"));
        assert!(s.contains("main.flow"));
        assert!(s.contains("/home/me/projects"));
    }

    #[test]
    fn format_recent_no_parent() {
        let s = format_recent(Path::new("main.flow"));
        assert_eq!(s, "main.flow");
    }
}
