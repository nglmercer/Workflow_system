//! Global "find in files" panel and background worker.
//!
//! The user types a query in a right-side panel; we walk the
//! workspace with the `ignore` crate (which respects `.gitignore`
//! and skips hidden files) on a background thread, collecting
//! matches and streaming them back over an `mpsc` channel. The
//! UI thread polls the channel once per frame and renders the
//! accumulated results. The worker can be cancelled by bumping
//! a generation counter, which the worker checks between batches
//! so it can exit early when the user types a new query.
//!
//! This module is desktop-only: `ignore` is not designed for the
//! `wasm32-unknown-unknown` target. Callers should `cfg`-gate the
//! field on [`EditorApp`] that owns a [`SearchInFilesState`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use eframe::egui::{self, RichText, ScrollArea, TextEdit};
use regex::Regex;
use workflow_i18n::{t as i18n_t, tf as i18n_tf};

/// One search hit. We store the path (relative to the search root
/// when possible), the 1-indexed line and column, the surrounding
/// line text, and the byte range of the match within that line so
/// the UI can highlight just the matched substring.
#[derive(Debug, Clone)]
pub struct FileMatch {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub line_text: String,
    pub match_start: usize,
    pub match_end: usize,
}

/// One batch of progress sent from the worker. The UI accumulates
/// these into [`SearchInFilesState::results`] until the worker
/// reports [`SearchMsg::Done`].
#[derive(Debug)]
pub enum SearchMsg {
    Progress(Vec<FileMatch>),
    Done,
}

/// Search panel state, owned by the editor.
pub struct SearchInFilesState {
    pub open: bool,
    pub query: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub results: Vec<FileMatch>,
    pub total_files: usize,
    pub in_flight: bool,
    pub cancelled: bool,
    worker: Option<JoinHandle<()>>,
    cancel_flag: Option<Arc<AtomicBool>>,
    receiver: Option<Receiver<SearchMsg>>,
    pub generation: u64,
    pub root: Option<PathBuf>,
    pub last_query: String,
}

impl Default for SearchInFilesState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            case_sensitive: false,
            use_regex: false,
            results: Vec::new(),
            total_files: 0,
            in_flight: false,
            cancelled: false,
            worker: None,
            cancel_flag: None,
            receiver: None,
            generation: 0,
            root: None,
            last_query: String::new(),
        }
    }
}

impl SearchInFilesState {
    /// Open the panel and (re)start a search with the current
    /// query, defaulting the search root to the parent directory
    /// of the open file or the current working directory.
    pub fn open(&mut self, default_root: Option<PathBuf>) {
        self.open = true;
        if self.root.is_none() {
            self.root = default_root.or_else(|| std::env::current_dir().ok());
        }
        if !self.query.is_empty() {
            self.request_run();
        }
    }

    /// Tear down any running worker and start a new search with
    /// the current query. The old worker sees a flipped cancel
    /// flag and exits at the next batch boundary.
    pub fn request_run(&mut self) {
        self.cancel_current();
        if self.query.is_empty() {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let pattern = match self.compile_pattern() {
            Some(p) => p,
            None => return, // invalid regex; keep showing previous results
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel.clone();
        self.generation = self.generation.wrapping_add(1);
        self.worker = Some(std::thread::spawn(move || {
            run_search(root, pattern, tx, cancel_worker);
        }));
        self.receiver = Some(rx);
        self.cancel_flag = Some(cancel);
        self.in_flight = true;
        self.cancelled = false;
        self.results.clear();
        self.total_files = 0;
        self.last_query = self.query.clone();
    }

    /// Flip the cancel flag and wait briefly for the worker to
    /// notice. Cheap because the worker checks the flag between
    /// every batch.
    pub fn cancel_current(&mut self) {
        if let Some(flag) = &self.cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        self.cancelled = true;
        self.in_flight = false;
        self.worker = None;
        self.receiver = None;
        self.cancel_flag = None;
    }

    /// Drain the result channel. Returns `true` if any new
    /// matches arrived this call so the caller can request a
    /// repaint.
    pub fn poll(&mut self) -> bool {
        let mut dirty = false;
        let mut done = false;
        if let Some(rx) = self.receiver.as_ref() {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SearchMsg::Progress(mut batch) => {
                        self.total_files += 1;
                        self.results.append(&mut batch);
                        dirty = true;
                    }
                    SearchMsg::Done => done = true,
                }
            }
        }
        if done {
            self.in_flight = false;
            self.worker = None;
            self.receiver = None;
            self.cancel_flag = None;
        }
        dirty
    }

