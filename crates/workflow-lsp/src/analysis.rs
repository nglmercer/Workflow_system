use lsp_types::{Position, Range};
use workflow_parser::ast::{FlowProgram, FunctionDef, GlobalVar, Stmt};
use workflow_parser::FlowParser;

/// A symbol in scope at a given position in the document.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ScopedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub detail: Option<String>,
    /// A short hover description.
    pub documentation: Option<String>,
    /// The full UTF-16/8 byte range that this symbol's name occupies in the
    /// source. Used for `textEdit` ranges in completions.
    pub name_range: Option<Range>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SymbolKind {
    Variable,
    Function,
    Parameter,
    Keyword,
    Value,
    Property,
}

/// A lightweight analysis of a document, including everything we need for
/// scope-aware hover and completion.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub program: Option<FlowProgram>,
    pub parse_error: Option<String>,
    /// Local variables and foreach item variables visible at each line.
    /// Populated by a simple linear scan, so it's an approximation, but good
    /// enough for hover/completion.
    pub scope_at: Vec<Vec<ScopedSymbol>>,
}

impl Analysis {
    pub fn analyze(source: &str) -> Self {
        let program = match FlowParser::parse_flow_program(source) {
            Ok(p) => Some(p),
            Err(err) => {
                // Even on parse failure we still try to give partial help.
                let mut analysis = Analysis::default();
                analysis.parse_error = Some(err);
                analysis.build_fallback(source);
                return analysis;
            }
        };
        let mut analysis = Analysis {
            program,
            parse_error: None,
            scope_at: Vec::new(),
        };
        analysis.build_scope(source);
        analysis
    }

    fn build_scope(&mut self, source: &str) {
        let line_count = source.lines().count().max(1);
        self.scope_at = vec![Vec::new(); line_count];

        let Some(program) = self.program.clone() else {
            return;
        };

        // Globals are visible everywhere.
        for g in &program.globals {
            self.push_global(g);
        }

        // Top-level functions.
        for f in &program.functions {
            self.push_function(f);
        }

        // Workflow bodies: scan stmts in order and collect locals.
        for w in &program.workflows {
            self.scan_stmts(&w.body, 0);
        }

        // Also scan function bodies for locals.
        for f in &program.functions {
            self.scan_stmts(&f.body, 0);
        }
    }

    fn build_fallback(&mut self, source: &str) {
        // On a parse error we still know nothing about the program, so leave
        // the scope table empty. Keyword/builtin completions are still useful.
        let line_count = source.lines().count().max(1);
        self.scope_at = vec![Vec::new(); line_count];
    }

    fn push_global(&mut self, g: &GlobalVar) {
        let symbol = ScopedSymbol {
            name: g.name.clone(),
            kind: SymbolKind::Variable,
            detail: Some("global variable".to_string()),
            documentation: None,
            name_range: None,
        };
        for line in self.scope_at.iter_mut() {
            line.push(symbol.clone());
        }
    }

    fn push_function(&mut self, f: &FunctionDef) {
        let sig = format!("fn {}({})", f.name, f.params.join(", "));
        let symbol = ScopedSymbol {
            name: f.name.clone(),
            kind: SymbolKind::Function,
            detail: Some(sig.clone()),
            documentation: Some(format!("Function `{}`", sig)),
            name_range: None,
        };
        for line in self.scope_at.iter_mut() {
            line.push(symbol.clone());
        }
    }

    /// Walk statements and append scoped symbols for the lines they cover.
    /// `depth` is the nesting level (used for debugging, not strictly needed).
    fn scan_stmts(&mut self, stmts: &[Stmt], _depth: usize) {
        for stmt in stmts {
            self.scan_stmt(stmt);
        }
    }

