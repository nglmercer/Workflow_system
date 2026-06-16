use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    /// Cache of loaded external flow files (path -> functions).
    /// Used to avoid re-reading and re-parsing the same file
    /// multiple times when multiple documents import it.
    external_flow_cache: HashMap<PathBuf, HashMap<String, crate::inference::FunctionSig>>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            analyses: HashMap::new(),
            inferences: HashMap::new(),
            versions: HashMap::new(),
            external_flow_cache: HashMap::new(),
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
                // Resolve cross-file imports to get functions from external .flow files
                let imported_functions =
                    self.resolve_flow_imports(&program, document_path.as_deref());
                Inference::analyze_with_path_and_imports(
                    &program,
                    content,
                    document_path.as_deref(),
                    &imported_functions,
                )
            }
            Err(_) => {
                let line_count = content.lines().count();
                Inference::empty(line_count)
            }
        };
        self.analyses.insert(uri.to_string(), analysis);
        self.inferences.insert(uri.to_string(), inference);
    }

    /// Resolve `import ... from "./other.flow"` statements and load
    /// functions from external .flow files. Returns a map of
    /// function names to their signatures from all imported files.
    fn resolve_flow_imports(
        &mut self,
        program: &workflow_parser::ast::FlowProgram,
        document_path: Option<&str>,
    ) -> HashMap<String, crate::inference::FunctionSig> {
        let mut imported_functions = HashMap::new();
        let doc_dir = document_path.and_then(|p| Path::new(p).parent());

        for import in &program.imports {
            if let workflow_parser::ast::ImportSource::Path(path) = &import.source {
                // Skip HTTP(S) URLs - only handle local file paths
                if path.starts_with("http://") || path.starts_with("https://") {
                    continue;
                }

                // Resolve the path relative to the document's directory
                let resolved_path = if let Some(dir) = doc_dir {
                    let base = if path.starts_with('/') {
                        PathBuf::from(path)
                    } else {
                        dir.join(path)
                    };
                    // Normalize the path (resolve .. and .)
                    self.normalize_path(&base)
                } else {
                    PathBuf::from(path)
                };

                // Load the external flow file if not already cached
                if !self.external_flow_cache.contains_key(&resolved_path) {
                    self.load_external_flow_file(&resolved_path);
                }

                // Get functions from the cached file
                if let Some(functions) = self.external_flow_cache.get(&resolved_path) {
                    for (name, sig) in functions {
                        imported_functions.insert(name.clone(), sig.clone());
                    }
                }
            }
        }

        imported_functions
    }

    /// Load an external .flow file and extract its function signatures.
    fn load_external_flow_file(&mut self, path: &Path) {
        let functions = match std::fs::read_to_string(path) {
            Ok(content) => {
                match workflow_parser::FlowParser::parse_flow_program(&content) {
                    Ok(program) => {
                        let document_path = path.to_str();
                        let inference =
                            Inference::analyze_with_path(&program, &content, document_path);
                        // Extract function signatures from the inference
                        inference.functions.clone()
                    }
                    Err(_) => HashMap::new(),
                }
            }
            Err(_) => HashMap::new(),
        };
        self.external_flow_cache
            .insert(path.to_path_buf(), functions);
    }

    /// Normalize a path by resolving `..` and `.` components.
    fn normalize_path(&self, path: &Path) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    components.pop();
                }
                std::path::Component::CurDir => {}
                other => components.push(other),
            }
        }
        components.iter().collect()
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
