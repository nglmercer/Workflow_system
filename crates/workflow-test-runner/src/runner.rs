//! The top-level `TestRunner` façade used by the CLI and the
//! editor's test panel.
//!
//! Two execution paths are exposed:
//!
//! - [`TestRunner::run_path`] — discover and run every
//!   `*.test.flow` under a path. Used by the CLI's
//!   `flow test <path>` command.
//! - [`TestRunner::run_source`] — parse a single in-memory
//!   buffer and run its tests. Used by the editor's "Run on
//!   buffer" path, where the file may not be saved yet.
//!
//! Both paths share the same per-file execution model
//! ([`execute::execute_test`]). `run_source` skips the discovery
//! step entirely; `run_path` walks the tree and pairs each
//! `*.test.flow` with its `*.flow` host before running.

use std::fs;
use std::path::Path;

use thiserror::Error;
use workflow_parser::FlowParser;

use crate::discovery::{discover, DiscoverEntry, DiscoverError};
use crate::execute::execute_test;
use crate::report::{RunReport, TestReport};

#[derive(Debug, Error)]
pub enum TestRunnerError {
    #[error("discovery failed: {0}")]
    Discover(#[from] DiscoverError),
    #[error("io error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error in {path}: {message}")]
    Parse { path: String, message: String },
}

/// Knobs for the runner. The default config is what every caller
/// uses today; the struct exists so future options (timeout per
/// test, fail-fast, name filter) can be added without changing
/// the public surface.
#[derive(Debug, Clone, Default)]
pub struct TestRunnerConfig {
    /// If set, only tests whose name contains this substring
    /// (case-sensitive) are executed. Other tests are reported
    /// as "skipped" with zero assertions. Defaults to `None` (run
    /// everything).
    pub name_filter: Option<String>,
}

pub struct TestRunner {
    config: TestRunnerConfig,
}

impl TestRunner {
    pub fn new(config: TestRunnerConfig) -> Self {
        Self { config }
    }

    pub fn with_default_config() -> Self {
        Self::new(TestRunnerConfig::default())
    }

    /// Discover and run every test under `path`. The path may be
    /// a file or a directory (recursive).
    pub fn run_path(&self, path: &Path) -> Result<RunReport, TestRunnerError> {
        let entries = discover(path)?;
        let root_label = path.to_string_lossy().into_owned();
        self.run_entries(&entries, &root_label)
    }

    /// Parse `source` as a single `.flow` program and run its
    /// tests. The same program supplies both the `TestDef`s and
    /// the `WorkflowDef`s; this matches the editor's
    /// "run on buffer" case where the user might have defined
    /// everything in one file.
    pub fn run_source(
        &self,
        source: &str,
        virtual_path: &str,
    ) -> Result<RunReport, TestRunnerError> {
        let program =
            FlowParser::parse_flow_program(source).map_err(|e| TestRunnerError::Parse {
                path: virtual_path.to_string(),
                message: e,
            })?;
        let tests: Vec<TestReport> = program
            .tests
            .iter()
            .filter(|t| self.name_matches(&t.name))
            .map(|t| execute_test(t, &program, virtual_path))
            .collect();
        Ok(RunReport::from_tests(virtual_path, tests))
    }

    fn run_entries(
        &self,
        entries: &[DiscoverEntry],
        root_label: &str,
    ) -> Result<RunReport, TestRunnerError> {
        let mut all_tests: Vec<TestReport> = Vec::new();
        for entry in entries {
            let test_source =
                fs::read_to_string(&entry.test_file).map_err(|e| TestRunnerError::Io {
                    path: entry.test_file.to_string_lossy().into_owned(),
                    source: e,
                })?;
            let test_program = FlowParser::parse_flow_program(&test_source).map_err(|e| {
                TestRunnerError::Parse {
                    path: entry.test_file.to_string_lossy().into_owned(),
                    message: e,
                }
            })?;
            let host_program = match &entry.host_file {
                Some(host) => {
                    let host_source =
                        fs::read_to_string(host).map_err(|e| TestRunnerError::Io {
                            path: host.to_string_lossy().into_owned(),
                            source: e,
                        })?;
                    let parsed = FlowParser::parse_flow_program(&host_source).map_err(|e| {
                        TestRunnerError::Parse {
                            path: host.to_string_lossy().into_owned(),
                            message: e,
                        }
                    })?;
                    Some(parsed)
                }
                None => None,
            };
            // The host program is the one that contains the
            // WorkflowDefs under test. If the test file and the
            // host are separate, the host wins. If there's no
            // host, fall back to the test file itself — the user
            // may have inlined the workflows.
            let host = host_program.as_ref().unwrap_or(&test_program);
            for test in &test_program.tests {
                if !self.name_matches(&test.name) {
                    continue;
                }
                all_tests.push(execute_test(test, host, &entry.test_file.to_string_lossy()));
            }
        }
        Ok(RunReport::from_tests(root_label, all_tests))
    }

    fn name_matches(&self, name: &str) -> bool {
        match &self.config.name_filter {
            None => true,
            Some(needle) => name.contains(needle.as_str()),
        }
    }
}
