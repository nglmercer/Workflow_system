# CLI

The `workflow` binary (`workflow-cli`, built with
`cargo run -p workflow-cli -- <subcommand> <args>`) is the
command-line entry point for the engine. It is a thin wrapper
around the libraries — argument parsing lives in
`crates/workflow-cli/src/main.rs`, and each subcommand is a
focused module under `crates/workflow-cli/src/commands/`.

## Subcommands

| Subcommand | Purpose | Async? |
|---|---|---|
| `validate <path>` | Load every rule under `<path>` and run `TriggerValidator` over the set. | no |
| `test <path>` | Discover `*.test.flow` under `<path>` and run them through the test runner. | no |
| `evaluate <path> -e EVENT [-d JSON] [-v JSON]` | Fire `EVENT` once through the engine. | yes |
| `export <input> -o <output>` | Convert a rule file between formats (YAML/JSON/TOML). | no |
| `watch <path> -e EVENT [-d JSON]` | Re-evaluate `EVENT` whenever a file under `<path>` changes. | yes |

`<path>` may be a single file or a directory. For the loaders
(`validate`, `evaluate`, `export`, `watch`) the loader walks
the directory recursively and picks up `.yaml`, `.yml`,
`.json`, `.toml`, and `.flow` files. For `test`, discovery
only matches `.test.flow` (see
[test-runner.md](test-runner.md)).

## `validate`

```bash
cargo run -p workflow-cli -- validate rules/
cargo run -p workflow-cli -- validate rules/workflows.flow
```

`validate` calls `TriggerLoader::load_rules_from_dir` (or
`load_rule` for a single file), then `TriggerValidator::validate_all`.
On success it prints `✓ All rules are valid` and exits 0.
On failure it prints each issue with the prefix
`✗ ERROR` or `⚠ WARN` to stderr and exits non-zero.

## `test`

```bash
cargo run -p workflow-cli -- test examples/
cargo run -p workflow-cli -- test examples/basic.test.flow --filter "Welcome"
cargo run -p workflow-cli -- test examples/ --json
```

`test` is the only subcommand that exits with a per-test
verdict: 0 if every test passed, 1 if any failed. See
[test-runner.md](test-runner.md) for the runner model and
the report shape.

Flags:

- `--filter <SUBSTRING>` — only run tests whose name
  contains the substring. Skipped tests are omitted from
  the report entirely (not marked as `✗`).
- `--json` — emit the `RunReport` as pretty-printed JSON
  instead of the human table. Useful for piping into other
  tools.

## `evaluate`

```bash
cargo run -p workflow-cli -- evaluate rules/ -e USER_REGISTERED \
  -d '{"userId":"123"}' -v '{"plan":"premium"}'
```

`evaluate` is the smoke-test for the legacy engine
(`workflow-engine::RuleEngine`): load the rules under
`<path>`, build an engine with the built-in action handlers
(`log_message`, `set_var`, `noop`), and call
`process_event_simple(event, data, vars)`. The output is a
human table of matched rules and the actions each one
executed, with `✓` / `✗` / `○` status markers. The engine
runs in `debug_mode: true, evaluate_all: true,
strict_actions: false` — see
[architecture.md](architecture.md) for the field meanings.

Flags:

- `-e, --event <NAME>` — the event to fire (required).
- `-d, --data <JSON>` — the event payload, parsed as JSON.
  Defaults to `{}`.
- `-v, --vars <JSON>` — variables attached to the event,
  parsed as JSON. Defaults to none.

## `export`

```bash
cargo run -p workflow-cli -- export rules/workflows.flow -o rules/workflows.json
cargo run -p workflow-cli -- export rules/ -o rules.json
```

`export` calls `TriggerLoader` to load `<input>` and
`RuleExporter::save_to_file` to write the result. The output
format is picked from the file extension:

| Extension | Format |
|---|---|
| `.yaml`, `.yml`, `.flow` | YAML |
| `.json` | JSON |
| `.toml` | TOML |

Anything else returns an `InvalidInput` error. The `.flow`
output is YAML, not the `.flow` DSL — the exporter
serialises the legacy `TriggerRule` shape; the DSL lives
elsewhere (see [language.md](language.md)).

## `watch`

```bash
cargo run -p workflow-cli -- watch rules/ -e FILE_CHANGED -d '{}'
```

`watch` polls the directory once per second, re-loads every
rule whose `mtime` is within the last two seconds or whose
path is new since the previous tick, and re-fires `EVENT`
through a fresh engine on every change. It uses
`debug_mode: false, evaluate_all: true, strict_actions:
false` — the same shape as `evaluate` minus debug output.
Press `Ctrl+C` to stop.

The watcher uses `TriggerLoader::collect_rule_files` for
discovery (which picks up `.yaml`, `.yml`, `.json`, `.toml`,
and `.flow`) and `TriggerLoader::load_rules_from_dir` for
the re-load. It does not use the `.test.flow` discovery
layer.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success — subcommand ran and produced the expected verdict (green for `test`, no validation issues for `validate`, no error for the others). |
| `1` | Subcommand-level failure (validation issues, test failures, engine error, parse error, I/O error). |
| `2` | clap-level failure (unknown subcommand, missing required flag, bad flag value). |

For `test`, the per-test verdict is folded into exit code
`0`/`1` and is *not* a subcommand error — a failing test
prints the table and exits 1, not an error message.

## Conventions inside the CLI

- **Library + thin binary.** Every subcommand is a focused
  module under `crates/workflow-cli/src/commands/`. The
  module's only public entry point is a `run(...)` function
  that takes typed arguments and returns a `WorkflowResult`
  (or `Result<ExitCode, _>` for `test`).
- **Inline module tests.** `#[cfg(test)] mod tests` at the
  bottom of each subcommand file, not under `tests/`.
- **Async surface is minimal.** Only `evaluate` and `watch`
  are `async`; the rest are pure CPU/IO and run synchronously.
- **Errors are user-readable.** The `thiserror` `Display`
  impls print `error: <message>` to stderr; clap prints its
  own usage on argument errors.
