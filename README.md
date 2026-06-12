# Workflow System

A Rust-based agnostic trigger/rule engine with WASM support for cross-language workflow execution.

## Features

- **Rule Engine**: Evaluate events against conditions and execute actions
- **Multiple Formats**: JSON, YAML, TOML, and custom `.flow` files
- **WASM Support**: Compile to WebAssembly for browser/Node.js use
- **CLI Tools**: Validate, evaluate, export, and watch workflows
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

### WASM Usage

```javascript
import init, { WasmRuleEngine } from "workflow-wasm";

await init();
const engine = new WasmRuleEngine(JSON.stringify({
    rules: [/* ... */],
    global_settings: {}
}));

const results = await engine.processEventSimple(
    "USER_REGISTERED",
    JSON.stringify({ userId: "123" })
);
```

## File Formats

### YAML (.yaml, .yml)
```yaml
- id: welcome-user
  on: USER_REGISTERED
  do:
    type: log_message
    params:
      message: "Welcome ${data.userId}!"
```

### JSON (.json)
```json
[{
    "id": "welcome-user",
    "on": "USER_REGISTERED",
    "do": {
        "type": "log_message",
        "params": { "message": "Welcome ${data.userId}!" }
    }
}]
```

### Flow (.flow)
Custom format using YAML syntax:
```yaml
- id: welcome-user
  name: Welcome User
  description: Greet new users
  on: USER_REGISTERED
  do:
    type: log_message
    params:
      message: "Welcome ${data.userId}!"
```

## Operators

| Operator | Description |
|----------|-------------|
| EQ, == | Equal |
| NEQ, != | Not Equal |
| GT, > | Greater Than |
| GTE, >= | Greater Than or Equal |
| LT, < | Less Than |
| LTE, <= | Less Than or Equal |
| IN | Value in Array |
| NOT_IN | Value not in Array |
| CONTAINS | String/Array contains |
| STARTS_WITH | String starts with |
| ENDS_WITH | String ends with |
| IS_EMPTY | Value is empty |
| IS_NULL | Value is null |
| HAS_KEY | Object has key |
| MATCHES | Regex match |
| RANGE | Number in range [min, max] |

## Built-in Actions

- `log_message` - Log a message with level (info/warn/error/debug)
- `set_var` - Set a variable in context
- `noop` - No operation (for testing)

## Custom Actions

Implement the `ActionHandler` trait:

```rust
use workflow_engine::ActionHandler;

struct MyHandler;

#[async_trait]
impl ActionHandler for MyHandler {
    fn action_type(&self) -> &str { "my_action" }

    async fn execute(
        &self,
        params: &Option<ActionParams>,
        context: &TriggerContext,
    ) -> WorkflowResult<serde_json::Value> {
        // Your logic here
        Ok(serde_json::json!({ "success": true }))
    }
}

engine.register_handler(Box::new(MyHandler));
```

## Building

```bash
# Native build
cargo build --release

# WASM build
wasm-pack build --target web --release crates/workflow-wasm
```

## Testing

```bash
cargo test --workspace
```
