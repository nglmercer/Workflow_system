use lsp_server::{Connection, Message};
use lsp_types::{InitializeParams, InitializeResult, ServerInfo};
use workflow_lsp::{capabilities, state::ServerState};

fn main() {
    let (connection, _io_threads) = Connection::stdio();

    let initialize_params: InitializeParams = serde_json::from_value(
        connection
            .initialize(serde_json::to_value(capabilities::server_capabilities()).unwrap())
            .unwrap(),
    )
    .unwrap();

    let mut state = ServerState::new();

    eprintln!("{}", workflow_i18n::t("lsp.server_starting"));
    eprintln!("Client capabilities: {:?}", initialize_params.capabilities);

    let result = InitializeResult {
        capabilities: capabilities::server_capabilities(),
        server_info: Some(ServerInfo {
            name: "flow-lsp".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    };

    connection
        .initialize(serde_json::to_value(result).unwrap())
        .unwrap();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req).unwrap() {
                    return;
                }
                handle_request(&connection, &mut state, req);
            }
            Message::Response(_resp) => {}
            Message::Notification(not) => {
                handle_notification(&connection, &mut state, not);
            }
        }
    }
}

fn handle_request(connection: &Connection, state: &mut ServerState, req: lsp_server::Request) {
    use workflow_lsp::handlers;
    match req.method.as_str() {
        "textDocument/hover" => handlers::handle_hover(connection, state, req),
        "textDocument/completion" => handlers::handle_completion(connection, state, req),
        "textDocument/definition" => handlers::handle_definition(connection, state, req),
        "textDocument/diagnostic" => handlers::handle_diagnostic(connection, state, req),
        _ => eprintln!("Unhandled request: {}", req.method),
    }
}

fn handle_notification(
    connection: &Connection,
    state: &mut ServerState,
    not: lsp_server::Notification,
) {
    use workflow_lsp::handlers;
    match not.method.as_str() {
        "textDocument/didOpen" => handlers::handle_did_open(connection, state, not),
        "textDocument/didChange" => handlers::handle_did_change(connection, state, not),
        _ => eprintln!("Unhandled notification: {}", not.method),
    }
}
