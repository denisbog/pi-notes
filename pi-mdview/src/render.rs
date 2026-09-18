//! The renderer: [`Document`] -> [`Line`]s.
//!
//! This is a line-for-line port of pi's `Markdown` component
//! (`pi-tui/dist/components/markdown.js`): `renderToken`, `renderInlineTokens`,
//! `renderList` and `renderTable`. The output is the same array-of-lines
//! structure pi hands to its terminal renderer, but with [`Span`]s instead of
//! embedded ANSI codes.

use crate::highlight::Highlighter;
use crate::latex;
use crate::md::{Block, Document, Inline, List, Table};
use crate::text::{wrap_line, Line, Span, Style};
use crate::theme::Theme;

/// Layout knobs, mirroring the `Markdown` component constructor arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// `paddingX`: left/right padding in columns.
    pub padding_x: usize,
    /// `paddingY`: blank lines above and below the document.
    pub padding_y: usize,
    /// Pad every line with spaces up to `width` (pi always does this).
    pub pad_to_width: bool,
    /// Convert LaTeX math to Unicode (`options.renderLatex !== false` in pi).
    pub render_latex: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            // Like pi's `Markdown` component when constructed with `paddingX = 0`;
            // the GUI sets its own padding.
            padding_x: 0,
            padding_y: 0,
            pad_to_width: true,
            render_latex: true,
        }
    }
}

/// Renders markdown documents the way pi renders them.
pub struct Renderer {
    theme: Theme,
    highlighter: Option<Highlighter>,
    options: RenderOptions,
}

impl Renderer {
    /// Creates a renderer with syntax highlighting for code blocks.
    pub fn new(theme: Theme) -> Self {
        Self {
            highlighter: Highlighter::new(&theme),
            theme,
            options: RenderOptions::default(),
        }
    }

    /// Creates a renderer without syntax highlighting (code blocks use the
    /// single `mdCodeBlock` color, like pi with an unknown language).
    pub fn plain(theme: Theme) -> Self {
        Self {
            theme,
            highlighter: None,
            options: RenderOptions::default(),
        }
    }

