# LSP

The `workflow-lsp` crate provides a language server for
`.flow` files. It exposes a **tower of three surfaces**:

1. A **standalone binary** (`flow-lsp`) that speaks the LSP
   over stdio JSON-RPC.
2. An **in-process library** API on
   `workflow_lsp::features::*` that the native editor
   embeds — no subprocess, no wire protocol.
3. A **stateful server core** (`workflow_lsp::ServerState`)
   that holds the open documents, the parsed AST, the
   inferred types, and the lint results. Both the binary
   and the in-process API use the same state.

## Source layout

```
crates/workflow-lsp/
  src/
    main.rs              binary entry point: stdio JSON-RPC
    capabilities.rs      server capabilities (advertised)
    state.rs             ServerState: open documents + per-doc analysis
    handlers.rs          JSON-RPC handlers (hover, completion, ...)
    analysis.rs          per-document AST walking & symbol table
    scope.rs             lexical ScopeIndex built from the AST
    inference/           type inference and value tracking
      mod.rs             Inference struct, Inference::new, Inference::lookup
      program.rs         program-level inference walker
      expr.rs            expression-level inference
      ty.rs              Type enum
      value.rs           Value enum, InferredBinding
      annotation.rs      //@T annotation parser
      builtins.rs        built-in function return types
      methods.rs         method/property tables for inferred types
      schema.rs          JSON-schema → Type
    features/            public in-process API
      mod.rs             diagnostics_at, completions_at, hover_at
      completion.rs      scope-aware + member-access completions
      typecheck.rs       argument-type-mismatch diagnostics
    lint/                lint passes run after parsing
      mod.rs             LintCx, run_all, // flow:disable directives
      unknown_identifier.rs
      unused_binding.rs
      unused_workflow.rs
      redundant_expression.rs
```

## Server capabilities

`workflow_lsp::capabilities::server_capabilities` advertises:

- `textDocument/sync: FULL` — the server expects the client
  to send the entire document on every change. (No
  incremental sync, no willSave/waitUntilSaved.)
- `hoverProvider: true`
- `completionProvider: { triggerCharacters: [".", "("] }`
- `definitionProvider: true`

The server does **not** currently implement
`textDocument/formatting`, `codeAction`, `rename`, or
`workspace/symbol`. The in-process API mirrors this — the
public functions on `features` are exactly the four above
plus `diagnostics_at`, plus the `Completion` / `Diagnostic`
/ `CompletionTextEdit` / `CompletionKind` / `DiagnosticSeverity`
data types that the editor renders.

## In-process API

The editor imports `workflow_lsp::features` and owns a
`workflow_lsp::ServerState`. After every buffer mutation it
calls `state.update_document(&uri, &text)`, then asks the
features for diagnostics, completions, or hover:

```rust
use workflow_lsp::features::{self, Diagnostic, Completion};
use workflow_lsp::ServerState;

let mut state = ServerState::new();
state.update_document(uri, source_text);

let diagnostics: Vec<Diagnostic> = features::diagnostics_at(&state, uri);
let completions: Vec<Completion> = features::completions_at(&state, uri, line, col);
let hover: Option<String> = features::hover_at(&state, uri, line, col);
```

The editor does **not** need to know about the JSON-RPC
protocol or the LSP `lsp_types::*` types. The
`Completion`/`Diagnostic` types are editor-facing shapes
that map cleanly onto the popup subsystem in
[editor.md](editor.md).

## JSON-RPC surface (the binary)

`flow-lsp` runs the same handlers over stdio:

- `textDocument/hover` → `handlers::handle_hover`
- `textDocument/completion` → `handlers::handle_completion`
- `textDocument/definition` → `handlers::handle_definition`
- `textDocument/diagnostic` → `handlers::handle_diagnostic`
- `textDocument/didOpen` → `handlers::handle_did_open`
- `textDocument/didChange` → `handlers::handle_did_change`

The hover and completion handlers internally call the same
`features::hover_at` and `features::completions_at` and then
adapt the result back into `lsp_types::CompletionItem` /
`lsp_types::Hover`. The `into_completion` adapter lives in
`features/completion.rs` and the `into_completion_with_type`
variant enriches the entry with inferred type info.

## Type inference

`features::hover_at` combines the legacy scope/symbol
documentation with the result of the inference layer so the
hover popup shows **what the binding is** (from the scope
table) **and** **what its type and current value are**
(from `Inference`):

- For a function reference, the popup shows
  `**returns:** T`, `**params:** (a: T1, b: T2)`, plus any
  `//@T` annotation as a code-fenced line.
- For a value reference, the popup shows
  `**type:** T`, `**value:** v`, plus the `//@T` annotation
  if one is present.
- For an identifier with no inference result (e.g. an
  unresolved external), the popup falls back to the scope
  detail and documentation strings.

The full inference engine lives in `inference/`. Highlights:

- `Inference::new(program, source)` builds the per-program
  type table. It walks `program.workflows` and
  `program.functions` to populate function signatures and
  per-binding types.
- `Inference::lookup(source, position)` resolves a cursor
  position to an `InferredBinding` with both `ty: Type` and
  `value: Option<Value>`.
- `inference::methods::methods_for(ty)` and
  `inference::methods::properties_for(ty)` drive the
  member-access completion for `data.items.` etc.
- `inference::builtins::*` provides the return types of
  `log`, `length`, `toUpperCase`, and other built-ins.

## Lint passes

`features::diagnostics_at` runs the lint layer after the
type checker. The lint layer is in `lint/`:

- **`unknown_identifier`** — flags references to names not
  in scope. (Events in `SCREAMING_SNAKE_CASE` are treated
  as external and exempt.)
- **`unused_binding`** — flags locals and parameters that
  are declared but never read.
- **`unused_workflow`** — flags workflows whose `on EVENT`
  clause matches no test in the corresponding
  `*.test.flow` and no emit anywhere in the program.
- **`redundant_expression`** — flags tautological
  expressions like `x == x` or `!true`.

The lint layer honours `// flow:disable NAME` directives
(parsed by `lint::parse_disable_directives`); each lint
checks `cx.disabled` before producing a diagnostic.

`features::typecheck::check_type_mismatches` runs before
the lints and reports `expected T, got U` errors for
argument/return-type mismatches, e.g. `log(42)` when `log`
expects a string, or `double("hi")` when `double` expects a
number.

## Scope index

`scope::build_scope_index(program, source)` builds a
flat `ScopeIndex` over the source text. It walks every
function and workflow body and records `Binding { name,
decl_span, scope }` entries, including the synthetic
binding for each `@import` (the import name is a free
identifier that the scope layer treats as a binding for
completions and hover).

The scope index is what `analysis::Analysis::lookup` uses
to resolve a `Position` to a `ScopedSymbol` for the hover
popup.

## Conventions inside the LSP

- **Library + thin binary.** `main.rs` is stdio
  initialisation, capability negotiation, and a
  `match msg.method.as_str()` dispatch. The handler bodies
  delegate to the same `features` API the editor uses.
- **The features crate is the source of truth.** New
  features should land in `features/*` first; the JSON-RPC
  handler is a thin adapter.
- **Lint diagnostics go through the same `Diagnostic`
  type.** The features API does not distinguish between
  parse errors, type errors, and lint warnings; the editor
  (and the `flow-lsp` binary) render them uniformly.
- **Tests are inline.** `#[cfg(test)] mod tests` at the
  bottom of each module, with a `examples_advanced_flow_lints_clean`
  regression test in `features/mod.rs` that asserts zero
  diagnostics on `examples/advanced.flow`.
