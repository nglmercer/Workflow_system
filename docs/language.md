# The `.flow` Language

The `.flow` format is the workflow-system DSL. It declares
events, workflows that fire on those events, the functions and
globals they depend on, and the sidecar tests that exercise
them. This document covers what a `.flow` program looks like
and how the parser, evaluator, and test runner interpret it.

A complete program parses to:

```rust
pub struct FlowProgram {
    pub imports: Vec<ImportStmt>,
    pub globals: Vec<VarDecl>,
    pub functions: Vec<FunctionDef>,
    pub workflows: Vec<WorkflowDef>,
    pub tests: Vec<TestDef>,
}
```

## Top-level structure

```flow
// Globals
var API_URL = "https://api.example.com"
var MAX_RETRIES = 3

// Functions
fn greet(name) {
  log("Hello " + name + "!")
  return true
}

// Workflows
workflow "Welcome New User" {
  on USER_REGISTERED
  if (data.plan == "premium") {
    greet(data.user.name)
    log("Premium user detected!")
  }
}

// Tests (only in *.test.flow files)
test "Premium user is greeted" {
  on USER_REGISTERED with { user: { name: "Ada" }, plan: "premium" }
  expect logs ["Hello Ada!", "Premium user detected!"]
}
```

## Events

Events are bare `SCREAMING_SNAKE_CASE` identifiers. They are
the trigger a workflow subscribes to with `on EVENT`. The
event doesn't have to be declared up front — it only needs to
match the `on` clause of at least one `WorkflowDef` (or the
test's `on`) for the test to find it.

The unused-workflow lint recognises an event as "external" if
it is imported via `@import EVENT from "./schema.json"` or
if it follows the `SCREAMING_SNAKE_CASE` convention.

## Workflows

A workflow is a named block that fires on a single event:

```flow
workflow "Welcome New User" {
  on USER_REGISTERED

  var userName = data.user.name

  if (data.plan == "premium") {
    log("Premium user detected!")
  } else {
    log("Standard user")
  }
}
```

Inside the body you can:

- **Declare locals** with `var name = expr`.
- **Call functions** with `fnName(args, ...)`. Built-in
  functions include `log(message)` (records into the
  workflow's log buffer — the value the test runner's
  `expect logs [...]` assertions read), `emit(event_name)`
  (records an event name into the emitted events list —
  the value the test runner's `expect emitted [...]`
  assertions read), plus the standard operators and any
  globals you declared at the top of the file.
- **Branch** with `if (cond) { … } else if (cond) { … } else { … }`.
- **Loop** with `foreach (item in collection) { … }` or
  `while (cond) { … }`. `break` and `continue` are
  supported.

`data` is the event payload — the same object the test runner
installs under the event name as a global, so
`data.user.name` is shorthand for `EVENT.user.name`.

## Globals and functions

Globals and functions are declared at the top level, before
any workflow:

```flow
var API_URL = "https://api.example.com"
var MAX_RETRIES = 3

fn processItem(item) {
  log("Processing: " + item.name)
  return item.active == true
}

fn calculateTotal(items) {
  var total = 0
  foreach (item in items) {
    total = total + item.price
  }
  return total
}
```

Workflows see every global and every function. The evaluator
shares `globals` and `functions` across all workflows in a
test run, but each workflow runs in a fresh sub-evaluator
with its own `logs` and `scope`.

## `@import` schemas

The test runner needs to know the shape of the event payload.
A `.flow` program can declare one `@import` per event, and
the runner loads the JSON file at parse time:

```flow
@import USER_REGISTERED from "./user_registered.json"
```

The keys in the JSON file become the default fields of the
event binding; the test's `with { … }` clause overlays on top
of those defaults. The binding name is what you reference
inside workflows — by convention, the same as the event name
(`USER_REGISTERED.email`, `USER_REGISTERED.user.name`).

A missing JSON file is reported as a failing assertion
*before* any workflow runs, so the user debugs the right
thing rather than chasing "null" values through their
workflows.

## `.flow` module imports

A `.flow` file can import functions and globals from another
`.flow` file using the `import` statement:

```flow
import utils from "./shared_utils.flow"
```

This merges the imported file's functions and globals into the
current evaluator. Functions are merged with first-writer-wins
semantics — if the host file already defines a function with
the same name, the imported version is skipped.

Example: a shared utilities file with common functions:

```flow
// shared_utils.flow
fn greet(name) {
  log("Hello " + name + "!")
  return true
}

fn formatCurrency(amount, currency) {
  return currency + " " + amount
}
```

And a workflow that imports and uses them:

```flow
// main.flow
import utils from "./shared_utils.flow"

workflow "Welcome" {
  on USER_REGISTERED
  greet(data.name)
}
```

## Type expressions

A workflow can carry a `//@` type annotation that documents
its input shape. The hover popup turns this into a structured
field table rather than a comment blob. The grammar is:

```text
T       = atom ( '[]' )*                    // right-assoc array
atom    = '{' ( NAME ':' T )* '}'           // object
        | '(' ( NAME ':' T )* ')'           // function params or
                                            // parenthesised list
        | NAME                              // primitive
NAME    = identifier (letters, digits, `_`)
```

Examples:

```flow
//@number
//@{ id: number, name: string, orders: { id: number, total: number }[] }[]
//@(x: number, y: number) -> number
```

The parser lives in
`crates/workflow-native-editor/src/popup/type_parser.rs`. The
hover renderer consumes the parsed `TypeExpr` and lays it out
as a 2-column field table (for objects), a chip + element
type (for arrays), or a parameter list with a `returns` row
(for functions).

## Tests

Tests live in sidecar `*.test.flow` files. A test is a
`TestDef`:

```flow
test "Premium user is greeted" {
  on USER_REGISTERED with { user: { name: "Ada" }, plan: "premium" }
  expect logs ["Hello Ada!", "Premium user detected!"]
}
```

The components:

- **`on EVENT with { … }`** — the trigger and the payload.
  `with { … }` is overlaid on top of the resolved schema for
  `EVENT`, so any field not mentioned in `with` falls back to
  the schema's default.
- **`expect logs [...]`** — element-wise equality against
  the captured log lines (in source order). The
  workflow's `log(...)` calls populate the log buffer; the
  assertion compares the collected array as a string.
- **`expect emitted [...]`** — element-wise equality
  against the list of events emitted via `emit("EVENT")`
  calls. The workflow's `emit(...)` calls populate the
  emitted events list; the assertion compares the collected
  array as a string. Example:
  ```flow
  workflow "User Signup" {
    on USER_REGISTERED
    log("Welcome!")
    emit("USER_ACTIVATED")
  }

  test "Signup emits activation" {
    on USER_REGISTERED
    expect emitted ["USER_ACTIVATED"]
    expect logs ["Welcome!"]
  }
  ```
- **`expect return <value>`** — equality against the
  workflow's final `return` value (or `null` if it fell
  off the end without a `return`).
- **`expect var NAME == <value>`** — equality against the
  workflow's final scope binding for `NAME`. A workflow
  that sets a local with `var greeting = "Hi"` and then
  falls off the end leaves `greeting` in scope; the test
  can read it back through this assertion.

The full set of `AssertKind` variants lives in
`crates/workflow-test-runner/src/assert.rs` — the four
shapes above are exactly the `AssertKind` enum's
discriminants.

A test passes iff every assertion passes **and** at least one
workflow in the host file has a matching `on` clause. See
[test-runner.md](test-runner.md) for the runner's execution
model and the report shape.

## Legacy rule format

The original trigger/rule engine consumes YAML, JSON, and
TOML files (`workflow-serialize`) that describe rules in the
older `id / on / do` shape. The `.flow` DSL is a richer
superset: the engine, the LSP, and the test runner can all
consume `.flow` files, and the legacy format is supported for
backwards compatibility. See the root
[`README.md`](../README.md) for the legacy operators and
custom-action handlers.