    pub fn options(mut self, options: RenderOptions) -> Self {
        self.options = options;
        self
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Swaps the palette, re-creating the syntax-highlighting theme.
    pub fn set_theme(&mut self, theme: Theme) {
        if self.theme != theme {
            self.theme = theme;
            self.highlighter = Highlighter::new(&self.theme);
        }
    }

    /// Parses and renders markdown at `width` columns.
    pub fn render(&self, source: &str, width: usize) -> Vec<Line> {
        self.render_document(&Document::parse(&source.replace('\t', "   ")), width)
    }

    /// Renders an already parsed document at `width` columns.
    pub fn render_document(&self, document: &Document, width: usize) -> Vec<Line> {
        if document.blocks.is_empty() {
            return Vec::new();
        }
        if document
            .blocks
            .iter()
            .all(|block| matches!(block, Block::Space))
        {
            return Vec::new();
        }

        let padding_x = self.options.padding_x;
        let content_width = width.saturating_sub(padding_x * 2).max(1);

        let mut lines = Vec::new();
        self.render_blocks(
            &document.blocks,
            content_width,
            self.theme.body(),
            &mut lines,
        );

        // pi wraps every rendered line at the content width right before padding.
        let mut wrapped = Vec::new();
        for line in lines {
            wrapped.extend(wrap_line(&line, content_width));
        }

        let mut out = Vec::with_capacity(wrapped.len() + self.options.padding_y * 2);
        let blank = Line::new();
        for _ in 0..self.options.padding_y {
            out.push(blank.clone());
        }
        let left = Line::from_text(" ".repeat(padding_x), Style::default());
        for line in wrapped {
            let mut line_with_margin = left.clone();
            line_with_margin.append(&line);
            if self.options.pad_to_width {
                line_with_margin.pad_end(width, Style::default());
            }
            out.push(line_with_margin);
        }
        for _ in 0..self.options.padding_y {
            out.push(blank.clone());
        }
        out
    }

    fn render_blocks(&self, blocks: &[Block], width: usize, ctx: Style, out: &mut Vec<Line>) {
        for (index, block) in blocks.iter().enumerate() {
            self.render_block(block, width, ctx, blocks.get(index + 1), out);
        }
    }

    /// `renderToken`.
    fn render_block(
        &self,
        block: &Block,
        width: usize,
        ctx: Style,
        next: Option<&Block>,
        out: &mut Vec<Line>,
    ) {
        let blank_after = matches!(next, Some(next) if !matches!(next, Block::Space));

        match block {
            Block::Space => out.push(Line::new()),
            Block::Heading { level, content } => {
                let heading_style = ctx.merge(self.theme.heading(*level));
                let prefix = if *level >= 3 {
                    format!("{} ", "#".repeat(*level as usize))
                } else {
                    String::new()
                };
                let mut first = true;
                for line in self.render_inlines(content, heading_style, None) {
                    let mut line = line;
                    if first && !prefix.is_empty() {
                        line.prepend(&Line::from_text(prefix.clone(), heading_style));
                    }
                    first = false;
                    out.extend(wrap_line(&line, width));
                }
                if blank_after {
                    out.push(Line::new());
                }
            }
            Block::Paragraph(content) => {
                for line in self.render_inlines(content, ctx, None) {
                    out.extend(wrap_line(&line, width));
                }
                // pi: paragraphs are not followed by a blank line before a list.
                if blank_after && !matches!(next, Some(Block::List(_))) {
                    out.push(Line::new());
                }
            }
            Block::Code { lang, text } => {
                out.push(Line::from_text(
                    format!("```{}", lang.as_deref().unwrap_or("")),
                    self.theme.code_block_border(),
                ));
                let indent = " ".repeat(self.theme.code_block_indent);
                let code = text.strip_suffix('\n').unwrap_or(text);
                for line in self.highlighted_lines(code, lang.as_deref()) {
                    let mut full = Line::from_text(indent.clone(), Style::default());
                    full.append(&line);
                    out.push(full);
                }
                out.push(Line::from_text("```", self.theme.code_block_border()));
                if blank_after {
                    out.push(Line::new());
                }
            }
            Block::List(list) => out.extend(self.render_list(list, 0, width, ctx)),
            Block::Quote(inner) => {
                let inner_width = width.saturating_sub(2).max(1);
                let quote_style = ctx.merge(self.theme.quote());
                let mut inner_lines = Vec::new();
                self.render_blocks(inner, inner_width, quote_style, &mut inner_lines);
                while inner_lines.last().is_some_and(Line::is_blank) {
                    inner_lines.pop();
                }
                for line in inner_lines {
                    for wrapped in wrap_line(&line, inner_width) {
                        let mut quoted = Line::from_text("│ ", self.theme.quote_border());
                        quoted.append(&wrapped);
                        out.push(quoted);
                    }
                }
                if blank_after {
                    out.push(Line::new());
                }
            }
            Block::Table(table) => out.extend(self.render_table(table, width, ctx, blank_after)),
            Block::Rule => {
                out.push(Line::from_text("─".repeat(width.min(80)), self.theme.hr()));
                if blank_after {
                    out.push(Line::new());
                }
            }
            Block::Html(raw) => {
                out.push(Line::from_text(raw.trim().to_string(), ctx));
            }
            Block::Math { text, display } => {
                for line in self.render_math(text, *display) {
                    out.push(Line::from_text(line, ctx));
                }
                if blank_after {
                    out.push(Line::new());
                }
            }
        }
    }

    /// `renderInlineTokens`.
    fn render_inlines(&self, inlines: &[Inline], ctx: Style, link: Option<&str>) -> Vec<Line> {
        let mut lines = vec![Line::new()];
        self.write_inlines(inlines, ctx, link, &mut lines);
        lines
    }

    fn write_inlines(
        &self,
        inlines: &[Inline],
        ctx: Style,
        link: Option<&str>,
        lines: &mut Vec<Line>,
    ) {
        for inline in inlines {
            match inline {
                Inline::Text(text) => lines
                    .last_mut()
                    .expect("always one line")
                    .push_styled(text, ctx, link),
                Inline::Code(code) => lines.last_mut().expect("always one line").push_styled(
                    code,
                    ctx.merge(self.theme.code()),
                    link,
                ),
                Inline::Strong(inner) => self.write_inlines(inner, ctx.bold(), link, lines),
                Inline::Em(inner) => self.write_inlines(inner, ctx.italic(), link, lines),
                Inline::Del(inner) => self.write_inlines(inner, ctx.strikethrough(), link, lines),
                Inline::Link { text, href } => {
                    let link_ctx = ctx.merge(self.theme.link());
                    self.write_inlines(text, link_ctx, link.or(Some(href.as_str())), lines);
                }
                Inline::Break => lines.push(Line::new()),
                Inline::Html(raw) => lines
                    .last_mut()
                    .expect("always one line")
                    .push_styled(raw, ctx, link),
                Inline::Math { text, display } => {
                    for (index, part) in self.render_math(text, *display).iter().enumerate() {
                        if index > 0 {
                            lines.push(Line::new());
                        }
                        lines
                            .last_mut()
                            .expect("always one line")
                            .push_styled(part, ctx, link);
                    }
                }
            }
        }
    }

    /// `renderList`.
    fn render_list(&self, list: &List, depth: usize, width: usize, ctx: Style) -> Vec<Line> {
        let mut lines = Vec::new();
        let indent = "    ".repeat(depth);
        let start_number = list.start;

        for (index, item) in list.items.iter().enumerate() {
            let is_last = index == list.items.len() - 1;
            let bullet = if list.ordered {
                format!("{}. ", start_number + index as u64)
            } else {
                "- ".to_string()
            };
            let task_marker = match item.task {
                Some(true) => "[x] ",
                Some(false) => "[ ] ",
                None => "",
            };
            let marker = format!("{bullet}{task_marker}");

            let mut first_prefix = Line::from_text(indent.clone(), ctx);
            first_prefix.push(&marker, ctx.merge(self.theme.list_bullet()));
            let first_prefix_width = first_prefix.width();

            let continuation = format!(
                "{indent}{}",
                " ".repeat(crate::text::visible_width(&marker))
            );
            let continuation_prefix = Line::from_text(continuation, ctx);

            let item_width = width.saturating_sub(first_prefix_width).max(1);
            let mut rendered_any_line = false;

            for block in &item.blocks {
                if let Block::List(nested) = block {
                    lines.extend(self.render_list(nested, depth + 1, width, ctx));
                    rendered_any_line = true;
                    continue;
                }

                let mut block_lines = Vec::new();
                self.render_block(block, item_width, ctx, None, &mut block_lines);
                for line in block_lines {
                    for wrapped in wrap_line(&line, item_width) {
                        let mut out = if rendered_any_line {
                            continuation_prefix.clone()
                        } else {
                            first_prefix.clone()
                        };
                        out.append(&wrapped);
                        lines.push(out);
                        rendered_any_line = true;
                    }
                }
            }

            if !rendered_any_line {
                lines.push(first_prefix);
            }
            if list.loose && !is_last {
                lines.push(Line::new());
            }
        }

        lines
    }

    /// `renderTable`.
    fn render_table(
        &self,
        table: &Table,
        width: usize,
        ctx: Style,
        blank_after: bool,
    ) -> Vec<Line> {
        let mut lines = Vec::new();
        let num_cols = table.header.len();
        if num_cols == 0 {
            return lines;
        }

        // Border overhead: "│ " + (n-1) * " │ " + " │" = 3n + 1
        let border_overhead = 3 * num_cols + 1;
        let available_for_cells = width.saturating_sub(border_overhead);

        if available_for_cells < num_cols {
            // Too narrow for a stable table: fall back to raw markdown.
            let raw = table.raw.trim_end_matches(['\n', '\r']);
            let mut fallback = wrap_line(&Line::from_text(raw, ctx), width);
            if blank_after {
                fallback.push(Line::new());
            }
            return fallback;
        }

        const MAX_UNBROKEN_WORD_WIDTH: usize = 30;

        let header_cells: Vec<Vec<Line>> = table
            .header
            .iter()
            .map(|cell| self.render_inlines(cell, ctx, None))
            .collect();
        let body_cells: Vec<Vec<Vec<Line>>> = table
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| self.render_inlines(cell, ctx, None))
                    .collect()
            })
            .collect();

        let mut natural_widths = vec![0usize; num_cols];
        let mut min_word_widths = vec![1usize; num_cols];

        for (index, cell) in header_cells.iter().enumerate() {
            let text = joined_plain(cell);
            natural_widths[index] = visible_width(&text);
            min_word_widths[index] = longest_word_width(&text, MAX_UNBROKEN_WORD_WIDTH).max(1);
        }
        for row in &body_cells {
            for (index, cell) in row.iter().enumerate() {
                if index >= num_cols {
                    continue;
                }
                let text = joined_plain(cell);
                natural_widths[index] = natural_widths[index].max(visible_width(&text));
                min_word_widths[index] = min_word_widths[index]
                    .max(longest_word_width(&text, MAX_UNBROKEN_WORD_WIDTH).max(1));
            }
        }

        let mut min_column_widths = min_word_widths.clone();
        let mut min_cells_width: usize = min_column_widths.iter().sum();

        if min_cells_width > available_for_cells {
            min_column_widths = vec![1usize; num_cols];
            let remaining = available_for_cells.saturating_sub(num_cols);
            if remaining > 0 {
                let total_weight: usize = min_word_widths
                    .iter()
                    .map(|width| width.saturating_sub(1))
                    .sum();
                let growth: Vec<usize> = min_word_widths
                    .iter()
                    .map(|width| {
                        let weight = width.saturating_sub(1);
                        (weight * remaining).checked_div(total_weight).unwrap_or(0)
                    })
                    .collect();
                for (index, extra) in growth.iter().enumerate() {
                    min_column_widths[index] += extra;
                }
                let allocated: usize = growth.iter().sum();
                let mut leftover = remaining.saturating_sub(allocated);
                let mut index = 0;
                while leftover > 0 && index < num_cols {
                    min_column_widths[index] += 1;
                    leftover -= 1;
                    index += 1;
                }
            }
            min_cells_width = min_column_widths.iter().sum();
        }

        let total_natural_width: usize = natural_widths.iter().sum::<usize>() + border_overhead;
        let column_widths: Vec<usize> = if total_natural_width <= width {
            natural_widths
                .iter()
                .zip(min_column_widths.iter())
                .map(|(natural, min)| (*natural).max(*min))
                .collect()
        } else {
            let total_grow_potential: usize = natural_widths
                .iter()
                .zip(min_column_widths.iter())
                .map(|(natural, min)| natural.saturating_sub(*min))
                .sum();
            let extra_width = available_for_cells.saturating_sub(min_cells_width);
            let mut widths: Vec<usize> = min_column_widths
                .iter()
                .zip(natural_widths.iter())
                .map(|(min, natural)| {
                    let delta = natural.saturating_sub(*min);
                    let grow = (delta * extra_width)
                        .checked_div(total_grow_potential)
                        .unwrap_or(0);
                    min + grow
                })
                .collect();

            let allocated: usize = widths.iter().sum();
            let mut remaining = available_for_cells.saturating_sub(allocated);
            while remaining > 0 {
                let mut grew = false;
                for index in 0..num_cols {
                    if remaining == 0 {
                        break;
                    }
                    if widths[index] < natural_widths[index] {
                        widths[index] += 1;
                        remaining -= 1;
                        grew = true;
                    }
                }
                if !grew {
                    break;
                }
            }
            widths
        };

        // Top border
        lines.push(border_line("┌─", "─┬─", "─┐", &column_widths, ctx));

        // Header (bold)
        let header_wrapped: Vec<Vec<Line>> = header_cells
            .iter()
            .enumerate()
            .map(|(index, cell)| wrap_cell(cell, column_widths[index]))
            .collect();
        let header_line_count = header_wrapped.iter().map(Vec::len).max().unwrap_or(1);
        for line_index in 0..header_line_count {
            let mut row = Line::from_text("│ ", ctx);
            for (col, cell) in header_wrapped.iter().enumerate() {
                if col > 0 {
                    row.push(" │ ", ctx);
                }
                let mut padded = cell.get(line_index).cloned().unwrap_or_default();
                padded.pad_end(column_widths[col], Style::default());
                row.append(&bolden(&padded));
            }
            row.push(" │", ctx);
            lines.push(row);
        }

        let separator = border_line("├─", "─┼─", "─┤", &column_widths, ctx);
        lines.push(separator.clone());

        for (row_index, row) in body_cells.iter().enumerate() {
            let row_wrapped: Vec<Vec<Line>> = row
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    let column = column_widths.get(index).copied().unwrap_or(1);
                    wrap_cell(cell, column)
                })
                .collect();
            let row_line_count = row_wrapped.iter().map(Vec::len).max().unwrap_or(1);
            for line_index in 0..row_line_count {
                let mut line = Line::from_text("│ ", ctx);
                for (col, cell) in row_wrapped.iter().enumerate() {
                    if col > 0 {
                        line.push(" │ ", ctx);
                    }
                    let mut padded = cell.get(line_index).cloned().unwrap_or_default();
                    padded.pad_end(column_widths[col], Style::default());
                    line.append(&padded);
                }
                line.push(" │", ctx);
                lines.push(line);
            }
            if row_index < body_cells.len() - 1 {
                lines.push(separator.clone());
            }
        }

        lines.push(border_line("└─", "─┴─", "─┘", &column_widths, ctx));

        if blank_after {
            lines.push(Line::new());
        }
        lines
    }

    fn highlighted_lines(&self, code: &str, lang: Option<&str>) -> Vec<Line> {
        let per_line = match &self.highlighter {
            Some(highlighter) => highlighter.highlight(code, lang),
            None => code
                .split('\n')
                .map(|line| vec![Span::new(line, self.theme.code_block())])
                .collect(),
        };
        per_line
            .into_iter()
            .map(|spans| {
                let mut line = Line::new();
                for span in spans {
                    line.push_span(span);
                }
                line
            })
            .collect()
    }

    fn render_math(&self, text: &str, display: bool) -> Vec<String> {
        // Math token text includes the surrounding newlines (`\n...\n`); pi trims
        // it before handing it to its LaTeX renderer.
        let text = text.trim();
        let rendered = if self.options.render_latex {
            latex::render(text)
        } else {
            text.to_string()
        };
        let rendered = if display {
            rendered
        } else {
            rendered.replace('\n', " ")
        };
        rendered.split('\n').map(str::to_string).collect()
    }
}

