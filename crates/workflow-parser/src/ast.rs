use serde::{Deserialize, Serialize};

/// A byte-offset range in the source text. `start` and `end` are
/// UTF-8 byte indices into the source. `start <= end`. Spans are
/// optional because the parser builds AST nodes via text-only helpers
/// (`parse_expr_text` etc.) that don't have a way to recover their
/// absolute byte position in the source — most production paths
/// leave spans as `None`, but `Spanned<T>` wrappers and a few
/// expression parsers can populate them. Consumers that need
/// reliable positions should fall back to a substring search when
/// `span()` returns `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        debug_assert!(start <= end, "Span start must be <= end");
        Self { start, end }
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Wraps a node with an optional source span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Option<Span>,
}

impl<T> Spanned<T> {
    pub fn new(node: T) -> Self {
        Self { node, span: None }
    }

    pub fn with_span(node: T, span: Span) -> Self {
        Self {
            node,
            span: Some(span),
        }
    }

    pub fn span(&self) -> Option<Span> {
        self.span
    }
}

/// Top-level AST node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowProgram {
    pub imports: Vec<ImportStmt>,
    pub globals: Vec<GlobalVar>,
    pub functions: Vec<FunctionDef>,
    pub workflows: Vec<WorkflowDef>,
}

/// Import statement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStmt {
    pub name: String,
    pub path: String,
}

/// Global variable declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalVar {
    pub name: String,
    pub value: Expr,
}

/// Function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

/// Workflow definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    pub event: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

/// Statement types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    VarDecl {
        name: String,
        value: Option<Expr>,
    },
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },
    Return {
        value: Option<Expr>,
    },
    Expr(Expr),
    Log(Expr),
    Foreach {
        item_var: String,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    On {
        event: String,
        params: Vec<String>,
    },
}

/// Expression types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    // Literals
    String(String),
    Number(f64),
    Bool(bool),
    Null,

    // Variables and references
    Var(String),
    Member {
        object: Box<Expr>,
        property: String,
    },

    // Operations
    BinaryOp {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },

    // Function call
    Call {
        name: String,
        args: Vec<Expr>,
    },

    // Array
    Array(Vec<Expr>),

    // Interpolated string
    InterpolatedString(Vec<InterpPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterpPart {
    Text(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
}

/// Convert a UTF-8 byte offset in `source` to a 0-based `(line, col)`.
/// `col` is in bytes (not characters) — it matches what `lsp_types::Position`
/// and the rest of the LSP/editor stack use.
pub fn byte_to_line_col(source: &str, byte: usize) -> (u32, u32) {
    let mut line: u32 = 0;
    let mut col: u32 = 0;
    for (i, ch) in source.char_indices() {
        if i >= byte {
            return (line, col);
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

impl Span {
    /// Convert this span to a `(start_line, start_col, end_line, end_col)`
    /// tuple suitable for the LSP `Diagnostic` shape. 0-indexed, with
    /// `col` in the same units as `byte_to_line_col` (bytes). Returns
    /// `None` if the span's byte range lies outside `source`.
    pub fn to_line_col(self, source: &str) -> Option<(u32, u32, u32, u32)> {
        if self.start > source.len() || self.end > source.len() {
            return None;
        }
        let (sl, sc) = byte_to_line_col(source, self.start);
        let (el, ec) = byte_to_line_col(source, self.end);
        Some((sl, sc, el, ec))
    }
}

impl Expr {
    pub fn string(s: impl Into<String>) -> Self {
        Self::String(s.into())
    }

    pub fn number(n: f64) -> Self {
        Self::Number(n)
    }

    pub fn var(name: impl Into<String>) -> Self {
        Self::Var(name.into())
    }

    pub fn call(name: impl Into<String>, args: Vec<Expr>) -> Self {
        Self::Call {
            name: name.into(),
            args,
        }
    }

    pub fn member(object: Expr, property: impl Into<String>) -> Self {
        Self::Member {
            object: Box::new(object),
            property: property.into(),
        }
    }

    pub fn binary(op: BinaryOp, left: Expr, right: Expr) -> Self {
        Self::BinaryOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }
}
