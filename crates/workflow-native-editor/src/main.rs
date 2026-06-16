mod app;
mod completion;
mod cursor;
mod diagnostics_panel;
mod file_browser;
mod file_io;
mod folding;
mod gutter;
mod highlight;
mod history;
mod home;
mod keybindings;
mod layouter;
mod popup;
mod recent;
mod shortcuts_window;
mod snippet;
mod test_panel;

use app::EditorApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Flow Native Editor",
        native_options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.window_rounding = egui::Rounding::same(4.0);
            visuals.panel_fill = egui::Color32::from_rgb(30, 30, 30);
            visuals.window_fill = egui::Color32::from_rgb(37, 37, 38);
            visuals.extreme_bg_color = egui::Color32::from_rgb(30, 30, 30);
            visuals.faint_bg_color = egui::Color32::from_rgb(45, 45, 48);
            visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(45, 45, 48);
            visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(180, 180, 180));
            visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(55, 55, 58);
            visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 200, 200));
            visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(70, 70, 75);
            visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 122, 204);
            visuals.selection.bg_fill = egui::Color32::from_rgb(0, 122, 204);
            visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
            cc.egui_ctx.set_visuals(visuals);
            Box::new(EditorApp::default()) as Box<dyn eframe::App>
        }),
    )
}
