//! The iced front-end.
//!
//! Rendering stays deliberately thin: [`Renderer`] produces [`Line`]s (the
//! equivalent of pi's styled terminal lines) and this module maps each
//! [`Span`] onto an iced `rich_text` span, one document-wide widget inside a
//! two-axis [`scrollable`]. That mirrors pi's own split between the markdown
//! component (`markdown.js`) and the TUI renderer.
//!
//! Three behaviours live here rather than in the renderer:
//!
//! * **Selection** — iced's `rich_text` has no selection support, so a
//!   [`mouse_area`] tracks press/move/release, maps pointer positions onto the
//!   document's character grid and the spans get a `background` highlight.
//! * **Smooth scrolling** — the same `mouse_area` captures wheel events (which
//!   otherwise scroll a fixed 60 px per notch) and a frame subscription eases
//!   the scrollable's offset towards a target with `scroll_to`.
//! * **Chrome** — line height, padding and the pi/Ghostty background.

use std::path::PathBuf;
use std::process::Command;

use iced::font::{Style as FontSlant, Weight};
use iced::keyboard::{self, key};
use iced::mouse;
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
    button, column, container, mouse_area, rich_text, row, scrollable,
    scrollable::{AbsoluteOffset, Scrollbar},
    text,
};
use iced::{
    clipboard, Color, Element, Font, Length, Padding, Point, Subscription, Task, Theme as IcedTheme,
};

use crate::md::Document;
use crate::render::{RenderOptions, Renderer};
use crate::text::{Line, Rgb, Span};
use crate::theme::Theme;

const DEFAULT_COLUMNS: usize = 100;
const MIN_COLUMNS: usize = 20;
const MAX_COLUMNS: usize = 400;
const DEFAULT_FONT_SIZE: f32 = 14.0;
/// Advance width of JetBrains Mono, in ems (it is exactly 0.6). Used only to
/// turn the window width into a column count for "fit" mode.
const MONOSPACE_ADVANCE: f32 = 0.6;
/// Line height factor. iced defaults to 1.3; a terminal is roughly 1.2-1.5, and
/// the renderer's table wrapping assumes a fixed cell height.
const LINE_HEIGHT_RATIO: f32 = 1.447;
/// Horizontal padding, in character cells (pi passes `paddingX = 2` for
/// assistant messages).
const PADDING_CELLS: usize = 2;
/// Vertical padding around the document, in logical pixels.
const PADDING_Y: f32 = 10.0;
/// Smooth-scroll easing applied per frame, and the snap distance in pixels.
const SCROLL_EASE: f32 = 0.3;
const SCROLL_SNAP: f32 = 0.5;
/// Id of the document scrollable, used to drive it programmatically.
const SCROLLABLE_ID: &str = "pi-mdview-document";

type IcedSpan<'a> = iced::widget::text::Span<'a, String, Font>;

/// State of the viewer application.
pub struct Viewer {
    path: Option<PathBuf>,
    source: String,
    document: Document,
    /// Rendered document: pi's `string[]` analogue.
    lines: Vec<Line>,
    /// Flat character offset of every rendered line (for selection).
    line_starts: Vec<usize>,
    /// The whole document as plain text (for copying).
    text: String,
    renderer: Renderer,
    theme: Theme,
    /// Document/UI font (pi's terminal font, i.e. JetBrains Mono by default).
    font: Font,
    columns: usize,
    fit: bool,
    window_width: f32,
    font_size: f32,
    status: String,

    // Selection
    selection: Option<(usize, usize)>,
    anchor: Option<usize>,
    selecting: bool,
    cursor: Option<Point>,

    // Smooth scrolling
    scroll_target: f32,
    scroll_displayed: f32,
    scroll_max: f32,
}

/// UI events.
#[derive(Debug, Clone)]
pub enum Message {
    Reload,
    Columns(usize),
    ToggleFit,
    ToggleTheme,
    FontSize(f32),
    Resized(f32),
    LinkClicked(String),
    /// Pointer moved over the document, in document coordinates.
    PointerMoved(Point),
    PointerPressed,
    PointerReleased,
    DoubleClicked,
    Wheel(mouse::ScrollDelta),
    /// Reported by the scrollable (scrollbar drags, keyboard, our own scrolls).
    Scrolled {
        offset: f32,
        content_height: f32,
        viewport_height: f32,
    },
    /// A rendered frame while a smooth scroll is in flight.
    Frame,
    KeyPressed(keyboard::Key, keyboard::Modifiers),
    Copy,
    SelectAll,
    ClearSelection,
}

