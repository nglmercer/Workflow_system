use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, ScrollArea, Ui, Vec2};
use lsp_types::{
    CompletionItem, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, HoverContents, InitializeParams, Position, Range,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams,
    Url, VersionedTextDocumentIdentifier,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use thiserror::Error;

const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 20.0;
const CHAR_WIDTH: f32 = 8.4;
const TAB_SIZE: usize = 2;

#[derive(Error, Debug)]
enum LspError {
    #[error("failed to start flow-lsp: {0}")]
    StartFailed(std::io::Error),
    #[error("failed to write to lsp: {0}")]
    WriteFailed(std::io::Error),
    #[error("failed to read from lsp: {0}")]
    ReadFailed(String),
    #[error("lsp response missing result")]
    MissingResult,
}

struct LspClient {
    stdin: ChildStdin,
    response_tx: Sender<Option<String>>,
    next_id: i64,
}

impl LspClient {
    fn start() -> Result<Self, LspError> {
        let mut child = Command::new("cargo")
            .args(["run", "-p", "workflow-lsp", "--bin", "flow-lsp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(LspError::StartFailed)?;

        let stdout = child.stdout.take().ok_or_else(|| {
            LspError::ReadFailed("failed to capture stdout".to_string())
        })?;
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
            response_tx,
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

    fn send_request<T: Serialize>(&mut self, id: i64, method: &str, params: T) -> Result<(), LspError> {
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

    fn recv_response(&self, id: i64) -> Result<Option<serde_json::Value>, LspError> {
        loop {
            match self.response_tx.recv() {
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

    fn open_document(&mut self, uri: &str, text: &str) -> Result<(), LspError> {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Url::parse(uri).unwrap_or_else(|_| Url::parse("file:///example.flow").unwrap()),
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

    fn change_document(&mut self, uri: &str, version: i32, text: &str) -> Result<(), LspError> {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: Url::parse(uri).unwrap_or_else(|_| Url::parse("file:///example.flow").unwrap()),
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

    fn completion(
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
                    uri: Url::parse(uri).unwrap_or_else(|_| Url::parse("file:///example.flow").unwrap()),
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

    fn hover(
        &mut self,
        uri: &str,
        line: usize,
        character: usize,
    ) -> Result<Option<String>, LspError> {
        let id = self.next_id;
        self.next_id += 1;

        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::parse(uri).unwrap_or_else(|_| Url::parse("file:///example.flow").unwrap()),
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
                    let hover: lsp_types::Hover = serde_json::from_value(value).unwrap_or(lsp_types::Hover {
                        contents: HoverContents::Scalar(lsp_types::MarkedString::String(String::new())),
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
}

#[derive(Clone)]
struct CompletionItemView {
    label: String,
    detail: Option<String>,
    insert_text: String,
}

struct EditorApp {
    text: String,
    lsp: Option<LspClient>,
    uri: String,
    version: i32,
    completions: Vec<CompletionItemView>,
    completion_visible: bool,
    completion_index: usize,
    completion_rect: Option<Rect>,
    hover_text: Option<String>,
    hover_rect: Option<Rect>,
    status: String,
    cursor_line: usize,
    cursor_col: usize,
}

impl Default for EditorApp {
    fn default() -> Self {
        let text = r#"workflow "Native Example" {
  on START
  var message = "Hello from native editor"
  log(message)
  if (data.plan == "premium") {
    log("Premium user")
  } else {
    log("Free user")
  }
  foreach (item in data.items) {
    log(item.name)
  }
}

fn double(x) {
  return x * 2
}"#
        .to_string();

        let mut app = Self {
            text,
            lsp: None,
            uri: "file:///example.flow".to_string(),
            version: 1,
            completions: Vec::new(),
            completion_visible: false,
            completion_index: 0,
            completion_rect: None,
            hover_text: None,
            hover_rect: None,
            status: "Starting LSP...".to_string(),
            cursor_line: 1,
            cursor_col: 1,
        };

        match LspClient::start() {
            Ok(mut lsp) => {
                let _ = lsp.open_document(&app.uri, &app.text);
                app.lsp = Some(lsp);
                app.status = "LSP ready".to_string();
            }
            Err(err) => {
                app.status = format!("LSP unavailable: {err}");
            }
        }

        app
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Flow Native Editor").font(FontId::proportional(16.0)));
                ui.separator();
                ui.label(format!("Ln {}, Col {}", self.cursor_line, self.cursor_col));
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    if ui.button("Clear").clicked() {
                        self.text.clear();
                        self.update_lsp_document();
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_editor(ui);
        });

        if self.completion_visible {
            self.render_completion_popup(ctx);
        }

        if let Some(hover) = &self.hover_text {
            if let Some(rect) = self.hover_rect {
                self.render_hover_popup(ctx, rect, hover);
            }
        }
    }
}

impl EditorApp {
    fn render_editor(&mut self, ui: &mut Ui) {
        let available = ui.available_size_before_wrap();
        let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click());

        ScrollArea::vertical()
            .horizontal_scroll(true)
            .auto_shrink([false; 2])
            .show_viewport(ui, |ui, viewport| {
                let mut painter = ui.painter();
                let mut cursor_pos = None;
                let mut hover_pos = None;

                let lines: Vec<&str> = self.text.split('\n').collect();
                let visible_start = viewport.min.y as usize;
                let visible_end = ((viewport.max.y - viewport.min.y) / LINE_HEIGHT + 2.0) as usize;

                for (line_idx, line) in lines.iter().enumerate() {
                    if line_idx < visible_start.saturating_sub(2) || line_idx > visible_start + visible_end {
                        continue;
                    }

                    let y = line_idx as f32 * LINE_HEIGHT - viewport.min.y + 10.0;
                    let x = 48.0 - viewport.min.x;

                    // Line number
                    painter.text(
                        Pos2::new(8.0, y + LINE_HEIGHT * 0.75),
                        egui::Align2::LEFT_TOP,
                        format!("{:3}", line_idx + 1),
                        FontId::monospace(FONT_SIZE),
                        Color32::from_gray(100),
                    );

                    // Highlighted text
                    let tokens = tokenize_line(line);
                    let mut x_offset = x;
                    for token in tokens {
                        let color = token_color(&token.kind);
                        painter.text(
                            Pos2::new(x_offset, y + LINE_HEIGHT * 0.75),
                            egui::Align2::LEFT_TOP,
                            &token.text,
                            FontId::monospace(FONT_SIZE),
                            color,
                        );
                        x_offset += token.text.len() as f32 * CHAR_WIDTH;
                    }

                    // Cursor
                    if line_idx == self.cursor_line.saturating_sub(1) {
                        let cursor_x = x + self.cursor_col.saturating_sub(1) as f32 * CHAR_WIDTH;
                        let cursor_y = y;
                        painter.line_segment(
                            [Pos2::new(cursor_x, cursor_y), Pos2::new(cursor_x, cursor_y + LINE_HEIGHT - 2.0)],
                            egui::Stroke::new(2.0, Color32::WHITE),
                        );
                        cursor_pos = Some(Pos2::new(cursor_x - viewport.min.x, cursor_y - viewport.min.y));
                    }
                }

                // Mouse hover detection
                if let Some(pos) = response.hover_pos() {
                    let local_x = pos.x + viewport.min.x;
                    let local_y = pos.y + viewport.min.y;
                    let line_idx = ((local_y - 10.0) / LINE_HEIGHT).round() as usize;
                    let col_idx = ((local_x - 48.0) / CHAR_WIDTH).round() as usize;

                    if line_idx < lines.len() {
                        hover_pos = Some((line_idx, col_idx));
                    }
                }

                if let Some((line_idx, col_idx)) = hover_pos {
                    if let Some(lsp) = &mut self.lsp {
                        if let Ok(Some(hover)) = lsp.hover(&self.uri, line_idx, col_idx) {
                            self.hover_text = Some(hover);
                            self.hover_rect = Some(Rect::from_min_max(
                                Pos2::new(local_x_for_col(col_idx, viewport), local_y_for_line(line_idx, viewport)),
                                Pos2::new(local_x_for_col(col_idx, viewport) + 200.0, local_y_for_line(line_idx, viewport) + 80.0),
                            ));
                        }
                    }
                }
            });
    }

    fn render_completion_popup(&mut self, ctx: &egui::Context) {
        if let Some(rect) = self.completion_rect {
            let width = 280.0;
            let height = (self.completions.len() as f32 * 24.0).min(200.0);
            let popup_rect = Rect::from_min_size(rect.min, Vec2::new(width, height));

            egui::Window::new("Completions")
                .fixed_pos(popup_rect.min)
                .fixed_size(Vec2::new(width, height))
                .resizable(false)
                .collapsible(false)
                .title_bar(false)
                .show(ctx, |ui| {
                    ScrollArea::vertical().show(ui, |ui| {
                        for (idx, item) in self.completions.iter().enumerate() {
                            let selected = idx == self.completion_index;
                            let bg = if selected {
                                Color32::from_rgb(40, 80, 140)
                            } else {
                                Color32::TRANSPARENT
                            };

                            let response = ui
                                .allocate_rect(
                                    Rect::from_min_size(ui.cursor().min, Vec2::new(width - 8.0, 24.0)),
                                    egui::Sense::click(),
                                );

                            if response.hovered() || selected {
                                ui.painter()
                                    .rect_filled(response.rect, 2.0, bg);
                            }

                            ui.label(RichText::new(&item.label).monospace());
                            if let Some(detail) = &item.detail {
                                ui.label(RichText::new(detail).size(10.0).color(Color32::GRAY));
                            }

                            if response.clicked() {
                                self.insert_completion(idx);
                            }
                        }
                    });
                });
        }
    }

    fn render_hover_popup(&mut self, ctx: &egui::Context, _rect: Rect, hover: &str) {
        egui::Window::new("Hover")
            .fixed_size(Vec2::new(240.0, 100.0))
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(RichText::new(hover).monospace());
            });
    }

    fn insert_completion(&mut self, idx: usize) {
        if idx >= self.completions.len() {
            return;
        }

        let item = &self.completions[idx];
        let cursor_pos = self.text.len();
        let before = &self.text[..cursor_pos];

        let word_start = before
            .char_indices()
            .rev()
            .find(|(_, ch)| !ch.is_alphanumeric() && *ch != '_' && *ch != '.')
            .map(|(idx, _)| idx + 1)
            .unwrap_or(0);

        let dot_match = before[word_start..].rfind('.');
        let insert_start = if dot_match.is_some() {
            word_start + dot_match.unwrap() + 1
        } else {
            word_start
        };

        let after = &self.text[cursor_pos..];
        self.text.replace_range(insert_start..cursor_pos, &item.insert_text);
        self.completion_visible = false;
        self.update_lsp_document();
    }

    fn update_lsp_document(&mut self) {
        self.version += 1;
        if let Some(lsp) = &mut self.lsp {
            let _ = lsp.change_document(&self.uri, self.version, &self.text);
        }
    }
}

#[derive(Clone)]
struct Token {
    text: String,
    kind: TokenKind,
}

#[derive(Clone, Copy)]
enum TokenKind {
    Keyword,
    String,
    Number,
    Comment,
    Function,
    Operator,
    Punctuation,
    Variable,
}

fn tokenize_line(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let bytes = line.as_bytes();

    while i < bytes.len() {
        let ch = bytes[i] as char;

        if ch == '/' && i + 1 < bytes.len() && bytes[i + 1] as char == '/' {
            tokens.push(Token {
                text: line[i..].to_string(),
                kind: TokenKind::Comment,
            });
            break;
        }

        if ch == '"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] as char == '"' {
                    i += 1;
                    break;
                }
                if bytes[i] as char == '\\' {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            tokens.push(Token {
                text: line[start..i].to_string(),
                kind: TokenKind::String,
            });
            continue;
        }

        if ch.is_ascii_digit() || (ch == '-' && i + 1 < bytes.len() && bytes[i + 1] as char is_ascii_digit) {
            let start = i;
            if ch == '-' {
                i += 1;
            }
            while i < bytes.len() && bytes[i] as char is_ascii_digit {
                i += 1;
            }
            if i < bytes.len() && bytes[i] as char == '.' {
                i += 1;
                while i < bytes.len() && bytes[i] as char is_ascii_digit {
                    i += 1;
                }
            }
            tokens.push(Token {
                text: line[start..i].to_string(),
                kind: TokenKind::Number,
            });
            continue;
        }

        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i] as char).is_ascii_alphanumeric() || bytes[i] as char == '_' {
                i += 1;
            }
            let word = &line[start..i];
            let kind = match word {
                "workflow" | "fn" | "var" | "if" | "else" | "foreach" | "in" | "on" | "return"
                | "true" | "false" | "null" | "import" | "from" | "emit" => TokenKind::Keyword,
                "log" | "len" | "to_string" | "to_number" => TokenKind::Function,
                _ => TokenKind::Variable,
            };
            tokens.push(Token {
                text: word.to_string(),
                kind,
            });
            continue;
        }

        if "+-*/%=<>!&|".contains(ch) {
            let start = i;
            if i + 1 < bytes.len() {
                let two = &line[i..i + 2];
                if matches!(two, "==" | "!=" | "<=" | ">=" | "&&" | "||") {
                    i += 2;
                    tokens.push(Token {
                        text: two.to_string(),
                        kind: TokenKind::Operator,
                    });
                    continue;
                }
            }
            i += 1;
            tokens.push(Token {
                text: line[start..i].to_string(),
                kind: TokenKind::Operator,
            });
            continue;
        }

        if "(){}[],.".contains(ch) {
            tokens.push(Token {
                text: ch.to_string(),
                kind: TokenKind::Punctuation,
            });
            i += 1;
            continue;
        }

        tokens.push(Token {
            text: ch.to_string(),
            kind: TokenKind::Variable,
        });
        i += 1;
    }

    tokens
}

fn token_color(kind: TokenKind) -> Color32 {
    match kind {
        TokenKind::Keyword => Color32::from_rgb(200, 120, 255),
        TokenKind::String => Color32::from_rgb(180, 220, 120),
        TokenKind::Number => Color32::from_rgb(255, 180, 100),
        TokenKind::Comment => Color32::from_gray(100),
        TokenKind::Function => Color32::from_rgb(100, 200, 255),
        TokenKind::Operator => Color32::from_rgb(255, 120, 120),
        TokenKind::Punctuation => Color32::from_gray(180),
        TokenKind::Variable => Color32::WHITE,
    }
}

fn local_x_for_col(col: usize, viewport: Rect) -> f32 {
    48.0 + col as f32 * CHAR_WIDTH - viewport.min.x
}

fn local_y_for_line(line: usize, viewport: Rect) -> f32 {
    line as f32 * LINE_HEIGHT - viewport.min.y + 10.0
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Flow Native Editor",
        native_options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(EditorApp::default()))
        }),
    )
}
