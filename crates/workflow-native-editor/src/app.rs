use super::highlight::{token_color, tokenize_line};
use super::lsp::LspClient;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, ScrollArea, Ui, Vec2};
use lsp_types::CompletionItem;

const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 20.0;
const CHAR_WIDTH: f32 = 8.4;

#[derive(Clone)]
pub struct CompletionItemView {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
}

pub struct EditorApp {
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
            self.handle_keyboard_input(ctx);
            self.render_editor(ui);
        });

        if self.completion_visible {
            self.render_completion_popup(ctx);
        }

        if let (Some(hover), Some(rect)) = (self.hover_text.clone(), self.hover_rect) {
            self.render_hover_popup(ctx, rect, &hover);
        }
    }
}

impl EditorApp {
    pub fn handle_keyboard_input(&mut self, ctx: &egui::Context) {
        if let Some(text) = ctx.input(|i| {
            i.events.iter().find_map(|event| {
                if let egui::Event::Text(text) = event {
                    Some(text.clone())
                } else {
                    None
                }
            })
        }) {
            self.insert_text(&text);
        }

        ctx.input(|i| {
            for event in &i.events {
                match event {
                    egui::Event::Key {
                        key: egui::Key::ArrowDown,
                        pressed: true,
                        modifiers,
                        ..
                    } if self.completion_visible && modifiers.ctrl => {
                        self.completion_index = (self.completion_index + 1)
                            .min(self.completions.len().saturating_sub(1));
                    }
                    egui::Event::Key {
                        key: egui::Key::ArrowUp,
                        pressed: true,
                        modifiers,
                        ..
                    } if self.completion_visible && modifiers.ctrl => {
                        self.completion_index = self.completion_index.saturating_sub(1);
                    }
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        modifiers,
                        ..
                    } if self.completion_visible && modifiers.ctrl => {
                        self.insert_completion(self.completion_index);
                    }
                    egui::Event::Key {
                        key: egui::Key::ArrowLeft,
                        pressed: true,
                        ..
                    } => self.move_cursor_left(),
                    egui::Event::Key {
                        key: egui::Key::ArrowRight,
                        pressed: true,
                        ..
                    } => self.move_cursor_right(),
                    egui::Event::Key {
                        key: egui::Key::ArrowUp,
                        pressed: true,
                        ..
                    } => self.move_cursor_up(),
                    egui::Event::Key {
                        key: egui::Key::ArrowDown,
                        pressed: true,
                        ..
                    } => self.move_cursor_down(),
                    egui::Event::Key {
                        key: egui::Key::Backspace,
                        pressed: true,
                        ..
                    } => self.delete_before_cursor(),
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        ..
                    } => self.insert_text("\n"),
                    egui::Event::Key {
                        key: egui::Key::Tab,
                        pressed: true,
                        ..
                    } => self.insert_text("  "),
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        ..
                    } => self.completion_visible = false,
                    _ => {}
                }
            }
        });
    }

    fn move_cursor_left(&mut self) {
        if self.cursor_col > 1 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 1 {
            self.cursor_line -= 1;
            let line = self
                .text
                .split('\n')
                .nth(self.cursor_line - 1)
                .unwrap_or("")
                .chars()
                .count();
            self.cursor_col = line + 1;
        }
    }

    fn move_cursor_right(&mut self) {
        let lines: Vec<&str> = self.text.split('\n').collect();
        if self.cursor_line <= lines.len() {
            let line_len = lines[self.cursor_line - 1].chars().count() + 1;
            if self.cursor_col < line_len {
                self.cursor_col += 1;
            } else if self.cursor_line < lines.len() {
                self.cursor_line += 1;
                self.cursor_col = 1;
            }
        }
    }

    fn move_cursor_up(&mut self) {
        if self.cursor_line > 1 {
            self.cursor_line -= 1;
        }
    }

    fn move_cursor_down(&mut self) {
        let line_count = self.text.split('\n').count();
        if self.cursor_line < line_count {
            self.cursor_line += 1;
        }
    }

    fn delete_before_cursor(&mut self) {
        if self.cursor_col > 1 {
            let pos = self.cursor_offset();
            if pos > 0 {
                self.text.remove(pos - 1);
                self.cursor_col -= 1;
                self.update_lsp_document();
            }
        } else if self.cursor_line > 1 {
            let pos = self.cursor_offset();
            if pos > 0 {
                self.text.remove(pos - 1);
                self.cursor_line -= 1;
                let line = self
                    .text
                    .split('\n')
                    .nth(self.cursor_line - 1)
                    .unwrap_or("")
                    .chars()
                    .count();
                self.cursor_col = line + 1;
                self.update_lsp_document();
            }
        }
    }

    fn insert_text(&mut self, text: &str) {
        let pos = self.cursor_offset();
        self.text.insert_str(pos, text);
        self.update_cursor_after_insert(text);
        self.update_lsp_document();
    }

    fn update_cursor_after_insert(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' {
                self.cursor_line += 1;
                self.cursor_col = 1;
            } else {
                self.cursor_col += 1;
            }
        }
    }

    fn cursor_offset(&self) -> usize {
        let mut offset = 0;
        for (idx, line) in self.text.split('\n').enumerate() {
            if idx + 1 == self.cursor_line {
                break;
            }
            offset += line.chars().count() + 1;
        }
        offset + self.cursor_col.saturating_sub(1)
    }

    fn render_editor(&mut self, ui: &mut Ui) {
        let available = ui.available_size_before_wrap();
        let (_rect, response) = ui.allocate_exact_size(available, egui::Sense::click());

        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_viewport(ui, |ui, viewport| {
                let painter = ui.painter();
                let mut cursor_pos = None;
                let mut hover_pos = None;

                let lines: Vec<&str> = self.text.split('\n').collect();
                let visible_start = viewport.min.y as usize;
                let visible_end = ((viewport.max.y - viewport.min.y) / LINE_HEIGHT + 2.0) as usize;

                for (line_idx, line) in lines.iter().enumerate() {
                    if line_idx < visible_start.saturating_sub(2)
                        || line_idx > visible_start + visible_end
                    {
                        continue;
                    }

                    let y = line_idx as f32 * LINE_HEIGHT - viewport.min.y + 10.0;
                    let x = 48.0 - viewport.min.x;

                    painter.text(
                        Pos2::new(8.0, y + LINE_HEIGHT * 0.75),
                        egui::Align2::LEFT_TOP,
                        format!("{:3}", line_idx + 1),
                        FontId::monospace(FONT_SIZE),
                        Color32::from_gray(100),
                    );

                    let tokens = tokenize_line(line);
                    let mut x_offset = x;
                    for token in tokens {
                        let color = token_color(token.kind);
                        painter.text(
                            Pos2::new(x_offset, y + LINE_HEIGHT * 0.75),
                            egui::Align2::LEFT_TOP,
                            &token.text,
                            FontId::monospace(FONT_SIZE),
                            color,
                        );
                        x_offset += token.text.len() as f32 * CHAR_WIDTH;
                    }

                    if line_idx == self.cursor_line.saturating_sub(1) {
                        let cursor_x = x + self.cursor_col.saturating_sub(1) as f32 * CHAR_WIDTH;
                        let cursor_y = y;
                        painter.line_segment(
                            [
                                Pos2::new(cursor_x, cursor_y),
                                Pos2::new(cursor_x, cursor_y + LINE_HEIGHT - 2.0),
                            ],
                            egui::Stroke::new(2.0, Color32::WHITE),
                        );
                        cursor_pos = Some(Pos2::new(
                            cursor_x - viewport.min.x,
                            cursor_y - viewport.min.y,
                        ));
                    }
                }

                if let Some(pos) = response.hover_pos() {
                    let local_x = pos.x + viewport.min.x;
                    let local_y = pos.y + viewport.min.y;
                    let line_idx = ((local_y - 10.0) / LINE_HEIGHT).round() as usize;
                    let col_idx = ((local_x - 48.0) / CHAR_WIDTH).round() as usize;

                    if line_idx < lines.len() {
                        hover_pos = Some((line_idx, col_idx, local_x, local_y));
                    }
                }

                if let Some((line_idx, col_idx, local_x, local_y)) = hover_pos {
                    if let Some(lsp) = &mut self.lsp {
                        if let Ok(Some(hover)) = lsp.hover(&self.uri, line_idx, col_idx) {
                            self.hover_text = Some(hover);
                            self.hover_rect = Some(Rect::from_min_max(
                                Pos2::new(local_x - viewport.min.x, local_y - viewport.min.y),
                                Pos2::new(
                                    local_x - viewport.min.x + 200.0,
                                    local_y - viewport.min.y + 80.0,
                                ),
                            ));
                        }
                    }
                }

                if cursor_pos.is_some() {
                    if let Some(lsp) = &mut self.lsp {
                        if let Ok(items) = lsp.completion(
                            &self.uri,
                            self.cursor_line.saturating_sub(1),
                            self.cursor_col.saturating_sub(1),
                        ) {
                            self.completions =
                                items.into_iter().map(completion_item_view).collect();
                            self.completion_visible = !self.completions.is_empty();
                            self.completion_index = 0;
                            if let Some(pos) = cursor_pos {
                                self.completion_rect =
                                    Some(Rect::from_min_size(pos, Vec2::new(280.0, 200.0)));
                            }
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
                    let mut clicked_idx = None;
                    ScrollArea::vertical().show(ui, |ui| {
                        for (idx, item) in self.completions.iter().enumerate() {
                            let selected = idx == self.completion_index;
                            let bg = if selected {
                                Color32::from_rgb(40, 80, 140)
                            } else {
                                Color32::TRANSPARENT
                            };

                            let response = ui.allocate_rect(
                                Rect::from_min_size(ui.cursor().min, Vec2::new(width - 8.0, 24.0)),
                                egui::Sense::click(),
                            );

                            if response.hovered() || selected {
                                ui.painter().rect_filled(response.rect, 2.0, bg);
                            }

                            ui.label(RichText::new(&item.label).monospace());
                            if let Some(detail) = &item.detail {
                                ui.label(RichText::new(detail).size(10.0).color(Color32::GRAY));
                            }

                            if response.clicked() {
                                clicked_idx = Some(idx);
                            }
                        }
                    });
                    if let Some(idx) = clicked_idx {
                        self.insert_completion(idx);
                    }
                });
        }
    }

    fn render_hover_popup(&mut self, _ctx: &egui::Context, rect: Rect, hover: &str) {
        egui::Window::new("Hover")
            .fixed_pos(rect.min)
            .fixed_size(Vec2::new(240.0, 100.0))
            .resizable(false)
            .collapsible(false)
            .show(_ctx, |ui| {
                ui.label(RichText::new(hover).monospace());
            });
    }

    fn insert_completion(&mut self, idx: usize) {
        if idx >= self.completions.len() {
            return;
        }

        let item = self.completions[idx].clone();
        let cursor_pos = self.cursor_offset();
        let before = &self.text[..cursor_pos];

        let word_start = before
            .char_indices()
            .rev()
            .find(|(_, ch)| !ch.is_alphanumeric() && *ch != '_' && *ch != '.')
            .map(|(idx, _)| idx + 1)
            .unwrap_or(0);

        let dot_match = before[word_start..].rfind('.');
        let insert_start = if let Some(dot_idx) = dot_match {
            word_start + dot_idx + 1
        } else {
            word_start
        };

        self.text
            .replace_range(insert_start..cursor_pos, &item.insert_text);
        self.cursor_col = self.cursor_col.saturating_sub(cursor_pos - insert_start)
            + item.insert_text.chars().count();
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

fn completion_item_view(item: CompletionItem) -> CompletionItemView {
    CompletionItemView {
        label: item.label,
        detail: item.detail,
        insert_text: item.insert_text.unwrap_or_default(),
    }
}
