//! Syntax highlighting, the Rust counterpart of pi's `highlight.js` pipeline.
//!
//! pi wraps `highlight.js` (languages registered eagerly, the rest loaded
//! lazily) and colors tokens through a `cli-highlight` theme built from the pi
//! theme keys `syntaxKeyword`, `syntaxString`, ... (see `buildCliHighlightTheme`
//! in `theme.js`). We do the same with `syntect`, feeding it a theme generated
//! from the same palette.

#[cfg(feature = "syntax")]
use crate::text::Rgb;
use crate::text::{Span, Style};
#[cfg(feature = "syntax")]
use crate::theme::SyntaxColors;
use crate::theme::Theme;

/// Highlighted line: a sequence of styled spans.
pub type HighlightedLine = Vec<Span>;

/// A source-code highlighter.
pub struct Highlighter {
    #[cfg(feature = "syntax")]
    inner: syntect::parsing::SyntaxSet,
    #[cfg(feature = "syntax")]
    theme: syntect::highlighting::Theme,
    /// Fallback color for code without a known language.
    fallback: Style,
}

impl Highlighter {
    /// Builds a highlighter. Returns `None` when the `syntax` feature is
    /// disabled or the bundled syntax definitions fail to load.
    pub fn new(theme: &Theme) -> Option<Self> {
        let fallback = theme.code_block();
        #[cfg(not(feature = "syntax"))]
        {
            let _ = theme;
            Some(Self { fallback })
        }
        #[cfg(feature = "syntax")]
        {
            let syntaxes = syntect::parsing::SyntaxSet::load_defaults_newlines();
            let theme_set = syntect::highlighting::ThemeSet::load_from_reader(
                &mut std::io::Cursor::new(tm_theme(&theme.syntax)),
            )
            .ok()?;
            Some(Self {
                inner: syntaxes,
                theme: theme_set,
                fallback,
            })
        }
    }

    /// True when `lang` resolves to a real grammar (pi skips highlighting for
    /// unknown languages instead of guessing).
    pub fn supports(&self, lang: &str) -> bool {
        #[cfg(not(feature = "syntax"))]
        {
            let _ = lang;
            false
        }
        #[cfg(feature = "syntax")]
        {
            self.inner
                .find_syntax_by_token(lang)
                .map(|syntax| syntax.name != "Plain Text")
                .unwrap_or(false)
        }
    }

    /// Highlights `code`, returning one [`Span`] list per line. Falls back to
    /// the plain `mdCodeBlock` style when the language is unknown.
    pub fn highlight(&self, code: &str, lang: Option<&str>) -> Vec<HighlightedLine> {
        match lang {
            Some(lang) if self.supports(lang) => self.highlight_known(code, lang),
            _ => code
                .split('\n')
                .map(|line| vec![Span::new(line, self.fallback)])
                .collect(),
        }
    }

    #[cfg(feature = "syntax")]
    fn highlight_known(&self, code: &str, lang: &str) -> Vec<HighlightedLine> {
        use syntect::easy::HighlightLines;
        use syntect::util::LinesWithEndings;

        let Some(syntax) = self.inner.find_syntax_by_token(lang) else {
            return self.highlight(code, None);
        };

        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut lines = Vec::new();

        for line in LinesWithEndings::from(code) {
            let ranges = highlighter
                .highlight_line(line, &self.inner)
                .unwrap_or_default();
            let mut spans = Vec::new();
            for (style, text) in ranges {
                let text = text.trim_end_matches(['\n', '\r']);
                if text.is_empty() {
                    continue;
                }
                spans.push(Span::new(text, syntect_style(style)));
            }
            lines.push(spans);
        }

        if lines.is_empty() {
            lines.push(Vec::new());
        }
        lines
    }

    #[cfg(not(feature = "syntax"))]
    fn highlight_known(&self, code: &str, _lang: &str) -> Vec<HighlightedLine> {
        self.highlight(code, None)
    }
}

