//! Test execution: parse a `.flow` source, run every `TestDef`,
//! and produce a [`RunReport`].
//!
//! Two entry points are exposed:
//!
//! - [`execute_test`] runs a single `TestDef` against a host
//!   program and returns a [`TestReport`].
//! - [`execute_tests_for_program`] runs every test in a parsed
//!   [`FlowProgram`] (used by the editor's "Run on buffer" path,
//!   where the source is in memory and there's no file on disk).
//!
//! Both functions share the same per-test execution model:
//! 1. Fresh [`FlowEvaluator`] with the host's globals and function
//!    definitions installed.
//! 2. For each `WorkflowDef` whose `on` matches the test's event,
//!    run it on a sub-evaluator that shares `functions` and
//!    `globals` but has its own `logs`, `scope`, and (later)
//!    `emitted`.
//! 3. Aggregate every assertion's [`AssertResult`] into the test
//!    report. The test passes iff every assertion passes AND at
//!    least one workflow matched.

use workflow_domain::TriggerContext;
use workflow_parser::ast::{FlowProgram, OnClause, TestDef, WorkflowDef};
use workflow_parser::evaluator::{FlowEvaluator, Value, WorkflowOutcome};

use crate::assert::{evaluate, AssertKind, AssertResult};
use crate::report::{RunReport, TestReport};

/// Run a single `TestDef` against a host program and return a
/// `TestReport`. The `host` is the parsed program containing the
/// `WorkflowDef`s under test (and any globals/functions the
/// workflows depend on).
pub fn execute_test(test: &TestDef, host: &FlowProgram, source_path: &str) -> TestReport {
    let matches: Vec<&WorkflowDef> = host
        .workflows
        .iter()
        .filter(|w| w.event == test.on.event)
        .collect();

    let mut asserts: Vec<AssertResult> = Vec::new();

    if matches.is_empty() {
        // Synthesize a single failing assertion so the report
        // shows up in the UI. This is the most common mistake
        // for new test authors (a typo in the event name), so
        // we want a clear "no workflow matched" message rather
        // than an empty green checkmark.
        asserts.push(AssertResult::fail(
            AssertKind::Logs,
            "",
            String::new(),
            format!("no workflow handles event '{}'", test.on.event),
        ));
        return TestReport {
            name: test.name.clone(),
            source_path: source_path.to_string(),
            event: test.on.event.clone(),
            asserts,
            matched_workflow_count: 0,
            passed: false,
        };
    }

    // Aggregate the outcome across all matching workflows. We
    // concatenate logs in workflow-definition order and merge
    // scopes (last writer wins for any given name, which is
    // what users expect when two workflows both set `greeting`).
    let mut combined_logs: Vec<String> = Vec::new();
    let mut combined_scope = std::collections::HashMap::new();
    let mut combined_return = Value::Null;

    for workflow in &matches {
        let ctx = trigger_context_from(&test.on);
        let mut evaluator = new_evaluator_with_program(host);
        let outcome = match evaluator.execute_workflow_with_result(workflow, &ctx) {
            Ok(out) => out,
            Err(e) => {
                asserts.push(AssertResult::fail(
                    AssertKind::Logs,
                    "",
                    String::new(),
                    format!("workflow '{}' errored: {}", workflow.name, e),
                ));
                continue;
            }
        };
        combined_logs.extend(outcome.logs);
        for (k, v) in outcome.scope {
            combined_scope.insert(k, v);
        }
        // The "last workflow's return value" is a reasonable
        // aggregate. Tests that care about a specific workflow's
        // return value should isolate it with a unique event.
        combined_return = outcome.return_value;
    }

    let combined = WorkflowOutcome {
        logs: combined_logs,
        return_value: combined_return,
        scope: combined_scope,
    };

    for clause in &test.expects {
        if let Some(result) = evaluate(clause, &combined) {
            asserts.push(result);
        }
    }

    let passed = asserts.iter().all(|a| a.passed);
    TestReport {
        name: test.name.clone(),
        source_path: source_path.to_string(),
        event: test.on.event.clone(),
        asserts,
        matched_workflow_count: matches.len(),
        passed,
    }
}