impl Viewer {
    /// Creates the viewer, loading `path` when given (otherwise a built-in
    /// demo document is shown). `font` is the family used for the document and
    /// the UI (pass [`crate::fonts::monospace`] for pi's terminal font).
    pub fn new(path: Option<PathBuf>, theme: Theme, font: Font) -> Self {
        let (source, status) = match &path {
            Some(path) => match std::fs::read_to_string(path) {
                Ok(source) => (source, format!("loaded {}", path.display())),
                Err(error) => (
                    DEMO_MARKDOWN.to_string(),
                    format!("cannot read {}: {error}", path.display()),
                ),
            },
            None => (DEMO_MARKDOWN.to_string(), "demo document".to_string()),
        };

        let mut viewer = Self {
            path,
            document: Document::parse(&source.replace('\t', "   ")),
            source,
            lines: Vec::new(),
            line_starts: Vec::new(),
            text: String::new(),
            renderer: Renderer::new(theme).options(RenderOptions {
                // Padding and line height are applied by the container/rich_text
                // below, so the renderer output is exactly the document grid.
                padding_x: 0,
                padding_y: 0,
                pad_to_width: false,
                render_latex: true,
            }),
            theme,
            font,
            columns: DEFAULT_COLUMNS,
            fit: true,
            window_width: 1100.0,
            font_size: DEFAULT_FONT_SIZE,
            status,
            selection: None,
            anchor: None,
            selecting: false,
            cursor: None,
            scroll_target: 0.0,
            scroll_displayed: 0.0,
            scroll_max: 0.0,
        };
        viewer.rebuild();
        viewer
    }

    pub fn title(&self) -> String {
        match &self.path {
            Some(path) => format!("pi-mdview — {}", path.display()),
            None => "pi-mdview — markdown viewer".to_string(),
        }
    }

    pub fn iced_theme(&self) -> IcedTheme {
        if self.theme.dark {
            IcedTheme::Dark
        } else {
            IcedTheme::Light
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::window::resize_events().map(|(_id, size)| Message::Resized(size.width)),
            iced::keyboard::listen().filter_map(|event| match event {
                keyboard::Event::KeyPressed { key, modifiers, .. } => {
                    Some(Message::KeyPressed(key, modifiers))
                }
                _ => None,
            }),
        ];

        // Frames are only needed while a smooth scroll is in flight. Keeping
        // this conditional matters: iced requests a redraw for every message
        // that is processed, so an unconditional frame subscription would spin
        // the event loop at the refresh rate forever.
        if self.is_animating() {
            subscriptions.push(iced::window::frames().map(|_| Message::Frame));
        }

        Subscription::batch(subscriptions)
    }

    /// True while the smooth-scroll animation has not settled yet.
    fn is_animating(&self) -> bool {
        (self.scroll_target - self.scroll_displayed).abs() > SCROLL_SNAP
    }

    /// Re-renders the document (call after any change to width, theme or text).
    fn rebuild(&mut self) {
        let content_columns = self.content_columns();
        self.lines = self
            .renderer
            .render_document(&self.document, content_columns);

        // Cache the flat text and per-line offsets used by selection and copy.
        self.line_starts = Vec::with_capacity(self.lines.len());
        let mut text = String::new();
        for line in &self.lines {
            self.line_starts.push(text.chars().count());
            for span in &line.spans {
                text.push_str(&span.text);
            }
            text.push('\n');
        }
        self.text = text;
        self.selection = None;
        self.anchor = None;
    }