/// Wraps a table cell to `width` columns.
fn wrap_cell(cell: &[Line], width: usize) -> Vec<Line> {
    let mut out = Vec::new();
    for line in cell {
        out.extend(wrap_line(line, width.max(1)));
    }
    if out.is_empty() {
        out.push(Line::new());
    }
    out
}

/// Builds a table border line such as `┌──┬──┐`.
fn border_line(left: &str, middle: &str, right: &str, widths: &[usize], ctx: Style) -> Line {
    let cells: Vec<String> = widths.iter().map(|width| "─".repeat(*width)).collect();
    Line::from_text(format!("{left}{}{right}", cells.join(middle)), ctx)
}

/// Sets `bold` on every span (pi wraps the whole padded header cell in bold).
fn bolden(line: &Line) -> Line {
    let mut out = Line::new();
    for span in &line.spans {
        out.push_span(Span {
            text: span.text.clone(),
            style: span.style.bold(),
            link: span.link.clone(),
        });
    }
    out
}

/// Concatenates the plain text of a cell; pi measures the whole rendered string.
fn joined_plain(lines: &[Line]) -> String {
    lines.iter().map(Line::plain).collect()
}

fn visible_width(text: &str) -> usize {
    crate::text::visible_width(text)
}

fn longest_word_width(text: &str, cap: usize) -> usize {
    text.split_whitespace()
        .map(visible_width)
        .max()
        .unwrap_or(0)
        .min(cap)
}

