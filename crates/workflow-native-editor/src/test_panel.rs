//! Bottom panel for runtime test results.
//!
//! Mirrors [`super::diagnostics_panel`]: a small panel that
//! renders a list of pass/fail rows for the most recent test run.
//! The panel is purely a *view* over a [`RunReport`] — the actual
//! test execution happens off-thread in [`EditorApp`] and the
//! result is delivered via a channel.
//!
//! The panel exposes a Run button and a (while-running) Cancel
//! button. Both are passed in as `impl FnOnce()` closures so this
//! module doesn't need to know about [`EditorApp`]'s state
//! machine — the integration glue lives in `app.rs`.

use crate::theme::Theme;
use eframe::egui::{self, Color32, RichText, ScrollArea};
use workflow_i18n::{t as i18n_t, tf as i18n_tf};
use workflow_test_runner::RunReport;

/// Render the test panel. Returns a status-bar message describing
/// the result of any user action this frame (currently "Copied N
/// results to clipboard"), or `None` if nothing happened.
pub fn show(
    ctx: &egui::Context,
    report: &Option<RunReport>,
    running: bool,
    on_run: impl FnOnce(),
    on_cancel: impl FnOnce(),
) -> Option<String> {
    let mut status: Option<String> = None;
    egui::TopBottomPanel::bottom("test_panel")
        .resizable(true)
        .default_height(140.0)
        .min_height(60.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(i18n_t("test_panel.title")).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    if running {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(i18n_t("test_panel.cancel")).small(),
                                )
                                .rounding(4.0),
                            )
                            .clicked()
                        {
                            on_cancel();
                        }
                    } else if ui
                        .add(
                            egui::Button::new(RichText::new(i18n_t("test_panel.run")).small())
                                .rounding(4.0),
                        )
                        .clicked()
                    {
                        on_run();
                    }
                    if let Some(r) = report {
                        if !running
                            && !r.tests.is_empty()
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(i18n_t("diagnostics.copy")).small(),
                                    )
                                    .rounding(4.0),
                                )
                                .clicked()
                        {
                            let text = format_report(r);
                            ctx.output_mut(|o| o.copied_text = text.clone());
                            status = Some(format!(
                                "Copied {} test result{} to clipboard",
                                r.tests.len(),
                                if r.tests.len() == 1 { "" } else { "s" }
                            ));
                        }
                    }
                });
            });

            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if running {
                        ui.label(RichText::new("Running…").italics());
                        return;
                    }
                    match report {
                        None => {
                            ui.label(RichText::new(i18n_t("test_panel.idle_hint")).weak());
                        }
                        Some(r) => render_report(ui, r),
                    }
                });
        });
    status
}

fn render_report(ui: &mut egui::Ui, report: &RunReport) {
    if report.tests.is_empty() {
        ui.label(RichText::new(i18n_t("test_panel.no_tests")).weak());
        return;
    }
    for t in &report.tests {
        render_test_row(ui, t);
    }
    ui.separator();
    ui.label(
        RichText::new(i18n_tf(
            "test_panel.summary",
            &[
                ("passed", &report.passed.to_string()),
                ("failed", &report.failed.to_string()),
            ],
        ))
        .strong(),
    );
}

fn render_test_row(ui: &mut egui::Ui, t: &workflow_test_runner::TestReport) {
    let (color, icon) = style_for_pass(t.passed);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).color(color));
        ui.label(RichText::new(format!("{} [on {}]", t.name, t.event)).color(color));
    });
    for a in t.asserts.iter().filter(|a| !a.passed) {
        let var = if a.var_name.is_empty() {
            String::new()
        } else {
            format!(" {}", a.var_name)
        };
        ui.label(
            RichText::new(format!(
                "    expect {}{}  actual: {}  expected: {}",
                a.kind.label(),
                var,
                a.actual,
                a.expected
            ))
            .color(Theme::test_pass(false)),
        );
    }
}

fn style_for_pass(passed: bool) -> (Color32, &'static str) {
    if passed {
        (Theme::test_pass(true), "✓")
    } else {
        (Theme::test_pass(false), "✗")
    }
}

fn format_report(report: &RunReport) -> String {
    let mut out = String::new();
    for (i, t) in report.tests.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mark = if t.passed {
            i18n_t("test_panel.report_pass")
        } else {
            i18n_t("test_panel.report_fail")
        };
        out.push_str(&format!("{} {} (event {})\n", mark, t.name, t.event));
        for a in &t.asserts {
            let var = if a.var_name.is_empty() {
                String::new()
            } else {
                format!(" {}", a.var_name)
            };
            let verdict = if a.passed {
                i18n_t("test_panel.report_verdict_pass")
            } else {
                i18n_t("test_panel.report_verdict_fail")
            };
            out.push_str(&format!(
                "    expect {}{} {}  actual={} expected={}\n",
                a.kind.label(),
                var,
                verdict,
                a.actual,
                a.expected
            ));
        }
    }
    out.push_str(&i18n_tf(
        "test_panel.report_summary",
        &[
            ("passed", &report.passed.to_string()),
            ("failed", &report.failed.to_string()),
        ],
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use workflow_test_runner::assert::AssertKind;
    use workflow_test_runner::{AssertResult, TestReport};

    fn report_one_pass() -> RunReport {
        let t = TestReport {
            name: "T1".to_string(),
            source_path: "x.test.flow".to_string(),
            event: "E".to_string(),
            asserts: vec![AssertResult::pass(
                AssertKind::Logs,
                "",
                "[\"hi\"]",
                "[\"hi\"]",
            )],
            matched_workflow_count: 1,
            passed: true,
        };
        RunReport::from_tests("x", vec![t])
    }

    fn report_one_fail() -> RunReport {
        let t = TestReport {
            name: "T1".to_string(),
            source_path: "x.test.flow".to_string(),
            event: "E".to_string(),
            asserts: vec![AssertResult::fail(
                AssertKind::Logs,
                "",
                "[\"hi\"]",
                "[\"bye\"]",
            )],
            matched_workflow_count: 1,
            passed: false,
        };
        RunReport::from_tests("x", vec![t])
    }

    #[test]
    fn format_report_marks_pass_and_fail() {
        let r = RunReport::from_tests(
            "x",
            vec![
                report_one_pass().tests[0].clone(),
                report_one_fail().tests[0].clone(),
            ],
        );
        let text = format_report(&r);
        assert!(text.contains(&format!("{} T1", i18n_t("test_panel.report_pass"))));
        assert!(text.contains(&format!("{} T1", i18n_t("test_panel.report_fail"))));
        assert!(text.contains(&i18n_tf(
            "test_panel.report_summary",
            &[("passed", "1"), ("failed", "1")]
        )));
    }

    #[test]
    fn format_report_handles_unbound_var() {
        let t = TestReport {
            name: "T2".to_string(),
            source_path: "x.test.flow".to_string(),
            event: "E".to_string(),
            asserts: vec![AssertResult::fail(AssertKind::Var, "x", "null", "\"hi\"")],
            matched_workflow_count: 0,
            passed: false,
        };
        let r = RunReport::from_tests("x", vec![t]);
        let text = format_report(&r);
        assert!(text.contains("var x"));
        assert!(text.contains("actual=null"));
    }
}

#[cfg(test)]
mod theme_tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn pass_color_matches_theme() {
        assert_eq!(style_for_pass(true).0, Theme::test_pass(true));
        assert_eq!(style_for_pass(false).0, Theme::test_pass(false));
    }
}
