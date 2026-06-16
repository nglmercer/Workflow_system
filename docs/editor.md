# Native Editor

The native editor (`workflow-native-editor`, binary name
`flow-editor`) is an egui/eframe desktop application for editing
`.flow` files. It embeds the language server in-process, renders
hover and completion popups, exposes side panels, and runs
`.test.flow` test suites on a background thread.

## Source layout

The crate follows the library + thin binary convention. The
real logic lives in the binary's modules, each a focused
single-concern file:

```
crates/workflow-native-editor/src/
  main.rs                entry point (eframe::run_native, dark visuals)
  app.rs                 EditorApp: top-level state, panels, command dispatch
  completion.rs          completion list state machine (open/closed, current index)
  cursor.rs              CursorPosition (line/col ↔ byte offset)
  diagnostics_panel.rs   side panel: parser/LSP diagnostics
  file_browser.rs        side panel: file tree
  file_io.rs             save / load / dirty-tracking helpers
  folding.rs             brace/keyword folding
  gutter.rs              line-number gutter with fold markers
  highlight.rs           syntax highlighting
  history.rs             undo/redo with typing coalescing
  home.rs                start screen (recent files, open file)
  keybindings.rs         global keymap (chords → Commands)
  layouter.rs            text layout / cursor positioning
  popup/                 hover + completion popup subsystem
  recent.rs              recent files list
  shortcuts_window.rs    F1 → keymap cheat sheet
  snippet.rs             snippet expansion ($1, $0, $TM_… placeholders)
  test_panel.rs          side panel: test report viewer
```

## Panel layout

The editor is a single `EditorApp` (in `app.rs`) that owns:

- A **central panel** with the `TextEdit`, gutter, folding
  markers, and overlay popups.
- A **left panel** (file browser) that is collapsible.
- A **right panel** with three tabs the user can switch
  between: **Diagnostics** (LSP errors and warnings),
  **Tests** (the test panel), and **Shortcuts** (the F1
  cheat sheet).
- A **top bar** with the file path, dirty marker, and the
  `Run` button that triggers the test runner.
- A **bottom status bar** with the current mode and a
  transient "Running tests…" message.

The right-panel tab is whichever the user opened last; there
is no implicit switching.

## Popup subsystem

The `popup/` directory holds the hover and completion popup
renderers. They are pure functions of `(ctx, state) → output`,
called once per frame from the central panel:

```
popup/
  mod.rs        public re-exports: show_completion, show_hover,
                HoverContent, HoverKind, HoverSignature, TypeExpr,
                TypeField, layout constants
  layout.rs     popup_frame, clamp_to_screen, layout constants
                (COMPLETION_WIDTH, COMPLETION_MAX_HEIGHT,
                 COMPLETION_ROW_HEIGHT, HOVER_MAX_WIDTH,
                 HOVER_MIN_WIDTH, SCREEN_EDGE_MARGIN)
  model.rs      HoverContent, HoverKind, HoverSignature,
                TypeExpr, TypeField, plus the from_markdown
                adapter that turns LSP hover output into a
                structured payload
  type_parser.rs  tiny recursive-descent parser for the
                  workflow type DSL (TypeExpr)
  markdown.rs   render_mini_markdown (handles **bold**,
                *italic*, `code`, and //@type annotations)
  hover.rs      show_hover, show_hover_markdown, the hover
                popup body renderer, the signature dispatcher,
                the type-table renderer
  completion.rs show_completion, the row renderer, the per-kind
                glyph + colour mapping
```

The public API stays at `popup::show_completion`,
`popup::show_hover`, and `popup::HoverContent::from_markdown`,
so the central editor in `app.rs` doesn't need to know about
the internal split.

### Hover popup

`HoverContent::from_markdown` ingests the legacy
`MarkupKind::Markdown` blob produced by the LSP without
changing the LSP protocol: split on blank lines, take the
first paragraph as the title, classify the rest by content,
let the renderer apply the colors. The renderer:

- Renders a colored **badge** (one of `@param`, `@event`,
  `@var`, `@fn`, `@type`, `@field`, `@error`, `@warn`, `@doc`)
  with a glyph prefix and a bold title.
- Renders the **signature** as a structured field table when
  it's a parsed `TypeExpr` (objects → 2-column table, arrays
  → chip + element type, functions → params + return, primitives
  → pill), or as a monospace label otherwise.
