//! iced bridge: rendered [`Line`]s → `rich_text` spans.
//!
//! The renderer produces a toolkit-independent grid of styled [`Span`]s (the
//! Rust equivalent of pi's `string[]` of ANSI lines). This module maps that
//! grid onto iced's `rich_text`, mirroring the span mapping the viewer
//! ([`crate::app`]) performs for its own document, so other iced applications
//! (e.g. `pi-notes`) can display documents rendered by pi-mdview too.

use iced::font::{Style as FontSlant, Weight};
use iced::widget::text::Span as IcedSpan;
use iced::{Color, Font};

use crate::text::{Line, Rgb, Span};
use crate::theme::Theme;

/// An iced text span produced by this module. `Link` is `String`, matching the
/// renderer's [`Span::link`].
pub type DocumentSpan<'a> = IcedSpan<'a, String, Font>;

/// Converts a renderer [`Rgb`] into an iced [`Color`].
pub fn rgb(color: Rgb) -> Color {
    Color::from_rgb8(color.0, color.1, color.2)
}

/// Converts a whole document's [`Line`]s into iced text spans (newline
/// separated), applying an optional flat character-range selection highlight.
///
/// The returned spans own their text, so the caller does not have to keep the
/// [`Line`]s alive for the lifetime of the produced [`iced::Element`].
pub fn document_spans(
    lines: &[Line],
    theme: &Theme,
    font: Font,
    selection: Option<(usize, usize)>,
) -> Vec<DocumentSpan<'static>> {
    let mut spans: Vec<DocumentSpan<'static>> = Vec::with_capacity(lines.len() * 2);
    let mut offset = 0usize;
    for (index, line) in lines.iter().enumerate() {
        push_line(&mut spans, line, theme, font, offset, selection);
        // Matches the offset model of the viewer: every line is followed by a
        // newline in the flat document text.
        offset += line
            .spans
            .iter()
            .map(|span| span.text.chars().count() + 1)
            .sum::<usize>();
        if index + 1 < lines.len() {
            spans.push(IcedSpan::new("\n"));
        }
    }
    spans
}

fn push_line(
    out: &mut Vec<DocumentSpan<'static>>,
    line: &Line,
    theme: &Theme,
    font: Font,
    line_start: usize,
    selection: Option<(usize, usize)>,
) {
    let mut offset = line_start;
    for span in &line.spans {
        let text = span.text.as_str();
        let length = text.chars().count();
        let (selected_from, selected_to) = match selection {
            Some((start, end)) => (
                start.saturating_sub(offset).min(length),
                end.saturating_sub(offset).min(length),
            ),
            None => (length, length),
        };

        let from_byte = byte_offset(text, selected_from);
        let to_byte = byte_offset(text, selected_to);

        if selected_from > 0 {
            out.push(styled(&text[..from_byte], span, theme, font, false));
        }
        if selected_to > selected_from {
            out.push(styled(&text[from_byte..to_byte], span, theme, font, true));
        }
        if selected_to < length {
            out.push(styled(&text[to_byte..], span, theme, font, false));
        }
        offset += length;
    }
}

fn styled(
    text: &str,
    span: &Span,
    theme: &Theme,
    font: Font,
    selected: bool,
) -> DocumentSpan<'static> {
    let mut out = IcedSpan::new(text.to_string());
    if selected {
        out = out.background(rgb(theme.selection_background));
    }
    if let Some(fg) = span.style.fg {
        out = out.color(rgb(fg));
    }
    if span.style.bold || span.style.italic {
        let mut styled_font = font;
        if span.style.bold {
            styled_font.weight = Weight::Bold;
        }
        if span.style.italic {
            styled_font.style = FontSlant::Italic;
        }
        out = out.font(styled_font);
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

/// Byte offset of the `index`-th character in `text` (or its length).
fn byte_offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderOptions, Renderer};

    fn render(source: &str) -> Vec<Line> {
        Renderer::new(Theme::pi_dark())
            .options(RenderOptions {
                padding_x: 0,
                padding_y: 0,
                pad_to_width: false,
                render_latex: true,
            })
            .render(source, 40)
    }

    fn plain(spans: &[DocumentSpan<'_>]) -> String {
        spans.iter().map(|span| span.text.to_string()).collect()
    }

    #[test]
    fn spans_reproduce_the_document_text() {
        let lines = render("# title\n\nplain **bold** text");
        let spans = document_spans(&lines, &Theme::pi_dark(), Font::default(), None);
        let text = plain(&spans);
        assert!(text.contains("title"));
        assert!(text.contains("plain bold text"), "got {text:?}");
    }

    #[test]
    fn selection_splits_spans_but_keeps_text() {
        let lines = render("hello world");
        let spans = document_spans(&lines, &Theme::pi_dark(), Font::default(), Some((0, 5)));
        assert_eq!(plain(&spans), "hello world");
        // The selected run carries the selection background.
        assert!(spans
            .iter()
            .any(|span| span.highlight.is_some() && &*span.text == "hello"));
    }
}
