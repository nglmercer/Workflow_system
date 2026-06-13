use std::collections::HashMap;

use lsp_types::Position;

pub struct ServerState {
    pub documents: HashMap<String, String>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    pub fn update_document(&mut self, uri: &str, content: &str) {
        self.documents.insert(uri.to_string(), content.to_string());
    }

    pub fn get_document(&self, uri: &str) -> Option<&String> {
        self.documents.get(uri)
    }

    pub fn get_word_at_position(&self, uri: &str, position: Position) -> Option<String> {
        let content = self.get_document(uri)?;
        let lines: Vec<&str> = content.lines().collect();
        let line = lines.get(position.line as usize)?;
        let chars: Vec<char> = line.chars().collect();

        let start = position.character as usize;
        let end = start;

        let mut word_start = start;
        let mut word_end = end;

        while word_start > 0
            && (chars[word_start - 1].is_alphanumeric() || chars.get(word_start - 1) == Some(&'_'))
        {
            word_start -= 1;
        }

        while word_end < chars.len()
            && (chars[word_end].is_alphanumeric() || chars[word_end] == '_')
        {
            word_end += 1;
        }

        if word_start < word_end {
            Some(chars[word_start..word_end].iter().collect())
        } else {
            None
        }
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}
