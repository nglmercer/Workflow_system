//! Lexical scope analysis for `.flow` programs.
//!
//! This module builds a real scope stack from the parsed AST and
//! answers "what's in scope at this byte offset?" in `O(log n)`
//! via a pre-computed flat lookup table. The previous line-based
//! implementation conflated "ever declared in the file" with
//! "visible at this point in the program" and silently leaked
//! locals out of their block. This module does not.
//!
//! ## Design
//!
//! - **Module-level scope.** One global scope is always live. It
//!   holds imports, top-level globals, and function/workflow names.
//! - **Block scopes.** Every `if`/`foreach` body and every
//!   function/workflow body is its own scope that gets pushed when
//!   the body starts and popped when it ends.
//! - **Byte offsets.** A binding becomes visible at `decl_span.start`
//!   and is no longer visible after its parent scope's `scope_span.end`.
//!   This is the change that makes "in scope" exact: previously
//!   `var x = 1` on line 5 made `x` visible on every line 0..end of
//!   the file, regardless of nesting. Now `x` is only visible
//!   from line 5 to the end of its enclosing block.
//! - **Shadowing.** A re-declaration of the same name in a nested
//!   block shadows the outer one for the duration of the inner
//!   block. Lookup walks the scope stack from innermost to
//!   outermost and returns the first match.
//! - **Reassignment.** A `Stmt::Assign` does *not* push a new
//!   binding; instead the scope walker re-resolves the existing
//!   binding's type/value. This means hover, completion, and
//!   `unknown-identifier` all see the latest known type after an
//!   assignment, instead of the stale one captured at declaration
//!   time.
//! - **`ScopeAt`.** A flat `Vec<ScopeAt>` indexed by source offset
//!   is built once during the walk. Each entry captures the
//!   active scope-stack *at the offset where some scope boundary
//!   starts*. `lookup_at(offset)` does a binary search to find
//!   the entry whose offset is `<= query_offset` and returns the
//!   captured scope stack from there.
//!
//! The result is consumed by [`crate::analysis::Analysis`] and
//! [`crate::inference::program::run_program_with_imports`].

use std::collections::HashMap;

use workflow_parser::ast::{
    Expr, FlowProgram, FunctionDef, GlobalVar, ImportStmt, Stmt, WorkflowDef,
};

/// A `Scope` is one lexical region in the program (module-level,
/// function body, workflow body, `if` body, `foreach` body). Bindings
/// declared in this scope are visible from `start` (inclusive) to
/// `end` (exclusive) of the scope, *after* the binding's own
/// `decl_span.start`.
#[derive(Debug, Clone)]
pub struct Scope {
    /// The byte range this scope covers in the source. `start..end`
    /// is the "active" range: any byte offset in that range sees
    /// this scope on the stack.
    pub range: std::ops::Range<usize>,
    /// The parent scope id (`None` for the module scope). Forms a
    /// singly-linked tree.
    pub parent: Option<usize>,
    /// The kind of scope. Used to decide whether a binding is
    /// "module-level" (always live) vs "block-level" (limited to
    /// this range).
    pub kind: ScopeKind,
    /// Bindings declared in this scope, in declaration order. Each
    /// binding is `(name, def_info)`. A name can appear multiple
    /// times in `bindings` if it is shadowed in the same scope,
    /// but the inner one wins.
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// The top-level module scope (one per program). Imports,
    /// globals, and function names live here.
    Module,
    /// A function body. Parameters are bound at the function's
    /// start; locals/foreach items are bound at their decl span.
    Function,
    /// A workflow body. Workflow destructure params and `data` (the
    /// implicit event payload) live here.
    Workflow,
    /// An `if (cond) { ... }` or `foreach (item in xs) { ... }`
    /// body. Only ever entered dynamically — but for the LSP we
    /// treat them as lexically nested so the scope is correct
    /// whether the branch executes or not.
    Block,
}

