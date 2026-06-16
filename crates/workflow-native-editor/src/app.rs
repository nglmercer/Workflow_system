use super::highlight::{token_color, tokenize_line};
use eframe::egui::{
    self, text::LayoutJob, Color32, FontId, Pos2, Rect, RichText, ScrollArea, TextEdit, TextFormat,
    Ui, Vec2,
};
use std::sync::Arc;
use workflow_lsp::features::{Completion, CompletionKind};
use workflow_lsp::ServerState;

const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 20.0;
const COMPLETION_WIDTH: f32 = 280.0;
const COMPLETION_MAX_HEIGHT: f32 = 200.0;
const COMPLETION_ROW_HEIGHT: f32 = 24.0;

#[derive(Default, Clone, Copy)]
struct CursorPosition {
    line: usize,
    col: usize,
}

pub struct EditorApp {
    text: String,
    lsp: ServerState,
    uri: String,
    completions: Vec<Completion>,
    completion_visible: bool,
    completion_index: usize,
    status: String,
    cursor: CursorPosition,
    hover_text: Option<String>,
    hover_pos: Option<Pos2>,
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

        let mut lsp = ServerState::new();
        let uri = "file:///example.flow".to_string();
        lsp.update_document(&uri, &text);

        Self {
            text,
            lsp,
            uri,
            completions: Vec::new(),
            completion_visible: false,
            completion_index: 0,
            status: "Ready".to_string(),
            cursor: CursorPosition { line: 1, col: 1 },
            hover_text: None,
            hover_pos: None,
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Flow Native Editor").font(FontId::proportional(16.0)));
                ui.separator();
                ui.label(format!("Ln {}, Col {}", self.cursor.line, self.cursor.col));
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    if ui.button("Clear").clicked() {
                        self.text.clear();
                        self.lsp.update_document(&self.uri, &self.text);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_editor(ui);
        });

        if self.completion_visible && !self.completions.is_empty() {
            self.render_completion_popup(ctx);
        }

        if let (Some(text), Some(pos)) = (self.hover_text.clone(), self.hover_pos) {
            self.render_hover_popup(ctx, pos, &text);
        }
    }
}

