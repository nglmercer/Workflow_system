# Taste (Continuously Learned by [CommandCode][cmd])

[cmd]: https://commandcode.ai/

# Architecture
- Prefer embedding companion services (LSP, parsers) as library crates with a thin binary entrypoint over spawning separate processes. Avoid process orchestration, path lookups, and JSON-RPC overhead when the same workspace produces both the editor and the service. Confidence: 0.90
- Expose a clean in-process API (e.g. `features::completions_at`, `features::hover_at`) on the service crate using plain Rust structs, so consumers can skip the JSON-RPC wire protocol entirely. The standalone binary becomes a thin adapter that delegates to the same functions. Confidence: 0.85
- When refactoring an editor's editor-service integration, target the removal of async startup states (`LspStatus::Progress`, "Building…", "Starting…") and crash-mode error variants entirely. A library call has no startup window and no failure mode to surface. Confidence: 0.80

# CLI
- After refactor work, run `cargo check --workspace` plus the relevant crate's test suite (e.g. `cargo test -p <crate> --lib`) to confirm no regressions, and smoke-test the editor binary with a short `timeout` to catch runtime panics on startup. Confidence: 0.75

# Code Style
- Modularize code: split large source files (e.g. 600+ line `app.rs`) into focused modules (e.g. `snippet.rs`, `keys.rs`, `completion.rs`) when multiple distinct concerns accumulate. Prefer one file per cohesive responsibility over keeping everything in one module. Confidence: 0.70