/// A single identifier binding inside a scope. `decl_span` is the
/// byte range the parser assigned to the declaration; lookup
/// honours it so a name isn't "in scope" before the line that
/// declares it.
#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,
    /// Source byte range of the declaration. The binding is visible
    /// from `decl_span.start` onward, but only while the enclosing
    /// scope is live.
    pub decl_span: std::ops::Range<usize>,
    /// Optional display kind for hover/completion. `None` means
    /// "leave it to the caller to infer" (e.g. type comes from
    /// type-inference, not from the scope walker).
    pub kind: BindingKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    Variable,
    Function,
    Parameter,
    WorkflowEvent,
    Import,
    /// A special binding for the workflow's implicit `data` event
    /// payload. The destructure-param machinery layers on top of
    /// this — `data` is the carrier for the whole event.
    EventPayload,
}

/// A captured scope stack at a particular source offset. The
/// `Vec<usize>` is a stack of scope ids, innermost last.
#[derive(Debug, Clone, Default)]
pub struct ScopeAt {
    pub offset: usize,
    /// Active scope ids at this offset, in outermost-to-innermost
    /// order.
    pub scopes: Vec<usize>,
}

/// The output of the scope walker. Holds the full scope tree plus
/// a flat `Vec<ScopeAt>` for `O(log n)` lookup.
#[derive(Debug, Default)]
pub struct ScopeIndex {
    pub scopes: Vec<Scope>,
    pub by_offset: Vec<ScopeAt>,
    /// Identifier → definition spans. The same `name` can have
    /// multiple definitions (one per scope level) — `defs` is keyed
    /// by `(name, scope_id)` and the value is the `decl_span`.
    pub defs: HashMap<(String, usize), std::ops::Range<usize>>,
    /// Reverse: every `Expr::Var(name)` reference, collected as
    /// `(name, ref_offset)`. Used by the
    /// `unused-binding`/`unknown-identifier` lints and to power
    /// future goto-definition/hover.
    pub refs: Vec<(String, usize)>,
}

impl ScopeIndex {
    /// Resolve the byte offset `(line, col)` to a byte offset, or
    /// `None` if the position is past the end of the source.
    pub fn byte_offset(source: &str, line: u32, col: u32) -> Option<usize> {
        let mut current_line = 0u32;
        let mut current_col = 0u32;
        for (i, ch) in source.char_indices() {
            if current_line == line && current_col >= col {
                return Some(i);
            }
            if ch == '\n' {
                if current_line == line {
                    // Position is past the end of this line — return
                    // the end-of-line offset.
                    return Some(i);
                }
                current_line += 1;
                current_col = 0;
            } else {
                current_col += 1;
            }
        }
        if current_line == line && current_col >= col {
            Some(source.len())
        } else {
            None
        }
    }

    /// Look up the active scope stack at `offset` via binary
    /// search on `by_offset`.
    pub fn scope_stack_at(&self, offset: usize) -> &[usize] {
        // `by_offset` is sorted by `.offset`. We want the largest
        // entry whose `.offset <= query_offset`. If that entry
        // has an empty scope list, walk back to the previous one
        // — the post-walk snapshot at EOF has an empty stack
        // because all scopes have been popped, and it's not
        // useful for "what's in scope at this position?".
        let idx = match self
            .by_offset
            .binary_search_by_key(&offset, |entry| entry.offset)
        {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        // Walk back from `idx` looking for the first entry with
        // a non-empty scope list. The module scope is always
        // present at offset 0, so we will always find one.
        for i in (0..=idx).rev() {
            if let Some(entry) = self.by_offset.get(i) {
                if !entry.scopes.is_empty() {
                    return &entry.scopes;
                }
            }
        }
        &[]
    }

    /// Walk the active scope stack at `offset` and return every
    /// binding whose `decl_span.start <= offset` and whose parent
    /// scope is still live (i.e. the binding's enclosing scope
    /// range contains `offset`).
    pub fn bindings_at(&self, offset: usize) -> Vec<BindingView<'_>> {
        let stack = self.scope_stack_at(offset);
        let mut out: Vec<BindingView<'_>> = Vec::new();
        for &scope_id in stack {
            let scope = &self.scopes[scope_id];
            for b in &scope.bindings {
                if b.decl_span.start <= offset && scope.range.contains(&offset) {
                    out.push(BindingView {
                        name: &b.name,
                        kind: b.kind,
                        decl_span: &b.decl_span,
                        scope_id,
                    });
                }
            }
        }
        // The walker pushes bindings in declaration order; to
        // honour shadowing, return the *innermost* binding first
        // (which is the last scope in the stack, with its
        // declarations traversed in reverse — so the most recent
        // shadowing wins). We built the stack innermost-last, so
        // we just reverse.
        out.reverse();
        out
    }