#[cfg(test)]
mod style_tests {
    use super::*;

    fn render(markdown: &str, width: usize) -> Vec<Line> {
        Renderer::new(Theme::pi_dark())
            .options(RenderOptions {
                pad_to_width: false,
                ..RenderOptions::default()
            })
            .render(markdown, width)
    }

    fn first_line_with(lines: &[Line], text: &str) -> Line {
        lines
            .iter()
            .find(|line| line.plain().contains(text))
            .unwrap_or_else(|| panic!("no line containing {text:?}"))
            .clone()
    }

    #[test]
    fn headings_use_md_heading_style() {
        let theme = Theme::pi_dark();
        let lines = render("# Title", 20);
        let heading = &lines[0];
        assert_eq!(heading.plain().trim_end(), "Title");
        assert!(heading
            .spans
            .iter()
            .all(|span| span.style.fg == Some(theme.md_heading)));
        assert!(heading.spans.iter().all(|span| span.style.bold));
        assert!(heading.spans.iter().all(|span| span.style.underline));
    }

    #[test]
    fn h3_keeps_its_hash_prefix() {
        let lines = render("### Title", 20);
        assert_eq!(lines[0].plain().trim_end(), "### Title");
    }

    #[test]
    fn inline_styles_merge() {
        let theme = Theme::pi_dark();
        let lines = render("plain **bold `code`**", 40);
        let line = first_line_with(&lines, "code");

        let bold_span = line
            .spans
            .iter()
            .find(|span| span.text.contains("bold"))
            .unwrap();
        assert!(bold_span.style.bold);
        assert_eq!(bold_span.style.fg, Some(theme.text));

        // Code inside bold keeps the code color and the bold decoration, like pi.
        let code_span = line
            .spans
            .iter()
            .find(|span| span.text.contains("code"))
            .unwrap();
        assert_eq!(code_span.style.fg, Some(theme.md_code));
        assert!(code_span.style.bold);
    }