    fn scan_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl { name, .. } => {
                let sym = ScopedSymbol {
                    name: name.clone(),
                    kind: SymbolKind::Variable,
                    detail: Some("local variable".to_string()),
                    documentation: None,
                    name_range: None,
                };
                for line in self.scope_at.iter_mut() {
                    line.push(sym.clone());
                }
            }
            Stmt::Foreach { item_var, body, .. } => {
                let sym = ScopedSymbol {
                    name: item_var.clone(),
                    kind: SymbolKind::Variable,
                    detail: Some("foreach item".to_string()),
                    documentation: None,
                    name_range: None,
                };
                for line in self.scope_at.iter_mut() {
                    line.push(sym.clone());
                }
                self.scan_stmts(body, 0);
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                self.scan_stmts(then_body, 0);
                if let Some(else_body) = else_body {
                    self.scan_stmts(else_body, 0);
                }
            }
            _ => {}
        }
    }

    /// Look up the word at the given position. If found, returns the symbol
    /// and a "context" string describing what kind of usage it was.
    pub fn lookup(&self, source: &str, position: Position) -> Option<ScopedSymbol> {
        let word = word_at(source, position)?;
        let line_idx = position.line as usize;
        if let Some(scope) = self.scope_at.get(line_idx) {
            if let Some(sym) = scope.iter().find(|s| s.name == word) {
                let mut found = sym.clone();
                if found.detail.is_none() {
                    found.detail = Some(format!("{:?}", found.kind).to_lowercase());
                }
                return Some(found);
            }
        }
        // Fall back to built-in keyword documentation.
        builtin_for(&word).map(|info| ScopedSymbol {
            name: word,
            kind: info.kind,
            detail: Some(info.detail.to_string()),
            documentation: Some(info.docs.to_string()),
            name_range: None,
        })
    }

    /// Get all symbols in scope at the given line.
    pub fn scope_at_position(&self, position: Position) -> &[ScopedSymbol] {
        self.scope_at
            .get(position.line as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// The text just before the cursor, restricted to the current line.
    /// Exposed for future trigger-character handling.
    #[allow(dead_code)]
    pub fn prefix_at(&self, source: &str, position: Position) -> String {
        let line = source.lines().nth(position.line as usize).unwrap_or("");
        let col = (position.character as usize).min(line.len());
        line[..col].to_string()
    }
}

struct BuiltinInfo {
    kind: SymbolKind,
    detail: &'static str,
    docs: &'static str,
}

fn builtin_for(word: &str) -> Option<BuiltinInfo> {
    let info = match word {
        "var" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Variable declaration",
            docs: "Declares a new local variable.\n\n```flow\nvar name = value\n```",
        },
        "fn" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Function definition",
            docs: "Defines a reusable function.\n\n```flow\nfn name(param1, param2) {\n  // body\n}\n```",
        },
        "workflow" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Workflow definition",
            docs: "Defines a workflow triggered by an event.\n\n```flow\nworkflow \"Name\" {\n  on EVENT\n  // statements\n}\n```",
        },
        "on" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Event trigger",
            docs: "Declares which event triggers this workflow.\n\n```flow\non EVENT_NAME\n```",
        },
        "if" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Conditional",
            docs: "Runs a block if the condition is true.\n\n```flow\nif (cond) { ... } else { ... }\n```",
        },
        "else" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Else branch",
            docs: "Branch of an `if` statement, taken when the condition is false.",
        },
        "foreach" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Loop over an iterable",
            docs: "Iterates over an array or string.\n\n```flow\nforeach (item in items) { ... }\n```",
        },
        "in" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Foreach separator",
            docs: "Separates the item variable from the iterable in a `foreach` loop.",
        },
        "return" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Return statement",
            docs: "Returns a value from the current function.",
        },
        "log" => BuiltinInfo {
            kind: SymbolKind::Function,
            detail: "log(message)",
            docs: "Prints a message to the console.\n\n```flow\nlog(\"Hello\")\n```",
        },
        "len" => BuiltinInfo {
            kind: SymbolKind::Function,
            detail: "len(value)",
            docs: "Returns the length of a string or array.",
        },
        "to_string" => BuiltinInfo {
            kind: SymbolKind::Function,
            detail: "to_string(value)",
            docs: "Converts a value to its string representation.",
        },
        "to_number" => BuiltinInfo {
            kind: SymbolKind::Function,
            detail: "to_number(value)",
            docs: "Converts a value to a number.",
        },
        "true" | "false" => BuiltinInfo {
            kind: SymbolKind::Value,
            detail: "Boolean literal",
            docs: "Boolean truth value.",
        },
        "null" => BuiltinInfo {
            kind: SymbolKind::Value,
            detail: "Null literal",
            docs: "Represents the absence of a value.",
        },
        "import" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Import statement",
            docs: "Imports another module by name.\n\n```flow\nimport name from \"path\"\n```",
        },
        "from" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Import source",
            docs: "Used in `import` to specify the source path.",
        },
        "emit" => BuiltinInfo {
            kind: SymbolKind::Keyword,
            detail: "Emit event",
            docs: "Emits a new event from inside a workflow.",
        },
        _ => return None,
    };
    Some(info)
}

/// Returns the identifier covering the character at `position`, or `None` if
/// the position is not on an identifier. Search walks the line in both
/// directions and only includes ASCII letters, digits, and underscore.
pub fn word_at(source: &str, position: Position) -> Option<String> {
    let line = source.lines().nth(position.line as usize)?;
    let bytes = line.as_bytes();
    let col = position.character as usize;

    if col > bytes.len() {
        return None;
    }

    let is_word_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    if col < bytes.len() && !is_word_byte(bytes[col]) {
        // Maybe the position is right after the word; back up one.
        if col == 0 || !is_word_byte(bytes[col - 1]) {
            return None;
        }
    }

    let mut start = col;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }
    Some(line[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_at_middle() {
        let src = "var foo = 42";
        assert_eq!(word_at(src, Position::new(0, 6)), Some("foo".to_string()));
    }

    #[test]
    fn test_word_at_underscore() {
        let src = "var my_var = 42";
        assert_eq!(
            word_at(src, Position::new(0, 8)),
            Some("my_var".to_string())
        );
    }

    #[test]
    fn test_word_at_no_word() {
        let src = "var = 42";
        assert_eq!(word_at(src, Position::new(0, 4)), None);
    }

    #[test]
    fn test_analysis_extracts_globals() {
        let src = "var x = 1\nworkflow \"W\" { on E\n log(x) }";
        let analysis = Analysis::analyze(src);
        let scope = analysis.scope_at_position(Position::new(2, 5));
        assert!(scope.iter().any(|s| s.name == "x"));
    }

    #[test]
    fn test_analysis_extracts_foreach_item() {
        let src = "workflow \"W\" { on E\n foreach (item in xs) { log(item) } }";
        let analysis = Analysis::analyze(src);
        let scope = analysis.scope_at_position(Position::new(1, 30));
        assert!(scope.iter().any(|s| s.name == "item"));
    }
}