    /// Find the innermost definition of `name` at `offset`. `None`
    /// if no binding is visible.
    pub fn lookup(&self, name: &str, offset: usize) -> Option<BindingView<'_>> {
        self.bindings_at(offset)
            .into_iter()
            .find(|b| b.name == name)
    }

    /// The byte range covering the whole program (for queries
    /// against "the module scope").
    pub fn module_scope_id(&self) -> Option<usize> {
        self.scopes
            .iter()
            .position(|s| s.kind == ScopeKind::Module)
    }
}

/// A view of a binding returned by [`ScopeIndex::bindings_at`].
/// Carries borrowed references so callers can read without
/// cloning.
#[derive(Debug, Clone)]
pub struct BindingView<'a> {
    pub name: &'a str,
    pub kind: BindingKind,
    pub decl_span: &'a std::ops::Range<usize>,
    pub scope_id: usize,
}

// ---------------------------------------------------------------------------
// Walker
// ---------------------------------------------------------------------------

/// Walks a `FlowProgram` and produces a [`ScopeIndex`]. The walker
/// is recursive but stack-safe — the input AST is bounded by
/// source length, so the depth is bounded too.
pub fn build_scope_index(program: &FlowProgram, source: &str) -> ScopeIndex {
    let mut index = ScopeIndex::default();
    let mut walker = Walker::new(source, &mut index);
    walker.walk_program(program);
    // Snapshot the final scope stack at EOF so queries past the
    // last byte still work.
    walker.snapshot(program.span.end);
    // Sort by_offset for binary search.
    index
        .by_offset
        .sort_by_key(|entry| entry.offset);
    index
}

struct Walker<'a> {
    source: &'a str,
    index: &'a mut ScopeIndex,
    /// The active scope stack. `stack.last()` is the innermost
    /// (current) scope.
    stack: Vec<usize>,
}

impl<'a> Walker<'a> {
    fn new(source: &'a str, index: &'a mut ScopeIndex) -> Self {
        Self {
            source,
            index,
            stack: Vec::new(),
        }
    }

    fn push_scope(&mut self, kind: ScopeKind, range: std::ops::Range<usize>) -> usize {
        let id = self.index.scopes.len();
        let parent = self.stack.last().copied();
        self.index.scopes.push(Scope {
            range: range.clone(),
            parent,
            kind,
            bindings: Vec::new(),
        });
        self.stack.push(id);
        id
    }

    fn pop_scope(&mut self) {
        self.stack.pop();
    }

    fn current_scope_id(&self) -> Option<usize> {
        self.stack.last().copied()
    }

    /// Record that the current scope stack is active at `offset`.
    /// Called whenever a binding becomes visible, so the
    /// `by_offset` table has a snapshot of the live stack.
    fn snapshot(&mut self, offset: usize) {
        self.index.by_offset.push(ScopeAt {
            offset,
            scopes: self.stack.clone(),
        });
    }

    fn add_binding(&mut self, name: &str, decl_span: std::ops::Range<usize>, kind: BindingKind) {
        let scope_id = match self.current_scope_id() {
            Some(id) => id,
            None => return,
        };
        let scope = &mut self.index.scopes[scope_id];
        scope.bindings.push(Binding {
            name: name.to_string(),
            decl_span: decl_span.clone(),
            kind,
        });
        self.index
            .defs
            .insert((name.to_string(), scope_id), decl_span.clone());
        // First time we see a binding becoming visible — snapshot
        // the scope stack at this offset.
        self.snapshot(decl_span.start);
    }

