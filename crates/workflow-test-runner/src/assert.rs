//! Assertion kinds and their results.
//!
//! Each `AssertKind` corresponds to one of the four `expect ...`
//! clauses accepted by the test grammar. The runner evaluates them
//! against the [`WorkflowOutcome`](workflow_parser::evaluator::WorkflowOutcome)
//! produced by [`FlowEvaluator`](workflow_parser::FlowEvaluator).
//!
//! The `passed` flag is the only field that matters to test
//! pass/fail aggregation; the other fields exist so the report
//! layer can render diffs.

use serde::{Deserialize, Serialize};
use workflow_parser::ast::ExpectClause;
use workflow_parser::evaluator::{Value, WorkflowOutcome};

/// Discriminator for the four assertion kinds. Mirrors
/// [`ExpectClause`] but stores only the discriminator — the value
/// payload is folded into [`AssertResult::expected`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssertKind {
    /// `expect logs [...]` — element-wise equality with the
    /// captured log strings.
    Logs,
    /// `expect emitted [...]` — element-wise equality with the
    /// emitted event list. The `.flow` evaluator records events
    /// emitted via `emit("EVENT")` calls in the outcome's
    /// `emitted` field.
    Emitted,
    /// `expect return <value>` — equality with the workflow's
    /// final `return` value (or `Null` if it fell off the end).
    Return,
    /// `expect var <name> == <value>` — equality with the
    /// workflow's final scope binding for `name`.
    Var,
}

impl AssertKind {
    pub fn label(self) -> &'static str {
        match self {
            AssertKind::Logs => "logs",
            AssertKind::Emitted => "emitted",
            AssertKind::Return => "return",
            AssertKind::Var => "var",
        }
    }
}

/// The outcome of evaluating a single `expect` clause against a
/// [`WorkflowOutcome`]. `passed` is the aggregate verdict; the
/// other fields are diagnostic context for the report layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertResult {
    pub kind: AssertKind,
    /// The var name when `kind == AssertKind::Var`, otherwise
    /// empty. We keep the name on the result (rather than relying
    /// on the caller to remember) so the report can render the
    /// full clause without a second lookup.
    pub var_name: String,
    pub passed: bool,
    /// What the workflow actually produced, in a form that can be
    /// rendered as text.
    pub actual: String,
    /// What the test expected, in a form that can be rendered as
    /// text.
    pub expected: String,
}

impl AssertResult {
    pub fn pass(
        kind: AssertKind,
        var_name: impl Into<String>,
        actual: impl Into<String>,
        expected: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            var_name: var_name.into(),
            passed: true,
            actual: actual.into(),
            expected: expected.into(),
        }
    }

    pub fn fail(
        kind: AssertKind,
        var_name: impl Into<String>,
        actual: impl Into<String>,
        expected: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            var_name: var_name.into(),
            passed: false,
            actual: actual.into(),
            expected: expected.into(),
        }
    }
}

/// Evaluate a single [`ExpectClause`] against a [`WorkflowOutcome`]
/// and produce an [`AssertResult`]. Returns `None` for
/// unsupported clause types.
///
/// The function never panics: any unexpected shape (e.g. comparing
/// a `String` to a `Number`) yields a `passed = false` result
/// rather than an error. This matches the policy that one test
/// should report every regression it can detect, not just the
/// first one.
pub fn evaluate(clause: &ExpectClause, outcome: &WorkflowOutcome) -> Option<AssertResult> {
    match clause {
        ExpectClause::Logs(expected) => {
            let actual = outcome.logs.clone();
            let passed = actual == *expected;
            let actual_text = vec_to_text(&actual);
            let expected_text = vec_to_text(expected);
            let result = if passed {
                AssertResult::pass(AssertKind::Logs, "", actual_text, expected_text)
            } else {
                AssertResult::fail(AssertKind::Logs, "", actual_text, expected_text)
            };
            Some(result)
        }
        ExpectClause::Emitted(expected) => {
            let actual = outcome.emitted.clone();
            let passed = actual == *expected;
            let actual_text = vec_to_text(&actual);
            let expected_text = vec_to_text(expected);
            let result = if passed {
                AssertResult::pass(AssertKind::Emitted, "", actual_text, expected_text)
            } else {
                AssertResult::fail(AssertKind::Emitted, "", actual_text, expected_text)
            };
            Some(result)
        }
        ExpectClause::Return(expected_json) => {
            let actual = value_to_json(&outcome.return_value);
            let passed = json_equal(&actual, expected_json);
            let result = if passed {
                AssertResult::pass(
                    AssertKind::Return,
                    "",
                    value_to_text(&outcome.return_value),
                    value_to_text_json(expected_json),
                )
            } else {
                AssertResult::fail(
                    AssertKind::Return,
                    "",
                    value_to_text(&outcome.return_value),
                    value_to_text_json(expected_json),
                )
            };
            Some(result)
        }
        ExpectClause::Var {
            name,
            value: expected_json,
        } => {
            let actual_value = outcome.scope.get(name).cloned().unwrap_or(Value::Null);
            let actual = value_to_json(&actual_value);
            let passed = json_equal(&actual, expected_json);
            let result = if passed {
                AssertResult::pass(
                    AssertKind::Var,
                    name.clone(),
                    value_to_text(&actual_value),
                    value_to_text_json(expected_json),
                )
            } else {
                AssertResult::fail(
                    AssertKind::Var,
                    name.clone(),
                    value_to_text(&actual_value),
                    value_to_text_json(expected_json),
                )
            };
            Some(result)
        }
    }
}

