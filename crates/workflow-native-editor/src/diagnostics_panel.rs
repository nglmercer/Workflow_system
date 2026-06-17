//! Bottom diagnostics panel.
//!
//! Renders the LSP `Diagnostic` list as a vertical scroll of
//! severity-colored rows. Owns the severity → (color, icon) mapping
//! so the rest of the editor doesn't need to know about it.
//!
//! Exposes a "Copy" button that pushes the current problems to the
//! OS clipboard in a stable plain-text format. The panel returns
//! an `Option<String>` describing the action (e.g.
//! `"Copied 3 problems to clipboard"`) so the caller can surface
//! it in the editor's status bar. Returning `None` means the user
//! did not interact with the panel this frame.

use workflow_i18n::t as i18n_t;
use eframe::egui::{self, Color32, RichText, ScrollArea};
use workflow_lsp::features::{Diagnostic, DiagnosticSeverity};

/// Render the diagnostics panel. Returns a status-bar message
/// describing the result of any user action this frame (currently
/// only the Copy button), or `None` if nothing happened.
pub fn show(ctx: &egui::Context, diagnostics: &[Diagnostic]) -> Option<String> {
    if diagnostics.is_empty() {
        return None;
    }
    let mut status: Option<String> = None;
    egui::TopBottomPanel::bottom("diagnostics")
        .resizable(true)
        .default_height(100.0)
        .min_height(40.0)
        .show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(i18n_t("diagnostics.title")).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Copy").small())
                            .rounding(4.0),
                    )
                    .clicked()
                {
                    let text = format_diagnostics(diagnostics);
                    ctx.output_mut(|o| o.copied_text = text.clone());
                    status = Some(format!(
                        "Copied {} problem{} to clipboard",
                        diagnostics.len(),
                        if diagnostics.len() == 1 { "" } else { "s" }
                    ));
                }
            });
        });
        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for diag in diagnostics {
                    render_row(ui, diag);
                }
            });
    });
    status
}

/// Serialize the diagnostics list to a stable plain-text form
/// suitable for pasting into an issue tracker. The format is one
/// diagnostic per line: `severity Ln N, Col M: message` so a human
/// can read it without any tooling.
fn format_diagnostics(diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for (i, diag) in diagnostics.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&severity_label(diag.severity));
        out.push_str(&format!(
            " Ln {}, Col {}: {}",
            diag.start_line + 1,
            diag.start_col + 1,
            diag.message
        ));
    }
    out
}

fn severity_label(severity: DiagnosticSeverity) -> String {
    match severity {
        DiagnosticSeverity::Error => i18n_t("diagnostics.severity_error"),
        DiagnosticSeverity::Warning => i18n_t("diagnostics.severity_warning"),
        DiagnosticSeverity::Info => i18n_t("diagnostics.severity_info"),
        DiagnosticSeverity::Hint => i18n_t("diagnostics.severity_hint"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(severity: DiagnosticSeverity, line: u32, col: u32, msg: &str) -> Diagnostic {
        Diagnostic {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
            severity,
            message: msg.to_string(),
            source: None,
            range: None,
        }
    }

    #[test]
    fn format_empty_list_is_empty_string() {
        let text = format_diagnostics(&[]);
        assert_eq!(text, "");
    }

    #[test]
    fn format_single_error() {
        let d = [diag(DiagnosticSeverity::Error, 4, 2, "expected `;`")];
        let text = format_diagnostics(&d);
        assert_eq!(text, format!("{} Ln 5, Col 3: expected `;`", i18n_t("diagnostics.severity_error")));
    }

    #[test]
    fn format_mixed_severities_newline_separated() {
        let d = [
            diag(DiagnosticSeverity::Error, 0, 0, "boom"),
            diag(DiagnosticSeverity::Warning, 1, 4, "be careful"),
        ];
        let text = format_diagnostics(&d);
        assert_eq!(
            text,
            format!("{}\n{}", format!("{} Ln 1, Col 1: boom", i18n_t("diagnostics.severity_error")), format!("{} Ln 2, Col 5: be careful", i18n_t("diagnostics.severity_warning")))
        );
    }
}
