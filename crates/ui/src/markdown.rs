//! Renderizado básico de Markdown con widgets de `egui`.
//!
//! Cubre lo que suele devolver un modelo: encabezados, párrafos, énfasis, código
//! en línea, listas, citas, reglas y enlaces. Los bloques de código se dibujan
//! aparte, como tarjetas descargables, así que aquí se trabaja sobre la prosa.

use eframe::egui::{self, Color32, FontId, TextStyle};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Dibuja un texto Markdown dentro de la interfaz.
pub fn render(ui: &mut egui::Ui, text: &str, wrap_width: f32) {
    let styles = Styles::from(&*ui);
    let mut block = Block::new(&styles);
    let mut lists: Vec<ListState> = Vec::new();
    let mut quote_depth = 0usize;
    let mut in_code_block = false;

    for event in Parser::new_ext(text, Options::empty()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => block.level = Some(level),
            Event::End(TagEnd::Heading(_)) => flush(ui, &mut block, &styles, wrap_width),

            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => flush(ui, &mut block, &styles, wrap_width),

            Event::Start(Tag::List(start)) => lists.push(ListState::new(start)),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                flush(ui, &mut block, &styles, wrap_width);
            }
            Event::Start(Tag::Item) => {
                flush(ui, &mut block, &styles, wrap_width);
                let indent = "    ".repeat(lists.len().saturating_sub(1));
                let prefix = match lists.last_mut() {
                    Some(list) if list.ordered => {
                        let number = list.next;
                        list.next += 1;
                        format!("{indent}{number}. ")
                    }
                    _ => format!("{indent}• "),
                };
                block.prefix = Some(prefix);
            }
            Event::End(TagEnd::Item) => flush(ui, &mut block, &styles, wrap_width),

            Event::Start(Tag::BlockQuote(..)) => quote_depth += 1,
            Event::End(TagEnd::BlockQuote(..)) => {
                quote_depth = quote_depth.saturating_sub(1);
                flush(ui, &mut block, &styles, wrap_width);
            }

            Event::Start(Tag::Strong) => block.strong += 1,
            Event::End(TagEnd::Strong) => block.strong = block.strong.saturating_sub(1),
            Event::Start(Tag::Emphasis) => block.emphasis += 1,
            Event::End(TagEnd::Emphasis) => block.emphasis = block.emphasis.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. }) => block.link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => block.link = None,

            Event::Start(Tag::CodeBlock(_)) => {
                flush(ui, &mut block, &styles, wrap_width);
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => in_code_block = false,

            Event::Text(text) => {
                if !in_code_block {
                    block.push(&text, Inline::Plain, &styles, quote_depth);
                }
            }
            Event::Code(text) => block.push(&text, Inline::Code, &styles, quote_depth),
            Event::SoftBreak => block.push(" ", Inline::Plain, &styles, quote_depth),
            Event::HardBreak => block.push("\n", Inline::Plain, &styles, quote_depth),

            Event::Rule => {
                flush(ui, &mut block, &styles, wrap_width);
                ui.add_space(2.0);
                ui.separator();
                ui.add_space(2.0);
            }

            _ => {}
        }
    }

    flush(ui, &mut block, &styles, wrap_width);
}

/// Estado de una lista en curso.
struct ListState {
    ordered: bool,
    next: u64,
}

impl ListState {
    fn new(start: Option<u64>) -> Self {
        Self {
            ordered: start.is_some(),
            next: start.unwrap_or(1),
        }
    }
}

/// Cómo se resalta un fragmento dentro del texto.
#[derive(Clone, Copy)]
enum Inline {
    Plain,
    Code,
}

/// Tipografías y colores derivados del tema activo.
struct Styles {
    body: FontId,
    mono: FontId,
    headings: [FontId; 6],
    text: Color32,
    link: Color32,
    code_background: Color32,
    quote: Color32,
}

impl From<&egui::Ui> for Styles {
    fn from(ui: &egui::Ui) -> Self {
        let visuals = ui.visuals();
        let body = TextStyle::Body.resolve(ui.style());
        let mono = TextStyle::Monospace.resolve(ui.style());
        let heading = TextStyle::Heading.resolve(ui.style());

        Self {
            headings: [
                heading.clone(),
                heading.clone(),
                FontId::new(heading.size - 2.0, heading.family.clone()),
                FontId::new(heading.size - 3.0, heading.family.clone()),
                body.clone(),
                body.clone(),
            ],
            body,
            mono,
            text: visuals.text_color(),
            link: visuals.hyperlink_color,
            code_background: visuals.faint_bg_color,
            quote: visuals.weak_text_color(),
        }
    }
}

/// Bloque de texto en construcción.
struct Block {
    job: egui::text::LayoutJob,
    level: Option<HeadingLevel>,
    strong: usize,
    emphasis: usize,
    link: Option<String>,
    prefix: Option<String>,
    quote_depth: usize,
}

impl Block {
    fn new(styles: &Styles) -> Self {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        job.append(
            "",
            0.0,
            egui::TextFormat {
                font_id: styles.body.clone(),
                color: styles.text,
                ..Default::default()
            },
        );

        Self {
            job,
            level: None,
            strong: 0,
            emphasis: 0,
            link: None,
            prefix: None,
            quote_depth: 0,
        }
    }

    fn push(&mut self, text: &str, inline: Inline, styles: &Styles, quote_depth: usize) {
        if self.prefix.is_none() && self.quote_depth == 0 && quote_depth > 0 {
            // primer fragmento de una cita
        }
        self.quote_depth = quote_depth;

        let mut text = text.to_string();
        if let Some(prefix) = self.prefix.take() {
            text = format!("{prefix}{text}");
            self.push_span(&text, Inline::Plain, styles);
            return;
        }

        self.push_span(&text, inline, styles);
    }

    fn push_span(&mut self, text: &str, inline: Inline, styles: &Styles) {
        let font_id = if matches!(inline, Inline::Code) {
            styles.mono.clone()
        } else {
            match self.level {
                Some(level) => styles.headings[heading_index(level)].clone(),
                None => styles.body.clone(),
            }
        };

        let mut color = styles.text;
        if self.link.is_some() {
            color = styles.link;
        } else if self.quote_depth > 0 {
            color = styles.quote;
        }

        let background = if matches!(inline, Inline::Code) {
            styles.code_background
        } else {
            Color32::TRANSPARENT
        };

        let mut format = egui::TextFormat {
            font_id,
            color,
            background,
            ..Default::default()
        };
        if self.strong > 0 {
            format.font_id.size += 0.5;
        }
        if self.emphasis > 0 {
            format.italics = true;
        }
        if self.link.is_some() {
            format.underline = egui::Stroke::new(1.0, styles.link);
        }

        self.job.append(text, 0.0, format);
    }
}

fn heading_index(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 0,
        HeadingLevel::H2 => 1,
        HeadingLevel::H3 => 2,
        HeadingLevel::H4 => 3,
        HeadingLevel::H5 => 4,
        HeadingLevel::H6 => 5,
    }
}

fn flush(ui: &mut egui::Ui, block: &mut Block, styles: &Styles, wrap_width: f32) {
    let text = block.job.text.trim().to_string();
    if text.is_empty() {
        block.job = Block::new(styles).job;
        block.level = None;
        block.prefix = None;
        return;
    }

    let mut job = std::mem::replace(&mut block.job, Block::new(styles).job);
    job.wrap.max_width = wrap_width;
    let is_heading = block.level.is_some();
    block.level = None;
    block.prefix = None;

    ui.add(egui::Label::new(job).wrap());
    ui.add_space(if is_heading { 6.0 } else { 8.0 });
}