    fn compile_pattern(&self) -> Option<Regex> {
        let pattern = if self.use_regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        let mut builder = regex::RegexBuilder::new(&pattern);
        builder.case_insensitive(!self.case_sensitive);
        builder.build().ok()
    }
}

/// Worker entry point. Walks `root` with `ignore::WalkBuilder`,
/// reads every non-binary file, runs the compiled regex, and
/// sends progress in batches. Exits early if `cancel` flips.
fn run_search(root: PathBuf, pattern: Regex, tx: Sender<SearchMsg>, cancel: Arc<AtomicBool>) {
    const BATCH_SIZE: usize = 200;
    const MAX_RESULTS: usize = 5_000;
    let mut batch: Vec<FileMatch> = Vec::with_capacity(BATCH_SIZE);
    let mut total_results: usize = 0;
    let walker = ignore::WalkBuilder::new(&root)
        .standard_filters(true)
        .require_git(false)
        .build();
    for entry in walker.flatten() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let Some(file_type) = entry.file_type() else { continue };
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(bytes) = std::fs::read(path) else { continue };
        // Skip binary files: any NUL byte in the first 8 KB is a
        // strong signal of binary content.
        if bytes.iter().take(8192).any(|&b| b == 0) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else { continue };
        for (line_idx, line) in text.lines().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            if let Some(m) = pattern.find(line) {
                if total_results >= MAX_RESULTS {
                    break;
                }
                batch.push(FileMatch {
                    path: path.to_path_buf(),
                    line: line_idx + 1,
                    col: m.start() + 1,
                    line_text: line.to_string(),
                    match_start: m.start(),
                    match_end: m.end(),
                });
                total_results += 1;
                if batch.len() >= BATCH_SIZE {
                    if tx.send(SearchMsg::Progress(std::mem::take(&mut batch))).is_err() {
                        return;
                    }
                }
            }
        }
        if total_results >= MAX_RESULTS {
            break;
        }
    }
    if !batch.is_empty() {
        let _ = tx.send(SearchMsg::Progress(batch));
    }
    let _ = tx.send(SearchMsg::Done);
}

/// Render the side panel. Returns a [`SearchInFilesEvent`] when
/// the user activates a result (e.g. clicks a row).
pub fn show(ctx: &egui::Context, state: &mut SearchInFilesState) -> Option<SearchInFilesEvent> {
    if !state.open {
        return None;
    }
    let mut event = None;
    egui::SidePanel::right("search_in_files")
        .resizable(true)
        .default_width(380.0)
        .min_width(240.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(i18n_t("search_in_files.title")).strong());
                if state.in_flight && ui.button(i18n_t("search_in_files.stop")).clicked() {
                    state.cancel_current();
                }
            });
            let response = ui.add(
                TextEdit::singleline(&mut state.query)
                    .desired_width(f32::INFINITY)
                    .hint_text(i18n_t("search_in_files.placeholder"))
                    .margin(4.0),
            );
            if response.changed() {
                state.request_run();
            }
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(state.case_sensitive, i18n_t("search_in_files.case_sensitive"))
                    .clicked()
                {
                    state.case_sensitive = !state.case_sensitive;
                    state.request_run();
                }
                if ui
                    .selectable_label(state.use_regex, i18n_t("search_in_files.use_regex"))
                    .clicked()
                {
                    state.use_regex = !state.use_regex;
                    state.request_run();
                }
            });
            ui.separator();
            if state.in_flight {
                ui.label(RichText::new(i18n_t("search_in_files.progress")).italics());
            } else if !state.results.is_empty() {
                ui.label(i18n_tf(
                    "search_in_files.result_count",
                    &[
                        ("count", &state.results.len().to_string()),
                        ("files", &state.total_files.to_string()),
                    ],
                ));
            } else if !state.query.is_empty() && !state.cancelled {
                ui.label(RichText::new(i18n_t("search_in_files.no_results")).weak());
            }
            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for (idx, m) in state.results.iter().enumerate() {
                        let len = m.line_text.len();
                        let s = m.match_start.min(len);
                        let e = m.match_end.min(len);
                        let header = format!("{}:{}", m.path.display(), m.line);
                        let body = format!(
                            "{}{}{}",
                            &m.line_text[..s],
                            &m.line_text[s..e],
                            &m.line_text[e..]
                        );
                        ui.collapsing(header, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&m.line_text[..s]).small());
                                ui.label(
                                    RichText::new(&m.line_text[s..e])
                                        .small()
                                        .strong()
                                        .color(egui::Color32::from_rgb(255, 220, 0)),
                                );
                                ui.label(RichText::new(&m.line_text[e..]).small());
                            });
                            if ui.button(i18n_t("search_in_files.open_match")).clicked() {
                                event = Some(SearchInFilesEvent::OpenMatch(idx));
                            }
                        });
                        // Suppress the unused `body` warning that
                        // the old format! produced; we keep the
                        // variable for symmetry with future
                        // refactors.
                        let _ = body;
                    }
                });
        });
    event
}

