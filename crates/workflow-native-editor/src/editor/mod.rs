//! Editor-internal modules.
//!
//! These hold pieces of `EditorApp` that have grown cohesive
//! enough to deserve their own file. Each module exposes
//! `pub(super)` free functions that take `&mut EditorApp` (or
//! `&EditorApp`) so the methods can hang off `EditorApp` while
//! living in a separate file. Splitting the implementations across
//! files keeps the `impl EditorApp` blocks short and the
//! responsibility of each one easy to audit.

pub(super) mod edit_ops;
pub(super) mod history_ops;
pub(super) mod import_hover;
pub(super) mod input;
pub(super) mod project;
pub(super) mod tests_runner;
pub(super) mod view;
