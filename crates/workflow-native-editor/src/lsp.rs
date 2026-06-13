use lsp_types::{
    CompletionItem, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, HoverContents, InitializeParams, Position,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Url, VersionedTextDocumentIdentifier,
};
use serde::Serialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LspError {
    #[error("failed to start flow-lsp: {0}")]
    StartFailed(std::io::Error),
    #[error("failed to write to lsp: {0}")]
    WriteFailed(std::io::Error),
    #[error("failed to read from lsp: {0}")]
    ReadFailed(String),
    #[error("lsp response missing result")]
    MissingResult,
}

pub struct LspClient {
    stdin: ChildStdin,
    response_rx: Receiver<Option<String>>,
    next_id: i64,
}

impl LspClient {
    pub fn start() -> Result<Self, LspError> {
        let mut child = Command::new("cargo")
            .args(["run", "-p", "workflow-lsp", "--bin", "flow-lsp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(LspError::StartFailed)?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::ReadFailed("failed to capture stdout".to_string()))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            LspError::WriteFailed(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "failed to capture stdin",
            ))
        })?;

        let (response_tx, response_rx) = channel::<Option<String>>();

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut content_length = None;
                let mut line = String::new();

                loop {
                    line.clear();
                    let bytes = reader.read_line(&mut line).unwrap_or(0);
                    if bytes == 0 {
                        let _ = response_tx.send(None);
                        return;
                    }

                    if line == "\r\n" {
                        break;
                    }

                    if let Some(value) = line
                        .trim()
                        .strip_prefix("Content-Length:")
                        .and_then(|v| v.parse::<usize>().ok())
                    {
                        content_length = Some(value);
                    }
                }

                let content_length = match content_length {
                    Some(len) => len,
                    None => continue,
                };

                let mut content = vec![0; content_length];
                if reader.read_exact(&mut content).is_err() {
                    let _ = response_tx.send(None);
                    return;
                }

                if let Ok(message) = String::from_utf8(content) {
                    let _ = response_tx.send(Some(message));
                }
            }
        });

        let mut client = Self {
            stdin,
            response_rx,
            next_id: 1,
        };

        client.initialize()?;

        Ok(client)
    }

    fn initialize(&mut self) -> Result<(), LspError> {
        let id = self.next_id;
        self.next_id += 1;

        let params = InitializeParams {
            capabilities: Default::default(),
            ..Default::default()
        };

        self.send_request(id, "initialize", params)?;

        match self.recv_response(id)? {
            Some(_) => Ok(()),
            None => Err(LspError::MissingResult),
        }
    }

    fn send_request<T: Serialize>(
        &mut self,
        id: i64,
        method: &str,
        params: T,
    ) -> Result<(), LspError> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let body = request.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        self.stdin
            .write_all(header.as_bytes())
            .map_err(LspError::WriteFailed)?;
        self.stdin
            .write_all(body.as_bytes())
            .map_err(LspError::WriteFailed)?;
        self.stdin.flush().map_err(LspError::WriteFailed)?;

        Ok(())
    }

    fn recv_response(&mut self, id: i64) -> Result<Option<serde_json::Value>, LspError> {
        loop {
            match self.response_rx.recv() {
                Ok(Some(message)) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message) {
                        if value.get("id").and_then(|v| v.as_i64()) == Some(id) {
                            return Ok(value.get("result").cloned());
                        }
                    }
                }
                Ok(None) | Err(_) => return Ok(None),
            }
        }
    }

    pub fn open_document(&mut self, uri: &str, text: &str) -> Result<(), LspError> {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Url::parse(uri)
                    .unwrap_or_else(|_| Url::parse("file:///example.flow").unwrap()),
                language_id: "flow".to_string(),
                version: 1,
                text: text.to_string(),
            },
        };

        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": params,
        });

        self.send_notification(notification)
    }

    pub fn change_document(&mut self, uri: &str, version: i32, text: &str) -> Result<(), LspError> {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: Url::parse(uri)
                    .unwrap_or_else(|_| Url::parse("file:///example.flow").unwrap()),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.to_string(),
            }],
        };

        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": params,
        });

        self.send_notification(notification)
    }

    pub fn completion(
        &mut self,
        uri: &str,
        line: usize,
        character: usize,
    ) -> Result<Vec<CompletionItem>, LspError> {
        let id = self.next_id;
        self.next_id += 1;

        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri)
                        .unwrap_or_else(|_| Url::parse("file:///example.flow").unwrap()),
                },
                position: Position {
                    line: line as u32,
                    character: character as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };

        self.send_request(id, "textDocument/completion", params)?;

        match self.recv_response(id)? {
            Some(value) => match serde_json::from_value::<CompletionResponse>(value) {
                Ok(CompletionResponse::Array(items)) => Ok(items),
                Ok(CompletionResponse::List(list)) => Ok(list.items),
                Err(_) => Ok(Vec::new()),
            },
            None => Ok(Vec::new()),
        }
    }

    pub fn hover(
        &mut self,
        uri: &str,
        line: usize,
        character: usize,
    ) -> Result<Option<String>, LspError> {
        let id = self.next_id;
        self.next_id += 1;

        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::parse(uri)
                    .unwrap_or_else(|_| Url::parse("file:///example.flow").unwrap()),
            },
            position: Position {
                line: line as u32,
                character: character as u32,
            },
        };

        self.send_request(id, "textDocument/hover", params)?;

        match self.recv_response(id)? {
            Some(value) => {
                if value.is_null() {
                    Ok(None)
                } else {
                    let hover: lsp_types::Hover =
                        serde_json::from_value(value).unwrap_or(lsp_types::Hover {
                            contents: HoverContents::Scalar(lsp_types::MarkedString::String(
                                String::new(),
                            )),
                            range: None,
                        });

                    match hover.contents {
                        HoverContents::Scalar(ms) => Ok(Some(match ms {
                            lsp_types::MarkedString::String(s) => s,
                            lsp_types::MarkedString::LanguageString(ls) => ls.value,
                        })),
                        HoverContents::Array(items) => {
                            let text = items
                                .into_iter()
                                .map(|ms| match ms {
                                    lsp_types::MarkedString::String(s) => s,
                                    lsp_types::MarkedString::LanguageString(ls) => ls.value,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            Ok(Some(text))
                        }
                        HoverContents::Markup(markup) => Ok(Some(markup.value)),
                    }
                }
            }
            None => Ok(None),
        }
    }

    fn send_notification(&mut self, notification: serde_json::Value) -> Result<(), LspError> {
        let body = notification.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        self.stdin
            .write_all(header.as_bytes())
            .map_err(LspError::WriteFailed)?;
        self.stdin
            .write_all(body.as_bytes())
            .map_err(LspError::WriteFailed)?;
        self.stdin.flush().map_err(LspError::WriteFailed)?;

        Ok(())
    }
}
