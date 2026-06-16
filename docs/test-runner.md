# Test Runner

The `workflow-test-runner` crate runs the `*.test.flow` sidecar
test suites. It is the same engine behind the CLI's
`flow test <path>` command and the editor's test panel, so a
green run on one is a green run on the other.

## Sidecar convention

A `.test.flow` file lives **next to** the `.flow` file that
defines the workflows it exercises. The runner pairs them by
stripping the `.test.flow` suffix:

```
examples/
  basic.flow            ← defines the WorkflowDefs under test
  basic.test.flow       ← defines the TestDefs
  advanced.flow
  advanced.test.flow
```

Pairing happens in `crates/workflow-test-runner/src/discovery.rs`:

1. `discover(path)` walks the tree and collects every regular
   file whose name ends with `.test.flow`.
2. For each match, `find_host_for` strips the `.test.flow`
   suffix and looks for the sibling `*.flow`. A missing host is
   **not** a discovery error — the test file may have inlined
   the workflows itself, in which case it acts as its own host.
3. The result is a `DiscoverEntry { test_file, host_file }`
   list, which the runner iterates.

There is no manifest and no embedded file list. The convention
is the contract: put `foo.test.flow` next to `foo.flow`.

## Three entry points

`TestRunner` exposes three run methods. They differ in **where
the source comes from**, not in how individual tests execute —
all three share the same per-test execution model in
`execute::execute_test`.

### `run_path(path)` — the CLI path

```rust
let runner = TestRunner::with_default_config();
let report = runner.run_path(Path::new("examples/basic.test.flow"))?;
```

`run_path` calls `discover(path)` (a file or a directory; if a
directory, the walk is recursive), then reads each test file
and its sibling host from disk and runs every `TestDef` in
order. The CLI's `flow test <path>` is a thin wrapper.

### `run_source(source, virtual_path)` — single-file mode

```rust
let runner = TestRunner::with_default_config();
let report = runner.run_source(buffer_text, "<buffer>")?;
```

`run_source` parses a single in-memory string as a `.flow`
program and uses **the same program** for both the `TestDef`s
and the `WorkflowDef`s. Use this when the user has defined
everything in one file. The editor's "Run on buffer" path used
to call this unconditionally.

### `run_source_with_host(test, test_path, host, host_path)` — sidecar on disk

```rust
let runner = TestRunner::with_default_config();
let report = runner.run_source_with_host(
    &unsaved_test_buffer,
    "<buffer.test.flow>",
    Some(&fs::read_to_string("examples/basic.flow")?),
    Some("examples/basic.flow"),
)?;
```

`run_source_with_host` is the in-memory equivalent of
`run_entries` for a single pair: the test buffer is unsaved
(so the editor doesn't have to write it to disk), and the
host is read separately and passed in. The host's path is
used to resolve the host's relative `@import` paths.

If `host_source` is `None`, the test buffer itself supplies
the `WorkflowDef`s (single-file mode, same as `run_source`).
This is the fallback the editor uses when the open file isn't
a `.test.flow` or the sibling host doesn't exist on disk.

### Why three methods

| Method | Source on disk | Source in memory | Discovery | Used by |
|---|---|---|---|---|
| `run_path` | test + host | — | yes (filesystem walk) | CLI `flow test` |
| `run_source` | — | combined test+host | no | editor single-file buffers |
| `run_source_with_host` | host only | test | no | editor sidecar `*.test.flow` buffers |

## What a test looks like

A `TestDef` is a `test "Name" { on EVENT with { … } expect … }`
block. The test's `with { … }` payload is overlaid on top of
the host's `@import` schema, so the schema provides defaults
and `with` overrides them. See [language.md](language.md) for
the full syntax.

```flow
test "Welcome logs premium user" {
  on USER_REGISTERED with { user: { name: "Ada" }, plan: "premium" }
  expect logs ["Hello Ada!", "Premium user detected!"]
}
```

## Execution model

For each `TestDef`:

1. **Match workflows.** Filter `host.workflows` by
   `w.event == test.on.event`. If the filter is empty, the
   runner synthesizes a single failing assertion with the
   message `no workflow handles event '<EVENT>'` and reports
   `matched_workflow_count: 0`. This is the most common
   mistake for new test authors (a typo in the event name),
   so the runner surfaces it explicitly rather than showing
   an empty green checkmark.
2. **Resolve imports.** Walk the host's `@import` list and
   load each JSON schema file from `host_source_dir`. A
   missing JSON file is reported as a failing assertion
   *before* the workflow runs, so the user debugs the right
   thing.
3. **Overlay the test payload.** Merge `test.on.with` on top
   of the resolved schema for `test.on.event`, and install
   the merged object into the evaluator's globals under the
   event name.
4. **Run matching workflows.** For each matched
   `WorkflowDef`, evaluate it on a fresh sub-evaluator that
   shares `functions` and `globals` but has its own `logs`
   and `scope`. Concatenate logs in workflow-definition
   order; merge scopes (last writer wins).
5. **Evaluate assertions.** Each `expect logs [...]`,
   `expect var NAME …`, etc. is evaluated against the
   aggregated outcome. The test passes iff every assertion
   passes **and** at least one workflow matched.

## Report shape

```rust
pub struct RunReport {
    pub root: String,
    pub tests: Vec<TestReport>,
    pub passed: usize,
    pub failed: usize,
}

pub struct TestReport {
    pub name: String,
    pub source_path: String,
    pub event: String,
    pub asserts: Vec<AssertResult>,
    pub matched_workflow_count: usize,
    pub passed: bool,
}
```

`RunReport` exposes a `passed` count and a `failed` count; the
aggregate verdict is the method `RunReport::all_passed()`, which
returns `self.failed == 0`. The CLI's exit code reads that
method, not a struct field.
The editor's test panel iterates `report.tests` to render
each result with a pass/fail marker, the matched-workflow
count, and the assertion list.

## Configuration

`TestRunnerConfig` exposes a single knob today: `name_filter`,
a substring filter applied to test names. The CLI exposes it
as a `--filter <SUBSTRING>` flag (the editor does not yet
expose it). Tests whose name does not contain the substring
are skipped — they never run, and the report omits them.

```rust
let runner = TestRunner::new(TestRunnerConfig {
    name_filter: Some("Greet".to_string()),
});
```

## Adding a new fixture

1. Create `foo.flow` next to `foo.test.flow`. The host
   declares globals, functions, and the `WorkflowDef`s.
2. Create `foo.test.flow`. Each `TestDef` is a `test "Name"
   { on EVENT with { … } expect … }` block.
3. Run `cargo run -p workflow-cli -- test foo.test.flow` to
   verify the suite passes from the CLI.
4. The editor's test panel will pick it up automatically
   when `foo.test.flow` is open, because the editor reads
   the sibling `foo.flow` and passes it to
   `run_source_with_host`.
