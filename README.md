# Workflow System

A Rust-based agnostic trigger/rule engine with WASM support for cross-language workflow execution.

## Documentation

Developer documentation lives in [`docs/`](docs/):

- [docs/language.md](docs/language.md) — the `.flow` DSL: events, workflows, type expressions, `@import` schemas, and the `on … with { … }` test shape.
- [docs/architecture.md](docs/architecture.md) — the workspace layout, the crate dependency graph, and the data flow from `.flow` source to executed workflows.
- [docs/cli.md](docs/cli.md) — the `workflow` binary: every subcommand, the loader's file-extension dispatch, and the exit-code conventions.
- [docs/test-runner.md](docs/test-runner.md) — the `*.test.flow` sidecar convention and the three runner entry points (`run_path`, `run_source`, `run_source_with_host`).
- [docs/editor.md](docs/editor.md) — the native egui editor: panel layout, the popup subsystem, the keybindings surface, and the test-panel pipeline.
- [docs/lsp.md](docs/lsp.md) — the language server: the `workflow_lsp::features::*` in-process API and the stdio JSON-RPC surface.
- [docs/wasm.md](docs/wasm.md) — the `wasm-bindgen` crate: the `WasmRuleEngine` class and the `executeFlow` entry point.
- [docs/actions.md](docs/actions.md) — the `ActionHandler` trait, the built-in handlers, and how to register a custom one.

## Features

- **Rule Engine**: Evaluate events against conditions and execute actions
- **`.flow` DSL**: a richer superset of the legacy rule format — workflows, type annotations, sidecar tests, JSON `@import` schemas
- **Multiple Formats**: JSON, YAML, TOML, and `.flow` files
- **WASM Support**: Compile to WebAssembly for browser/Node.js use
- **CLI Tools**: Validate, evaluate, export, and watch workflows
- **Native Editor**: egui/eframe desktop app with in-process LSP, hover/completion popups, and a built-in test runner
- **Extensible**: Custom action handlers via `ActionHandler` trait

## Quick Start

### CLI Usage

```bash
# Validate rules
cargo run -p workflow-cli -- validate rules/

# Evaluate an event
cargo run -p workflow-cli -- evaluate rules/ -e USER_REGISTERED -d '{"userId":"123"}'

# Export between formats
cargo run -p workflow-cli -- export rules/workflows.flow -o output.json

# Watch for changes
cargo run -p workflow-cli -- watch rules/ -e TEST -d '{}'

# Run sidecar `*.test.flow` test suites
cargo run -p workflow-cli -- test examples/basic.test.flow
```

### Rust Usage

```rust
use workflow_engine::RuleEngine;
use workflow_actions::builtin_handlers;
use workflow_serialize::TriggerLoader;

let rules = TriggerLoader::load_rules_from_dir("rules")?;
let mut engine = RuleEngine::new(RuleEngineConfig {
    rules,
    global_settings: GlobalSettings::default(),
});

for handler in builtin_handlers() {
    engine.register_handler(handler);
}

let results = engine.process_event_simple(
    "USER_REGISTERED",
    serde_json::json!({"userId": "123"}),
    None,
).await?;
```

The native editor (`cargo run -p workflow-native-editor --bin
flow-editor`) has a built-in test panel that runs the
`*.test.flow` test suite for the open file. See
[docs/editor.md](docs/editor.md).