    fn reload(&mut self) {
        let Some(path) = self.path.clone() else {
            self.status = "no file to reload".to_string();
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(source) => {
                self.source = source;
                self.document = Document::parse(&self.source.replace('\t', "   "));
                self.rebuild();
                self.status = format!("reloaded {}", path.display());
            }
            Err(error) => {
                self.status = format!("cannot read {}: {error}", path.display());
            }
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Reload => self.reload(),
            Message::Columns(columns) => {
                let columns = columns.clamp(MIN_COLUMNS, MAX_COLUMNS);
                if columns != self.columns {
                    self.columns = columns;
                    self.fit = false;
                    self.rebuild();
                }
            }
            Message::ToggleFit => {
                self.fit = !self.fit;
                if self.fit {
                    self.columns = self.columns_for_window();
                    self.rebuild();
                }
            }
            Message::ToggleTheme => {
                self.theme = if self.theme.dark {
                    Theme::pi_light()
                } else {
                    Theme::pi_dark()
                };
                self.renderer.set_theme(self.theme);
                self.rebuild();
            }
            Message::FontSize(size) => {
                self.font_size = size.clamp(9.0, 28.0);
                if self.fit {
                    self.columns = self.columns_for_window();
                }
                self.rebuild();
            }
            Message::Resized(width) => {
                self.window_width = width;
                if self.fit {
                    let columns = self.columns_for_window();
                    if columns != self.columns {
                        self.columns = columns;
                        self.rebuild();
                    }
                }
            }
            Message::LinkClicked(url) => {
                self.status = match open_url(&url) {
                    Ok(()) => format!("opened {url}"),
                    Err(error) => format!("cannot open {url}: {error}"),
                };
            }

            // --- selection -------------------------------------------------
            Message::PointerMoved(position) => {
                self.cursor = Some(position);
                if self.selecting {
                    self.extend_selection(self.point_to_offset(position));
                }
            }
            Message::PointerPressed => {
                self.selecting = true;
                self.selection = None;
                self.anchor = self.cursor.map(|position| self.point_to_offset(position));
            }
            Message::PointerReleased => {
                self.selecting = false;
                self.anchor = None;
                if matches!(self.selection, Some((start, end)) if start == end) {
                    self.selection = None;
                }
            }
            Message::DoubleClicked => {
                if let Some(position) = self.cursor {
                    let offset = self.point_to_offset(position);
                    self.selection = Some(self.word_bounds(offset));
                }
            }
            Message::KeyPressed(key, modifiers) => match (modifiers.command(), key.as_ref()) {
                (true, keyboard::Key::Character("c")) => {
                    return self.copy_selection();
                }
                (true, keyboard::Key::Character("a")) => {
                    self.selection = Some((0, self.text.chars().count()));
                }
                (_, keyboard::Key::Named(key::Named::Escape)) => {
                    self.selection = None;
                }
                _ => {}
            },
            Message::Copy => return self.copy_selection(),
            Message::SelectAll => {
                self.selection = Some((0, self.text.chars().count()));
            }
            Message::ClearSelection => self.selection = None,

            // --- smooth scrolling -----------------------------------------
            Message::Wheel(delta) => {
                let y = match delta {
                    // One notch is one text line, whatever the wheel reports
                    // (X11 often sends 3 "lines" per notch).
                    mouse::ScrollDelta::Lines { y, .. } => y.clamp(-1.0, 1.0) * self.line_height(),
                    mouse::ScrollDelta::Pixels { y, .. } => y,
                };
                // iced's convention: positive y means scrolling up.
                let max = if self.scroll_max > 0.0 {
                    self.scroll_max
                } else {
                    f32::INFINITY
                };
                self.scroll_target = (self.scroll_target - y).clamp(0.0, max);
            }
            Message::Scrolled {
                offset,
                content_height,
                viewport_height,
            } => {
                self.scroll_max = (content_height - viewport_height).max(0.0);
                if (offset - self.scroll_displayed).abs() > 1.0 {
                    // Something else moved the viewport (scrollbar drag,
                    // keyboard). Adopt its position instead of fighting it.
                    self.scroll_displayed = offset;
                    self.scroll_target = offset;
                } else {
                    self.scroll_displayed = offset;
                }
            }
            Message::Frame => {
                let difference = self.scroll_target - self.scroll_displayed;
                if difference.abs() > SCROLL_SNAP {
                    self.scroll_displayed += difference * SCROLL_EASE;
                } else if difference != 0.0 {
                    self.scroll_displayed = self.scroll_target;
                } else {
                    return Task::none();
                }

                return iced::widget::operation::scroll_to(
                    SCROLLABLE_ID,
                    AbsoluteOffset {
                        x: None,
                        y: Some(self.scroll_displayed),
                    },
                );
            }
        }
        Task::none()
    }

    /// Number of columns handed to the renderer (total width minus padding).
    fn content_columns(&self) -> usize {
        self.columns.saturating_sub(PADDING_CELLS * 2).max(1)
    }

