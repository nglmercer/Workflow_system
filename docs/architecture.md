# Architecture

This document describes the **Workspace** layout: the crates, their
responsibilities, the dependency graph between them, and the data
flow that turns a `.flow` source file into executed workflows.

## Crate map

| Crate | Path | Kind | Responsibility |
|---|---|---|---|
| `workflow-domain` | `crates/workflow-domain` | lib | Core data types: `TriggerContext`, `Value`, `Condition`, `Event`, `Rule`, action definitions, settings, validation. No I/O, no parsing — the shared vocabulary every other crate speaks. |
| `workflow-parser` | `crates/workflow-parser` | lib | The `.flow` language: pest grammar (`flow.pest`), AST, parser, and a tree-walking evaluator that can run a single workflow in-process. |
| `workflow-engine` | `crates/workflow-engine` | lib | The `RuleEngine` and the `ActionHandler` trait. Takes parsed rules, dispatches events through conditions, and invokes handlers. Adds cooldowns, state-machine transitions, and an emitter. |
| `workflow-actions` | `crates/workflow-actions` | lib | Built-in action handlers (`log_message`, `set_var`, `noop`) the engine can register without users writing Rust. |
| `workflow-serialize` | `crates/workflow-serialize` | lib | Loads and saves the legacy YAML/JSON/TOML rule format. Translates them into `workflow-domain` types so the engine can consume them. |
| `workflow-cli` | `crates/workflow-cli` | bin | The `flow` binary: `validate`, `test`, `evaluate`, `export`, `watch`. Thin wrapper around the libraries. |
| `workflow-lsp` | `crates/workflow-lsp` | bin | A tower-based language server: hover, completion, go-to-definition, diagnostics, lint. Parses `.flow` files and indexes the workspace. |
| `workflow-native-editor` | `crates/workflow-native-editor` | bin | The `flow-editor` desktop app, built on egui/eframe. Hosts the LSP, renders popups, drives the test runner, and exposes side panels (file browser, diagnostics, test report, keymap cheat sheet). |
| `workflow-test-runner` | `crates/workflow-test-runner` | lib | The `*.test.flow` sidecar runner. Shared by the CLI's `flow test` subcommand and the editor's test panel. See [test-runner.md](test-runner.md). |
| `workflow-wasm` | `crates/workflow-wasm` | lib | wasm-bindgen bindings so the engine runs in a browser or Node.js. |

The TypeScript sources under `src/` (`parser.ts`, `evaluator.ts`,
`types.ts`) are an in-progress playground port of the parser and
evaluator. They are not part of the Rust build and are not yet
feature-complete.

## Dependency graph

The graph is a strict DAG. No cycles.

```
            ┌──────────────────┐
            │ workflow-domain  │  (no workspace deps)
            └────────┬─────────┘
                     │
        ┌────────────┼────────────┐
        │            │            │
        ▼            ▼            ▼
  workflow-parser  workflow-engine  workflow-serialize
        │            │   │
        │            │   ▼
        │            │ workflow-actions
        │            │
        └─────┬──────┘
              │
              ▼
       workflow-cli    workflow-lsp    workflow-test-runner
                                   │            │
                                   ▼            ▼
                              workflow-native-editor

  workflow-wasm  (depends on engine + actions; no edge from
                  workflow-native-editor — the editor does not
                  consume the WASM binding)
```

Concrete edges (selected):

- `workflow-parser` depends on `workflow-domain` for AST node
  types and the `Value` enum.
- `workflow-engine` depends on `workflow-domain` and
  `workflow-actions` (built-in handlers are a default set the
  engine registers).
- `workflow-serialize` depends on `workflow-domain` and
  `workflow-engine` (the loader produces engine-ready `Rule`s).
- `workflow-cli` depends on everything it exposes: `engine`,
  `actions`, `serialize`, `parser`, `lsp`, `test-runner`.
- `workflow-lsp` depends on `workflow-parser` and
  `workflow-domain` — it parses `.flow` files and indexes their
  AST.