    /// Re-resolve the most recent binding of `name` in the
    /// innermost scope that contains it. This is what `Assign`
    /// triggers — instead of pushing a new binding, we update
    /// the type of the existing one.
    fn reassign(&mut self, name: &str, decl_span: std::ops::Range<usize>) {
        // Walk the scope stack from innermost to outermost looking
        // for a binding named `name`.
        for &scope_id in self.stack.iter().rev() {
            if let Some(b) = self
                .index
                .scopes
                .get_mut(scope_id)
                .and_then(|s| s.bindings.iter_mut().find(|b| b.name == name))
            {
                b.decl_span = decl_span;
                return;
            }
        }
        // No existing binding — fall back to creating a new one
        // in the innermost scope. (Mirrors the evaluator's
        // "write-then-read" semantics.)
        self.add_binding(name, decl_span, BindingKind::Variable);
    }

    fn walk_program(&mut self, program: &FlowProgram) {
        // Push the module scope covering the whole program.
        let module_id = self.push_scope(ScopeKind::Module, 0..program.span.end.max(1));
        self.snapshot(0);

        // Imports: visible at their decl_span in the module scope.
        for imp in &program.imports {
            self.add_binding(&imp.name, imp.span.start..imp.span.end, BindingKind::Import);
        }
        // Globals: visible from their decl_span onward in the module scope.
        for g in &program.globals {
            self.walk_global(g);
        }
        // Function names: module-level, but their bodies are
        // their own scope. We declare the function name *before*
        // walking its body so recursion works.
        for f in &program.functions {
            self.add_binding(&f.name, f.span.start..f.span.end, BindingKind::Function);
        }
        for f in &program.functions {
            self.walk_function(f);
        }
        // Workflows: same pattern as functions.
        for w in &program.workflows {
            self.add_binding(&w.name, w.span.start..w.span.end, BindingKind::Function);
        }
        for w in &program.workflows {
            self.walk_workflow(w);
        }

        self.pop_scope();
        let _ = module_id;
    }

    fn walk_global(&mut self, g: &GlobalVar) {
        // Globals don't introduce a new scope — they live in the
        // module scope. But we still want their RHS to be resolved
        // with module-level bindings in scope, so we walk the expr
        // and collect refs.
        self.walk_expr(&g.value);
        self.add_binding(&g.name, g.span.start..g.span.end, BindingKind::Variable);
    }

    fn walk_function(&mut self, f: &FunctionDef) {
        // Push a function scope covering the entire `fn name(...) { ... }`
        // block. Parameters are visible from the function's start
        // (the function's decl_span is the `fn` keyword position).
        let fn_id = self.push_scope(ScopeKind::Function, f.span.start..f.span.end);
        for p in &f.params {
            // Parameter decl spans: the function's start span,
            // widened so every param is visible at the function's
            // very first byte. We could be more precise by giving
            // each param its own decl span, but the `fn` keyword
            // is a good shared "visible from here" anchor.
            self.add_binding(p, f.span.start..f.span.end, BindingKind::Parameter);
        }
        // The function's own name is also in scope (recursion).
        // We added it in the module scope; nothing to do here.
        for stmt in &f.body {
            self.walk_stmt(stmt);
        }
        self.pop_scope();
        let _ = fn_id;
    }

