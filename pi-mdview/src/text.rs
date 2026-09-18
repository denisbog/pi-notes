//! Styled-text model: the Rust counterpart of pi's `string[]` lines of ANSI text.
//!
//! pi renders markdown into an array of strings, each string a terminal line that
//! carries embedded SGR codes. This module models the same thing structurally:
//! a [`Line`] is a sequence of [`Span`]s (text + style), which keeps the renderer
//! headless and testable while the iced layer can map spans onto `rich_text`.

use std::fmt::Write as _;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A 24-bit color, independent of the GUI toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b)
    }

    /// Parses `#rrggbb`, `rrggbb`, `#rgb`.
    pub fn from_hex(hex: &str) -> Self {
        let hex = hex.trim().trim_start_matches('#');
        let expand = |c: u8| -> u8 {
            // 'f' -> 0xff
            let d = (c as char).to_digit(16).unwrap_or(0) as u8;
            d * 16 + d
        };
        let bytes = hex.as_bytes();
        if hex.len() == 3 {
            Self(expand(bytes[0]), expand(bytes[1]), expand(bytes[2]))
        } else {
            let v = u32::from_str_radix(hex, 16).unwrap_or(0);
            Self(
                ((v >> 16) & 0xff) as u8,
                ((v >> 8) & 0xff) as u8,
                (v & 0xff) as u8,
            )
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    fn ansi_fg(self, out: &mut String) {
        let _ = write!(out, "\x1b[38;2;{};{};{}m", self.0, self.1, self.2);
    }
}

/// A text style. `None` colors mean "inherit the surrounding color".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl Style {
    pub const fn new() -> Self {
        Self {
            fg: None,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }

    pub const fn fg(color: Rgb) -> Self {
        Self {
            fg: Some(color),
            ..Self::new()
        }
    }

    pub fn with_fg(mut self, color: Rgb) -> Self {
        self.fg = Some(color);
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    /// Overlay `other` on `self`. This mirrors pi's `stylePrefix` re-application:
    /// an inline style (code, link, ...) wins over the surrounding color while
    /// decorations from the surrounding style (e.g. heading bold) are preserved.
    pub fn merge(self, other: Style) -> Style {
        Style {
            fg: other.fg.or(self.fg),
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            underline: self.underline || other.underline,
            strikethrough: self.strikethrough || other.strikethrough,
        }
    }

    pub fn is_plain(&self) -> bool {
        self.fg.is_none() && !self.bold && !self.italic && !self.underline && !self.strikethrough
    }
}

/// A run of text with one style.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    /// Target URL when this run is part of a markdown link.
    pub link: Option<String>,
}

impl Span {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
            link: None,
        }
    }

    pub fn with_link(mut self, url: impl Into<String>) -> Self {
        self.link = Some(url.into());
        self
    }

    pub fn width(&self) -> usize {
        UnicodeWidthStr::width(self.text.as_str())
    }
}

/// One rendered line, the structural equivalent of one element of pi's
/// `Markdown.render(width): string[]`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl Line {
    pub fn new() -> Self {
        Self { spans: Vec::new() }
    }

    pub fn from_text(text: impl Into<String>, style: Style) -> Self {
        let text = text.into();
        if text.is_empty() {
            return Self::new();
        }
        Self {
            spans: vec![Span::new(text, style)],
        }
    }

    /// Appends text with `style`, coalescing with the previous span when possible.
    pub fn push(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            if last.style == style && last.link.is_none() {
                last.text.push_str(text);
                return;
            }
        }
        self.spans.push(Span::new(text, style));
    }

    pub fn push_styled(&mut self, text: &str, style: Style, link: Option<&str>) {
        if text.is_empty() {
            return;
        }
        if let (Some(last), None) = (self.spans.last_mut(), link) {
            if last.style == style && last.link.is_none() {
                last.text.push_str(text);
                return;
            }
        }
        let mut span = Span::new(text, style);
        span.link = link.map(str::to_owned);
        self.spans.push(span);
    }

    pub fn push_span(&mut self, span: Span) {
        if span.text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            if last.style == span.style && last.link == span.link {
                last.text.push_str(&span.text);
                return;
            }
        }
        self.spans.push(span);
    }

    pub fn append(&mut self, other: &Line) {
        for span in &other.spans {
            self.push_span(span.clone());
        }
    }

    /// Prefix this line with another (used for list markers / quote borders).
    pub fn prepend(&mut self, other: &Line) {
        let mut spans = other.spans.clone();
        spans.append(&mut self.spans);
        self.spans = spans;
    }

    /// True when the line has no visible text.
    pub fn is_blank(&self) -> bool {
        self.spans.iter().all(|s| s.text.trim().is_empty())
    }

    pub fn width(&self) -> usize {
        self.spans.iter().map(Span::width).sum()
    }

    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    /// Removes trailing whitespace, mirroring pi's `line.trimEnd()`.
    pub fn trim_end(&mut self) {
        while let Some(last) = self.spans.last_mut() {
            let trimmed = last.text.trim_end();
            if trimmed.len() == last.text.len() {
                break;
            }
            let trimmed_len = trimmed.len();
            last.text.truncate(trimmed_len);
            if last.text.is_empty() {
                self.spans.pop();
            } else {
                break;
            }
        }
    }

    pub fn trim_start(&mut self) {
        while let Some(first) = self.spans.first_mut() {
            let trimmed = first.text.trim_start();
            if trimmed.len() == first.text.len() {
                break;
            }
            first.text = trimmed.to_string();
            if first.text.is_empty() {
                self.spans.remove(0);
            } else {
                break;
            }
        }
    }

    /// Pads the line with spaces until it is `width` columns wide.
    pub fn pad_end(&mut self, width: usize, style: Style) {
        let current = self.width();
        if current < width {
            let pad = " ".repeat(width - current);
            self.push(&pad, style);
        }
    }

    /// Renders to an ANSI string, for parity comparisons against pi's output.
    pub fn to_ansi(&self) -> String {
        let mut out = String::new();
        for span in &self.spans {
            if span.style.is_plain() {
                out.push_str(&span.text);
                continue;
            }
            span.style.fg.unwrap_or(Rgb(0, 0, 0)).ansi_fg(&mut out);
            if span.style.bold {
                out.push_str("\x1b[1m");
            }
            if span.style.italic {
                out.push_str("\x1b[3m");
            }
            if span.style.underline {
                out.push_str("\x1b[4m");
            }
            if span.style.strikethrough {
                out.push_str("\x1b[9m");
            }
            out.push_str(&span.text);
            out.push_str("\x1b[0m");
        }
        out
    }
}

