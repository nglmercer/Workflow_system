//! Rendering of the completion popup and the hover popup.
//!
//! Both popups are pure functions of `(ctx, state) -> output` so they
//! can be unit-tested and reused without coupling to `EditorApp`.
//!
//! The module is split into focused submodules:
//!
//! - [`layout`]     — shared frame/clamp helpers and the layout constants
//!   consumed by both popups.
//! - [`model`]      — the data types (`HoverContent`, `HoverKind`,
//!   `HoverSignature`, `TypeExpr`, `TypeField`) and the markdown
//!   adapter that turns LSP output into a structured payload.
//! - [`type_parser`]— the recursive-descent parser for the workflow
//!   type DSL (`//@{ id: number, orders: { ... }[] }[]`).
//! - [`markdown`]   — the mini-markdown renderer for hover body text.
//! - [`hover`]      — the hover popup renderer.
//! - [`completion`] — the completion popup renderer.
//!
//! The public API (`show_completion`, `show_hover`, `HoverContent`)
//! is re-exported at this level so callers don't need to know the
//! internal split.

mod completion;
mod hover;
mod layout;
mod markdown;
mod model;
mod type_parser;

pub use completion::show_completion;
// Re-export the kind enum so `crate::theme::Theme` (a sibling
// module) can use it without depending on the private
// `popup::completion` module.
#[allow(unused_imports)]
pub use hover::{show_hover, show_hover_markdown};
#[allow(unused_imports)]
pub use layout::{
    COMPLETION_MAX_HEIGHT, COMPLETION_ROW_HEIGHT, COMPLETION_WIDTH, HOVER_MAX_WIDTH,
    HOVER_MIN_WIDTH,
};
#[allow(unused_imports)]
pub use model::{type_to_type_expr, HoverContent, HoverKind, HoverSignature, TypeExpr, TypeField};
pub use workflow_lsp::features::CompletionKind;
