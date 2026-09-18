//! pi's markdown theme, ported from `theme/dark.json` / `theme/light.json`.
//!
//! pi resolves markdown colors through the theme keys `mdHeading`, `mdCode`, ...
//! (see `getMarkdownTheme()` in `dist/modes/interactive/theme/theme.js`).
//! [`Theme`] mirrors that mapping one-to-one, so the renderer produces the same
//! colors as the terminal UI.

use crate::text::{Rgb, Style};

/// Syntax-highlighting palette (`syntax*` theme keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxColors {
    pub comment: Rgb,
    pub keyword: Rgb,
    pub function: Rgb,
    pub variable: Rgb,
    pub string: Rgb,
    pub number: Rgb,
    pub type_: Rgb,
    pub operator: Rgb,
    pub punctuation: Rgb,
}

/// Everything the markdown renderer needs from a pi theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub dark: bool,
    /// Background of the document surface. For the dark theme this is the
    /// background pi actually renders on: the terminal's, i.e. Ghostty's
    /// built-in default (`#282c34`).
    pub background: Rgb,
    /// Background for toolbar/panel chrome.
    pub panel_background: Rgb,
    /// Text-selection highlight (pi's `selectedBg`).
    pub selection_background: Rgb,

    pub text: Rgb,
    pub muted: Rgb,

    pub md_heading: Rgb,
    pub md_link: Rgb,
    pub md_link_url: Rgb,
    pub md_code: Rgb,
    pub md_code_block: Rgb,
    pub md_code_block_border: Rgb,
    pub md_quote: Rgb,
    pub md_quote_border: Rgb,
    pub md_hr: Rgb,
    pub md_list_bullet: Rgb,

    /// `theme.codeBlockIndent ?? "  "`
    pub code_block_indent: usize,

    pub syntax: SyntaxColors,
}

impl Theme {
    /// pi's built-in dark theme (`theme/dark.json`).
    pub fn pi_dark() -> Self {
        Self {
            dark: true,
            // Ghostty's default background — what pi is actually drawn on in a
            // default terminal, exactly like the bundled font.
            background: Rgb::from_hex("#282c34"),
            panel_background: Rgb::from_hex("#343541"),
            selection_background: Rgb::from_hex("#3a3a4a"),
            text: Rgb::from_hex("#d4d4d4"),
            muted: Rgb::from_hex("#808080"),
            md_heading: Rgb::from_hex("#f0c674"),
            md_link: Rgb::from_hex("#81a2be"),
            md_link_url: Rgb::from_hex("#666666"),
            md_code: Rgb::from_hex("#8abeb7"),
            md_code_block: Rgb::from_hex("#b5bd68"),
            md_code_block_border: Rgb::from_hex("#808080"),
            md_quote: Rgb::from_hex("#808080"),
            md_quote_border: Rgb::from_hex("#808080"),
            md_hr: Rgb::from_hex("#808080"),
            md_list_bullet: Rgb::from_hex("#8abeb7"),
            code_block_indent: 2,
            syntax: SyntaxColors {
                comment: Rgb::from_hex("#6A9955"),
                keyword: Rgb::from_hex("#569CD6"),
                function: Rgb::from_hex("#DCDCAA"),
                variable: Rgb::from_hex("#9CDCFE"),
                string: Rgb::from_hex("#CE9178"),
                number: Rgb::from_hex("#B5CEA8"),
                type_: Rgb::from_hex("#4EC9B0"),
                operator: Rgb::from_hex("#D4D4D4"),
                punctuation: Rgb::from_hex("#D4D4D4"),
            },
        }
    }

    /// pi's built-in light theme (`theme/light.json`).
    pub fn pi_light() -> Self {
        Self {
            dark: false,
            background: Rgb::from_hex("#ffffff"),
            panel_background: Rgb::from_hex("#f8f8f8"),
            selection_background: Rgb::from_hex("#d0d0e0"),
            text: Rgb::from_hex("#1f2328"),
            muted: Rgb::from_hex("#6c6c6c"),
            md_heading: Rgb::from_hex("#9a7326"),
            md_link: Rgb::from_hex("#547da7"),
            md_link_url: Rgb::from_hex("#767676"),
            md_code: Rgb::from_hex("#5a8080"),
            md_code_block: Rgb::from_hex("#588458"),
            md_code_block_border: Rgb::from_hex("#6c6c6c"),
            md_quote: Rgb::from_hex("#6c6c6c"),
            md_quote_border: Rgb::from_hex("#6c6c6c"),
            md_hr: Rgb::from_hex("#6c6c6c"),
            md_list_bullet: Rgb::from_hex("#588458"),
            code_block_indent: 2,
            syntax: SyntaxColors {
                comment: Rgb::from_hex("#008000"),
                keyword: Rgb::from_hex("#0000FF"),
                function: Rgb::from_hex("#795E26"),
                variable: Rgb::from_hex("#001080"),
                string: Rgb::from_hex("#A31515"),
                number: Rgb::from_hex("#098658"),
                type_: Rgb::from_hex("#267F99"),
                operator: Rgb::from_hex("#000000"),
                punctuation: Rgb::from_hex("#000000"),
            },
        }
    }

    /// Base style applied to plain body text (`defaultTextStyle` in pi).
    pub fn body(&self) -> Style {
        Style::fg(self.text)
    }

    /// Overrides the document background (e.g. `--background '#1e1e1e'`).
    pub fn with_background(mut self, background: Rgb) -> Self {
        self.background = background;
        self
    }

    /// Heading style: H1 is bold+underline, everything else bold, all in
    /// `mdHeading`.
    pub fn heading(&self, level: u8) -> Style {
        let style = Style::fg(self.md_heading).bold();
        if level == 1 {
            style.underline()
        } else {
            style
        }
    }

    pub fn link(&self) -> Style {
        Style::fg(self.md_link).underline()
    }

    pub fn code(&self) -> Style {
        Style::fg(self.md_code)
    }

    pub fn code_block(&self) -> Style {
        Style::fg(self.md_code_block)
    }

    pub fn code_block_border(&self) -> Style {
        Style::fg(self.md_code_block_border)
    }

    pub fn quote(&self) -> Style {
        Style::fg(self.md_quote).italic()
    }

    pub fn quote_border(&self) -> Style {
        Style::fg(self.md_quote_border)
    }

    pub fn hr(&self) -> Style {
        Style::fg(self.md_hr)
    }

    pub fn list_bullet(&self) -> Style {
        Style::fg(self.md_list_bullet)
    }

    pub fn muted(&self) -> Style {
        Style::fg(self.muted)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::pi_dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_hex_parsing() {
        assert_eq!(Rgb::from_hex("#f0c674"), Rgb(240, 198, 116));
        assert_eq!(Rgb::from_hex("fff"), Rgb(255, 255, 255));
        assert_eq!(Rgb::from_hex("000080"), Rgb(0, 0, 128));
    }

    #[test]
    fn heading_levels() {
        let theme = Theme::pi_dark();
        assert!(theme.heading(1).bold && theme.heading(1).underline);
        assert!(theme.heading(2).bold && !theme.heading(2).underline);
    }
}