/// Render a `Vec<String>` as a JSON-ish text. The test panel and
/// the CLI both want a single line of text per assertion, so we
/// keep this minimal: a bracketed, comma-separated list with
/// double-quoted strings (matching the source syntax).
fn vec_to_text(v: &[String]) -> String {
    let parts: Vec<String> = v.iter().map(|s| format!("\"{}\"", s)).collect();
    format!("[{}]", parts.join(", "))
}

/// Convert a [`Value`] into a `serde_json::Value` for comparison.
/// The parser's `Value` already has a `to_json` method, so this is
/// a thin re-export kept here to avoid leaking the parser import
/// into the report layer.
fn value_to_json(v: &Value) -> serde_json::Value {
    v.to_json()
}

/// Render a [`Value`] as a human-readable string for the report
/// layer. Falls back to `to_string` for collection types.
fn value_to_text(v: &Value) -> String {
    v.to_string()
}

/// Render a `serde_json::Value` as a string for comparison. Booleans
/// and null render without quotes; numbers as their literal form;
/// strings without surrounding quotes (so they match the captured
/// log format); objects/arrays via their `to_string` form.
fn value_to_text_json(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Deep equality between two `serde_json::Value`s. The default
/// `PartialEq` impl is sufficient except for one common case:
/// `serde_json::Value::Number(42)` (an integer) does not equal
/// `serde_json::Value::Number(42.0)` (a float) even though they're
/// the same number. The `.flow` runtime always returns floats, but
/// test authors will write integer literals in `expect return 42`,
/// so we do a numeric fallback when both sides are numbers.
fn json_equal(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    if a == b {
        return true;
    }
    if let (serde_json::Value::Number(x), serde_json::Value::Number(y)) = (a, b) {
        if let (Some(xf), Some(yf)) = (x.as_f64(), y.as_f64()) {
            return xf == yf;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use workflow_parser::evaluator::Value;

    fn outcome_with(
        logs: Vec<String>,
        ret: Value,
        scope: HashMap<String, Value>,
    ) -> WorkflowOutcome {
        WorkflowOutcome {
            logs,
            emitted: Vec::new(),
            return_value: ret,
            scope,
        }
    }

    #[test]
    fn logs_pass_when_equal() {
        let outcome = outcome_with(vec!["hi".to_string()], Value::Null, HashMap::new());
        let r = evaluate(&ExpectClause::Logs(vec!["hi".to_string()]), &outcome).unwrap();
        assert!(r.passed);
        assert_eq!(r.kind, AssertKind::Logs);
    }

    #[test]
    fn logs_fail_when_mismatch() {
        let outcome = outcome_with(vec!["hi".to_string()], Value::Null, HashMap::new());
        let r = evaluate(&ExpectClause::Logs(vec!["bye".to_string()]), &outcome).unwrap();
        assert!(!r.passed);
        assert_eq!(r.actual, "[\"hi\"]");
        assert_eq!(r.expected, "[\"bye\"]");
    }

    #[test]
    fn var_unbound_is_null() {
        let outcome = outcome_with(vec![], Value::Null, HashMap::new());
        let r = evaluate(
            &ExpectClause::Var {
                name: "x".to_string(),
                value: serde_json::json!("hi"),
            },
            &outcome,
        )
        .unwrap();
        assert!(!r.passed);
        assert_eq!(r.actual, "null");
    }

    #[test]
    fn return_value_passes_when_null() {
        let outcome = outcome_with(vec![], Value::Null, HashMap::new());
        let r = evaluate(&ExpectClause::Return(serde_json::Value::Null), &outcome).unwrap();
        assert!(r.passed);
    }

    #[test]
    fn emitted_pass_when_equal() {
        let mut outcome = outcome_with(vec![], Value::Null, HashMap::new());
        outcome.emitted = vec!["EVENT_A".to_string(), "EVENT_B".to_string()];
        let r = evaluate(
            &ExpectClause::Emitted(vec!["EVENT_A".to_string(), "EVENT_B".to_string()]),
            &outcome,
        )
        .unwrap();
        assert!(r.passed);
        assert_eq!(r.kind, AssertKind::Emitted);
    }

    #[test]
    fn emitted_fail_when_mismatch() {
        let mut outcome = outcome_with(vec![], Value::Null, HashMap::new());
        outcome.emitted = vec!["EVENT_A".to_string()];
        let r = evaluate(
            &ExpectClause::Emitted(vec!["EVENT_B".to_string()]),
            &outcome,
        )
        .unwrap();
        assert!(!r.passed);
        assert_eq!(r.actual, "[\"EVENT_A\"]");
        assert_eq!(r.expected, "[\"EVENT_B\"]");
    }

    #[test]
    fn emitted_empty_passes_when_expected_empty() {
        let outcome = outcome_with(vec![], Value::Null, HashMap::new());
        let r = evaluate(&ExpectClause::Emitted(vec![]), &outcome).unwrap();
        assert!(r.passed);
    }

    #[test]
    fn emitted_fail_when_expected_nonempty_but_actual_empty() {
        let outcome = outcome_with(vec![], Value::Null, HashMap::new());
        let r = evaluate(
            &ExpectClause::Emitted(vec!["EVENT_A".to_string()]),
            &outcome,
        )
        .unwrap();
        assert!(!r.passed);
    }
}
