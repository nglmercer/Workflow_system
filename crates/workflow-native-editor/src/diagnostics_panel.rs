//! Bottom diagnostics panel.
//!
//! Renders the LSP `Diagnostic` list as a vertical scroll of
//! severity-colored rows. Owns the severity → (color, icon) mapping
//! so the rest of the editor doesn't need to know about it.

use eframe::egui::{self, Color32, RichText, ScrollArea};
use workflow_lsp::features::{Diagnostic, DiagnosticSeverity};

const MAX_HEIGHT: f32 = 100.0;

pub fn show(ctx: &egui::Context, diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        return;
    }
    egui::TopBottomPanel::bottom("diagnostics").show(ctx, |ui| {
        ui.label(RichText::new("Problems").strong());
        ScrollArea::vertical()
            .max_height(MAX_HEIGHT)
            .show(ui, |ui| {
                for diag in diagnostics {
                    render_row(ui, diag);
                }
            });
    });
}

fn render_row(ui: &mut egui::Ui, diag: &Diagnostic) {
    let (color, icon) = style_for(diag.severity);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).color(color));
        ui.label(
            RichText::new(format!(
                "Ln {}, Col {}: {}",
                diag.start_line + 1,
                diag.start_col + 1,
                diag.message
            ))
            .color(color),
        );
    });
}

fn style_for(severity: DiagnosticSeverity) -> (Color32, &'static str) {
    match severity {
        DiagnosticSeverity::Error => (Color32::from_rgb(255, 80, 80), "✗"),
        DiagnosticSeverity::Warning => (Color32::from_rgb(255, 200, 50), "⚠"),
        DiagnosticSeverity::Info | DiagnosticSeverity::Hint => (Color32::GRAY, "ℹ"),
    }
}