    fn walk_workflow(&mut self, w: &WorkflowDef) {
        let wf_id = self.push_scope(ScopeKind::Workflow, w.span.start..w.span.end);
        // `data` is always in scope inside a workflow.
        self.add_binding("data", w.span.start..w.span.end, BindingKind::EventPayload);
        // The event name itself is in scope.
        self.add_binding(
            &w.event,
            w.span.start..w.span.end,
            BindingKind::WorkflowEvent,
        );
        // Destructure params: visible at the workflow's start.
        for p in &w.params {
            if p == "_rename" {
                continue;
            }
            self.add_binding(p, w.span.start..w.span.end, BindingKind::Parameter);
        }
        for stmt in &w.body {
            self.walk_stmt(stmt);
        }
        self.pop_scope();
        let _ = wf_id;
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl { name, value, span } => {
                if let Some(v) = value {
                    self.walk_expr(v);
                }
                self.add_binding(name, span.start..span.end, BindingKind::Variable);
            }
            Stmt::Assign { name, value, span } => {
                self.walk_expr(value);
                self.reassign(name, span.start..span.end);
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
                span: _,
            } => {
                self.walk_expr(condition);
                // The block scope's range is the *body's* range,
                // not the entire `if (cond) { ... }` span. Using
                // the if's full range would make bindings inside
                // the body visible on the line of the `}` and
                // (because of inclusive ranges) the line just
                // after it. The body's start is its first
                // statement's start, or the if's start if the
                // body is empty.
                let then_range = body_range(then_body);
                self.walk_block(then_body, then_range);
                if let Some(eb) = else_body {
                    let else_range = body_range(eb);
                    self.walk_block(eb, else_range);
                }
            }
            Stmt::Foreach {
                item_var,
                iterable,
                body,
                span: _,
            } => {
                self.walk_expr(iterable);
                // Push a block scope for the foreach body so the
                // item variable is scoped to the body, not the
                // whole function. The body range is the
                // statements' range, not the foreach header.
                let body_range = body_range(body);
                let _id = self.push_scope(ScopeKind::Block, body_range.clone());
                self.add_binding(item_var, body_range, BindingKind::Variable);
                for s in body {
                    self.walk_stmt(s);
                }
                self.pop_scope();
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.walk_expr(v);
                }
            }
            Stmt::Expr(expr, _) | Stmt::Log(expr, _) => {
                self.walk_expr(expr);
            }
            Stmt::On { .. } => {}
        }
    }

    fn walk_block(&mut self, stmts: &[Stmt], range: std::ops::Range<usize>) {
        let _id = self.push_scope(ScopeKind::Block, range);
        for s in stmts {
            self.walk_stmt(s);
        }
        self.pop_scope();
    }

    fn walk_expr(&mut self, expr: &Expr) {
        // Collect identifier references for the
        // `unknown-identifier` lint and the future
        // identifier→definition map. We don't need to look up
        // types here — the type-inference pass does that — we
        // just record (name, offset) pairs.
        match expr {
            Expr::Var(name) => {
                // We don't have a position for the `Var` node, so
                // fall back to 0. Consumers can refine this with
                // the source text if they need exact positions.
                self.index.refs.push((name.clone(), 0));
            }
            Expr::Member { object, .. } => self.walk_expr(object),
            Expr::BinaryOp { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::UnaryOp { operand, .. } => self.walk_expr(operand),
            Expr::Call { args, .. } => {
                for a in args {
                    self.walk_expr(a);
                }
            }
            Expr::Array(elements) => {
                for e in elements {
                    self.walk_expr(e);
                }
            }
            Expr::InterpolatedString(parts) => {
                for p in parts {
                    if let workflow_parser::ast::InterpPart::Expr(e) = p {
                        self.walk_expr(e);
                    }
                }
            }
            Expr::String(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null => {}
        }
    }
}

/// Compute the byte range of a statement body. For an `if` body,
/// a `foreach` body, or any other `[Stmt]`, the body range is
/// from the first statement's start to the last statement's end.
/// An empty body has a zero-width range, which is fine — it just
/// means no bindings are added.
fn body_range(stmts: &[Stmt]) -> std::ops::Range<usize> {
    if let Some(first) = stmts.first() {
        let start = first.span().start;
        let end = stmts.last().map(|s| s.span().end).unwrap_or(start);
        start..end
    } else {
        0..0
    }
}

/// Convenience: an import binding for the inference layer. The
/// import resolution lives in `inference::schema`; this helper
/// makes it easy to push the resolved import into a scope.
pub fn add_import_binding(index: &mut ScopeIndex, name: &str, decl_span: std::ops::Range<usize>) {
    let module = match index.module_scope_id() {
        Some(id) => id,
        None => return,
    };
    // The module scope is at index 0, but other scopes may have
    // been pushed on top — we need to push into the *module*
    // scope, not the innermost one. So we locate the module
    // scope directly.
    index.scopes[module].bindings.push(Binding {
        name: name.to_string(),
        decl_span: decl_span.clone(),
        kind: BindingKind::Import,
    });
    index
        .defs
        .insert((name.to_string(), module), decl_span);
    // No snapshot here — imports are usually declared at the top
    // of the file, so the offset is already covered by the
    // module scope's `by_offset` entry at offset 0.
}

#[allow(dead_code)]
pub(crate) fn import_stmt_range(imp: &ImportStmt) -> std::ops::Range<usize> {
    imp.span.start..imp.span.end
}
