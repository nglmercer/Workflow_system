//! JSON-RPC handlers for the standalone `flow-lsp` server.
//!
//! Each handler is a thin adapter that turns an `lsp_server::Request` into
//! a call on `crate::features` (the in-process API), and packages the
//! result back into LSP types.

use lsp_server::{Connection, Message};
use lsp_types::{
    CompletionList, Diagnostic, DiagnosticSeverity, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverContents, HoverParams, MarkupContent, MarkupKind, Position,
    PublishDiagnosticsParams, Range,
};

use crate::features;
use crate::state::ServerState;

pub fn handle_hover(connection: &Connection, state: &ServerState, req: lsp_server::Request) {
    let params: HoverParams = serde_json::from_value(req.params).unwrap();
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let hover = features::hover_at(
        state,
        uri.as_str(),
        position.line as usize,
        position.character as usize,
    )
    .map(|body| Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: body,
        }),
        range: None,
    });

    let resp = lsp_server::Response::new_ok(req.id, serde_json::to_value(hover).unwrap());
    connection.sender.send(Message::Response(resp)).unwrap();
}

pub fn handle_completion(connection: &Connection, state: &ServerState, req: lsp_server::Request) {
    let params: lsp_types::CompletionParams = serde_json::from_value(req.params).unwrap();
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let completions = features::completions_at(
        state,
        uri.as_str(),
        position.line as usize,
        position.character as usize,
    );

    // The in-process API returns our own `Completion` struct. Convert back
    // to `lsp_types::CompletionItem` for the wire format.
    let items: Vec<lsp_types::CompletionItem> = completions
        .into_iter()
        .map(|c| {
            let kind = match c.kind {
                features::CompletionKind::Keyword => lsp_types::CompletionItemKind::KEYWORD,
                features::CompletionKind::Function => lsp_types::CompletionItemKind::FUNCTION,
                features::CompletionKind::Variable => lsp_types::CompletionItemKind::VARIABLE,
                features::CompletionKind::Value => lsp_types::CompletionItemKind::VALUE,
                features::CompletionKind::Property => lsp_types::CompletionItemKind::PROPERTY,
                features::CompletionKind::Field => lsp_types::CompletionItemKind::FIELD,
                features::CompletionKind::File => lsp_types::CompletionItemKind::FILE,
            };
            lsp_types::CompletionItem {
                label: c.label,
                kind: Some(kind),
                detail: c.detail,
                documentation: None,
                ..Default::default()
            }
        })
        .collect();

    let response = CompletionList {
        is_incomplete: false,
        items,
    };

    let resp = lsp_server::Response::new_ok(req.id, serde_json::to_value(response).unwrap());
    connection.sender.send(Message::Response(resp)).unwrap();
}

pub fn handle_definition(connection: &Connection, _state: &ServerState, req: lsp_server::Request) {
    let _params: GotoDefinitionParams = serde_json::from_value(req.params).unwrap();
    let response: Option<GotoDefinitionResponse> = None;
    let resp = lsp_server::Response::new_ok(req.id, serde_json::to_value(response).unwrap());
    connection.sender.send(Message::Response(resp)).unwrap();
}

pub fn handle_diagnostic(connection: &Connection, state: &ServerState, req: lsp_server::Request) {
    let params: lsp_types::DocumentDiagnosticParams = serde_json::from_value(req.params).unwrap();
    let uri = params.text_document.uri;

    let diagnostics = if let Some(content) = state.get_document(uri.as_str()) {
        match workflow_parser::FlowParser::parse_program(content) {
            Ok(_) => vec![],
            Err(e) => vec![Diagnostic {
                range: Range {
                    start: Position::new(0, 0),
                    end: Position::new(0, 0),
                },
                severity: Some(DiagnosticSeverity::ERROR),
                message: e,
                ..Default::default()
            }],
        }
    } else {
        vec![]
    };

    let notification = lsp_server::Notification::new(
        "textDocument/publishDiagnostics".to_string(),
        PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        },
    );

    connection
        .sender
        .send(Message::Notification(notification))
        .unwrap();
}

pub fn handle_did_open(
    _connection: &Connection,
    state: &mut ServerState,
    not: lsp_server::Notification,
) {
    let params: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(not.params).unwrap();
    let uri = params.text_document.uri;
    let content = params.text_document.text;

    state.update_document(uri.as_str(), &content);
}

pub fn handle_did_change(
    _connection: &Connection,
    state: &mut ServerState,
    not: lsp_server::Notification,
) {
    let params: lsp_types::DidChangeTextDocumentParams =
        serde_json::from_value(not.params).unwrap();
    let uri = params.text_document.uri;

    if let Some(change) = params.content_changes.into_iter().next() {
        state.update_document(uri.as_str(), &change.text);
    }
}