/// Visible width of a string in terminal columns.
pub fn visible_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Splits a [`Line`] into whitespace / word tokens, mirroring pi's
/// `splitIntoTokensWithAnsi`: spaces are their own tokens and wide (CJK)
/// graphemes break into individual tokens.
pub(crate) fn split_tokens(line: &Line) -> Vec<Line> {
    let mut tokens: Vec<Line> = Vec::new();
    let mut current = Line::new();
    let mut current_is_space: Option<bool> = None;

    let flush = |current: &mut Line, tokens: &mut Vec<Line>| {
        if !current.spans.is_empty() {
            tokens.push(std::mem::take(current));
        }
    };

    for span in &line.spans {
        for ch in span.text.chars() {
            let is_space = ch == ' ';
            let wide = UnicodeWidthChar::width(ch).unwrap_or(0) >= 2;

            if !is_space && wide {
                flush(&mut current, &mut tokens);
                let mut token = Line::new();
                token.push_styled(&ch.to_string(), span.style, span.link.as_deref());
                tokens.push(token);
                current_is_space = None;
                continue;
            }

            if let Some(kind) = current_is_space {
                if kind != is_space {
                    flush(&mut current, &mut tokens);
                }
            }
            current_is_space = Some(is_space);

            current.push_styled(&ch.to_string(), span.style, span.link.as_deref());
        }
    }
    flush(&mut current, &mut tokens);

    if tokens.is_empty() {
        tokens.push(Line::new());
    }
    tokens
}

/// Breaks a token that is wider than `width` into chunks of at most `width`
/// columns (pi's `breakLongWord`).
pub(crate) fn break_long(token: &Line, width: usize) -> Vec<Line> {
    let width = width.max(1);
    let mut out: Vec<Line> = Vec::new();
    let mut current = Line::new();
    let mut current_width = 0usize;

    for span in &token.spans {
        for ch in span.text.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if current_width + w > width && current_width > 0 {
                out.push(std::mem::take(&mut current));
                current_width = 0;
            }
            current.push_styled(&ch.to_string(), span.style, span.link.as_deref());
            current_width += w;
        }
    }
    out.push(current);
    out
}

/// Word-wraps a line to `width` columns. Mirrors pi's `wrapTextWithAnsi`:
/// long words are broken per character, lines are `trim_end`ed, and styles are
/// carried across the wrap boundary (implicit here, since spans carry styles).
pub fn wrap_line(line: &Line, width: usize) -> Vec<Line> {
    if line.spans.is_empty() {
        return vec![Line::new()];
    }
    if line.width() <= width {
        return vec![line.clone()];
    }

    let tokens = split_tokens(line);
    let mut wrapped: Vec<Line> = Vec::new();
    let mut current = Line::new();

    for token in tokens {
        let token_width = token.width();
        let is_space = token.is_blank();

        if token_width > width && !is_space {
            if !current.spans.is_empty() {
                current.trim_end();
                wrapped.push(std::mem::take(&mut current));
            }
            let mut broken = break_long(&token, width);
            let last = broken.pop().unwrap_or_default();
            wrapped.append(&mut broken);
            current = last;
            continue;
        }

        if current.width() + token_width > width && current.width() > 0 {
            current.trim_end();
            wrapped.push(std::mem::take(&mut current));
            if !is_space {
                current = token;
            }
        } else {
            current.append(&token);
        }
    }

    if !current.spans.is_empty() {
        wrapped.push(current);
    }

    if wrapped.is_empty() {
        return vec![Line::new()];
    }
    for line in &mut wrapped {
        line.trim_end();
    }
    wrapped
}

/// Wraps every line of a slice.
pub fn wrap_lines(lines: &[Line], width: usize) -> Vec<Line> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        out.extend(wrap_line(line, width));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_line(s: &str) -> Line {
        Line::from_text(s, Style::default())
    }

    #[test]
    fn wraps_at_words() {
        let line = text_line("hello world foo");
        let wrapped = wrap_line(&line, 12);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0].plain(), "hello world");
        assert_eq!(wrapped[1].plain(), "foo");
    }

    #[test]
    fn breaks_long_words() {
        let line = text_line("abcdefghij");
        let wrapped = wrap_line(&line, 4);
        assert_eq!(
            wrapped.iter().map(Line::plain).collect::<Vec<_>>(),
            vec!["abcd", "efgh", "ij"]
        );
    }

    #[test]
    fn wide_chars() {
        let line = text_line("日本語テスト");
        let wrapped = wrap_line(&line, 6);
        assert_eq!(
            wrapped.iter().map(Line::plain).collect::<Vec<_>>(),
            vec!["日本語", "テスト"]
        );
    }
}
