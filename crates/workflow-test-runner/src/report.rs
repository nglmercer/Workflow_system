//! Run report types: the per-test and per-run aggregates that the
//! CLI and the editor's test panel render.
//!
//! These types are pure data; the runner fills them in as it
//! executes each test. The CLI's `flow test` command and the
//! editor's `test_panel` both consume [`RunReport`] and project it
//! to whatever their surface needs (a CLI table, a status bar, an
//! egui list).

use serde::{Deserialize, Serialize};

use crate::assert::AssertResult;

/// The verdict for a single `test "..."` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    /// The test name as written in the source (the string
    /// immediately after the `test` keyword).
    pub name: String,
    /// The path of the file the test was loaded from. Used by the
    /// editor's test panel to display "this test lives in
    /// `foo.test.flow`" and to support per-file filtering.
    pub source_path: String,
    /// The event the test was driven by. The editor's test panel
    /// uses this to dim tests that no workflow currently handles.
    pub event: String,
    /// Every assertion that ran, in source order. The test passes
    /// iff every entry has `passed = true` AND
    /// [`matched_workflow_count`](TestReport::matched_workflow_count)
    /// is at least one.
    pub asserts: Vec<AssertResult>,
    /// How many workflows in the host program subscribed to the
    /// test's event. Zero means the test never exercised any
    /// workflow — the runner reports this as a failure with a
    /// synthetic assertion so the user sees the problem.
    pub matched_workflow_count: usize,
    /// `true` iff the test passed (every assertion passed AND
    /// matched at least one workflow).
    pub passed: bool,
}

/// The aggregate of one full `TestRunner::run_*` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub tests: Vec<TestReport>,
    /// Total passed across all tests.
    pub passed: usize,
    /// Total failed across all tests.
    pub failed: usize,
    /// The source path or virtual path the run was driven from.
    /// For a directory run this is the directory; for a single
    /// source string it is the virtual path the caller passed in.
    pub root: String,
}

impl RunReport {
    pub fn from_tests(root: impl Into<String>, tests: Vec<TestReport>) -> Self {
        let mut passed = 0;
        let mut failed = 0;
        for t in &tests {
            if t.passed {
                passed += 1;
            } else {
                failed += 1;
            }
        }
        Self {
            tests,
            passed,
            failed,
            root: root.into(),
        }
    }

    /// `true` iff every test passed.
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}
