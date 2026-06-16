# Actions

The engine dispatches every matched rule's actions to an
`ActionHandler` — a trait defined in
`workflow-engine/src/actions.rs`. The built-in handlers
live in `workflow-actions` and cover the common cases;
anything beyond that is a Rust user who implements the
trait and registers it on the engine.

## The `ActionHandler` trait

```rust
use async_trait::async_trait;
use workflow_domain::{ActionParams, TriggerContext, WorkflowResult};

#[async_trait]
pub trait ActionHandler: Send + Sync {
    fn action_type(&self) -> &str;

    async fn execute(
        &self,
        params: &Option<ActionParams>,
        context: &TriggerContext,
    ) -> WorkflowResult<serde_json::Value>;
}
```

- `action_type()` — the string the engine matches against
  each `Action::action_type` in a rule. For example, the
  `log_message` handler returns `"log_message"`, and any
  rule action with `type: log_message` is dispatched to it.
- `execute()` — the actual side effect. The handler gets
  the rule's parameters (deserialised into
  `ActionParams`) and the trigger context (event name,
  payload, vars). It returns a `serde_json::Value` that
  becomes the `ExecutedAction::result` field on the
  engine's output.

`ActionHandler` is `Send + Sync` so the engine can keep
handlers in a `HashMap<String, Arc<dyn ActionHandler>>`
and dispatch them from any async task.

## Built-in handlers

`workflow-actions` ships three:

| Action type | File | Parameters | What it does |
|---|---|---|---|
| `log_message` | `log_message.rs` | `{ message: string }` | Prints the message to the configured log sink. The default sink writes to stdout; `RuleEngineConfig` does not currently expose a sink hook, so handlers that want a different sink need to be the only one in the chain. |
| `set_var` | `set_var.rs` | `{ name: string, value: any }` | Writes a key into the engine's per-event variable table. Subsequent handlers in the same rule see the new value through `context.vars`. |
| `noop` | `noop.rs` | `{}` | A no-op. Useful as a placeholder while a real action is being developed, or for testing the dispatch path. |

`workflow_actions::builtin_handlers() -> Vec<Box<dyn ActionHandler>>`
returns one of each. The CLI, the WASM crate, and the
README's "Rust Usage" example all call this once after
constructing the engine:

```rust
use workflow_actions::builtin_handlers;

let mut engine = RuleEngine::new(config);
for handler in builtin_handlers() {
    engine.register_handler(handler);
}
```

`RuleEngine::register_handler(Box<dyn ActionHandler>)`
stores the handler under `handler.action_type()`. Calling
`register_handler` twice with handlers of the same
`action_type` replaces the first — this is how the engine
lets you override a built-in.

## Writing a custom handler

Implement the trait and register it. Example: a handler
that POSTs the event payload to a webhook.

```rust
use async_trait::async_trait;
use serde::Deserialize;
use workflow_domain::{ActionParams, TriggerContext, WorkflowError, WorkflowResult};
use workflow_engine::ActionHandler;

pub struct WebhookHandler {
    client: reqwest::Client,
    url: String,
}

#[derive(Deserialize)]
struct WebhookParams {
    url: Option<String>,   // optional override
    payload_template: Option<String>,
}

#[async_trait]
impl ActionHandler for WebhookHandler {
    fn action_type(&self) -> &str { "webhook" }

    async fn execute(
        &self,
        params: &Option<ActionParams>,
        context: &TriggerContext,
    ) -> WorkflowResult<serde_json::Value> {
        let p: WebhookParams = match params {
            Some(ActionParams::Json(v)) => serde_json::from_value(v.clone())
                .map_err(|e| WorkflowError::Serialization(e.to_string()))?,
            _ => WebhookParams { url: None, payload_template: None },
        };
        let target = p.url.as_deref().unwrap_or(&self.url);
        let body = p.payload_template
            .unwrap_or_else(|| serde_json::to_string(&context.data)?);
        let res = self.client.post(target).body(body).send().await
            .map_err(|e| WorkflowError::Io(e.into()))?;
        Ok(serde_json::json!({ "status": res.status().as_u16() }))
    }
}
```

Then register it:

```rust
engine.register_handler(Box::new(WebhookHandler {
    client: reqwest::Client::new(),
    url: "https://hooks.example.com/flow".into(),
}));
```

And reference it from a rule:

```yaml
- id: notify-on-signup
  on: USER_REGISTERED
  do:
    - type: webhook
      url: "https://hooks.example.com/flow"
      payload_template: |
        {"event": "USER_REGISTERED", "userId": "{{data.userId}}"}
```

## Action groups, cooldowns, and state machines

A single rule's `do` list can be wrapped in an
`ActionGroup` (in `workflow-domain::action.rs`) with a
`mode: ActionGroupMode` of `Sequential`, `Parallel`, or
`Race`. Sequential runs each action in order and short-
circuits on the first failure; Parallel fans them out with
`tokio::spawn`; Race runs them all and keeps the first
result.

`workflow-domain::action::Action` also carries:

- `cooldown_ms: Option<u64>` — a per-action cooldown,
  enforced by `cooldown::CooldownTracker` so the same
  action doesn't fire twice within the window.
- `foreach: Option<ForeachConfig>` — iterate the action
  over an array field on the event payload. The legacy
  engine runs the action once per item; the `.flow`
  evaluator does not yet honour `foreach` (it uses
  `Stmt::Foreach` at the language level instead).
- `while: Option<WhileConfig>` — loop the action while a
  condition holds, with a `max_iterations: 1000` default
  cap. As above, this is legacy-engine only.
- `retry: Option<RetryPolicy>` — retry on failure with a
  count, delay, and backoff multiplier.

These are part of the legacy rule format — see the README
quick start for a YAML example. The `.flow` DSL does not
yet model them as first-class statements; reach for the
rule format when you need them.

## Engine integration

The `RuleEngine` itself is a thin dispatcher:

```rust
pub struct RuleEngine {
    rules: Vec<TriggerRule>,
    handlers: HashMap<String, Arc<dyn ActionHandler>>,
    cooldowns: CooldownTracker,
    state_machine: StateMachine,
    emitter: EventEmitter,
}
```

`process_event_simple(event, data, vars)` (and the
structured `process_event`) returns
`Vec<TriggerResult>`, where each `TriggerResult` has
`rule_id`, `success`, and `executed_actions:
Vec<ExecutedAction>`. `ExecutedAction` carries the
`action_type`, the `result: Option<Value>`, the
`error: Option<String>`, and a `skipped: Option<String>`
(for cooldown / disabled-rule short-circuits).

For an in-process integration, see the
[engine + handlers example](#rust-usage) in the root
README.
