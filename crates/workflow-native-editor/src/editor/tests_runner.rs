//! Test runner integration.
//!
//! Spawns a background thread that parses the in-memory buffer
//! plus an optional sidecar host file, runs every `test` block,
//! and streams the [`workflow_test_runner::RunReport`] back over
//! an `mpsc` channel. The main loop polls the channel once per
//! frame and stores the result in
//! [`EditorApp::test_report`].
//!
//! Three methods are exposed:
//!
//! - [`EditorApp::run_tests`] — kick off a new run (no-op if one
//!   is already in flight).
//! - [`EditorApp::cancel_tests`] — flip the shared cancel flag
//!   and surface a status message. The runner doesn't actually
//!   check the flag mid-flight; that's a known limitation, kept
//!   here so the Cancel button can render truthfully.
//! - [`EditorApp::poll_test_result`] — non-blocking drain of the
//!   result channel.
//!
//! [`EditorApp::open_search_result`] (in `search_in_files`) also
//! lives near the test code in spirit, but it manipulates the
//! find bar and cursor more than the test runner, so it stays
//! where it is in `app.rs`.

use std::path::PathBuf;

use super::super::EditorApp;
use workflow_i18n::t as i18n_t;

impl EditorApp {
    /// Kick off a test run on the in-memory buffer. Spawns a
    /// background thread that parses the buffer, runs every
    /// `test` block, and sends the result back via a channel.
    /// The main loop polls the channel and stores the result in
    /// `self.test_report`. If a run is already in flight this is
    /// a no-op (the panel disables the button while running).
    pub(crate) fn run_tests(&mut self) {
        if self.tests_running {
            return;
        }
        let source = self.text.clone();
        let virtual_path = self
            .file_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<buffer>".to_string());

        // If the open file is a sidecar `*.test.flow`, look for
        // its sibling `*.flow` on disk and feed both to the
        // runner. The test buffer is the source of truth for
        // the `TestDef`s, but the `WorkflowDef`s live in the
        // host file — without it, every test would report
        // "no workflow handles event '<X>'".
        let sidecar: Option<PathBuf> = self.file_path.as_ref().and_then(|p| {
            let name = p.file_name()?.to_str()?;
            let stem = name.strip_suffix(".test.flow")?;
            let host = p.with_file_name(format!("{stem}.flow"));
            if host.exists() {
                Some(host)
            } else {
                None
            }
        });
        let (host_source, host_path): (Option<String>, Option<String>) = match &sidecar {
            Some(p) => match std::fs::read_to_string(p) {
                Ok(s) => (Some(s), Some(p.to_string_lossy().into_owned())),
                Err(_) => (None, None),
            },
            None => (None, None),
        };

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runner = workflow_test_runner::TestRunner::with_default_config();
            let report = runner
                .run_source_with_host(
                    &source,
                    &virtual_path,
                    host_source.as_deref(),
                    host_path.as_deref(),
                )
                .unwrap_or_else(|e| {
                    workflow_test_runner::RunReport::from_tests(
                        &virtual_path,
                        vec![workflow_test_runner::TestReport {
                            name: "<runner>".to_string(),
                            source_path: virtual_path.clone(),
                            event: String::new(),
                            asserts: vec![workflow_test_runner::AssertResult::fail(
                                workflow_test_runner::AssertKind::Logs,
                                "",
                                String::new(),
                                format!("runner error: {}", e),
                            )],
                            matched_workflow_count: 0,
                            passed: false,
                        }],
                    )
                });
            let _ = tx.send(report);
        });
        self.test_receiver = Some(rx);
        self.tests_running = true;
        self.status = i18n_t("app.status_running_tests");
    }

    /// Called by the test panel's Cancel button. We don't
    /// actually cancel the in-flight run (the runner completes
    /// its current test and reports), but we flip the cancel
    /// flag for future use and surface a status message.
    pub(crate) fn cancel_tests(&mut self) {
        if let Some(flag) = &self.test_cancel {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.status = i18n_t("app.status_cancel_requested");
    }

    /// Drain the test result channel. Called once per frame.
    pub(crate) fn poll_test_result(&mut self) {
        if let Some(rx) = &self.test_receiver {
            match rx.try_recv() {
                Ok(report) => {
                    self.test_report = Some(report);
                    self.tests_running = false;
                    self.test_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.tests_running = false;
                    self.test_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }
}
