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
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Box::new(EditorApp::default()) as Box<dyn eframe::App>
        }),
    )
}
