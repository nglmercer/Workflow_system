use serde::{Deserialize, Serialize};

/// A byte-offset range in the source text. `start` and `end` are
/// UTF-8 byte indices into the source. `start <= end`. Every
/// statement, declaration, function/workflow and block carries a
/// `span` field; the parser populates it from the pest
/// `Pair::as_span()` byte ranges. The struct is `Option`-less for
/// statements because we have full position information from the
/// pest grammar; the few `Expr` nodes that come out of the
/// text-based `parse_expr_text` helper don't have a span, which is
/// fine because the scope walker only needs statement/block spans.
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

    /// True if the byte offset falls inside this span.
    pub fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset <= self.end
    }
}

/// Wraps a node with an optional source span. Kept for places that
/// want to attach a span to a node that doesn't have one as a
/// required field (e.g. expression sub-trees coming out of the
/// text-based parser).
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

impl Stmt {
    /// The byte range this statement occupies in the source. Used
    /// by the scope walker to decide where a declaration's
    /// binding becomes visible.
    pub fn span(&self) -> Span {
        match self {
            Stmt::VarDecl { span, .. } => *span,
            Stmt::Assign { span, .. } => *span,
            Stmt::If { span, .. } => *span,
            Stmt::Return { span, .. } => *span,
            Stmt::Expr(_, span) | Stmt::Log(_, span) => *span,
            Stmt::Foreach { span, .. } => *span,
            Stmt::On { span, .. } => *span,
        }
    }
}

/// Test definition. Lives in sidecar `*.test.flow` files and is
/// consumed by `workflow-test-runner`. The runner synthesises a
/// `TriggerContext` from `on_clause` and runs every matching
/// `WorkflowDef` in the host program, then checks each
/// `expect_clause` against the captured logs / events / return
/// value / final scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDef {
    pub name: String,
    pub on: OnClause,
    pub expects: Vec<ExpectClause>,
}

/// The `on <EVENT> with { ... }` clause of a test. The `data`
/// payload is a `serde_json::Value`; missing fields become `Null`
/// at runtime, matching the evaluator's permissive member-access
/// behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnClause {
    pub event: String,
    pub data: serde_json::Value,
}

/// One `expect ...` line. The runner checks each clause in order
/// and aggregates pass/fail into the test report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExpectClause {
    /// `expect logs [...]` — element-wise equality with the
    /// captured log strings.
    Logs(Vec<String>),
    /// `expect emitted [...]` — element-wise equality with the
    /// list of events emitted via `emit("EVENT")` calls.
    Emitted(Vec<String>),
    /// `expect return <value>` — the workflow's last `return`
    /// expression result, or `Null` if the workflow fell off the
    /// end with no return.
    Return(serde_json::Value),
    /// `expect var <name> == <value>` — the workflow's final
    /// scope binding for `name`, or `Null` if unbound (the
    /// assertion will fail in that case).
    Var {
        name: String,
        value: serde_json::Value,
    },
}

/// Top-level AST node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowProgram {
    pub imports: Vec<ImportStmt>,
    pub globals: Vec<GlobalVar>,
    pub functions: Vec<FunctionDef>,
    pub workflows: Vec<WorkflowDef>,
    /// Sidecar `test "..." { ... }` blocks. Empty for non-test files.
    #[serde(default)]
    pub tests: Vec<TestDef>,
    /// Span of the entire program in the source (used as the
    /// "global" parent scope's extent for byte-offset lookups).
    #[serde(default)]
    pub span: Span,
}

/// Where an imported binding comes from. The parser produces one of
/// these for every import statement; downstream consumers (the LSP
/// typechecker, the engine loader) decide how to resolve it.
///
/// - `Path` covers both filesystem paths and `http(s)://` URLs — the
///   resolver distinguishes them by prefix.
/// - `Inline` holds a JSON value embedded in the program. The most
///   useful shape is an object whose keys become the fields of the
///   binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum ImportSource {
    /// A path or URL given as a string literal in the import. The
    /// resolver is responsible for distinguishing local files from
    /// `http(s)://` URLs.
    Path(String),
    /// An inline JSON value (typically a JSON object literal). The
    /// parser produces this when the `from` clause is an object
    /// instead of a string.
    Inline(serde_json::Value),
}

/// Import statement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStmt {
    pub name: String,
    pub source: ImportSource,
    #[serde(default)]
    pub span: Span,
}

/// Global variable declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalVar {
    pub name: String,
    pub value: Expr,
    #[serde(default)]
    pub span: Span,
}

/// Function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    /// Span of the whole `fn name(...) { ... }` block. Used as
    /// the scope extent for the function's parameters.
    #[serde(default)]
    pub span: Span,
}

/// Workflow definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    pub event: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    /// Span of the whole `workflow "..." { ... }` block. Used as
    /// the scope extent for the workflow's destructure params.
    #[serde(default)]
    pub span: Span,
}

/// Statement types. Every variant carries a `span: Span` so the
/// scope walker can compute byte-offset ranges without doing
/// string searches. The `span` covers the entire syntactic
/// construct, including any nested `if`/`foreach` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    VarDecl {
        name: String,
        value: Option<Expr>,
        span: Span,
    },
    /// Reassignment to an existing variable, e.g.
    /// `total = total + 1`. Distinct from `VarDecl` (which
    /// requires the `var` keyword) and from `Expr` (which
    /// only wraps expressions — there's no expression node
    /// for an assignment in the current AST).
    Assign {
        name: String,
        value: Expr,
        span: Span,
    },
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Expr(Expr, Span),
    Log(Expr, Span),
    Foreach {
        item_var: String,
        iterable: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    On {
        event: String,
        params: Vec<String>,
        span: Span,
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
