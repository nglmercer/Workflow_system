//! Rendering of the keyboard-shortcuts help window.
//!
//! The window is an `egui::Window` with a movable title bar that
//! lists every binding in the editor's [`Keymap`] alongside a short
//! description of the command it triggers. It is opened via the
//! `F1` key (mapped to [`Command::ShowShortcuts`]) or the toolbar
//! button, and dismissed with `Esc` (handled by the editor's main
//! loop) or the in-window Close button.
//!
//! We deliberately avoid `egui::Window::open(&mut bool)`: that
//! method holds a `&mut` borrow for the duration of the call, which
//! blocks the inner closure from reading the same flag. The window
//! is instead shown conditionally on the caller's flag, and the
//! caller polls [`esc_pressed`] to close on `Esc`.

use eframe::egui::{self, Color32, FontId, RichText, ScrollArea, Vec2};
use crate::theme::Theme;
use workflow_i18n::t as i18n_t;

use super::keybindings::Keymap;

pub const SHORTCUTS_WIDTH: f32 = 480.0;
pub const SHORTCUTS_HEIGHT: f32 = 420.0;

/// Render the shortcuts help window when `open` is `true`. The
/// caller owns the open flag and should also call [`esc_pressed`]
/// to close the window on `Esc`.
pub fn show(ctx: &egui::Context, open: &mut bool, keymap: &Keymap) {
    if !*open {
        return;
    }
    egui::Window::new(i18n_t("shortcuts.title"))
        .fixed_size(Vec2::new(SHORTCUTS_WIDTH, SHORTCUTS_HEIGHT))
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(i18n_t("shortcuts.press_esc"))
                        .small()
                        .color(Color32::GRAY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    if ui.button(i18n_t("shortcuts.close")).clicked() {
                        *open = false;
                    }
                });
            });
            ui.separator();
            ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("shortcuts_grid")
                    .num_columns(2)
                    .striped(true)
                    .spacing([24.0, 4.0])
                    .min_col_width(140.0)
                    .show(ui, |ui| {
                        for (label, cmd) in keymap.bindings() {
                            ui.label(
                                RichText::new(label)
                                    .monospace()
                                    .font(FontId::monospace(13.0))
                                    .color(Theme::chord_label()),
                            );
                            ui.label(cmd);
                            ui.end_row();
                        }
                    });
            });
        });
}

/// True if the user pressed `Esc` this frame. The editor's main
/// loop should call this and close the shortcuts window when the
/// window is open.
pub fn esc_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                }
            )
        })
    })
}
