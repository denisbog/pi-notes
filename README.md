# pi-notes workspace

A Cargo **workspace** holding two Rust tools for working with pi sessions:

- **`pi-notes`** — a desktop GUI written in **Rust** with the **iced** GUI
  framework that:

- **Displays the last pi message and its thinking block** as rendered Markdown
  (headings, lists, quotes, code blocks, **tables**, and LaTeX **symbolic
  notation** converted to Unicode).
- **Stores notes** from the UI (save the current message + thinking as a
  Markdown note).
- **Browses stored notes** through a collapsible file tree, showing notes
  grouped by project.
- Is **callable from pi** as a `/notes` command, and generates per-session
  HTML usage reports with `/report` (via the `pi-session-inspector` crate).
- **`pi-session-inspector`** — a CLI that scans pi session logs and emits
  token-consumption / pricing / cache-loss reports (text, CSV, or a
  self-contained HTML page).

The GUI was built starting from the official iced examples (`markdown`,
`table`, `gallery`) in the local iced checkout at `~/projects/rust/iced`
(iced `0.14-dev`, which ships a markdown widget with table support).

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

### Symbolic notation

Iced's markdown widget has no native LaTeX support, so `pi-notes/src/symbols.rs`
pre-processes the raw Markdown before parsing, converting common TeX commands
(`\alpha`, `\sum`, `\mathbb{R}`, `\sqrt{x}`, `x^2`, `a \leq b`, …) into Unicode
(α, ∑, ℝ, √x, x², a ≤ b, …). Markdown **tables** are passed through untouched
and rendered natively by pulldown-cmark inside iced.

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
# build both tools (workspace)
cargo build --release

# install both binaries on PATH
install -m755 target/release/pi-notes ~/.local/bin/pi-notes
install -m755 target/release/pi-session-inspector ~/.local/bin/pi-session-inspector

# install the pi extension so `/notes` and `/report` work inside pi
mkdir -p ~/.pi/agent/extensions
cp extension/notes.ts ~/.pi/agent/extensions/notes.ts
```

> Building compiles the local iced checkout from source, so the first build
> takes a few minutes. Only the lightweight `tiny-skia` renderer is enabled.

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

## Project layout

```
pi-notes/
├── Cargo.toml                        # workspace: pi-notes + pi-session-inspector
├── extension/
│   └── notes.ts                      # pi extension registering /notes (GUI) and /report (HTML)
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
│       ├── symbols.rs  LaTeX -> Unicode symbolic-notation converter
│       └── tree.rs     flatten notes tree into visible rows
└── pi-session-inspector/             # CLI usage/price report tool
    ├── Cargo.toml
    ├── src/            (see REPORTING.md for its full docs)
    ├── docs/
    └── REPORTING.md
```

## Tests

```bash
cargo test        # runs tests for both workspace crates
cargo test -p pi-notes
cargo test -p pi-session-inspector
```

The `pi-notes` tests validate symbol conversion (including that tables are left
intact), tree-following session parsing, the editor round-trip, and note
save/browse (asserting notes land under `~/.pi/agent/notes/<project>/`).
