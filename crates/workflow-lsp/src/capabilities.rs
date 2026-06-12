use lsp_types::{
    CompletionOptions, HoverProviderCapability, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind,
};

pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![".".to_string(), "(".to_string()]),
            ..Default::default()
        }),
        definition_provider: Some(lsp_types::OneOf::Left(true)),
        ..Default::default()
    }
}
