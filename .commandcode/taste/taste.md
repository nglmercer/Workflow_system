# Taste (Continuously Learned by [CommandCode][cmd])

[cmd]: https://commandcode.ai/

# Architecture
- Prefer embedding companion services (LSP, parsers) as library crates with a thin binary entrypoint over spawning separate processes. Avoid process orchestration, path lookups, and JSON-RPC overhead when the same workspace produces both the editor and the service. Confidence: 0.90
- Expose a clean in-process API (e.g. `features::completions_at`, `features::hover_at`) on the service crate using plain Rust structs, so consumers can skip the JSON-RPC wire protocol entirely. The standalone binary becomes a thin adapter that delegates to the same functions. Confidence: 0.85
- When refactoring an editor's editor-service integration, target the removal of async startup states (`LspStatus::Progress`, "Building…", "Starting…") and crash-mode error variants entirely. A library call has no startup window and no failure mode to surface. Confidence: 0.80

# CLI
- After refactor work, run `cargo check --workspace` plus the relevant crate's test suite (e.g. `cargo test -p <crate> --lib`) to confirm no regressions, and smoke-test the editor binary with a short `timeout` to catch runtime panics on startup. Confidence: 0.75
- Run `cargo clippy` after changes and fix all warnings it surfaces (e.g. `clippy::collapsible_match`, `clippy::unnecessary_cast`) — the user expects zero clippy warnings on the workspace. Confidence: 0.80
- After making code or API changes (new public methods, changed function signatures, new modules), update all related documentation to reflect them — the user expects docs to stay in sync with the code. Confidence: 0.80

# Code Style
- Modularize code: split large source files (e.g. 600+ line `app.rs`) into focused modules (e.g. `snippet.rs`, `keys.rs`, `completion.rs`) when multiple distinct concerns accumulate. Prefer one file per cohesive responsibility over keeping everything in one module. Confidence: 0.70
- Place sidecar test files next to their host as `foo.test.<ext>` (e.g. `hello.test.flow` paired with `hello.flow`). The runner's `find_host_for` strips `.test.<ext>` and looks for the matching base file in the same directory. Confidence: 0.80

# Testing
- For module-level tests, use the inline `#[cfg(test)] mod tests { use super::*; }` pattern placed at the bottom of the same source file, not a separate `tests/<name>.rs` integration file. Integration tests (`tests/smoke.rs`) are reserved for end-to-end tests that need fixtures on disk or exercise the public API across crate boundaries. Confidence: 0.80
- Smoke-test the editor binary with a short `timeout` (e.g. `timeout 3 cargo run -p <editor> --bin <bin>`) to catch runtime panics on startup after wiring new panels or state into `App::default`. Confidence: 0.70
- Add `tests/fixtures/` directories with paired host + test files for end-to-end runner coverage. The convention is `tests/fixtures/<name>.flow` (host) next to `tests/fixtures/<name>.test.flow` (test) — the discovery layer relies on this naming to pair them automatically. Confidence: 0.75

# Workspace
- Member crates use `version.workspace = true`, `edition.workspace = true`, and `license.workspace = true` in their `Cargo.toml` to inherit from the root `[workspace.package]` block. External dependencies are declared once in the root `[workspace.dependencies]` and referenced from members as `{ workspace = true }`. Confidence: 0.80
- Inter-crate dependencies go through the workspace dependency table (e.g. `workflow-domain = { path = "../workflow-domain" }`); a new crate that depends on the parser or domain types must add the path dep to its own `Cargo.toml`. Confidence: 0.75