- Renders the **body** through `render_mini_markdown` inside
  a `ScrollArea` capped at 180px high.

The `render_type_compact` helper is wrapped in a `ui.vertical`
sub-UI at the top, so its recursive `ui.indent` calls always
land on a vertical parent. This matters because
`render_type_compact` is invoked from `ui.horizontal(...)`
contexts (the field table's "Type" column, the `Array of`
row, the `returns` row) and a `ui.indent` inside a horizontal
layout would panic.

### Completion popup

`show_completion(ctx, completions, current_index, cursor_pos)`
clamps the popup inside the screen rect, opens a frameless
egui `Window` with the completion list, and returns the index
of any item the user clicked (or `None`). Each row shows a
per-kind glyph (`>` keyword, `f` function, `v` variable, `=`
value, `.` property/field, `[]` file), the label, an optional
right-aligned `detail`, and a `>` selection marker for the
currently-highlighted item.

## Keybindings

`keybindings.rs` is a small data-driven keymap that maps
[`Chord`]s to [`Command`]s. It supports **chords** — a key
like `Ctrl+K` followed by `Ctrl+L` is treated as a single
command — which mirrors VS Code-style multi-key shortcuts
without exploding the `Command` enum.

Handlers run once per frame, *before* the central panel, so
they can `consume_key` the relevant events and prevent the
embedded `TextEdit` from also seeing them. Press `F1` to open
the **Shortcuts** panel, which is generated from the same
keymap.

## Running tests

The Run button (and `Ctrl+Enter`) calls `EditorApp::run_tests`,
which:

1. Reads the current buffer into an owned `String`.
2. If the open file is a sidecar `*.test.flow`, reads the
   sibling `*.flow` from disk and constructs the host path
   from the open file's path.
3. Spawns a worker thread that calls
   `TestRunner::run_source_with_host` with both the test
   buffer and the host buffer.
4. Sends the resulting `RunReport` over an `mpsc::channel`
   back to the main thread, which the central panel polls
   once per frame and feeds to the test panel.

If the open file is **not** a `.test.flow` (or the sidecar is
missing), the host arguments are `None` and the runner falls
back to single-file mode (the test buffer is its own host).
This is the same shape as the CLI's `flow test <path>`, just
without the on-disk discovery.

## LSP integration

The editor embeds the language server as a library — no
subprocess, no JSON-RPC, no startup window. The
`workflow-lsp` crate exposes plain Rust functions on
`workflow_lsp::features`:

```rust
pub fn diagnostics_at(state: &ServerState, uri: &str) -> Vec<Diagnostic>;
pub fn completions_at(
    state: &ServerState,
    uri: &str,
    line: usize,
    character: usize,
) -> Vec<Completion>;
pub fn hover_at(
    state: &ServerState,
    uri: &str,
    line: usize,
    character: usize,
) -> Option<String>;
```

The editor owns a `workflow_lsp::ServerState` (it lives at
the top of `EditorApp`) and keeps it in sync by calling
`lsp.update_document(&self.uri, &self.text)` on every
mutation. Hover and completion take the live `ServerState`,
the document URI (e.g. `file:///…/foo.flow`), and a
0-indexed `(line, col)` cursor position. The hover handler
returns Markdown text; the editor funnels that through
`popup::HoverContent::from_markdown` to render the
structured popup. The standalone `flow-lsp` binary
(`crates/workflow-lsp/src/main.rs`) re-uses the same
`handlers.rs` entry points over stdio JSON-RPC.

## Conventions inside the editor

- **Inline module tests.** Each module ends with a
  `#[cfg(test)] mod tests` block; no separate
  `tests/<name>.rs` files.
- **Smoke-test the binary.** After wiring new panels or state
  into `EditorApp::default`, run
  `timeout 3 cargo run -p workflow-native-editor --bin flow-editor`
  to catch runtime panics on startup.
- **Zero clippy warnings.** `cargo clippy -p workflow-native-editor --all-targets`
  should be clean. The popup subsystem uses `#[allow(unused_imports)]`
  on its `pub use` re-exports because the public surface is
  larger than the in-crate consumers; tests in submodules
  reach the items through `super::` directly.