/// Run every test in `program` (i.e. every `TestDef` in
/// `program.tests`) and return a [`RunReport`] rooted at
/// `root_path`. The same `program` is used as both the test
/// source and the host — sidecar test files that include the
/// workflows they exercise inline, plus the editor's "run on
/// buffer" path, both hit this.
pub fn execute_tests_for_program(program: &FlowProgram, root_path: &str) -> RunReport {
    let tests: Vec<TestReport> = program
        .tests
        .iter()
        .map(|t| execute_test(t, program, root_path))
        .collect();
    RunReport::from_tests(root_path, tests)
}

/// Build a `FlowEvaluator` preloaded with the host program's
/// globals and function definitions. Each test gets a fresh one
/// to avoid cross-test leakage.
fn new_evaluator_with_program(host: &FlowProgram) -> FlowEvaluator {
    let mut ev = FlowEvaluator::new();
    ev.load_program(host);
    ev
}

/// Convert a test's `OnClause` into a `TriggerContext` the
/// evaluator understands. The `id` and `timestamp` fields are
/// filler values — tests don't currently inspect them.
fn trigger_context_from(on: &OnClause) -> TriggerContext {
    TriggerContext {
        event: on.event.clone(),
        timestamp: 0,
        data: on.data.clone(),
        vars: None,
        id: None,
    }
}

// `TriggerContext` and `WorkflowError` are public from
// workflow-domain. The `Value` re-export keeps the import set
// narrow in this module.
#[allow(unused_imports)]
use workflow_domain::TriggerContext as _UnusedTriggerContext;
#[allow(unused_imports)]
use workflow_parser::evaluator::Value as _UnusedValue;

#[cfg(test)]
mod tests {
    use super::*;
    use workflow_parser::FlowParser;

    fn parse(src: &str) -> FlowProgram {
        FlowParser::parse_flow_program(src).unwrap()
    }

    #[test]
    fn runs_workflow_and_passes_log_assertion() {
        let host = parse(
            r#"workflow "Greet" {
  on HELLO
  log("hi " + data.name)
}"#,
        );
        let program = parse(
            r#"test "Greet works" {
  on HELLO with { name: "Ada" }
  expect logs ["hi Ada"]
}"#,
        );
        let report = execute_test(&program.tests[0], &host, "host.flow");
        assert!(report.passed, "report: {:#?}", report);
        assert_eq!(report.matched_workflow_count, 1);
    }

    #[test]
    fn fails_when_workflow_missing() {
        let host = parse("workflow \"Other\" { on OTHER }\n");
        let program = parse(
            r#"test "Never matches" {
  on HELLO
  expect logs []
}"#,
        );
        let report = execute_test(&program.tests[0], &host, "host.flow");
        assert!(!report.passed);
        assert_eq!(report.matched_workflow_count, 0);
    }

    #[test]
    fn fails_on_wrong_log() {
        let host = parse(
            r#"workflow "Greet" {
  on HELLO
  log("hi")
}"#,
        );
        let program = parse(
            r#"test "Wrong log" {
  on HELLO
  expect logs ["bye"]
}"#,
        );
        let report = execute_test(&program.tests[0], &host, "host.flow");
        assert!(!report.passed);
    }

    #[test]
    fn assert_var_picks_up_workflow_scope() {
        let host = parse(
            r#"workflow "Calc" {
  on E
  var total = 5 + 7
}"#,
        );
        let program = parse(
            r#"test "Total" {
  on E
  expect var total == 12
}"#,
        );
        let report = execute_test(&program.tests[0], &host, "h.flow");
        assert!(report.passed, "{:#?}", report);
    }

    #[test]
    fn assert_return_captures_value() {
        let host = parse(
            r#"workflow "Double" {
  on E
  return data.x * 2
}"#,
        );
        let program = parse(
            r#"test "Doubles" {
  on E with { x: 21 }
  expect return 42
}"#,
        );
        let report = execute_test(&program.tests[0], &host, "h.flow");
        assert!(report.passed, "{:#?}", report);
    }
}
