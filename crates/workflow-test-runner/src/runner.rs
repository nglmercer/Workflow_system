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
use std::path::{Path, PathBuf};

use thiserror::Error;
use workflow_parser::FlowParser;
use workflow_plugins::WorkflowPluginManager;

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
    /// If set, plugins are loaded from this directory before
    /// running tests. Defaults to `None` (no plugins).
    pub plugin_dir: Option<String>,
}

pub struct TestRunner {
    config: TestRunnerConfig,
    plugin_manager: Option<WorkflowPluginManager>,
}

impl TestRunner {
    pub fn new(config: TestRunnerConfig) -> Self {
        let plugin_manager = config.plugin_dir.as_ref().map(|dir| {
            let mut pm = WorkflowPluginManager::new(dir);
            pm.load_all();
            pm
        });
        Self {
            config,
            plugin_manager,
        }
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
            .map(|t| {
                execute_test(
                    t,
                    &program,
                    Path::new(""),
                    virtual_path,
                    self.plugin_manager.as_ref(),
                )
            })
            .collect();
        Ok(RunReport::from_tests(virtual_path, tests))
    }

    /// Parse an in-memory test buffer and run its tests against
    /// an in-memory host. Used by the editor's "Run on buffer"
    /// path when the user has a sidecar pair open: `test_source`
    /// is the unsaved `*.test.flow` buffer, `host_source` is the
    /// matching `*.flow` (read from disk by the caller). If
    /// `host_source` is `None`, the test buffer itself supplies
    /// the `WorkflowDef`s (single-file mode, same as
    /// [`Self::run_source`]).
    ///
    /// `host_path` is used to resolve the host's relative
    /// `@import` paths; it can be empty when the host has no
    /// imports or when the test buffer is the host.
    pub fn run_source_with_host(
        &self,
        test_source: &str,
        test_path: &str,
        host_source: Option<&str>,
        host_path: Option<&str>,
    ) -> Result<RunReport, TestRunnerError> {
        let test_program =
            FlowParser::parse_flow_program(test_source).map_err(|e| TestRunnerError::Parse {
                path: test_path.to_string(),
                message: e,
            })?;
        let host_program = match (host_source, host_path) {
            (Some(src), Some(path)) => {
                Some(
                    FlowParser::parse_flow_program(src).map_err(|e| TestRunnerError::Parse {
                        path: path.to_string(),
                        message: e,
                    })?,
                )
            }
            _ => None,
        };
        let host = host_program.as_ref().unwrap_or(&test_program);
        let host_dir: PathBuf = host_path
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        let tests: Vec<TestReport> = test_program
            .tests
            .iter()
            .filter(|t| self.name_matches(&t.name))
            .map(|t| {
                execute_test(
                    t,
                    host,
                    &host_dir,
                    test_path,
                    self.plugin_manager.as_ref(),
                )
            })
            .collect();
        Ok(RunReport::from_tests(test_path, tests))
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
                // The host's import paths are resolved relative
                // to the *host file's* directory, not the test
                // file's. With the sidecar convention the two
                // share a directory, so either would work, but
                // using the host's path is the safer default
                // (it works even if a future discovery layout
                // splits them).
                let host_dir = entry
                    .host_file
                    .as_ref()
                    .and_then(|p| p.parent())
                    .unwrap_or_else(|| entry.test_file.parent().unwrap_or(Path::new("")));
                all_tests.push(execute_test(
                    test,
                    host,
                    host_dir,
                    &entry.test_file.to_string_lossy(),
                    self.plugin_manager.as_ref(),
                ));
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