    fn columns_for_window(&self) -> usize {
        let advance = (self.font_size * MONOSPACE_ADVANCE).max(1.0);
        let usable = (self.window_width - 32.0).max(advance);
        ((usable / advance).floor() as usize).clamp(MIN_COLUMNS, MAX_COLUMNS)
    }

    fn cell_width(&self) -> f32 {
        self.font_size * MONOSPACE_ADVANCE
    }

    fn line_height(&self) -> f32 {
        self.font_size * LINE_HEIGHT_RATIO
    }

    /// Maps a pointer position (relative to the top-left of the text) to a
    /// character offset in the document.
    fn point_to_offset(&self, position: Point) -> usize {
        if self.line_starts.is_empty() {
            return 0;
        }
        let line = ((position.y.max(0.0)) / self.line_height()).floor() as usize;
        let line = line.min(self.lines.len().saturating_sub(1));
        let column = ((position.x.max(0.0)) / self.cell_width()).floor() as usize;
        self.line_starts[line] + self.char_offset_in_line(line, column)
    }

    /// Character offset of a visual column within one rendered line.
    fn char_offset_in_line(&self, line: usize, column: usize) -> usize {
        let mut cells = 0usize;
        let mut chars = 0usize;
        if let Some(rendered) = self.lines.get(line) {
            for span in &rendered.spans {
                for ch in span.text.chars() {
                    let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if cells + width > column {
                        return chars;
                    }
                    cells += width;
                    chars += 1;
                }
            }
        }
        chars
    }

    fn extend_selection(&mut self, head: usize) {
        let anchor = self.anchor.unwrap_or(head);
        self.selection = Some((anchor.min(head), anchor.max(head)));
    }

    fn word_bounds(&self, offset: usize) -> (usize, usize) {
        let chars: Vec<char> = self.text.chars().collect();
        if chars.is_empty() {
            return (0, 0);
        }
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let offset = offset.min(chars.len() - 1);
        if !is_word(chars[offset]) {
            return (offset, (offset + 1).min(chars.len()));
        }
        let mut start = offset;
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        let mut end = offset;
        while end < chars.len() && is_word(chars[end]) {
            end += 1;
        }
        (start, end)
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection?;
        if start >= end {
            return None;
        }
        Some(self.text.chars().skip(start).take(end - start).collect())
    }

