//! `pi-mdview` — a markdown viewer for Rust/iced that renders documents the way
//! pi does.
//!
//! pi's TUI has its own markdown pipeline: `marked` parses the document, a
//! component walks the tokens and emits an array of styled terminal lines, and
//! the TUI renderer paints that array. This crate ports that pipeline
//! structure one-to-one:
//!
//! ```text
//! markdown ──parse──▶ Document (blocks/inlines)         [md.rs]
//!                       │
//!                       ├─ inline styles ──▶ Theme       [theme.rs]
//!                       ├─ code blocks ────▶ Highlighter [highlight.rs]
//!                       └─ math ───────────▶ LaTeX       [latex.rs]
//!                       │
//!                    layout (width-aware wrap, tables, lists, quotes)
//!                       ▼
//!                    Vec<Line> = Vec<Vec<Span + Style>>   [render.rs, text.rs]
//!                       │
//!                       └─ iced ──▶ rich_text spans       [app.rs]
//! ```
//!
//! The intermediate [`Line`] representation is toolkit-independent: it is the
//! rust equivalent of pi's `string[]` of ANSI-styled lines, which keeps the
//! renderer testable and lets the iced layer stay thin.

pub mod highlight;
pub mod latex;
pub mod md;
pub mod render;
pub mod text;
pub mod theme;

#[cfg(feature = "gui")]
pub mod app;
#[cfg(feature = "gui")]
pub mod fonts;

pub use md::{Block, Document, Inline, List, ListItem, Table};
pub use render::{RenderOptions, Renderer};
pub use text::{visible_width, wrap_line, wrap_lines, Line, Rgb, Span, Style};
pub use theme::{SyntaxColors, Theme};

/// Renders `source` at `width` columns with pi's default (dark) theme.
pub fn render_markdown(source: &str, width: usize) -> Vec<Line> {
    Renderer::new(Theme::pi_dark()).render(source, width)
}

/// Renders `source` at `width` columns with an explicit theme.
pub fn render_markdown_with(theme: Theme, source: &str, width: usize) -> Vec<Line> {
    Renderer::new(theme).render(source, width)
}

/// The plain text of every rendered line (styles dropped).
pub fn plain_lines(lines: &[Line]) -> Vec<String> {
    lines.iter().map(Line::plain).collect()
}
