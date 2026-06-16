# Workflow System — Documentation

This directory contains the developer documentation for the
**Workflow System**: a Rust workspace that defines an event-driven
DSL (the `.flow` format), an evaluator, a CLI, a language server,
a native editor, a WASM binding, and a test runner for `.flow`
programs.

If you are new to the project, read the guides in this order:

1. **[language.md](language.md)** — the `.flow` DSL: events,
   workflows, the type expression syntax, the `on … with { … }`
   test shape, and `@import` schemas. The rest of the docs assume
   you can read a `.flow` program.
2. **[architecture.md](architecture.md)** — the workspace
   layout, the crate dependency graph, and the data flow from
   `.flow` source to executed workflows.
3. **[cli.md](cli.md)** — the `workflow` binary: every
   subcommand, the loader's file-extension dispatch, and the
   exit-code conventions.
4. **[test-runner.md](test-runner.md)** — the `*.test.flow`
   sidecar convention, the three runner entry points
   (`run_path`, `run_source`, `run_source_with_host`), and how
   the CLI and the editor use them.
5. **[editor.md](editor.md)** — the native editor: panel layout,
   the popup subsystem (`crates/workflow-native-editor/src/popup/`),
   the keybindings surface, and how LSP features wire in.
6. **[lsp.md](lsp.md)** — the language server: the
   `workflow_lsp::features::*` in-process API the editor
   uses, the stdio JSON-RPC surface the `flow-lsp` binary
   speaks, and the type-inference / lint layers.
7. **[wasm.md](wasm.md)** — the `wasm-bindgen` crate: the
   `WasmRuleEngine` class, the `.flow` `executeFlow`
   entry point, and the JS-side conventions.
8. **[actions.md](actions.md)** — the `ActionHandler`
   trait, the built-in handlers (`log_message`, `set_var`,
   `noop`), and how to register a custom one.

## Conventions

- **Sidecar tests**: a `.test.flow` file lives next to its
  sibling `.flow` host file. The runner pairs them by stripping
  the `.test.flow` suffix. See [test-runner.md](test-runner.md).
- **Library + thin binary**: every crate that ships a binary
  keeps its real logic in a `lib.rs` and re-uses it from
  `main.rs`. The CLI, the native editor, and the test runner all
  follow this pattern.
- **Inline module tests**: tests live in `#[cfg(test)] mod
  tests` blocks at the bottom of the file they exercise, except
  for end-to-end fixtures under `tests/fixtures/` which need
  files on disk.
- **Workspace dependency table**: every cross-crate dependency
  goes through the root `[workspace.dependencies]` block and is
  referenced as `{ workspace = true }` from member crates.

## Source layout

```
crates/
  workflow-domain/         core types: TriggerContext, Value, conditions
  workflow-parser/         pest grammar, AST, evaluator
  workflow-engine/         RuleEngine, ActionHandler trait
  workflow-actions/        built-in action handlers
  workflow-serialize/      YAML/JSON/TOML loaders for the rules format
  workflow-cli/            `flow validate|test|evaluate|export|watch`
  workflow-lsp/            tower-based LSP with hover, completion, etc.
  workflow-native-editor/  egui/eframe desktop editor
  workflow-test-runner/    `*.test.flow` sidecar test runner
  workflow-wasm/           wasm-bindgen bindings for browser/Node

examples/                  .flow + .test.flow example suites
rules/                     hand-written rules in the legacy format
src/                       TypeScript playground (parser.ts / evaluator.ts)
```

See [architecture.md](architecture.md) for the crate-by-crate
responsibilities and dependency edges.