/// Converts a syntect style to our toolkit-independent [`Style`].
#[cfg(feature = "syntax")]
fn syntect_style(style: syntect::highlighting::Style) -> Style {
    use syntect::highlighting::FontStyle;

    let mut out = Style::new();
    if style.foreground.a != 0 {
        out.fg = Some(Rgb(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        ));
    }
    if style.font_style.contains(FontStyle::BOLD) {
        out.bold = true;
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        out.italic = true;
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        out.underline = true;
    }
    out
}

/// The `.tmTheme` equivalent of pi's `buildCliHighlightTheme()`. Generated from
/// the `syntax*` theme keys instead of being hardcoded, so both built-in pi
/// palettes work.
#[cfg(feature = "syntax")]
fn tm_theme(colors: &SyntaxColors) -> String {
    let comment = colors.comment.to_hex();
    let keyword = colors.keyword.to_hex();
    let function = colors.function.to_hex();
    let variable = colors.variable.to_hex();
    let string = colors.string.to_hex();
    let number = colors.number.to_hex();
    let type_ = colors.type_.to_hex();
    let operator = colors.operator.to_hex();
    let punctuation = colors.punctuation.to_hex();

    // Order matters: syntect lets later rules win on equally specific scopes.
    let mut rules = String::new();
    let mut push = |name: &str, scope: &str, fg: &str, extra: &str| {
        rules.push_str(&format!(
            "<dict><key>name</key><string>{name}</string>\
             <key>scope</key><string>{scope}</string>\
             <key>settings</key><dict><key>foreground</key><string>{fg}</string>{extra}</dict></dict>"
        ));
    };

    push("Comment", "comment", &comment, "");
    push("String", "string", &string, "");
    push("Regex", "string.regexp", &string, "");
    push("Number", "constant.numeric", &number, "");
    // pi maps highlight.js `literal` to the number color.
    push(
        "Literal",
        "constant.language, constant.other, support.constant",
        &number,
        "",
    );
    push(
        "Type",
        "entity.name.type, entity.name.class, entity.name.struct, support.type, support.class, support.macro",
        &type_,
        "",
    );
    push(
        "Function",
        "entity.name.function, support.function, variable.function, meta.function-call",
        &function,
        "",
    );
    push(
        "Variable",
        "variable, entity.other.attribute-name",
        &variable,
        "",
    );
    // `storage` covers `fn`/`def`/`function`/`let`/`const`/`int`, which
    // highlight.js reports as keywords; syntect also uses it for some built-in
    // type names, which is the one place these grammars disagree.
    push(
        "Keyword",
        "keyword, storage, storage.type, storage.modifier, keyword.other",
        &keyword,
        "",
    );
    push("Operator", "keyword.operator", &operator, "");
    push("Punctuation", "punctuation", &punctuation, "");
    push(
        "Emphasis",
        "markup.italic",
        &operator,
        "<key>fontStyle</key><string>italic</string>",
    );
    push(
        "Strong",
        "markup.bold",
        &operator,
        "<key>fontStyle</key><string>bold</string>",
    );

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
<key>name</key><string>pi</string>
<key>settings</key>
<array>
<dict><key>settings</key><dict>
<key>foreground</key><string>{operator}</string>
<key>background</key><string>#1E1E1E</string>
</dict></dict>
{rules}
</array>
</dict>
</plist>
"#
    )
}

#[cfg(all(test, feature = "syntax"))]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn highlights_rust_keywords() {
        let theme = Theme::pi_dark();
        let highlighter = Highlighter::new(&theme).expect("highlighter");
        assert!(highlighter.supports("rust"));
        let lines = highlighter.highlight("fn main() {}\n", Some("rust"));
        assert_eq!(lines.len(), 1);
        let has_keyword_color = lines[0]
            .iter()
            .any(|span| span.style.fg == Some(theme.syntax.keyword));
        assert!(has_keyword_color, "expected a keyword-colored span");
    }

    #[test]
    fn unknown_language_falls_back() {
        let theme = Theme::pi_dark();
        let highlighter = Highlighter::new(&theme).expect("highlighter");
        let lines = highlighter.highlight("hello\n", Some("not-a-language"));
        assert_eq!(lines[0][0].style.fg, Some(theme.md_code_block));
    }
}