    #[test]
    fn links_are_underlined_and_clickable() {
        let theme = Theme::pi_dark();
        let lines = render("[site](https://example.com)", 40);
        let span = lines[0]
            .spans
            .iter()
            .find(|span| span.text.contains("site"))
            .unwrap();
        assert_eq!(span.style.fg, Some(theme.md_link));
        assert!(span.style.underline);
        assert_eq!(span.link.as_deref(), Some("https://example.com"));
        assert_eq!(lines[0].plain().trim_end(), "site");
    }

    #[test]
    fn list_bullets_and_quotes_use_their_colors() {
        let theme = Theme::pi_dark();

        let lines = render("- item", 20);
        let bullet = lines[0]
            .spans
            .iter()
            .find(|span| span.text == "- ")
            .unwrap();
        assert_eq!(bullet.style.fg, Some(theme.md_list_bullet));

        let lines = render("> quoted", 20);
        let border = lines[0]
            .spans
            .iter()
            .find(|span| span.text == "│ ")
            .unwrap();
        assert_eq!(border.style.fg, Some(theme.md_quote_border));
        let content = lines[0]
            .spans
            .iter()
            .find(|span| span.text.contains("quoted"))
            .unwrap();
        assert_eq!(content.style.fg, Some(theme.md_quote));
        assert!(content.style.italic);
    }

