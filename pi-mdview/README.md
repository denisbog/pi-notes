# pi-mdview

A markdown viewer for **Rust + iced** that renders documents the way
[**pi**](https://github.com/badlogic/pi-mono)'s TUI does.

pi has its own markdown pipeline (`packages/tui/src/components/markdown.ts`):
`marked` parses the document, a component walks the tokens and turns them into
an array of styled terminal lines, and the TUI renderer paints that array. This
crate ports that pipeline **structure for structure** into Rust, then hands the
resulting lines to iced.

```text
markdown ──parse──▶ Document (blocks + inlines)              src/md.rs
                     │
                     ├─ inline/block styles ──▶ Theme        src/theme.rs
                     ├─ code blocks ──────────▶ Highlighter  src/highlight.rs
                     └─ math ─────────────────▶ LaTeX        src/latex.rs
                     │
                  layout: width-aware wrapping, lists, quotes, tables
                     ▼
                  Vec<Line> = Vec<Vec<Span + Style>>          src/render.rs + src/text.rs
                     │
                     └─ iced ──▶ one `rich_text` in a scrollable   src/app.rs
                                          (span bridge: src/widget.rs)
```

`Line` is the Rust equivalent of pi's `string[]` of ANSI lines. Keeping that
intermediate representation toolkit-independent means the renderer is headless
(it even emits ANSI via `Line::to_ansi()`), fully testable, and the iced layer
stays thin.

## pi ↔ pi-mdview map

| pi (TypeScript) | pi-mdview (Rust) |
|---|---|
| `marked` lexer + custom LaTeX/strikethrough tokenizers | `pulldown-cmark` events → `Document` (`md.rs`), with explicit `space` blocks like `marked` |
| `theme/dark.json`, `theme/light.json` | `Theme::pi_dark()`, `Theme::pi_light()` (`theme.rs`) |
| `getMarkdownTheme()` | `Theme::{heading, link, code, code_block, quote, hr, list_bullet, …}` |
| `highlight.js` + `buildCliHighlightTheme()` | `syntect` + a `.tmTheme` generated from the `syntax*` palette (`highlight.rs`) |
| `renderLatex()` in `latex.js` | `latex::render()` (`latex.rs`) — inline Unicode, see limitations |
| `renderToken` / `renderInlineTokens` | `Renderer::render_block` / `write_inlines` (`render.rs`) |
| `renderList` | `Renderer::render_list` |
| `renderTable` | `Renderer::render_table` (same width-allocation algorithm) |
| `wrapTextWithAnsi` | `text::wrap_line` (word wrap, long-word breaking, CJK, trailing-space trimming) |
| `paddingX` / `paddingY` | `RenderOptions` |
| terminal font (JetBrains Mono under Ghostty) | bundled `fonts/JetBrainsMono-*.ttf` (`fonts.rs`) |

## Usage

```bash
# view a file (window opens with a demo document when no file is given)
cargo run -p pi-mdview -- README.md

# light palette
cargo run -p pi-mdview -- README.md --light

# different terminal background (#rrggbb)
cargo run -p pi-mdview -- README.md --background '#1e1e1e'
```

The toolbar has: a reload button, column width −/+, fit-to-window, dark/light
toggle, font size and a Copy button. Links are clickable (`xdg-open` / `open` /
`start`).

### As a library

```rust
use pi_mdview::{render_markdown, plain_lines, Renderer, Theme};

// pi's dark theme, 80 columns
let lines = render_markdown("# hello\n\nsome **bold** text", 80);
assert_eq!(lines[0].plain().trim_end(), "hello");

// or keep a renderer around and re-render on resize
let mut renderer = Renderer::new(Theme::pi_dark());
let lines = renderer.render(&source, 100);
```

With the `gui` feature (enabled by default) the `widget` module maps rendered
lines onto iced `rich_text` spans — the same span mapping the viewer uses for
its document, exposed so `pi-notes` (and other iced applications) can embed
pi-mdview's rendering:

```rust
use pi_mdview::{document_spans, fonts};

let lines = pi_mdview::render_markdown(source, columns);
let spans = document_spans(&lines, &Theme::pi_dark(), fonts::monospace(), None);
let document = iced::widget::rich_text(spans)
    .font(fonts::monospace())
    .size(14.0)
    .line_height(iced::widget::text::LineHeight::Relative(1.447))
    .width(iced::Length::Shrink)
    .wrapping(iced::widget::text::Wrapping::None);
```

Cargo features:

- `gui` (default) — the iced front-end and the `pi-mdview` binary.
- `syntax` (default) — syntax-highlighted code blocks via `syntect`. Without it
  code blocks use the single `mdCodeBlock` color, exactly like pi does for an
  unknown language.

Headless build (no iced, no syntect): `cargo test -p pi-mdview --no-default-features`.

## Font

pi has no font setting of its own — it draws its UI in the **terminal's** font.
This machine runs **Ghostty** with an empty config, and Ghostty's built-in
default font is **JetBrains Mono** (Ghostty embeds it), so what pi shows on
screen is JetBrains Mono. To match it, `pi-mdview` bundles JetBrains Mono
(`fonts/`: static Regular/Bold/Italic/BoldItalic, SIL OFL 1.1, see
`fonts/OFL.txt`) and registers it with iced as the default family. JetBrains
Mono is exactly 0.6 em wide per cell and covers the box-drawing block, so table
borders and wrapping line up exactly like they do in the terminal.

Use a different terminal font with:

```bash
pi-mdview README.md --font-family "Fira Code"              # installed family
pi-mdview README.md --font ~/fonts/MyTerm.ttf --font-family "My Term"
```

The bundled family name is asserted against the faces' `name` tables in a unit
test, because a wrong family name makes iced silently fall back to a system
font.

## Background

For the same reason there is no pi font, there is no pi background — the
terminal paints it. The dark theme therefore defaults to **`#282c34`**, which is
Ghostty's built-in default background (and the light theme to `#ffffff`).
Override it for a different terminal theme with `--background '#rrggbb'`.

The text-selection highlight uses pi's `selectedBg` (`#3a3a4a` dark /
`#d0d0e0` light), and the toolbar uses pi's `userMessageBg`.

