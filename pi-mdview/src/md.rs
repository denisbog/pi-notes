//! Markdown parsing, the Rust counterpart of pi's `marked` token stream.
//!
//! pi lexes markdown with `marked` and then walks the resulting tokens
//! (`renderToken` / `renderInlineTokens`). pulldown-cmark emits a flat event
//! stream instead, so [`parse`] rebuilds the same block/inline tree shape
//! before rendering.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// A block-level element.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    Code {
        lang: Option<String>,
        text: String,
    },
    List(List),
    Quote(Vec<Block>),
    Table(Table),
    Rule,
    /// A blank line between blocks. pi's `marked` lexer emits `space` tokens
    /// for these, and they matter: they are what separates two consecutive
    /// lists, and they appear as empty quote lines inside blockquotes.
    Space,
    /// Raw HTML block, rendered as plain text like pi does.
    Html(String),
    Math {
        text: String,
        display: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub ordered: bool,
    pub start: u64,
    pub loose: bool,
    pub items: Vec<ListItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// `Some(checked)` for `- [ ]` / `- [x]` task list items.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub header: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
    /// Raw markdown source, used by pi when the table is too narrow to render.
    pub raw: String,
}

/// An inline element.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    Strong(Vec<Inline>),
    Em(Vec<Inline>),
    Del(Vec<Inline>),
    Link {
        text: Vec<Inline>,
        href: String,
    },
    /// Soft or hard line break.
    Break,
    Math {
        text: String,
        display: bool,
    },
    /// Inline HTML, rendered as plain text.
    Html(String),
}

/// A parsed markdown document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub source: String,
}

impl Document {
    pub fn parse(source: &str) -> Self {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_MATH);

