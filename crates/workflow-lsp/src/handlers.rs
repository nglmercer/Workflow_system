use lsp_server::{Connection, Message};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, Diagnostic, DiagnosticSeverity,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams, MarkupContent,
    MarkupKind, Position, PublishDiagnosticsParams, Range,
};

use crate::state::ServerState;

pub fn handle_hover(connection: &Connection, state: &ServerState, req: lsp_server::Request) {
    let params: HoverParams = serde_json::from_value(req.params).unwrap();
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let response = if let Some(word) = state.get_word_at_position(uri.as_str(), position) {
        let hover_text = match word.as_str() {
            "var" => "Variable declaration\n```flow\nvar name = value\n```",
            "fn" => "Function definition\n```flow\nfn name(params) { body }\n```",
            "workflow" => "Workflow definition\n```flow\nworkflow \"Name\" { on EVENT ... }\n```",
            "on" => "Event trigger\n```flow\non EVENT_NAME\n```",
            "if" => "Conditional statement\n```flow\nif (condition) { ... } else { ... }\n```",
            "else" => "Else branch of if statement",
            "return" => "Return from function",
            "log" => "Log a message\n```flow\nlog(\"message\")\n```",
            "true" | "false" => "Boolean literal",
            "null" => "Null literal",
            _ => &format!("Unknown: {}", word),
        };

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_text.to_string(),
            }),
            range: None,
        })
    } else {
        None
    };

    let resp = lsp_server::Response::new_ok(req.id, serde_json::to_value(response).unwrap());
    connection.sender.send(Message::Response(resp)).unwrap();
}

pub fn handle_completion(connection: &Connection, _state: &ServerState, req: lsp_server::Request) {
    let params: lsp_types::CompletionParams = serde_json::from_value(req.params).unwrap();
    let _uri = params.text_document_position.text_document.uri;

    let items = vec![
        CompletionItem {
            label: "var".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Variable declaration".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "fn".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Function definition".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "workflow".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Workflow definition".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "on".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Event trigger".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "if".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Conditional statement".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "else".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Else branch".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "return".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Return statement".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "log".to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some("Log a message".to_string()),
            insert_text: Some("log(\"${1:message}\")".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "true".to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Boolean true".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "false".to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Boolean false".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "null".to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Null value".to_string()),
            ..Default::default()
        },
    ];

    let response = Some(CompletionList {
        is_incomplete: false,
        items,
    });

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
