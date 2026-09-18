# pi-notes workspace

A Cargo **workspace** holding three Rust tools for working with pi sessions:

- **`pi-notes`** — a desktop GUI written in **Rust** with the **iced** GUI
  framework that:

- **Displays the last pi message and its thinking block** as rendered Markdown
  (headings, lists, quotes, code blocks, **tables**, and inline LaTeX math),
  rendered by **`pi-mdview`** — the same pipeline as pi's TUI. Notes display
  exactly as they would in pi, including syntax-highlighted code blocks and
  pi's theme colors.
- **Stores notes** from the UI (save the current message + thinking as a
  Markdown note).
- **Browses stored notes** through a collapsible file tree, showing notes
  grouped by project.
- Is **callable from pi** as a `/notes` command, and generates per-session
  HTML usage reports with `/report` (via the `pi-session-inspector` crate).
- **`pi-mdview`** — a standalone **Markdown viewer** in Rust + iced that
  renders documents the way pi's TUI does: it is a structural port of pi's
  markdown pipeline (`marked` tokens → themed styled lines → width-aware
  wrapping → renderer), verified line-for-line against pi's own renderer with
  golden parity tests. It bundles **JetBrains Mono** — the font pi renders in
  under the default Ghostty setup — and defaults to Ghostty's `#282c34`
  background, so code, wrapping and table borders look the same as in the
  terminal. Text is selectable (drag / double-click / Ctrl+C), scrolling is
  eased rather than stepped, and the line height is a roomy 1.5×. Use it as a
  binary (`cargo run -p pi-mdview -- file.md`) or as a library. See
  [`pi-mdview/README.md`](pi-mdview/README.md).
- **`pi-session-inspector`** — a CLI that scans pi session logs and emits
  token-consumption / pricing / cache-loss reports (text, CSV, a
  self-contained HTML page, or the same data model as JSON for TUI front-ends).