- `workflow-test-runner` depends on `workflow-parser` and
  `workflow-domain` — tests drive the parser's evaluator against
  parsed `WorkflowDef`s.
- `workflow-native-editor` depends on `workflow-lsp`,
  `workflow-test-runner`, and indirectly on the parser. It
  embeds the LSP via the same library API instead of spawning a
  subprocess.
- `workflow-wasm` depends on `workflow-engine` and
  `workflow-actions` (the engine + its default handlers, exposed
  to JS).

`Cargo.toml` is the source of truth — see the
`[workspace.dependencies]` block at the workspace root and each
member's `Cargo.toml`. Workspace version, edition, and license
flow from the root `[workspace.package]` into every member via
`version.workspace = true` etc.

## Data flow

### Reading a `.flow` file and running it

```
.flow source
   │
   ▼  FlowParser::parse_flow_program   (workflow-parser)
FlowProgram { imports, globals, functions, workflows, tests }
   │
   ▼  populate_imports on JSON @import files
FlowProgram with globals populated
   │
   ▼  FlowEvaluator::new(program)       (workflow-parser)
   │       │
   │       │ for each WorkflowDef whose on == event
   │       ▼
   │   evaluator.run(workflow)          (workflow-parser)
   │       │
   │       ▼
   │   WorkflowOutcome { logs, scope, return }
   │
   ▼  assert::evaluate
AssertResult (one per expected log/var/etc.)
   │
   ▼  report aggregation
TestReport  →  RunReport
```

The evaluator is **stateless across workflows**; each
`WorkflowDef` runs in a fresh sub-evaluator that shares
`functions` and `globals` but has its own `logs` and `scope`.
The runner concatenates logs in workflow-definition order and
merges scopes (last writer wins for any given name), which is
what users expect when two workflows both set `greeting`.

### The legacy rule format

`workflow-serialize::TriggerLoader` reads `.yaml`/`.yml`/`.json`/
`.toml` rule files and produces `workflow_domain::Rule` values.
The `workflow-engine::RuleEngine` consumes those, registers
`ActionHandler` implementations, and runs
`process_event_simple(event, data)`:

```
Rule files  →  TriggerLoader::load_rules_from_dir
                      │
                      ▼
                   Vec<Rule>    →   RuleEngine::new
                                          │
                  process_event_simple ───┤
                                          ▼
                                    evaluate conditions
                                          │
                                          ▼
                                    invoke handlers
                                          │
                                          ▼
                                    Vec<HandlerResult>
```

The `.flow` DSL is a richer superset: the engine, the LSP, and
the test runner can all consume it; the legacy format is
supported for backwards compatibility.

### The editor's runtime

The native editor embeds the LSP as a library (`workflow-lsp`
exposes plain Rust functions, not a JSON-RPC wire protocol), and
spawns a worker thread for test runs. The popup subsystem
(`crates/workflow-native-editor/src/popup/`) is a pure-function
renderer that the central editor calls once per frame with the
current hover / completion state. See
[editor.md](editor.md).

## Workspace conventions

- **`version.workspace = true` etc.** on every member
  `Cargo.toml` so edition/version/license stay in lockstep.
- **Cross-crate deps go through the workspace table.** A new
  crate that needs the parser adds
  `workflow-parser = { path = "../workflow-parser" }` in its
  `Cargo.toml`.
- **Library + thin binary.** `workflow-cli`, `workflow-lsp`, and
  `workflow-native-editor` are binary crates whose real logic
  lives in their libraries (or in sibling library crates). The
  `main.rs` is just argument parsing and process setup.
- **Tests are inline.** `#[cfg(test)] mod tests` blocks at the
  bottom of the source file, except for `tests/fixtures/` and
  `tests/smoke.rs` which need files on disk.
- **Doc comments are first-class.** Every public module opens
  with a `//!` block describing its responsibility; the `cargo
  doc --workspace` output is the canonical API reference.
