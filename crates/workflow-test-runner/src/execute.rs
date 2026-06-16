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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use workflow_domain::TriggerContext;
use workflow_parser::ast::{FlowProgram, ImportSource, ImportStmt, OnClause, TestDef, WorkflowDef};
use workflow_parser::evaluator::{FlowEvaluator, Value, WorkflowOutcome};
use workflow_parser::FlowParser;

use crate::assert::{evaluate, AssertKind, AssertResult};
use crate::report::{RunReport, TestReport};

/// Run a single `TestDef` against a host program and return a
/// `TestReport`. The `host` is the parsed program containing the
/// `WorkflowDef`s under test (and any globals/functions the
/// workflows depend on). `host_source_dir` is the directory
/// containing the host's source file, used to resolve relative
/// import paths (e.g. the JSON schemas referenced by
/// `@import <name> from "./user_registered.json"`).
pub fn execute_test(
    test: &TestDef,
    host: &FlowProgram,
    host_source_dir: &Path,
    source_path: &str,
) -> TestReport {
    let matches: Vec<&WorkflowDef> = host
        .workflows
        .iter()
        .filter(|w| w.event == test.on.event)
        .collect();

    let mut asserts: Vec<AssertResult> = Vec::new();

    // Resolve every `@import` (or `import <name> from "*.json"`)
    // in the host program up front, before checking matches.
    // A missing JSON file is a real problem the user needs to
    // hear about, so we surface it as a synthetic failing
    // assertion even if workflows do match — otherwise the
    // tests would silently log "null" everywhere and the user
    // would be debugging the wrong thing.
    let mut import_resolution = populate_imports(&host.imports, host_source_dir);
    for failure in &import_resolution.failures {
        asserts.push(AssertResult::fail(
            AssertKind::Logs,
            "",
            String::new(),
            format!("import resolution failed: {}", failure),
        ));
    }

    // If the test's event matches an `@import` name, overlay
    // the test's `with` payload onto the import. This is what
    // makes the test runner's `with { ... }` clause the source
    // of truth at test time: the schema provides the defaults
    // (so workflows that read fields not mentioned in `with`
    // still get reasonable values), and `with` overrides them
    // for the fields the test cares about.
    let merged_data = merge_test_payload(&import_resolution.globals, &test.on);
    import_resolution
        .globals
        .insert(test.on.event.clone(), merged_data.clone());

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
    let mut combined_emitted: Vec<String> = Vec::new();
    let mut combined_scope = std::collections::HashMap::new();
    let mut combined_return = Value::Null;

    for workflow in &matches {
        let ctx = trigger_context_from_data(&test.on, merged_data.to_json());
        let mut evaluator = new_evaluator_with_program(
            host,
            &import_resolution.globals,
            &import_resolution.flow_programs,
        );
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
        combined_emitted.extend(outcome.emitted);
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
        emitted: combined_emitted,
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
        .map(|t| execute_test(t, program, Path::new(""), root_path))
        .collect();
    RunReport::from_tests(root_path, tests)
}

/// Build a `FlowEvaluator` preloaded with the host program's
/// globals, function definitions, `@import`-bound payloads, and
/// any `.flow` module imports for shared functions. Each test
/// gets a fresh one to avoid cross-test leakage.
fn new_evaluator_with_program(
    host: &FlowProgram,
    imported_globals: &HashMap<String, Value>,
    flow_programs: &[FlowProgram],
) -> FlowEvaluator {
    let mut ev = FlowEvaluator::new();
    ev.load_program(host);
    ev.populate_globals(imported_globals.clone());
    for program in flow_programs {
        ev.merge_program(program);
    }
    ev
}

/// Build a `TriggerContext` whose `data` is the merged payload
/// for the test. The runner uses this when the host program
/// has an `@import` whose name matches the test's event — the
/// merge is already in `data`, so workflows that destructure
/// (`on E (data)`) see the merged value rather than the raw
/// `with` payload.
fn trigger_context_from_data(on: &OnClause, data: serde_json::Value) -> TriggerContext {
    TriggerContext {
        event: on.event.clone(),
        timestamp: 0,
        data,
        vars: None,
        id: None,
    }
}

/// Merge the test's `with` payload onto the import whose name
/// matches the test's event. Returns the merged JSON value, or
/// the test's `with` payload unchanged if no matching import
/// exists (which is the right behavior for workflows that
/// don't use `@import`).
pub fn merge_test_payload(imports: &HashMap<String, Value>, on: &OnClause) -> Value {
    let import_value = match imports.get(&on.event) {
        Some(v) => v.clone(),
        None => return Value::from_json(&on.data),
    };
    let import_json = import_value.to_json();
    let merged = merge_json(&import_json, &on.data);
    Value::from_json(&merged)
}

/// Shallow merge: `overlay` fields win over `base` fields.
/// Nested objects are merged recursively; arrays and scalars
/// are replaced wholesale. We don't try to be clever about
/// partial array updates — `expect items: [{...}]` should
/// fully replace the schema's default items, not append to
/// them.
fn merge_json(base: &serde_json::Value, overlay: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            let mut out = base_map.clone();
            for (k, v) in overlay_map {
                let merged = match out.get(k) {
                    Some(existing) => merge_json(existing, v),
                    None => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        (_, overlay) => overlay.clone(),
    }
}

/// The result of resolving the host program's `@import` (or
/// `import <name> from "*.json"`) declarations into runtime
/// bindings. `globals` is keyed by import name; `failures`
/// carries human-readable diagnostics for any import that
/// couldn't be resolved. `flow_programs` holds parsed `.flow`
/// modules whose functions/globals can be merged into the
/// evaluator.
#[derive(Debug, Default)]
struct ImportResolution {
    globals: HashMap<String, Value>,
    flow_programs: Vec<FlowProgram>,
    failures: Vec<String>,
}

/// Walk every import in the host program and resolve the ones
/// that bind a payload (i.e. JSON paths and inline JSON
/// literals) or import `.flow` modules for shared functions.
fn populate_imports(imports: &[ImportStmt], source_dir: &Path) -> ImportResolution {
    let mut out = ImportResolution::default();
    for import in imports {
        match &import.source {
            ImportSource::Inline(value) => {
                out.globals
                    .insert(import.name.clone(), Value::from_json(value));
            }
            ImportSource::Path(path_str) => {
                if is_json_path(path_str) {
                    let resolved = resolve_relative(source_dir, path_str);
                    match std::fs::read_to_string(&resolved) {
                        Ok(contents) => {
                            match serde_json::from_str::<serde_json::Value>(&contents) {
                                Ok(value) => {
                                    out.globals
                                        .insert(import.name.clone(), Value::from_json(&value));
                                }
                                Err(e) => out.failures.push(format!(
                                    "{} from {}: invalid JSON: {}",
                                    import.name,
                                    resolved.display(),
                                    e
                                )),
                            }
                        }
                        Err(e) => out.failures.push(format!(
                            "{} from {}: {}",
                            import.name,
                            resolved.display(),
                            e
                        )),
                    }
                } else if is_flow_path(path_str) {
                    let resolved = resolve_relative(source_dir, path_str);
                    match std::fs::read_to_string(&resolved) {
                        Ok(contents) => match FlowParser::parse_flow_program(&contents) {
                            Ok(program) => {
                                out.flow_programs.push(program);
                            }
                            Err(e) => out.failures.push(format!(
                                "{} from {}: parse error: {}",
                                import.name,
                                resolved.display(),
                                e
                            )),
                        },
                        Err(e) => out.failures.push(format!(
                            "{} from {}: {}",
                            import.name,
                            resolved.display(),
                            e
                        )),
                    }
                }
            }
        }
    }
    out
}

/// True when `path_str` (the raw string from an import) ends in
/// `.json` (case-insensitive). Used to filter the host's
/// imports down to the ones the test runner can inject as a
/// payload.
fn is_json_path(path_str: &str) -> bool {
    Path::new(path_str)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

/// True when `path_str` ends in `.flow` (case-insensitive).
/// Used to detect module imports that should be parsed and
/// merged into the evaluator for shared function access.
fn is_flow_path(path_str: &str) -> bool {
    Path::new(path_str)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("flow"))
}

/// Resolve `path_str` against `source_dir`. Absolute paths are
/// passed through unchanged; relative ones are joined onto
/// `source_dir`. Returns a `PathBuf` even when the file is
/// missing — the caller decides how to report that.
fn resolve_relative(source_dir: &Path, path_str: &str) -> PathBuf {
    let candidate = Path::new(path_str);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        source_dir.join(candidate)
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
        let report = execute_test(&program.tests[0], &host, Path::new(""), "host.flow");
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
        let report = execute_test(&program.tests[0], &host, Path::new(""), "host.flow");
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
        let report = execute_test(&program.tests[0], &host, Path::new(""), "host.flow");
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
        let report = execute_test(&program.tests[0], &host, Path::new(""), "h.flow");
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
        let report = execute_test(&program.tests[0], &host, Path::new(""), "h.flow");
        assert!(report.passed, "{:#?}", report);
    }

    #[test]
    fn resolves_json_import_into_globals() {
        // The host uses an @import-bound name directly in the
        // workflow body. The runner is responsible for
        // resolving the JSON file and injecting it as a
        // global binding.
        use workflow_parser::ast::ImportSource;
        let mut host = parse(
            r#"workflow "Greet" {
  on E
  log("Hi " + USER.email)
}"#,
        );
        host.imports.push(ImportStmt {
            name: "USER".to_string(),
            source: ImportSource::Inline(serde_json::json!({ "email": "ada@example.com" })),
            span: workflow_parser::ast::Span::default(),
        });
        let program = parse(
            r#"test "Greet uses imported USER" {
  on E
  expect logs ["Hi ada@example.com"]
}"#,
        );
        let report = execute_test(&program.tests[0], &host, Path::new(""), "h.flow");
        assert!(report.passed, "report: {:#?}", report);
    }

    #[test]
    fn reports_missing_import_file() {
        // If the @import points at a JSON file that doesn't
        // exist, the runner surfaces a synthetic failing
        // assertion rather than silently letting the workflow
        // log "null" everywhere.
        use workflow_parser::ast::ImportSource;
        let mut host = parse(
            r#"workflow "Greet" {
  on E
  log("Hi " + USER.email)
}"#,
        );
        host.imports.push(ImportStmt {
            name: "USER".to_string(),
            source: ImportSource::Path("./does_not_exist.json".to_string()),
            span: workflow_parser::ast::Span::default(),
        });
        let program = parse(
            r#"test "Greet" {
  on E
  expect logs ["Hi ada@example.com"]
}"#,
        );
        let report = execute_test(&program.tests[0], &host, Path::new("."), "h.flow");
        assert!(!report.passed);
        // The failure is in the synthetic import-resolution
        // assertion, not in the logs assertion, but both
        // count as failures. Verify at least one failure
        // mentions the import.
        let any_import_failure = report
            .asserts
            .iter()
            .any(|a| !a.passed && a.expected.contains("import resolution failed"));
        assert!(
            any_import_failure,
            "expected an import-resolution failure in {:#?}",
            report
        );
    }

    #[test]
    fn emit_records_events_in_outcome() {
        let host = parse(
            r#"workflow "Emit" {
  on E
  emit("EVENT_A")
  emit("EVENT_B")
  log("done")
}"#,
        );
        let program = parse(
            r#"test "Emit works" {
  on E
  expect emitted ["EVENT_A", "EVENT_B"]
  expect logs ["done"]
}"#,
        );
        let report = execute_test(&program.tests[0], &host, Path::new(""), "h.flow");
        assert!(report.passed, "report: {:#?}", report);
    }

    #[test]
    fn emit_conditional_events() {
        let host = parse(
            r#"workflow "Conditional Emit" {
  on E
  if (data.premium == true) {
    emit("PREMIUM_EVENT")
    log("premium")
  } else {
    emit("FREE_EVENT")
    log("free")
  }
}"#,
        );
        let program = parse(
            r#"test "Premium emit" {
  on E with { premium: true }
  expect emitted ["PREMIUM_EVENT"]
  expect logs ["premium"]
}"#,
        );
        let report = execute_test(&program.tests[0], &host, Path::new(""), "h.flow");
        assert!(report.passed, "report: {:#?}", report);
    }
}