    fn copy_selection(&mut self) -> Task<Message> {
        match self.selected_text() {
            Some(text) => {
                self.status = format!("copied {} characters", text.chars().count());
                clipboard::write(text)
            }
            None => {
                self.status = "nothing selected".to_string();
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let toolbar = row![
            button("Reload").on_press(Message::Reload),
            button("−").on_press(Message::Columns(self.columns.saturating_sub(4))),
            text(format!("{} cols", self.columns)).width(Length::Fixed(70.0)),
            button("+").on_press(Message::Columns(self.columns + 4)),
            button(if self.fit { "Fit: on" } else { "Fit: off" }).on_press(Message::ToggleFit),
            button(if self.theme.dark {
                "Theme: dark"
            } else {
                "Theme: light"
            })
            .on_press(Message::ToggleTheme),
            button("A−").on_press(Message::FontSize(self.font_size - 1.0)),
            button("A+").on_press(Message::FontSize(self.font_size + 1.0)),
            button("Copy").on_press(Message::Copy),
            text(&self.status).size(13.0),
        ]
        .spacing(6)
        .padding(8)
        .align_y(iced::Alignment::Center);

        let background = self.theme.background;
        let panel = self.theme.panel_background;
        let text_color = self.theme.text;

        let document = rich_text(self.build_spans())
            .font(self.font)
            .size(self.font_size)
            .line_height(LineHeight::Relative(LINE_HEIGHT_RATIO))
            .width(Length::Shrink)
            .wrapping(Wrapping::None)
            .color(rgb(text_color))
            .on_link_click(Message::LinkClicked);

        // The mouse area sits exactly on the text, so its local coordinates are
        // the document's character grid. It also captures wheel events, which
        // is what lets us scroll smoothly instead of iced's fixed 60 px notch.
        let area = mouse_area(document)
            .on_press(Message::PointerPressed)
            .on_release(Message::PointerReleased)
            .on_move(Message::PointerMoved)
            .on_double_click(Message::DoubleClicked)
            .on_scroll(Message::Wheel)
            .interaction(mouse::Interaction::Text);

        let horizontal_padding = self.cell_width() * PADDING_CELLS as f32;
        let padded = container(area)
            .padding(Padding {
                top: PADDING_Y,
                bottom: PADDING_Y,
                left: horizontal_padding,
                right: horizontal_padding,
            })
            .width(Length::Shrink);

        let body = scrollable(padded)
            .id(SCROLLABLE_ID)
            .direction(scrollable::Direction::Both {
                vertical: Scrollbar::new(),
                horizontal: Scrollbar::new(),
            })
            .on_scroll(|viewport| Message::Scrolled {
                offset: viewport.absolute_offset().y,
                content_height: viewport.content_bounds().height,
                viewport_height: viewport.bounds().height,
            })
            .width(Length::Fill)
            .height(Length::Fill);

        container(column![
            container(toolbar).style(move |_theme| container::Style {
                background: Some(rgb(panel).into()),
                ..container::Style::default()
            }),
            body
        ])
        .style(move |_theme| container::Style {
            background: Some(rgb(background).into()),
            text_color: Some(rgb(text_color)),
            ..container::Style::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Builds the document's iced spans, applying the selection highlight.
    fn build_spans(&self) -> Vec<IcedSpan<'_>> {
        let mut spans = Vec::with_capacity(self.lines.len() * 2);
        for (index, line) in self.lines.iter().enumerate() {
            self.push_line(&mut spans, line, self.line_starts[index]);
            if index + 1 < self.lines.len() {
                // `&'static str` fragment: no allocation per frame.
                spans.push(iced::widget::span::<String, Font>("\n"));
            }
        }
        spans
    }

    fn push_line<'a>(&'a self, spans: &mut Vec<IcedSpan<'a>>, line: &'a Line, line_start: usize) {
        let mut offset = line_start;
        for span in &line.spans {
            let text = span.text.as_str();
            let length = text.chars().count();
            let (selected_from, selected_to) = match self.selection {
                Some((start, end)) => (
                    start.saturating_sub(offset).min(length),
                    end.saturating_sub(offset).min(length),
                ),
                None => (length, length),
            };

            let from_byte = byte_offset(text, selected_from);
            let to_byte = byte_offset(text, selected_to);

            if selected_from > 0 {
                spans.push(self.styled_span(&text[..from_byte], span, false));
            }
            if selected_to > selected_from {
                spans.push(self.styled_span(&text[from_byte..to_byte], span, true));
            }
            if selected_to < length {
                spans.push(self.styled_span(&text[to_byte..], span, false));
            }

            offset += length;
        }
    }

    fn styled_span<'a>(&'a self, text: &'a str, span: &'a Span, selected: bool) -> IcedSpan<'a> {
        let mut out = iced::widget::span::<String, Font>(text);
        if selected {
            out = out.background(rgb(self.theme.selection_background));
        }
        if let Some(fg) = span.style.fg {
            out = out.color(rgb(fg));
        }
        if span.style.bold || span.style.italic {
            let mut font = self.font;
            if span.style.bold {
                font.weight = Weight::Bold;
            }
            if span.style.italic {
                font.style = FontSlant::Italic;
            }
            out = out.font(font);
        }
        if span.style.underline {
            out = out.underline(true);
        }
        if span.style.strikethrough {
            out = out.strikethrough(true);
        }
        if let Some(link) = &span.link {
            out = out.link(link.clone());
        }
        out
    }
}

/// Byte offset of the `index`-th character in `text` (or its length).
fn byte_offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

fn rgb(color: Rgb) -> Color {
    Color::from_rgb8(color.0, color.1, color.2)
}

/// Best-effort "open in browser" for clicked links.
fn open_url(url: &str) -> std::io::Result<()> {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(target_os = "windows") {
        ("cmd", vec!["/C", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    Command::new(program).args(args).spawn().map(|_| ())
}

const DEMO_MARKDOWN: &str = r#"# pi-mdview

This viewer renders markdown the way **pi** does: same block spacing, same
list markers, same box-drawing tables and the same theme colors.

## Features

- headings, **bold**, *italic*, ~~strikethrough~~ and `inline code`
- nested lists
  - with continuation indentation
- [x] task list items
- [ ] like this one

> Blockquotes keep pi's border and italic styling.
>
> > Nested quotes indent too.

| Feature | Status | Notes |
|---------|--------|-------|
| Tables | done | box-drawing borders |
| Wrapping | done | width-aware |
| Code | done | syntax highlighted |

```rust
fn main() {
    println!("rendered by pi-mdview");
}
```

Inline math such as $\alpha^2 + \beta$ is converted to Unicode.

---

Select text with the mouse (double-click selects a word, Ctrl/Cmd+C copies).
Pass a markdown file on the command line to view it:

```bash
cargo run -p pi-mdview -- README.md
```
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn viewer(source: &str, columns: usize) -> Viewer {
        let mut viewer = Viewer::new(None, Theme::pi_dark(), crate::fonts::monospace());
        viewer.source = source.to_string();
        viewer.document = Document::parse(&source.replace('\t', "   "));
        viewer.columns = columns;
        viewer.fit = false;
        viewer.rebuild();
        viewer
    }

    #[test]
    fn maps_pointer_to_line_starts() {
        let viewer = viewer("- alpha\n- beta\n", 40);
        assert_eq!(
            viewer.point_to_offset(Point::new(0.0, 0.0)),
            viewer.line_starts[0]
        );
        let second_line = viewer.point_to_offset(Point::new(0.0, viewer.line_height() * 1.5));
        assert_eq!(second_line, viewer.line_starts[1]);
    }

    #[test]
    fn maps_columns_to_characters() {
        let viewer = viewer("hello world", 40);
        let cell = viewer.cell_width();
        assert_eq!(viewer.point_to_offset(Point::new(0.0, 0.0)), 0);
        assert_eq!(
            viewer.point_to_offset(Point::new(cell * 6.5, 0.0)),
            6,
            "the 6th cell is the 'w' of world"
        );
        assert_eq!(
            viewer.point_to_offset(Point::new(cell * 500.0, 0.0)),
            "hello world".chars().count(),
            "past the end of a line clamps to its end"
        );
    }

    #[test]
    fn wide_characters_occupy_two_cells() {
        let viewer = viewer("日本語 test", 40);
        let cell = viewer.cell_width();
        assert_eq!(viewer.point_to_offset(Point::new(cell * 0.5, 0.0)), 0);
        assert_eq!(viewer.point_to_offset(Point::new(cell * 2.5, 0.0)), 1);
        assert_eq!(viewer.point_to_offset(Point::new(cell * 6.5, 0.0)), 3);
    }

    #[test]
    fn selects_words() {
        let viewer = viewer("hello world", 40);
        assert_eq!(viewer.word_bounds(7), (6, 11));
        assert_eq!(viewer.word_bounds(1), (0, 5));
    }

    #[test]
    fn selected_text_is_copied_by_range() {
        let mut viewer = viewer("alpha beta", 40);
        viewer.selection = Some((6, 10));
        assert_eq!(viewer.selected_text().as_deref(), Some("beta"));
        viewer.selection = Some((3, 3));
        assert_eq!(viewer.selected_text(), None);
    }

    #[test]
    fn wheel_starts_a_smooth_scroll() {
        let mut viewer = viewer(&"line\n".repeat(500), 40);
        assert!(!viewer.is_animating());

        viewer.update(Message::Wheel(mouse::ScrollDelta::Lines {
            x: 0.0,
            y: -3.0,
        }));
        assert!(viewer.is_animating(), "wheel should start the animation");
        assert!(viewer.scroll_target > 0.0);

        let target = viewer.scroll_target;
        viewer.update(Message::Frame);
        assert!(viewer.scroll_displayed > 0.0);
        assert!(viewer.scroll_displayed < target, "eases towards the target");

        while viewer.is_animating() {
            viewer.update(Message::Frame);
        }
        assert!(
            (viewer.scroll_target - viewer.scroll_displayed).abs() <= SCROLL_SNAP,
            "animation settles within the snap distance"
        );
    }

    #[test]
    fn wheel_never_scrolls_above_the_top() {
        let mut viewer = viewer("only one line", 40);
        viewer.update(Message::Wheel(mouse::ScrollDelta::Lines { x: 0.0, y: 5.0 }));
        assert_eq!(viewer.scroll_target, 0.0);
        assert!(!viewer.is_animating());
    }

    #[test]
    fn content_columns_reserve_padding() {
        let viewer = viewer("x", 40);
        assert_eq!(viewer.content_columns(), 40 - PADDING_CELLS * 2);
    }
}
