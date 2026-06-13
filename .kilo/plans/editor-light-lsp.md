# Plan: Lightweight Editor + LSP Autocomplete/Hightlight for .flow

**Status:** DRAFT — awaiting your input on the tradeoff question below.

---

## Clarifying Question

> **Should the frontend migrate from the existing Preact+custom-textarea editor to a lightweight native library (like CodeMirror 6 or a minimal embeddable), or keep the current Preact approach and just replace the heavy custom textarea/innerHTML trick with a lighter dependency?**

| Option | Pros | Cons |
|--------|------|------|
| **A — Keep Preact, add a light editor lib** (e.g. CodeMirror 6, or even `textarea` + clean TS tokenizer only — drop the innerHTML overlay trick) | Smallest diff; existing component structure stays; WASM + LSP backend already wired | Still in web (not "native") |
| **B — Full native: Tauri/WebView + Rust host** | True native; host IPC for LSP instead of stdio-over-websocket; best performance | ~10× more code; Tauri setup, Rust desktop runtime, new build chain |
| **C — Replace web editor with a minimal Rust-internal editor via WASM** (e.g. expose LSP over WASM, render in webview) | Reuses LSP as-is; no stdio plumbing; lighter runtime | WASM overhead; browser render target still needed |

**My initial recommendation: Option A** — your LSP already works (LSP over stdio with stdio-to-websocket proxy from the frontend). The real pain in the current editor is the textarea+innerHTML overlay trick (cursor jitter, no native multi-cursor, slow re-render on large files). A small CM6 or even a clean custom `<textarea>` + `requestIdleCallback` tokenizer solves the immediate issues without a platform rewrite. If you want a truly native desktop app, that's a separate track; for an example/prototype the web path wins on iteration speed.

Please confirm which option you want me to plan for, or I will proceed with **Option A (keep Preact + replace innerHTML trick with a lighter approach)**.

---

## Shared Prerequisites (all options)

- Confirm `workflow-lsp` binary can be launched from the frontend (stdio proxy → WebSocket).
- Verify WASM parser (`workflow-wasm`) exports enough to feed the LSP or a lightweight completion engine as a fallback when LSP is unavailable.

---

## Option A — Plan: Replace innerHTML overlay with a lighter textarea + tokenizer

### Step 1 — Profile the existing highlight path
- Instrument `www/highlight.ts` `tokenize()` to measure time per 10 / 100 / 1000 lines.
- Identify if O(n²) behavior exists (string concatenation in innerHTML rebuild).

### Step 2 — Introduce a small tokenizer renderer
- Replace the `<pre>` overlay with a synchronized `<pre>` that rebuilds its innerHTML **only on `change` events**, not on every `scroll`/`selection` event.
- Batch tokenizer work: `requestIdleCallback(() => tokenizeAndRender(code))`.
- Cache tokenizer output keyed by `(code.length, contentHash)` to skip redundant re-tokenize on scroll.

### Step 3 — Swap innerHTML for textContent where possible
- Highlight layer renders spans only for colored tokens; non-colored whitespace stays outside the colored spans to reduce DOM size.

### Step 4 — Simplify autocomplete (drop heavy scope cache)
- `www/autocomplete.ts` currently caches scope by `code.length:cursorPos:JSON.stringify(schema).length` — change to a hash of `(code, cursorPos)` with a small LRU (max 32 entries) to prevent memory growth.
- Move `isInsideString / isInsideComment` to a shared module used by both autocomplete and the LSP client (so the LSP proxy can reuse them if needed).

### Step 5 — LSP proxy endpoint (WebSocket)
- Add a tiny Node/Bun server script (`www/lsp-proxy.ts`) spawned by Vite in dev that:
  1. Launches `flow-lsp --stdio`.
  2. Exposes `ws://localhost:<port>/lsp`.
  3. Translates JSON-RPC ↔ stdio.
- Frontend connects via WebSocket only when user explicitly enables "LSP mode" (feature flag, opt-in), falling back to the existing pure-TS autocomplete when unavailable.

### Step 6 — Feature flag & fallback
- Add `enableLsp = false` to `www/types.ts`.
- `Editor.tsx` reads `code, cursorPos` and dispatches to:
  - `getCompletionsTS()` (existing) when flag off.
  - `lspProxy.request('textDocument/completion', { ... })` when flag on.
- On WebSocket close / error, log once and silently fall back to TS path.

### Step 7 — Validation
- Run `npm run typecheck` and `bun run typecheck`.
- Manual smoke test: paste 500 lines of `.flow`, type fast, ensure no jitter.
- Verify LSP hover/completion round-trip via proxy with a unit test against a fixture `.flow` string.

---

## Option B — Plan: Tauri / native WebView app (+ optional lighter sketch)

### Step 1 — Scaffold Tauri
- `cargo init --lib` in `crates/workflow-lsp-host` or reuse `workflow-lsp` as a Tauri command layer.
- `npm create tauri-app` style init into `native/` subfolder.

### Step 2 — Embed LSP in-process
- LSP stdio → Tauri sidecar or in-process LSP handler using the existing `workflow-lsp` crate.
- Expose `completions(text, pos)` and `hover(text, pos)` as Tauri commands.

### Step 3 — Choose a rendering surface
- WebView 2 / WebKit = easiest; reuses the Preact UI.
- True native: wgpu + egui or Druid — but you lose WASM integration; not recommended.

### Step 4 — File I/O & watch
- Tauri FS API for opening/saving `.flow` files cheaply.
- `notify` crate for filesystem watch, fed into LSP `textDocument/didChange`.

### Step 5 — Validation
- `cargo check --workspace`, `tauri dev`, smoke tests.

---

## Option C — Plan: Rust-internal editor rendered via WASM (brief)

- Expose `tower-lsp` runtime inside `workflow-wasm` or a new `workflow-editor-core` crate.
- WebView (WebKit) loads a single `index.html` + WASM bundle; UI rendered via a tiny canvas renderer (e.g. `femtovg` through WASM) or into a hidden `<textarea>` with an overlay.
- Keeps stdio out entirely; SPA is a "hosted" runtime.

Not recommended unless the web bundle size of Option A becomes a blocker — it requires ~10k LoC of new rendering code before you even get completions working.

---

## Sequencing & Effort Summary

| Option | Estimated effort | Reuses existing code | Recommended for prototype? |
|--------|-----------------|----------------------|----------------------------|
| A — Light web editor | ~3–5 days | LSP, WASM, parser, chore | **Yes** |
| B — Tauri native | ~2–3 weeks | LSP, parser | Later |
| C — WASM editor | ~3–4 weeks | Parser only | No |

---

## Open Decisions

1. **Do you want pre-built LSP completions in web from day one, or is "good-enough TS completions + syntax highlight" sufficient for the example?**  
   — LSP proxy adds plumbing; if you only want the *concept* of highlighting + lightweight completions, Option A Step 1–3 alone is enough (≈1 day).

2. **What is the target codebase size you expect to handle?**  
   — Files > 5k lines will change whether we need a Virtual DOM / windowing layer (e.g. `react-virtualized` style) or simple re-render is fine.

3. **Do you want multi-file workspace support in the editor?**  
   — LSP supports it natively; the TS-only path does not without more plumbing.

---

## Next Action

Await your answer to (1) above and your pick between Option A / B / C. Once chosen I will lock the plan to that option and produce a step-by-step file/function-level implementation plan.