## Viewer behaviour (iced front-end)

iced's `rich_text` has no text-selection support, so the viewer implements the
shell interactions itself on top of the rendered character grid (`src/app.rs`):

- **Selection** — drag to select, double-click to select a word, `Ctrl`/`Cmd`+`A`
to select all, `Esc` to clear. Selected runs get a `background` highlight while
the `rich_text` spans are built. `Ctrl`/`Cmd`+`C` or the toolbar's **Copy**
button puts the selection on the clipboard. Positions are mapped from pixels to
document offsets using the monospace grid (0.6 em per cell, `unicode-width` for
wide characters).
- **Smooth scrolling** — the wheel step iced uses is a hard-coded 60 px per
notch. The viewer captures wheel events with a `mouse_area` and eases the
scrollable's offset towards a target one frame at a time via `scroll_to`. One
wheel notch scrolls exactly one text line (pixel deltas from trackpads are used
as-is). A subscription to `window::frames()` is only active while the animation
is in flight, so an idle viewer uses no CPU. Scrollbar drags and keyboard
scrolling still work and are adopted by the animation.
- **Line height** — `1.5 ×` font size (iced's default is `1.3`), applied both to
the `rich_text` and to the selection/scroll math so the grid stays exact.

## Rendering parity with pi

The port is verified against pi's actual renderer, not just against eyeballing:

- `tools/pi_render.mjs` runs pi's `Markdown` component with an *identity theme*
  (so the output is the pure structure pi produces) and prints the lines as JSON.
- `tools/update-golden.sh` stores its output as `tools/golden/pi-<width>.txt`.
- `tests/parity.rs` checks two things:
  1. `matches_pi_golden_files` — our renderer equals the checked-in golden
     files at widths 40/60/80/100 (works without Node).
  2. `matches_live_pi_renderer` — re-runs pi's renderer live and compares again
     (skipped when Node/pi-tui are missing; set `PI_TUI_PATH` to the
     `@earendil-works/pi-tui` package directory).

```bash
cargo test -p pi-mdview --no-default-features
PI_TUI_PATH=~/.nvm/versions/node/*/lib/node_modules/@earendil-works/pi-coding-agent/node_modules/@earendil-works/pi-tui \
  cargo test -p pi-mdview --no-default-features matches_live

# regenerate goldens after changing the renderer
PI_TUI_PATH=... pi-mdview/tools/update-golden.sh 40 60 80 100
```

The fixture `tools/parity.md` covers headings, wrapping, nested/ordered/task
lists, loose lists, blockquotes (including nested), code fences with and
without a language, tables at narrow widths, rules and inline styling.
`tools/sample.md` is the "everything" sample used for manual comparison.

## Known differences from pi

- **Display math.** pi ships a full 2-D LaTeX layout engine
  (`latex.js`, ~1300 lines) that stacks fractions and placed limits
  (`∑` with limits above/below). `latex.rs` converts math to inline Unicode
  instead (`\frac{a}{b}` → `a/b`). This is the only structural difference the
  parity fixture excludes; inline math such as `\alpha^2 + \beta` matches.
- **Images.** pi can inline images through Kitty/iTerm2 graphics protocols;
  they are not rendered here.
- **Hyperlinks.** pi emits OSC 8 sequences when the terminal supports them;
  the GUI makes link text clickable instead of printing the URL in parentheses.
- **Syntax highlighting coverage.** pi registers ~20 languages eagerly and the
  rest lazily via highlight.js; `syntect` ships its own grammar set, and a few
  tokens are categorized differently (`u32` in Rust is scoped like a
  `storage.type` in syntect while highlight.js calls it a built-in type).
  The palette and the scope→color mapping are otherwise the same.
