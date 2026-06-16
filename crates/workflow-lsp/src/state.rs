use std::collections::HashMap;

use lsp_types::Position;

use crate::analysis::Analysis;
use crate::inference::Inference;

pub struct ServerState {
    pub documents: HashMap<String, String>,
    pub analyses: HashMap<String, Analysis>,
    pub inferences: HashMap<String, Inference>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            analyses: HashMap::new(),
            inferences: HashMap::new(),
        }
    }

    pub fn update_document(&mut self, uri: &str, content: &str) {
        self.documents.insert(uri.to_string(), content.to_string());
        let analysis = Analysis::analyze(content);
        let inference = match workflow_parser::FlowParser::parse_flow_program(content) {
            Ok(program) => Inference::analyze(&program, content),
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

    /// Convenience: look up the word at the given position without going
    /// through the full analysis pipeline. Exposed for the editor or for
    /// future tests that only need word-level access.
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
