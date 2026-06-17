//! `flow test` subcommand. Discovers and runs every
//! `*.test.flow` under a path and prints a pass/fail table.
//!
//! Exits 0 on success, 1 if any test fails. Supports
//! `--json` for machine-readable output and `--filter` for
//! substring matching against test names.

use std::path::Path;
use std::process::ExitCode;
use workflow_i18n::tf as i18n_tf;

use thiserror::Error;
use workflow_test_runner::{TestRunner, TestRunnerConfig};

#[derive(Debug, Error)]
pub enum TestCmdError {
    #[error("test runner: {0}")]
    Runner(#[from] workflow_test_runner::TestRunnerError),
    #[error("serialize: {0}")]
    Serde(#[from] serde_json::Error),
}

pub fn run(path: &str, filter: Option<&str>, json: bool) -> Result<ExitCode, TestCmdError> {
    let config = TestRunnerConfig {
        name_filter: filter.map(str::to_string),
    };
    let runner = TestRunner::new(config);
    let report = runner.run_path(Path::new(path))?;

    if json {
        let s = serde_json::to_string_pretty(&report)?;
        println!("{}", s);
    } else {
        print_human(&report);
    }

    if report.all_passed() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

fn print_human(report: &workflow_test_runner::RunReport) {
    println!("{}", i18n_tf("cli.tests_under", &[("root", &report.root)]));
    for t in &report.tests {
        let mark = if t.passed { "✓" } else { "✗" };
        println!(
            "  {} {} [event: {}, {} assertions]",
            mark,
            t.name,
            t.event,
            t.asserts.len()
        );
        for a in &t.asserts {
            if a.passed {
                continue;
            }
            let var = if a.var_name.is_empty() {
                String::new()
            } else {
                format!(" {}", a.var_name)
            };
            println!(
                "      expect {}{} failed: actual={}, expected={}",
                a.kind.label(),
                var,
                a.actual,
                a.expected
            );
        }
    }
    println!(
        "{}",
        i18n_tf(
            "cli.tests_passed",
            &[
                ("passed", &report.passed.to_string()),
                ("failed", &report.failed.to_string())
            ]
        )
    );
}