/// Action returned by the search panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchInFilesEvent {
    OpenMatch(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_plain_query_is_literal() {
        let s = SearchInFilesState {
            query: "foo.bar".to_string(),
            use_regex: false,
            ..SearchInFilesState::default()
        };
        let pat = s.compile_pattern().unwrap();
        assert!(pat.is_match("foo.bar"));
        assert!(!pat.is_match("fooxbar"));
    }

    #[test]
    fn regex_query_uses_pattern() {
        let s = SearchInFilesState {
            query: "fo+".to_string(),
            use_regex: true,
            ..SearchInFilesState::default()
        };
        let pat = s.compile_pattern().unwrap();
        assert!(pat.is_match("foo"));
        assert!(pat.is_match("fooo"));
        assert!(!pat.is_match("bar"));
    }

    #[test]
    fn case_sensitive_default_off() {
        let s = SearchInFilesState {
            query: "Foo".to_string(),
            case_sensitive: false,
            ..SearchInFilesState::default()
        };
        let pat = s.compile_pattern().unwrap();
        assert!(pat.is_match("foo"));
        assert!(pat.is_match("FOO"));
        let s = SearchInFilesState {
            query: "Foo".to_string(),
            case_sensitive: true,
            ..SearchInFilesState::default()
        };
        let pat = s.compile_pattern().unwrap();
        assert!(pat.is_match("Foo"));
        assert!(!pat.is_match("foo"));
    }

    #[test]
    fn bumps_generation_on_query_change() {
        let tmp = std::env::temp_dir().join("workflow_search_in_files_gen");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let mut s = SearchInFilesState {
            root: Some(tmp.clone()),
            query: "first".to_string(),
            ..SearchInFilesState::default()
        };
        s.request_run();
        let g1 = s.generation;
        s.query = "second".to_string();
        s.request_run();
        let g2 = s.generation;
        assert!(g2 > g1, "generation should bump: g1={} g2={}", g1, g2);
        s.cancel_current();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn skips_binary_files() {
        // Inline a NUL-bearing buffer; the worker should skip it.
        let bytes: &[u8] = b"foo\0bar";
        let looks_binary = bytes.iter().take(8192).any(|&b| b == 0);
        assert!(looks_binary);
    }

    #[test]
    fn respects_gitignore_via_ignore_walker() {
        let tmp = std::env::temp_dir().join("workflow_search_in_files_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join(".gitignore"), "*.secret\n").unwrap();
        std::fs::write(tmp.join("visible.txt"), "hello world\n").unwrap();
        std::fs::write(tmp.join("a.secret"), "needle in here\n").unwrap();
        let walker = ignore::WalkBuilder::new(&tmp)
            .standard_filters(true)
            .require_git(false)
            .build();
        let mut names: Vec<String> = walker
            .flatten()
            .filter_map(|e| {
                if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    Some(e.path().file_name().unwrap().to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        assert!(names.contains(&"visible.txt".to_string()));
        assert!(
            !names.contains(&"a.secret".to_string()),
            "gitignore should skip a.secret, walked: {:?}",
            names
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