        let events: Vec<(Event<'_>, Range<usize>)> = Parser::new_ext(source, options)
            .into_offset_iter()
            .collect();

        let mut parser = EventParser {
            source,
            events,
            pos: 0,
            pending_task: None,
        };
        let blocks = parser.parse_blocks();

        Self {
            blocks,
            source: source.to_string(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

struct EventParser<'a> {
    source: &'a str,
    events: Vec<(Event<'a>, Range<usize>)>,
    pos: usize,
    /// Set when a `TaskListMarker` is seen; consumed by the enclosing list item.
    pending_task: Option<bool>,
}

impl<'a> EventParser<'a> {
    fn at_end(&self) -> bool {
        self.pos >= self.events.len()
    }

    fn peek(&self) -> Option<&Event<'a>> {
        self.events.get(self.pos).map(|(event, _)| event)
    }

    fn peek_range(&self) -> Option<Range<usize>> {
        self.events.get(self.pos).map(|(_, range)| range.clone())
    }

    /// Consumes the pending `End` event of the container we just parsed.
    fn expect_end(&mut self, expected: TagEnd) {
        match self.peek() {
            Some(Event::End(end)) if *end == expected => {
                self.pos += 1;
            }
            _ => {
                // Tolerate malformed input rather than panicking on it.
            }
        }
    }

    fn parse_blocks(&mut self) -> Vec<Block> {
        let mut blocks = Vec::new();
        while !self.at_end() {
            let Some(event) = self.peek() else { break };
            if matches!(event, Event::End(_)) {
                break;
            }

            // Mirror `marked`'s `space` tokens: a blank line between two blocks
            // becomes an explicit empty block. Container start/end ranges in
            // pulldown-cmark cover the whole container (including its trailing
            // blank lines), so the reliable signal is the line right before the
            // block, not the byte gap between ranges.
            let block_start = self.peek_range().map(|range| range.start).unwrap_or(0);
            if !blocks.is_empty() && previous_line_is_blank(self.source, block_start) {
                blocks.push(Block::Space);
            }
            match event {
                Event::Start(Tag::Paragraph) => {
                    self.pos += 1;
                    let content = self.parse_inlines();
                    self.expect_end(TagEnd::Paragraph);
                    if !content.is_empty() {
                        blocks.push(Block::Paragraph(content));
                    }
                }
                Event::Start(Tag::Heading { level, .. }) => {
                    let tag_level = *level;
                    let level = heading_level(tag_level);
                    self.pos += 1;
                    let content = self.parse_inlines();
                    self.expect_end(TagEnd::Heading(tag_level));
                    blocks.push(Block::Heading { level, content });
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => {
                            let info = info.trim();
                            if info.is_empty() {
                                None
                            } else {
                                Some(info.split_whitespace().next().unwrap_or("").to_string())
                            }
                        }
                        CodeBlockKind::Indented => None,
                    };
                    self.pos += 1;
                    let text = self.collect_text();
                    self.expect_end(TagEnd::CodeBlock);
                    blocks.push(Block::Code { lang, text });
                }
                Event::Start(Tag::List(start)) => {
                    let start = *start;
                    self.pos += 1;
                    let list = self.parse_list(start);
                    blocks.push(Block::List(list));
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    self.pos += 1;
                    let inner = self.parse_blocks();
                    self.pos += 1; // consume End(BlockQuote)
                    blocks.push(Block::Quote(inner));
                }
                Event::Start(Tag::Table(_)) => {
                    let raw = self
                        .peek_range()
                        .map(|range| self.source[range].to_string())
                        .unwrap_or_default();
                    self.pos += 1;
                    let table = self.parse_table(raw);
                    blocks.push(Block::Table(table));
                }
                Event::Start(Tag::HtmlBlock) => {
                    self.pos += 1;
                    let html = self.collect_text();
                    self.expect_end(TagEnd::HtmlBlock);
                    blocks.push(Block::Html(html));
                }
                Event::Rule => {
                    self.pos += 1;
                    blocks.push(Block::Rule);
                }
                Event::DisplayMath(text) => {
                    let text = text.to_string();
                    self.pos += 1;
                    blocks.push(Block::Math {
                        text,
                        display: true,
                    });
                }
                Event::TaskListMarker(checked) => {
                    self.pending_task = Some(*checked);
                    self.pos += 1;
                }
                Event::End(_) => break,
                // Inline content that is not wrapped in a paragraph (rare).
                _ => {
                    let content = self.parse_inlines();
                    if !content.is_empty() {
                        blocks.push(Block::Paragraph(content));
                    }
                }
            }
        }
        blocks
    }

    fn parse_inlines(&mut self) -> Vec<Inline> {
        let mut out = Vec::new();
        let mut buffer = String::new();

        macro_rules! flush {
            () => {
                if !buffer.is_empty() {
                    out.push(Inline::Text(std::mem::take(&mut buffer)));
                }
            };
        }

        while !self.at_end() {
            let Some(event) = self.peek() else { break };
            match event {
                Event::End(_) => break,
                Event::Text(text) => {
                    buffer.push_str(text);
                    self.pos += 1;
                }
                Event::Code(code) => {
                    flush!();
                    out.push(Inline::Code(code.to_string()));
                    self.pos += 1;
                }
                Event::InlineMath(math) => {
                    flush!();
                    out.push(Inline::Math {
                        text: math.to_string(),
                        display: false,
                    });
                    self.pos += 1;
                }
                Event::DisplayMath(math) => {
                    flush!();
                    out.push(Inline::Math {
                        text: math.to_string(),
                        display: true,
                    });
                    self.pos += 1;
                }
                Event::InlineHtml(html) | Event::Html(html) => {
                    buffer.push_str(html);
                    self.pos += 1;
                }
                Event::SoftBreak | Event::HardBreak => {
                    flush!();
                    out.push(Inline::Break);
                    self.pos += 1;
                }
                Event::FootnoteReference(label) => {
                    buffer.push_str(label);
                    self.pos += 1;
                }
                Event::TaskListMarker(checked) => {
                    self.pending_task = Some(*checked);
                    self.pos += 1;
                }
                Event::Start(tag) => {
                    flush!();
                    match tag {
                        Tag::Strong => {
                            self.pos += 1;
                            let inner = self.parse_inlines();
                            self.expect_end(TagEnd::Strong);
                            out.push(Inline::Strong(inner));
                        }
                        Tag::Emphasis => {
                            self.pos += 1;
                            let inner = self.parse_inlines();
                            self.expect_end(TagEnd::Emphasis);
                            out.push(Inline::Em(inner));
                        }
                        Tag::Strikethrough => {
                            self.pos += 1;
                            let inner = self.parse_inlines();
                            self.expect_end(TagEnd::Strikethrough);
                            out.push(Inline::Del(inner));
                        }
                        Tag::Link { dest_url, .. } => {
                            let href = dest_url.to_string();
                            self.pos += 1;
                            let inner = self.parse_inlines();
                            self.expect_end(TagEnd::Link);
                            out.push(Inline::Link { text: inner, href });
                        }
                        // A block-level tag ends the implicit paragraph of a
                        // tight list item (pulldown-cmark does not wrap it in
                        // `Start(Paragraph)`). Leave it for `parse_blocks`.
                        tag if is_block_tag(tag) => break,
                        // Anything else nested inline (e.g. images): keep its
                        // text content.
                        _ => {
                            self.pos += 1;
                        }
                    }
                }
                Event::Rule => break,
            }
        }
        flush!();
        out
    }

    /// Concatenates raw text of a code/html block.
    fn collect_text(&mut self) -> String {
        let mut text = String::new();
        while let Some(event) = self.peek() {
            match event {
                Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                    text.push_str(t);
                    self.pos += 1;
                }
                Event::End(_) => break,
                _ => {
                    self.pos += 1;
                }
            }
        }
        text
    }

    fn parse_list(&mut self, start: Option<u64>) -> List {
        let ordered = start.is_some();
        let mut items = Vec::new();
        let mut ranges: Vec<Range<usize>> = Vec::new();

        while let Some(Event::Start(Tag::Item)) = self.peek() {
            let range = self.peek_range().unwrap_or(0..0);
            self.pos += 1;
            let blocks = self.parse_blocks();
            self.expect_end(TagEnd::Item);
            let task = self.pending_task.take();
            items.push(ListItem { task, blocks });
            ranges.push(range);
        }
        let end_range = self.peek_range().unwrap_or(0..0);
        self.expect_end(TagEnd::List(ordered));
        let _ = end_range;

        // A list is "loose" when a blank line separates its items (pi's
        // `token.loose`, which adds a blank line between items).
        let loose = ranges
            .iter()
            .skip(1)
            .any(|range| previous_line_is_blank(self.source, range.start));

        List {
            ordered,
            start: start.unwrap_or(1),
            loose,
            items,
        }
    }

    fn parse_table(&mut self, raw: String) -> Table {
        let mut header = Vec::new();
        let mut rows = Vec::new();

        if let Some(Event::Start(Tag::TableHead)) = self.peek() {
            self.pos += 1;
            while let Some(Event::Start(Tag::TableCell)) = self.peek() {
                self.pos += 1;
                let cell = self.parse_inlines();
                self.expect_end(TagEnd::TableCell);
                header.push(cell);
            }
            self.expect_end(TagEnd::TableHead);
        }

        while let Some(Event::Start(Tag::TableRow)) = self.peek() {
            self.pos += 1;
            let mut row = Vec::new();
            while let Some(Event::Start(Tag::TableCell)) = self.peek() {
                self.pos += 1;
                let cell = self.parse_inlines();
                self.expect_end(TagEnd::TableCell);
                row.push(cell);
            }
            self.expect_end(TagEnd::TableRow);
            rows.push(row);
        }

        self.expect_end(TagEnd::Table);

        Table { header, rows, raw }
    }
}

/// True when the source line right before `start` is blank (ignoring
/// blockquote markers), i.e. the block at `start` is preceded by a blank line.
fn previous_line_is_blank(source: &str, start: usize) -> bool {
    let bytes = source.as_bytes();
    let start = start.min(bytes.len());

    // Beginning of the line containing `start`.
    let mut line_start = start;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    if line_start == 0 {
        return false;
    }

    // Previous line, excluding its trailing newline.
    let previous_end = line_start - 1;
    let mut previous_start = previous_end;
    while previous_start > 0 && bytes[previous_start - 1] != b'\n' {
        previous_start -= 1;
    }

    is_blank_line(&source[previous_start..previous_end])
}

/// A line that is empty after stripping blockquote markers and whitespace.
fn is_blank_line(line: &str) -> bool {
    let mut rest = line.trim_start();
    while let Some(stripped) = rest.strip_prefix('>') {
        rest = stripped.trim_start();
    }
    rest.trim().is_empty()
}

/// Block-level tags that terminate an inline run.
fn is_block_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::Item
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::MetadataBlock(_)
    )
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_and_paragraphs() {
        let doc = Document::parse("# Title\n\nBody **bold** text\n");
        // heading, blank line (pi's `space` token), paragraph
        assert_eq!(doc.blocks.len(), 3);
        assert!(matches!(&doc.blocks[0], Block::Heading { level: 1, .. }));
        assert_eq!(doc.blocks[1], Block::Space);
        assert!(matches!(&doc.blocks[2], Block::Paragraph(_)));
    }

    #[test]
    fn parses_task_lists() {
        let doc = Document::parse("- [x] done\n- [ ] todo\n");
        let Block::List(list) = &doc.blocks[0] else {
            panic!("expected list")
        };
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[0].task, Some(true));
        assert_eq!(list.items[1].task, Some(false));
    }

    #[test]
    fn parses_tables() {
        let doc = Document::parse("| a | b |\n|---|---|\n| 1 | 2 |\n");
        let Block::Table(table) = &doc.blocks[0] else {
            panic!("expected table")
        };
        assert_eq!(table.header.len(), 2);
        assert_eq!(table.rows.len(), 1);
        assert!(table.raw.contains("| a | b |"));
    }

    #[test]
    fn parses_math() {
        let doc = Document::parse("Inline $a^2$ math\n");
        let Block::Paragraph(inlines) = &doc.blocks[0] else {
            panic!("expected paragraph")
        };
        assert!(inlines.iter().any(|i| matches!(i, Inline::Math { .. })));
    }

    #[test]
    fn emits_space_between_lists() {
        let doc = Document::parse("- a\n\n1. b\n");
        assert!(matches!(doc.blocks[0], Block::List(_)));
        assert_eq!(doc.blocks[1], Block::Space);
        assert!(matches!(doc.blocks[2], Block::List(_)));
    }

    #[test]
    fn quote_blank_line_is_space() {
        let doc = Document::parse("> a\n>\n> > b\n");
        let Block::Quote(inner) = &doc.blocks[0] else {
            panic!("expected quote")
        };
        assert!(inner.iter().any(|block| matches!(block, Block::Space)));
    }
}
