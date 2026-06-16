//! Library API for the Flow LSP.
//!
//! This crate can be used in two ways:
//!
//! 1. As a standalone server binary (`flow-lsp`) that speaks JSON-RPC over
//!    stdin/stdout — useful for VS Code, Helix, Neovim, etc.
//! 2. As a library that the `workflow-native-editor` imports directly,
//!    avoiding any process spawn, JSON serialization, or path lookups.

pub mod analysis;
pub mod capabilities;
pub mod features;
pub mod handlers;
pub mod state;

pub use state::ServerState;