impl EditorApp {
    fn render_editor(&mut self, ui: &mut Ui) {
        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let mut text = std::mem::take(&mut self.text);
                let output = TextEdit::multiline(&mut text)
                    .font(FontId::monospace(FONT_SIZE))
                    .desired_width(f32::INFINITY)
                    .layouter(&mut |ui, t, wrap_width| layout_flow(ui, t, wrap_width))
                    .show(ui);
                self.text = text;

                let response = output.response;
                let galley = output.galley;

                if response.changed() {
                    self.lsp.update_document(&self.uri, &self.text);
                }

                if let Some(range) = &output.cursor_range {
                    let primary = range.primary;
                    let line = primary.rcursor.row + 1;
                    let col = primary.rcursor.column + 1;
                    if line != self.cursor.line || col != self.cursor.col {
                        self.cursor = CursorPosition { line, col };
                    }
                }

                if response.changed() && should_request_completion(ui, &self.text, self.cursor) {
                    self.request_completion();
                } else if response.changed() {
                    self.completion_visible = false;
                }

                self.update_hover(response.rect, &galley, response.hover_pos());
            });
    }

    fn request_completion(&mut self) {
        let items = workflow_lsp::features::completions_at(
            &self.lsp,
            &self.uri,
            self.cursor.line - 1,
            self.cursor.col - 1,
        );
        self.completions = items;
        self.completion_visible = !self.completions.is_empty();
        if self.completion_visible && self.completion_index >= self.completions.len() {
            self.completion_index = 0;
        }
    }

    fn update_hover(&mut self, rect: Rect, galley: &Arc<egui::Galley>, hover_pos: Option<Pos2>) {
        let pos = match hover_pos {
            Some(p) => p,
            None => {
                self.hover_text = None;
                self.hover_pos = None;
                return;
            }
        };
        if !rect.contains(pos) {
            self.hover_text = None;
            self.hover_pos = None;
            return;
        }
        let local = pos - rect.min;
        let mut line_idx = 0usize;
        let mut best_y = f32::NEG_INFINITY;
        for (idx, row) in galley.rows.iter().enumerate() {
            let y = row.rect.min.y;
            if y <= local.y && y >= best_y {
                best_y = y;
                line_idx = idx;
            }
        }
        if line_idx >= galley.rows.len() {
            self.hover_text = None;
            self.hover_pos = None;
            return;
        }
        let row = &galley.rows[line_idx];
        let row_min_x = row.rect.min.x;
        let col = row
            .glyphs
            .iter()
            .take_while(|g| g.pos.x + g.size.x - row_min_x <= local.x)
            .count();

        if let Some(text) =
            workflow_lsp::features::hover_at(&self.lsp, &self.uri, line_idx, col)
        {
            self.hover_text = Some(text);
            self.hover_pos = Some(pos);
        } else {
            self.hover_text = None;
            self.hover_pos = None;
        }
    }

    fn render_completion_popup(&mut self, ctx: &egui::Context) {
        if self.completions.is_empty() {
            return;
        }
        let height =
            (self.completions.len() as f32 * COMPLETION_ROW_HEIGHT).min(COMPLETION_MAX_HEIGHT);

        let area = ctx.available_rect();
        let popup_rect = Rect::from_min_size(
            Pos2::new(area.min.x + 16.0, area.max.y - height - 16.0),
            Vec2::new(COMPLETION_WIDTH, height),
        );

        let mut clicked_idx = None;
        let current_index = self.completion_index;
        let max_index = self.completions.len().saturating_sub(1);

        egui::Window::new("Completions")
            .fixed_pos(popup_rect.min)
            .fixed_size(Vec2::new(COMPLETION_WIDTH, height))
            .resizable(false)
            .collapsible(false)
            .title_bar(false)
            .show(ctx, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    for (idx, item) in self.completions.iter().enumerate() {
                        let selected = idx == current_index;
                        let bg = if selected {
                            Color32::from_rgb(40, 80, 140)
                        } else {
                            Color32::TRANSPARENT
                        };
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(COMPLETION_WIDTH - 8.0, COMPLETION_ROW_HEIGHT),
                            egui::Sense::click(),
                        );
                        if response.hovered() || selected {
                            ui.painter().rect_filled(rect, 2.0, bg);
                        }
                        let color = color_for_kind(item.kind);
                        ui.painter().text(
                            rect.min + Vec2::new(6.0, 4.0),
                            egui::Align2::LEFT_TOP,
                            &item.label,
                            FontId::monospace(13.0),
                            color,
                        );
                        if let Some(detail) = &item.detail {
                            ui.painter().text(
                                rect.min + Vec2::new(160.0, 6.0),
                                egui::Align2::LEFT_TOP,
                                detail,
                                FontId::proportional(11.0),
                                Color32::GRAY,
                            );
                        }
                        if response.clicked() {
                            clicked_idx = Some(idx);
                        }
                    }
                });
            });

        ctx.input(|i| {
            for event in &i.events {
                match event {
                    egui::Event::Key {
                        key: egui::Key::ArrowDown,
                        pressed: true,
                        ..
                    } if self.completion_visible => {
                        self.completion_index = (self.completion_index + 1).min(max_index);
                    }
                    egui::Event::Key {
                        key: egui::Key::ArrowUp,
                        pressed: true,
                        ..
                    } if self.completion_visible => {
                        self.completion_index = self.completion_index.saturating_sub(1);
                    }
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        ..
                    } if self.completion_visible => {
                        self.insert_completion(self.completion_index);
                    }
                    _ => {}
                }
            }
        });

        if let Some(idx) = clicked_idx {
            self.insert_completion(idx);
        }
    }

    fn render_hover_popup(&self, ctx: &egui::Context, pos: Pos2, text: &str) {
        egui::Window::new("Hover")
            .fixed_pos(pos + Vec2::new(12.0, 12.0))
            .fixed_size(Vec2::new(260.0, 100.0))
            .resizable(false)
            .collapsible(false)
            .title_bar(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(text)
                        .monospace()
                        .color(Color32::from_gray(220)),
                );
            });
    }

    fn insert_completion(&mut self, idx: usize) {
        if idx >= self.completions.len() {
            return;
        }
        let item = self.completions[idx].clone();
        let insert_text = item.insert_text.unwrap_or(item.label);

        let before = self.text.clone();
        let lines: Vec<&str> = before.split('\n').collect();
        let line_idx = self.cursor.line.saturating_sub(1);
        let col_idx = self.cursor.col.saturating_sub(1);
        let mut offset = 0usize;
        for (i, line) in lines.iter().enumerate() {
            if i == line_idx {
                offset += col_idx.min(line.chars().count());
                break;
            } else {
                offset += line.chars().count() + 1;
            }
        }
        let after = &self.text[offset..];
        let word_start = before[..offset]
            .char_indices()
            .rev()
            .find(|(_, ch)| !ch.is_alphanumeric() && *ch != '_' && *ch != '.')
            .map(|(i, _)| i + 1)
            .unwrap_or(0);
        let prefix = &self.text[..word_start];
        self.text = format!("{}{}{}", prefix, insert_text, after);
        self.completion_visible = false;
        self.lsp.update_document(&self.uri, &self.text);
    }
}

fn color_for_kind(kind: CompletionKind) -> Color32 {
    match kind {
        CompletionKind::Keyword => Color32::from_rgb(200, 120, 255),
        CompletionKind::Function => Color32::from_rgb(100, 200, 255),
        CompletionKind::Variable => Color32::from_rgb(220, 220, 220),
        CompletionKind::Value => Color32::from_rgb(180, 220, 120),
        CompletionKind::Property => Color32::from_rgb(255, 200, 100),
    }
}

fn layout_flow(ui: &egui::Ui, text: &str, wrap_width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.first_row_min_height = LINE_HEIGHT;

    let default_format = TextFormat {
        font_id: FontId::monospace(FONT_SIZE),
        color: Color32::from_gray(220),
        ..Default::default()
    };

    let lines: Vec<&str> = text.split('\n').collect();
    for (line_idx, raw_line) in lines.iter().enumerate() {
        let tokens = tokenize_line(raw_line);
        if tokens.is_empty() {
            job.append("", 0.0, default_format.clone());
        } else {
            for token in tokens {
                let mut format = default_format.clone();
                format.color = token_color(token.kind);
                job.append(&token.text, 0.0, format);
            }
        }
        if line_idx + 1 < lines.len() {
            job.append("\n", 0.0, default_format.clone());
        }
    }

    ui.fonts(|f| f.layout_job(job))
}

fn should_request_completion(ui: &Ui, text: &str, cursor: CursorPosition) -> bool {
    let line = match text.lines().nth(cursor.line - 1) {
        Some(l) => l,
        None => return false,
    };
    let col = cursor.col.saturating_sub(1).min(line.len());
    let bytes = line.as_bytes();

    if col > 0 {
        let prev = bytes[col - 1] as char;
        if prev.is_ascii_alphanumeric() || prev == '_' {
            return true;
        }
        if prev == '.' {
            return true;
        }
    }

    ui.input(|i| {
        i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::Space,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.ctrl || modifiers.command
            )
        })
    })
}
