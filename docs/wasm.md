# WASM

The `workflow-wasm` crate is a `wasm-bindgen` binding that
exposes the engine to JavaScript — both in the browser and
in Node.js. The same crate provides the legacy rule-engine
API and the `.flow` DSL API; the two are layered on top of
the same `RuleEngine` / `FlowEvaluator` types the rest of
the workspace uses.

## Building

The crate is gated on the `wasm32-unknown-unknown` target.
A standard build with `cargo build -p workflow-wasm` will
**not** produce a usable artifact on a native target; you
need:

```bash
# Install the target once
rustup target add wasm32-unknown-unknown

# Build the bindings
cargo build -p workflow-wasm --target wasm32-unknown-unknown --release

# Or, with wasm-bindgen-cli on PATH
wasm-bindgen target/wasm32-unknown-unknown/release/workflow_wasm.wasm \
  --out-dir pkg \
  --typescript
```

The crate has no `main.rs` and no `[[bin]]`; the
`wasm-pack`-style build target is the library's `cdylib`
output.

## Public API

The crate exports one class and a handful of free
functions. Every method takes JSON strings and returns
JSON-compatible values, so the JS surface is
straightforward.

### `WasmRuleEngine` (class)

The legacy engine wrapped for JS:

```ts
import { WasmRuleEngine } from "workflow-wasm";

const engine = new WasmRuleEngine(JSON.stringify({
  rules: [],
  global_settings: { debug_mode: true, evaluate_all: true, strict_actions: false },
}));

const results = await engine.processEventSimple(
  "USER_REGISTERED",
  JSON.stringify({ userId: "123" }),
  JSON.stringify({ plan: "premium" }), // or null
);
```

`processEventSimple` returns the array of
`{ rule_id, success, executed_actions: [...] }` records the
CLI's `evaluate` subcommand prints. `processEvent` is the
structured variant that takes a `TriggerEvent` JSON object
instead of an event name + payload pair.

The constructor registers the **built-in action handlers**
(`log_message`, `set_var`, `noop`) by default — see
[actions.md](actions.md). If you bring your own handlers in
Rust, the WASM build is the one path where that isn't
supported: the JS side sees the engine as an opaque
black box and the only handlers available are the built-ins.

### Free functions

```ts
import {
  loadRulesFromJson,    loadRulesFromYaml,
  exportRulesToJson,    exportRulesToYaml,
  validateRules,
  executeFlow,          parseFlow,
} from "workflow-wasm";
```

| Function | Shape | Purpose |
|---|---|---|
| `loadRulesFromJson(s)` | `(json: string) => Rule[]` | Parse a JSON rules document into the `TriggerRule` shape. |
| `loadRulesFromYaml(s)` | `(yaml: string) => Rule[]` | Same, for YAML. |
| `exportRulesToJson(rulesJson)` | `(rulesJson: string) => string` | Re-serialise a `Rule[]` (as JSON) to canonical JSON. |
| `exportRulesToYaml(rulesJson)` | `(rulesJson: string) => string` | Same, to canonical YAML. |
| `validateRules(rulesJson)` | `(rulesJson: string) => ValidationResult` | Run `TriggerValidator::validate_all` over the array. |
| `executeFlow(source, eventDataJson)` | `(source: string, data: string) => FlowResult[]` | Parse a `.flow` program and run every workflow against the event payload. See below. |
| `parseFlow(source)` | `(source: string) => FlowParseResult` | Parse-only entry point; returns the function/workflow/import names without running anything. |

### `executeFlow`

The `.flow` entry point. It parses the source with
`FlowParser::parse_flow_program`, instantiates a
`FlowEvaluator`, and runs **every** `WorkflowDef` in the
program against the supplied event data. The result is an
array of:

```ts
interface FlowResult {
  workflow: string;          // the workflow's display name
  logs: string[];            // captured log() calls
  success: boolean;          // false if execution errored
  error: string | null;      // error message when success is false
}
```

The evaluator does not run workflows selectively based on
`on EVENT` — it iterates the workflow list in source order.
This matches the design used by `examples/basic.flow`: each
test fires its `with { … }` payload as the event data and
expects every workflow to either produce a `log(...)` that
the test asserts on or to be a no-op. To target a single
event from JS, filter the returned array by `workflow` and
ignore the rest, or call `parseFlow` first to get the list
of `WorkflowInfo { name, event, params }` and run only the
ones you care about.

Panics inside the evaluator are caught with
`std::panic::catch_unwind` and converted into a `JsError`
so the JS side never sees an aborted WASM module.

### `parseFlow`

Returns a structure useful for editor-side introspection
without running anything:

```ts
interface FlowParseResult {
  functions: string[];       // every fn NAME
  workflows: WorkflowInfo[]; // every workflow's name/event/params
  imports: string[];         // every import path
}
```

This is the entry point the in-browser playground uses to
populate a sidebar of workflows before the user clicks
"Run".

## Conventions inside the WASM crate

- **No custom handler registration on the JS side.** The
  `WasmRuleEngine` constructor registers the built-ins and
  never exposes the `register_handler` mutator. If you
  need a custom action, build a Rust binary that
  re-exports it and ship that to the browser instead.
- **All inputs are JSON strings, all outputs are
  `JsValue`.** The crate never takes a `JsValue` for a
  complex input — the boundary is always a JSON string the
  Rust side deserialises. This keeps the binding thin and
  the failures (`Invalid data JSON: ...`) legible.
- **`serde-wasm-bindgen` for outputs.** `Rule[]` and
  `FlowResult[]` come back as plain JS objects thanks to
  `serde-wasm-bindgen::to_value`.
- **Panics are caught.** The `.flow` evaluator wraps
  `execute_flow_inner` in `catch_unwind` and converts the
  payload into a `JsError`. The legacy engine path does
  *not* wrap — engine errors propagate through the normal
  `WorkflowError` type and become a `JsError` at the
  boundary.

## Pairing with the editor

The WASM crate is independent of the native editor: the
editor does **not** consume `workflow-wasm`. The two are
siblings in the dependency graph (see
[architecture.md](architecture.md)) — the editor hosts
the LSP; the WASM crate is for in-browser rule evaluation.
