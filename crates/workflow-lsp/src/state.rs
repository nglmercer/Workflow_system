use std::collections::HashMap;

use lsp_types::Position;

use crate::analysis::Analysis;
use crate::inference::Inference;

pub struct ServerState {
    pub documents: HashMap<String, String>,
    pub analyses: HashMap<String, Analysis>,
    pub inferences: HashMap<String, Inference>,
    /// Monotonically increasing version per document. Incremented
    /// on every [`update_document`] call so consumers can detect
    /// stale caches.
    pub versions: HashMap<String, i32>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            analyses: HashMap::new(),
            inferences: HashMap::new(),
            versions: HashMap::new(),
        }
    }

    pub fn update_document(&mut self, uri: &str, content: &str) {
        self.documents.insert(uri.to_string(), content.to_string());
        let version = self.versions.get(uri).map_or(1, |v| v + 1);
        self.versions.insert(uri.to_string(), version);
        let analysis = Analysis::analyze(content);
        let document_path = uri.strip_prefix("file://").map(str::to_string);
        let inference = match workflow_parser::FlowParser::parse_flow_program(content) {
            Ok(program) => {
                Inference::analyze_with_path(&program, content, document_path.as_deref())
            }
            Err(_) => {
                let line_count = content.lines().count();
                Inference::empty(line_count)
            }
        };
        self.analyses.insert(uri.to_string(), analysis);
        self.inferences.insert(uri.to_string(), inference);
    }

    pub fn get_document(&self, uri: &str) -> Option<&String> {
        self.documents.get(uri)
    }

    pub fn get_analysis(&self, uri: &str) -> Option<&Analysis> {
        self.analyses.get(uri)
    }

    pub fn get_inference(&self, uri: &str) -> Option<&Inference> {
        self.inferences.get(uri)
    }

    /// Returns the current version of the document at `uri`, or
    /// `None` if the document has never been opened.
    pub fn get_version(&self, uri: &str) -> Option<i32> {
        self.versions.get(uri).copied()
    }

    /// Check whether the cached analysis for `uri` is still at the
    /// given `version`. Returns `false` if the document has been
    /// modified since the version was captured.
    pub fn is_current(&self, uri: &str, version: i32) -> bool {
        self.versions.get(uri).is_some_and(|&v| v == version)
    }

    #[allow(dead_code)]
    pub fn get_word_at_position(&self, uri: &str, position: Position) -> Option<String> {
        let content = self.get_document(uri)?;
        crate::analysis::word_at(content, position)
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}