    #[test]
    fn code_blocks_use_fence_and_code_colors() {
        let theme = Theme::pi_dark();
        let lines = render("```rust\nlet x = 1;\n```", 30);
        assert_eq!(lines[0].plain().trim_end(), "```rust");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.md_code_block_border));

        // The indent is plain, the highlighted code follows it.
        let code_line = &lines[1];
        assert!(code_line.plain().starts_with("  "));

        let last = lines.last().unwrap();
        assert_eq!(last.plain().trim_end(), "```");
        assert_eq!(last.spans[0].style.fg, Some(theme.md_code_block_border));
    }

    #[test]
    fn code_without_language_uses_md_code_block() {
        let theme = Theme::pi_dark();
        let lines = render("```\nhello\n```", 30);
        let code = lines[1]
            .spans
            .iter()
            .find(|span| span.text.contains("hello"))
            .unwrap();
        assert_eq!(code.style.fg, Some(theme.md_code_block));
    }

    #[test]
    fn strikethrough_and_italic_set_styles() {
        let lines = render("~~gone~~ *slanted*", 30);
        let gone = lines[0]
            .spans
            .iter()
            .find(|span| span.text.contains("gone"))
            .unwrap();
        assert!(gone.style.strikethrough);
        let slanted = lines[0]
            .spans
            .iter()
            .find(|span| span.text.contains("slanted"))
            .unwrap();
        assert!(slanted.style.italic);
    }

    #[test]
    fn tables_render_with_pi_borders() {
        let lines = render("| a | b |\n|---|---|\n| 1 | 2 |", 30);
        let plain: Vec<String> = lines.iter().map(|line| line.plain()).collect();
        assert_eq!(plain[0].trim_end(), "┌───┬───┐");
        assert_eq!(plain[1].trim_end(), "│ a │ b │");
        assert_eq!(plain[2].trim_end(), "├───┼───┤");
        assert!(plain[3].trim_end().starts_with("│ 1 │ 2 │"));
        assert_eq!(plain[4].trim_end(), "└───┴───┘");
    }

    #[test]
    fn hr_is_capped_at_80_columns() {
        let lines = render("---", 200);
        assert_eq!(lines[0].plain().chars().count(), 80);
    }

    #[test]
    fn empty_document_renders_nothing() {
        assert!(render("   \n", 40).is_empty());
    }
}
