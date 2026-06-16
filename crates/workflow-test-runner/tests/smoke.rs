//! End-to-end smoke tests for the test runner.
//!
//! These run against the fixtures in `tests/fixtures/`. They
//! exercise the same code paths the CLI and the editor's test
//! panel hit, so a green suite here means the runner is wired
//! correctly end-to-end.

use std::path::Path;

use workflow_test_runner::{TestRunner, TestRunnerConfig};

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn run_hello_fixtures_all_pass() {
    let runner = TestRunner::with_default_config();
    let report = runner
        .run_path(&fixtures().join("hello.test.flow"))
        .expect("run hello");
    assert_eq!(report.passed, 4, "report: {:#?}", report);
    assert_eq!(report.failed, 0);
    for t in &report.tests {
        assert!(t.passed, "test {} should pass, got: {:#?}", t.name, t);
        assert!(t.matched_workflow_count >= 1);
    }
}

#[test]
fn run_failing_fixtures_reports_failures() {
    let runner = TestRunner::with_default_config();
    let report = runner
        .run_path(&fixtures().join("failing.test.flow"))
        .expect("run failing");
    assert!(!report.all_passed());
    // Both tests should fail; the first because the log
    // string is wrong, the second because the var is
    // unbound.
    assert_eq!(report.failed, 2, "report: {:#?}", report);
    let names: Vec<&str> = report.tests.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"Wrong log"));
    assert!(names.contains(&"Unbound var"));
}

#[test]
fn name_filter_only_runs_matching_tests() {
    let runner = TestRunner::new(TestRunnerConfig {
        name_filter: Some("Greet".to_string()),
    });
    let report = runner
        .run_path(&fixtures().join("hello.test.flow"))
        .expect("run");
    // 2 of 4 hello tests have "Greet" in the name; "Double" and
    // "Empty emitted" are filtered out.
    assert_eq!(report.tests.len(), 2);
    assert!(report.tests.iter().all(|t| t.passed));
}

#[test]
fn run_source_uses_in_memory_buffer() {
    // The editor's "Run on buffer" path uses run_source with
    // the in-memory text. Confirm the same runner can be
    // driven that way without a host file on disk.
    let source = "workflow \"Greet\" {\n  on E\n  log(\"hi \" + data.name)\n}\n\ntest \"Greets\" {\n  on E with { name: \"Ada\" }\n  expect logs [\"hi Ada\"]\n}\n";
    let runner = TestRunner::with_default_config();
    let report = runner.run_source(source, "<buffer>").expect("run source");
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 0);
}

#[test]
fn run_source_with_host_pairs_in_memory_test_with_in_memory_host() {
    // The editor's sidecar `*.test.flow` path: the test buffer
    // is unsaved, but the matching `*.flow` host is read from
    // disk. The two sources are passed separately to the
    // runner; without the host, every test would report
    // "no workflow handles event 'E'".
    let host = "workflow \"Greet\" {\n  on E\n  log(\"hi \" + data.name)\n}\n";
    let test = "test \"Greets\" {\n  on E with { name: \"Ada\" }\n  expect logs [\"hi Ada\"]\n}\n";
    let runner = TestRunner::with_default_config();
    let report = runner
        .run_source_with_host(test, "<buffer.test.flow>", Some(host), Some("/tmp/host.flow"))
        .expect("run with host");
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 0);
    assert!(report.tests[0].matched_workflow_count >= 1);
}

/// Path to the workspace's `examples/` directory. Used by the
/// integration tests that exercise the example test suites
/// (basic.test.flow and advanced.test.flow).
fn examples() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

#[test]
fn run_basic_example_tests() {
    let runner = TestRunner::with_default_config();
    let report = runner
        .run_path(&examples().join("basic.test.flow"))
        .expect("run basic");
    assert_eq!(report.passed, 7, "report: {:#?}", report);
    assert_eq!(report.failed, 0);
}

#[test]
fn run_advanced_example_tests() {
    let runner = TestRunner::with_default_config();
    let report = runner
        .run_path(&examples().join("advanced.test.flow"))
        .expect("run advanced");
    assert_eq!(report.passed, 7, "report: {:#?}", report);
    assert_eq!(report.failed, 0);
}