The repo also ships three self-contained **pi TUI extensions** (TypeScript) that
save and browse the same Markdown notes from inside pi, and render the usage
report in the terminal — a Session Tree-style note picker (`/note`), a notes
browser/editor (`/notes-view`), and a report viewer (`/report-view`).
See [TUI extensions (inside pi)](#tui-extensions-inside-pi).

The GUI was built starting from the official iced examples (`markdown`,
`table`, `gallery`) in the local iced checkout at `~/projects/rust/iced`
(iced `0.14-dev`). Markdown rendering is no longer done by iced's markdown
widget: `pi-notes` renders through the `pi-mdview` crate, so its documents are
wrapped, themed and highlighted identically to pi's TUI.

---

## How it works

### Locating the current session

The `/notes` extension (see `extension/notes.ts`) obtains the session
of the current running pi instance from `ctx.sessionManager.getSessionFile()`
and passes it to the app explicitly as a `--session <file>` argument. It also
passes `--leaf <entry-id>` — the id of the message pi is currently displaying.

The app never scans the session directory or falls back to the most recently
updated file. Because pi sessions are **trees** and `/tree` lets you select any
previous message, the displayed position is not always the last entry: the app
walks the active branch path from the given leaf up to the root and shows the
most recent assistant message (and its thinking) on that path. Without a
`--leaf` argument it defaults to the last entry in file order (pi's default
leaf when a session is opened).

Session files are JSONL. The app parses the entries and independently extracts:

- **message** — the most recent non-empty `text` content block across assistant
  messages (the visible reply), and
- **thinking** — the most recent non-empty `thinking` content block (the
  reasoning, which pi often attaches to later tool-use turns).

### Markdown rendering

Markdown is rendered by the **`pi-mdview`** crate, the same renderer that
powers the standalone pi-mdview viewer. `pi-notes` feeds the raw Markdown
(message, thinking, notes) to `pi_mdview::Renderer` at the pane's actual
column width and maps the resulting styled lines onto an iced `rich_text` via
`pi_mdview::document_spans`. Rendering therefore matches pi's TUI: width-aware
wrapping, box-drawing tables, pi's dark theme colors, `syntect`-highlighted
code blocks and inline LaTeX math (`$\alpha^2$` → α²). Rendered documents are
memoised per `(source, width)` so resizing does not re-highlight code.

### Notes storage hierarchy

Notes are saved under the same convention pi uses for session directories:

```
~/.pi/agent/sessions/--home-denis-llm--/<timestamp>_<id>.jsonl   (pi sessions)
~/.pi/agent/notes/--home-denis-llm--/<timestamp>_<title>.md       (this app)
```

The `<project-dir>` is the working directory with `/` replaced by `-`, wrapped
in `--…--`, exactly mirroring pi. The left pane browses the whole
`~/.pi/agent/notes/` tree so you can explore notes from all your projects.

---

## Build & install

```bash
# build all tools (workspace)
cargo build --release

# install the binaries on PATH
install -m755 target/release/pi-notes ~/.local/bin/pi-notes
install -m755 target/release/pi-mdview ~/.local/bin/pi-mdview
install -m755 target/release/pi-session-inspector ~/.local/bin/pi-session-inspector

# view any markdown file the way pi renders it
pi-mdview README.md

# install the pi extensions so `/notes` (GUI), `/report` (HTML),
# `/report-view` (TUI report), `/note` and `/notes-view` (TUI) work inside pi
mkdir -p ~/.pi/agent/extensions
cp extension/notes.ts extension/session-notes.ts extension/notes-viewer.ts \
  extension/report-view.ts ~/.pi/agent/extensions/
cp -r extension/notes-shared ~/.pi/agent/extensions/notes-shared
# then run /reload inside pi
```

> Building compiles the local iced checkout from source, so the first build
> takes a few minutes. Only the lightweight `tiny-skia` renderer is enabled.
> `pi-mdview`'s renderer library can be built and tested without iced at all
> (`cargo test -p pi-mdview --no-default-features`).

## Usage

Inside a pi session, type:

```
/notes
```

The extension launches the app passing the current pi session file as
`--session <file>` plus `--leaf <id>` for the message pi is currently
displaying (the `/tree` selection). An explicit path can be given to open any
session:

```
/notes /path/to/session.jsonl
```

Generate an HTML usage report (token/cache/pricing) for the current session
with the `pi-session-inspector` crate in this workspace, stored next to the
notes files and overwritten on every run (reports are per session):

```
/report [path-to-session.jsonl]
```

The report is written to `~/.pi/agent/notes/<project>/usage-report.html`,
regenerated (overwritten) each time `/report` is run, and then **opened in
your default browser** automatically.

Render the same report **inside pi** (no browser) with:

```
/report-view [path-to-session.jsonl]
```

This asks `pi-session-inspector --json` for the exact data model the HTML page
embeds and draws it with pi's TUI components: the stats row, the log-scaled
fresh-input **histogram** (amber = large input, red = cache lost, `▲` markers),
the **cache-loss timeline** and **large-request** list, and a scrollable
**request timeline** (usage chips, `next call` estimate, thinking, assistant
text, tool calls and results) — see
[TUI extensions (inside pi)](#tui-extensions-inside-pi).

In the UI:

- The **thinking block is shown first**, displayed as a quotation (indented,
  bordered block), then the message underneath it. Click the **▶/▼ Thinking**
  button to collapse or expand the thinking block.
- **Save note** writes the current message + thinking to
  `~/.pi/agent/notes/<project>/` (optionally titled via the text field).
- The **left tree** shows stored notes (with a fixed **Stored notes** header);
  click a directory to expand/collapse and a `.md` file to view it.
- **Edit** (in the note-view header) opens the currently viewed note in your
  system editor (`$VISUAL`, then `$EDITOR`, then a known editor on `PATH`, else
  `xdg-open`). When the editor closes the note is reloaded automatically.
- **Rename** (in the note-view header) swaps the header for an inline input,
  pre-filled with the current file stem — type a new name and confirm with the
  **Rename** button (or **Cancel**). The file keeps its `.md` extension and
  location.
- **Remove** (in the note-view header) asks **"Remove this note?"** for
  confirmation before permanently deleting the file; **Cancel** (or starting a
  rename) dismisses the prompt.
- When no session is loaded, the viewer shows a static placeholder instead of
  empty panes (this also appears on standalone launch without `--session`).
- **Refresh** re-reads the session file.

You can also run it standalone:

```bash
pi-notes --session /path/to/session.jsonl
```

## TUI extensions (inside pi)

Two self-contained pi **TUI** extensions in `extension/` read/write the same
`~/.pi/agent/notes/<project>/` tree as the GUI:

- **`session-notes.ts`** — `/note` opens a filterable entry list styled like
  pi's built-in **Session Tree** (`/tree`): same title, key hints and
  `Type to search:` line. Navigate with `↑/↓` (page with `←/→`/`PgUp`/`PgDn`,
  branch with `ctrl+←/→`), copy with `ctrl+x`, edit entry labels with
  `shift+l` (toggle label times with `shift+t`). Switch views with the tree
  filters `ctrl+d/t/u/l/a` or cycle with `ctrl+o` (`ctrl+shift+o` backwards),
  and type to search. `Tab` toggles an entry; selected entries **stay visible
  even when they no longer match** the filter/search. `ctrl+p` toggles a peek
  preview of the highlighted entry (full Markdown): on wide terminals the list
  splits vertically with the preview on the right, otherwise the preview
  replaces the list. While peeking, `Enter` moves focus into the preview so
  `↑/↓`/`PgUp`/`PgDn` scroll it and `Esc` returns to the list; the
  filter/cycle and copy/label shortcuts keep working while peeking, so you can
  decide whether to include an entry. `Enter` saves the selected entries when
  not peeking, `ctrl+w` saves from anywhere, and cancelling the title prompt
  saves nothing. The note is written to
  `~/.pi/agent/notes/<project>/<timestamp>_<title>.md`.
- **`notes-viewer.ts`** — `/notes-view` browses stored notes with the same
  Session Tree-style chrome (title, wrapping key hints, `Type to search:`):
  a notes tree on the left, the selected note rendered as Markdown on the
  right. All actions are on ctrl+ keys:
  - `↑/↓` move · `←/→` fold · `PgUp/PgDn` page · `ctrl+←/→` branch ·
    `ctrl+x` copy · `ctrl+r` rename · `ctrl+d` delete (confirm with
    `enter`/`ctrl+d`) · `ctrl+s` scope, `ctrl+o` cycle (all/current project) ·
    `ctrl+e` edit
  - `Enter` opens a note and moves focus to the content pane, where `↑/↓`
    scroll the note and `Esc` returns to the list.
  - type to **search** (matches file names, projects, and note bodies);
    `Esc` clears the search, then closes the viewer.
  - `ctrl+e` opens the selected note in `nvim` inside a new **herdr** tab
    (requires pi to run inside herdr). When pi quits, that tab is closed
    again, and the note is re-read from disk the next time you interact with
    the viewer.
- **`report-view.ts`** — `/report-view` renders the `pi-session-inspector`
  usage report **inside pi**, using the exact data model the HTML `/report`
  page embeds (fetched with `pi-session-inspector --json`):
  - a stats row (input, cached, output, reasoning, requests, large input,
    cache-loss events, lost tokens/cost, end-of-session context, cost,
    duration), a log-scaled fresh-input **histogram** (amber = large input,
    red = cache lost, plus an `▲` marker row) and a compact legend;
  - the **cache-loss timeline** (per event: gap, `cache A → B (D%)`, re-sent
    tokens, cost) and the top-five **large requests** list;
  - the **request timeline**: a scrollable list on the left (index, time,
    model, input/cached/output/cost) and the selected request's detail on the
    right (usage chips, the `next call` re-sent-token estimate, thinking,
    assistant text rendered as Markdown, and every tool call with its JSON
    arguments and results).

  Keys: `↑/↓` move · `enter`/`tab` focus detail (then `↑/↓`/`PgUp`/`PgDn`
  scroll) · `n`/`N` jump to next/previous flagged request (large input or
  cache loss) · `[`/`]` switch session when the selector matched several ·
  `h`/`s`/`f` toggle the histogram / stats / flagged lists · `y` copy the
  request as Markdown to the clipboard · `r` regenerate · `Esc` back/close.

Install the TUI extensions next to the GUI extension and reload pi:

```bash
cp extension/session-notes.ts extension/notes-viewer.ts \
  extension/report-view.ts ~/.pi/agent/extensions/
cp -r extension/notes-shared ~/.pi/agent/extensions/notes-shared
# then run /reload inside pi
```

The extensions share small note-storage, formatting and Markdown-preview
helpers (`notes-shared/shared.ts`, in a subdirectory so pi does not load it as
an extension). `session-notes` and `notes-viewer` use the same preview "peek"
view; while peeking / viewing content, the footer shows the current position as
`lines 1-18/60`.

## Project layout

```
pi-notes/
├── Cargo.toml                        # workspace: pi-notes + pi-mdview + pi-session-inspector
├── extension/
│   ├── notes.ts                      # pi extension registering /notes (GUI) and /report (HTML)
│   ├── session-notes.ts              # /note: save session entries as Markdown notes
│   ├── notes-viewer.ts               # /notes-view: browse/view/rename/delete/edit stored notes
│   ├── report-view.ts                # /report-view: render the usage report inside pi
│   └── notes-shared/
│       └── shared.ts                 # shared storage, formatting and peek/Markdown helpers
├── pi-notes/                         # iced GUI
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs     entry point (calls App::run)
│       ├── app.rs      iced application: state, update, view
│       ├── editor.rs   open the current note in the system editor
│       ├── session.rs  parse pi JSONL session trees; follow the active path
│       │               to the selected leaf (message + thinking)
│       ├── notes.rs    note storage + file-tree loading (mirrors pi hierarchy);
│       │               rename + delete note helpers
│       └── tree.rs     flatten notes tree into visible rows
├── pi-mdview/                        # standalone Markdown viewer + renderer used by pi-notes
│   ├── Cargo.toml
│   ├── README.md                     # architecture, parity tests, known differences
│   ├── src/
│   │   ├── lib.rs      public API (render_markdown, Renderer, Theme, ...)
│   │   ├── md.rs       pulldown-cmark events -> Document (pi's `marked` tokens)
│   │   ├── render.rs   renderToken/renderList/renderTable port + wrapping
│   │   ├── text.rs     Line/Span/Style model + wrapTextWithAnsi port
│   │   ├── theme.rs    pi dark/light markdown + syntax palettes
│   │   ├── highlight.rs syntect + generated .tmTheme (pi's highlight.js theme)
│   │   ├── latex.rs    LaTeX -> Unicode math
│   │   ├── widget.rs   Line -> iced `rich_text` spans (used by pi-notes + the viewer)
│   │   ├── app.rs      viewer front-end (rich_text + scrollable + selection)
│   │   └── main.rs     `pi-mdview` binary
│   ├── tests/parity.rs   golden + live parity against pi's renderer
│   ├── examples/         dump_events, dump_scopes, dump_highlight, render_file
│   └── tools/            pi_render.mjs oracle, parity fixture, golden files
└── pi-session-inspector/             # CLI usage/price report tool
    ├── Cargo.toml
    ├── src/            (see REPORTING.md for its full docs)
    ├── docs/
    └── REPORTING.md
```

## Tests

```bash
cargo test        # runs tests for all workspace crates
cargo test -p pi-notes
cargo test -p pi-mdview --no-default-features   # headless (no iced/syntect)
cargo test -p pi-session-inspector
```

The `pi-notes` tests validate tree-following session parsing, the editor
round-trip, and note save/browse (asserting notes land under
`~/.pi/agent/notes/<project>/`).

The `pi-mdview` tests validate the markdown pipeline line-for-line against
pi's own TUI renderer (golden files plus a live comparison when Node and
pi-tui are available), plus style assertions for every theme key.
