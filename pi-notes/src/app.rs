//! The iced application: state, update loop and view.
//!
//! Layout
//! ┌──────────────────────────────────────────────────────────┐
//! │ toolbar: pi-notes · project dir · [Refresh] [Save note]  │
//! ├───────────────┬──────────────────────────────────────────┤
//! │ notes tree    │ tabs [Message|Thinking]  · source label  │
//! │ (left pane)   │──────────────────────────────────────────┤
//! │               │ pi-mdview renderer (matches pi's TUI)    │
//! ├───────────────┴──────────────────────────────────────────┤
//! │ status bar                                               │
//! └──────────────────────────────────────────────────────────┘

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
    button, center, column, container, mouse_area, opaque, responsive, rich_text, row, rule,
    scrollable, stack, text, text_input, Space,
};
use iced::{window, Color, Element, Length, Task, Theme};

use pi_mdview::fonts;
use pi_mdview::{Line as MdLine, RenderOptions, Renderer, Theme as MarkdownTheme};

use crate::notes::{self, TreeNode};
use crate::session;
use crate::tree;

/// Whether the (collapsible) thinking block is currently shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThinkingState {
    Expanded,
    Collapsed,
}

/// The kind of content shown in each [`pane_grid`] pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Tree,
    Viewer,
}

#[derive(Debug, Clone)]
enum Message {
    ToggleThinking,
    RefreshSession,
    SaveNote,
    EditCurrent,
    Edited(Result<bool, String>),
    NodeClicked(PathBuf),
    TitleChanged(String),
    LinkClicked(String),
    RenameNote,
    RenameChanged(String),
    RenameConfirm,
    CancelRename,
    RemoveNote,
    ConfirmRemove,
    CancelRemove,
    Resized(pane_grid::ResizeEvent),
    FilterChanged(String),
}

pub struct App {
    session: Option<session::Session>,
    cwd: String,
    thinking_state: ThinkingState,

    /// Raw markdown of the current pi session's reply and thinking block.
    message_source: String,
    thinking_source: String,
    notes_root: PathBuf,
    tree_root: TreeNode,
    expanded: HashSet<PathBuf>,
    rows: Vec<tree::Row>,
    selected_note: Option<PathBuf>,
    leaf_id: Option<String>,

    renaming: bool,
    rename_input: String,
    confirming_remove: bool,
    filter: String,
    filter_error: bool,

    viewing_note: bool,
    /// Raw markdown of the note currently open in the viewer.
    note_source: Option<String>,

    title: String,
    status: String,
    source_label: String,
    theme: Theme,

    /// pi-mdview's renderer (the engine behind the pi-mdview viewer) plus a
    /// cache of rendered documents keyed by `(source hash, columns)`.
    markdown: MarkdownRenderer,

    panes: pane_grid::State<Pane>,
}

/// Font/layout constants matching the pi-mdview viewer, so pi-notes renders on
/// the same character grid as pi's TUI.
const FONT_SIZE: f32 = 14.0;
/// Advance width of JetBrains Mono, in ems (it is exactly 0.6).
const MONOSPACE_ADVANCE: f32 = 0.6;
/// Line height factor (same as the pi-mdview viewer).
const LINE_HEIGHT_RATIO: f32 = 1.447;

/// Number of markdown columns that fit in `width` logical pixels.
fn columns_for_width(width: f32) -> usize {
    let advance = (FONT_SIZE * MONOSPACE_ADVANCE).max(1.0);
    ((width / advance).floor() as usize).clamp(20, 400)
}

/// Wraps pi-mdview's [`Renderer`] and memoises its output. iced re-runs a
/// `responsive` closure on every layout pass, and pi's renderer (which runs
/// `syntect` over code blocks) is far too expensive to redo each time.
struct MarkdownRenderer {
    renderer: Renderer,
    theme: MarkdownTheme,
    cache: RefCell<HashMap<(u64, usize), Vec<MdLine>>>,
}

impl MarkdownRenderer {
    fn new() -> Self {
        let theme = MarkdownTheme::pi_dark();
        // Match the viewer: it applies padding and line height itself, and
        // does not pad lines to the full width.
        let renderer = Renderer::new(theme).options(RenderOptions {
            padding_x: 0,
            padding_y: 0,
            pad_to_width: false,
            render_latex: true,
        });
        Self {
            renderer,
            theme,
            cache: RefCell::new(HashMap::new()),
        }
    }

    fn lines(&self, source: &str, columns: usize) -> Vec<MdLine> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut hasher);
        let key = (hasher.finish(), columns);

        if let Some(lines) = self.cache.borrow().get(&key) {
            return lines.clone();
        }

        let lines = self.renderer.render(source, columns);
        let mut cache = self.cache.borrow_mut();
        // A handful of recent widths is plenty; resizing would otherwise grow
        // the cache without bound.
        if cache.len() >= 32 {
            cache.clear();
        }
        cache.insert(key, lines.clone());
        lines
    }
}

impl App {
    pub fn run() -> iced::Result {
        // Register the terminal font pi renders with (JetBrains Mono) so the
        // renderer's 0.6 em cell grid and box-drawing tables line up.
        let mut application = iced::application(App::new, App::update, App::view)
            .theme(App::theme)
            .title("pi-notes")
            .default_font(fonts::monospace())
            .window(window::Settings {
                size: iced::Size::new(1180.0, 760.0),
                ..Default::default()
            });
        for bytes in fonts::all() {
            application = application.font(bytes);
        }
        application.run()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn new() -> (Self, Task<Message>) {
        let notes_root = notes::notes_root();
        let mut app = App {
            session: None,
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            thinking_state: ThinkingState::Expanded,
            message_source: String::new(),
            thinking_source: String::new(),
            notes_root,
            tree_root: TreeNode {
                name: "notes".into(),
                path: notes::notes_root(),
                is_dir: true,
                children: Vec::new(),
            },
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected_note: None,
            leaf_id: None,
            renaming: false,
            rename_input: String::new(),
            confirming_remove: false,
            filter: String::new(),
            filter_error: false,
            viewing_note: false,
            note_source: None,
            title: String::new(),
            status: String::new(),
            source_label: String::from("no session loaded"),
            theme: Theme::TokyoNight,
            markdown: MarkdownRenderer::new(),
            panes: pane_grid::State::with_configuration(
                pane_grid::Configuration::Split {
                    axis: pane_grid::Axis::Vertical,
                    ratio: 0.30,
                    a: Box::new(pane_grid::Configuration::Pane(Pane::Tree)),
                    b: Box::new(pane_grid::Configuration::Pane(Pane::Viewer)),
                },
            ),
        };

        // Optional --session <path> and --leaf <entry-id> overrides. The
        // extension passes both: --session is the session file and --leaf is
        // the id of the message pi is currently displaying (the /tree
        // position), which is not always the latest entry.
        let mut override_path: Option<PathBuf> = None;
        let mut leaf_id: Option<String> = None;
        let args: Vec<String> = std::env::args().collect();
        if let Some(pos) = args.iter().position(|a| a == "--session") {
            override_path = args.get(pos + 1).map(PathBuf::from);
        }
        if let Some(pos) = args.iter().position(|a| a == "--leaf") {
            leaf_id = args.get(pos + 1).cloned();
        }

        let session_file = override_path
            .filter(|p| p.exists())
            .or_else(session::resolve_session_file);
        app.leaf_id = leaf_id.clone();
        match session_file {
            Some(path) => match session::load_session(&path, leaf_id.as_deref()) {
                Ok(s) => {
                    app.session = Some(s.clone());
                    app.cwd = s
                        .cwd
                        .clone()
                        .unwrap_or_else(|| app.cwd.clone());
                    app.message_source = s.message.clone();
                    app.thinking_source = s.thinking.clone();
                    app.source_label = format!("session: {}", s.path.display());
                    let at = s
                        .leaf_id
                        .as_deref()
                        .map(|id| format!(" at {id}"))
                        .unwrap_or_default();
                    app.status = if s.message.trim().is_empty() {
                        format!("Loaded {}{at} — no assistant reply at this position", s.path.display())
                    } else {
                        format!(
                            "Loaded {}{at} — msg {} chars, thinking {} chars",
                            s.path.display(),
                            s.message.trim().chars().count(),
                            s.thinking.trim().chars().count()
                        )
                    };
                }
                Err(e) => {
                    app.status = format!("Failed to load session: {e}");
                }
            },
            None => {
                app.status =
                    "No session. Launch from inside pi so PI_SESSION_FILE is set, or pass --session <file>."
                        .to_string();
            }
        }

        app.refresh_tree();
        (app, Task::none())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleThinking => {
                self.thinking_state = match self.thinking_state {
                    ThinkingState::Expanded => ThinkingState::Collapsed,
                    ThinkingState::Collapsed => ThinkingState::Expanded,
                };
                Task::none()
            }
            Message::RefreshSession => {
                let path = session::env_session_file();
                match path {
                    Some(path) => {
                        match session::load_session(&path, self.leaf_id.as_deref()) {
                            Ok(s) => {
                                self.session = Some(s.clone());
                                self.cwd = s.cwd.clone().unwrap_or_else(|| self.cwd.clone());
                                self.message_source = s.message.clone();
                                self.thinking_source = s.thinking.clone();
                                let at = s
                                    .leaf_id
                                    .as_deref()
                                    .map(|id| format!(" at {id}"))
                                    .unwrap_or_default();
                                self.status =
                                    format!("Refreshed from {}{at}", path.display());
                            }
                            Err(e) => self.status = format!("Refresh failed: {e}"),
                        }
                    }
                    None => self.status =
                        "No session. PI_SESSION_FILE is not set — run from inside pi.".to_string(),
                }
                Task::none()
            }
            Message::SaveNote => {
                let content = self.build_note_markdown();
                let cwd = self.cwd.clone();
                let title = self.title.clone();
                match notes::save_note(&cwd, &title, &content) {
                    Ok(path) => {
                        self.status = format!("Saved note: {}", path.display());
                        self.title.clear();
                        self.refresh_tree();
                    }
                    Err(e) => self.status = format!("Save failed: {e}"),
                }
                Task::none()
            }
            Message::EditCurrent => match &self.selected_note {
                Some(path) if self.viewing_note => {
                    let path = path.clone();
                    Task::perform(
                        async move { crate::editor::open_in_editor(&path) },
                        Message::Edited,
                    )
                }
                _ => {
                    self.status = "Nothing to edit — open a note first.".to_string();
                    Task::none()
                }
            },
            Message::Edited(result) => match result {
                Ok(true) => {
                    // Editor closed; reload the note so edits are visible.
                    if let Some(path) = self.selected_note.clone() {
                        self.load_note(&path);
                    }
                    Task::none()
                }
                Ok(false) => {
                    self.status =
                        "Editor launched — click the note again to reload after editing."
                            .to_string();
                    Task::none()
                }
                Err(e) => {
                    self.status = format!("Edit failed: {e}");
                    Task::none()
                }
            },
            Message::NodeClicked(path) => {
                if path.is_dir() {
                    if self.expanded.contains(&path) {
                        self.expanded.remove(&path);
                    } else {
                        self.expanded.insert(path);
                    }
                    self.rebuild_rows();
                } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    self.load_note(&path);
                }
                Task::none()
            }
            Message::TitleChanged(t) => {
                self.title = t;
                Task::none()
            }
            Message::LinkClicked(url) => {
                self.status = format!("Link (not opened): {url}");
                Task::none()
            }
            Message::RenameNote => {
                self.confirming_remove = false;
                if let Some(path) = &self.selected_note {
                    let stem = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.rename_input = stem;
                    self.renaming = true;
                }
                Task::none()
            }
            Message::RenameChanged(t) => {
                self.rename_input = t;
                Task::none()
            }
            Message::RenameConfirm => {
                if let Some(path) = self.selected_note.clone() {
                    match notes::rename_note(&path, &self.rename_input) {
                        Ok(new_path) => {
                            self.renaming = false;
                            self.status = format!("Renamed note: {}", new_path.display());
                            self.selected_note = Some(new_path.clone());
                            self.source_label = format!("note: {}", new_path.display());
                            self.load_note(&new_path);
                            self.refresh_tree();
                        }
                        Err(e) => self.status = format!("Rename failed: {e}"),
                    }
                }
                Task::none()
            }
            Message::CancelRename => {
                self.renaming = false;
                Task::none()
            }
            Message::RemoveNote => {
                if self.viewing_note && self.selected_note.is_some() {
                    self.renaming = false;
                    self.confirming_remove = true;
                }
                Task::none()
            }
            Message::ConfirmRemove => {
                if let Some(path) = self.selected_note.clone() {
                    match notes::delete_note(&path) {
                        Ok(()) => {
                            self.renaming = false;
                            self.confirming_remove = false;
                            self.viewing_note = false;
                            self.note_source = None;
                            self.selected_note = None;
                            self.source_label = String::from("no note selected");
                            self.status = format!("Removed note: {}", path.display());
                            self.refresh_tree();
                        }
                        Err(e) => {
                            self.confirming_remove = false;
                            self.status = format!("Remove failed: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::CancelRemove => {
                self.confirming_remove = false;
                Task::none()
            }
            Message::Resized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
                Task::none()
            }
            Message::FilterChanged(f) => {
                self.filter = f;
                let trimmed = self.filter.trim();
                self.filter_error = !trimmed.is_empty() && regex::Regex::new(trimmed).is_err();
                Task::none()
            }
        }
    }

    /// Combined Markdown written out when the user saves the current message
    /// as a note.
    fn build_note_markdown(&self) -> String {
        let mut out = String::new();
        if let Some(s) = &self.session {
            // Same order as the viewer: thinking block first, then the message.
            if !s.thinking.trim().is_empty() {
                out.push_str("# Thinking\n\n");
                out.push_str(s.thinking.trim());
                out.push_str("\n\n");
            }
            if !s.message.trim().is_empty() {
                out.push_str("# Message\n\n");
                out.push_str(s.message.trim());
                out.push('\n');
            }
        }
        out
    }

    fn refresh_tree(&mut self) {
        self.tree_root = notes::load_tree(&self.notes_root);
        self.rebuild_rows();
    }

    /// Read a note file and show it in the viewer.
    fn load_note(&mut self, path: &std::path::Path) {
        match std::fs::read_to_string(path) {
            Ok(md) => {
                self.note_source = Some(md);
                self.selected_note = Some(path.to_path_buf());
                self.viewing_note = true;
                self.source_label = format!("note: {}", path.display());
                self.status = format!("Viewing note: {}", path.display());
            }
            Err(e) => self.status = format!("Open failed: {} — {e}", path.display()),
        }
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        tree::flatten(&self.tree_root, &self.expanded, &mut self.rows);
    }

    // ---------------------------------------------------------------- view

    fn view(&self) -> Element<'_, Message> {
        let base = self.base_view();
        if self.confirming_remove {
            // Ask for confirmation via a modal pop-up over the whole window.
            confirm_modal(base, self.confirm_remove_dialog(), Message::CancelRemove)
        } else if self.renaming {
            // Rename the note via a modal pop-up with a text field.
            confirm_modal(base, self.rename_dialog(), Message::CancelRename)
        } else {
            base
        }
    }

    fn base_view(&self) -> Element<'_, Message> {
        let toolbar = self.toolbar();
        let body = container(
            PaneGrid::new(&self.panes, |_id, pane_state, _is_maximized| {
                let content = match pane_state {
                    Pane::Tree => self.tree_pane(),
                    Pane::Viewer => self.viewer_pane(),
                };
                pane_grid::Content::new(content)
            })
            .spacing(8)
            .on_resize(10, Message::Resized),
        )
        .padding(8)
        .height(Length::Fill);
        let status = container(
            text(&self.status)
                .size(12)
                .color(self.theme.extended_palette().primary.weak.text),
        )
        .padding([4, 10])
        .width(Length::Fill);

        column![toolbar, body, rule::horizontal(1), status]
            .height(Length::Fill)
            .into()
    }

    /// The pop-up asking the user to confirm deletion of the selected note.
    fn confirm_remove_dialog(&self) -> Element<'_, Message> {
        container(
            column![
                text("Remove note?").size(18),
                text(
                    "This permanently deletes the note file and cannot be undone.",
                )
                .size(13)
                .color(self.theme.extended_palette().secondary.weak.text),
                row![
                    button(text("Cancel"))
                        .on_press(Message::CancelRemove)
                        .padding([6, 14])
                        .style(button::secondary),
                    button(text("Remove"))
                        .on_press(Message::ConfirmRemove)
                        .padding([6, 14])
                        .style(button::danger),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(16)
            .align_x(iced::Alignment::Center),
        )
        .width(340)
        .padding(20)
        .style(container::rounded_box)
        .into()
    }

    /// The pop-up for renaming the selected note.
    fn rename_dialog(&self) -> Element<'_, Message> {
        container(
            column![
                text("Rename note").size(18),
                text(
                    "Enter a new name for the note. It keeps its .md extension and location.",
                )
                .size(13)
                .color(self.theme.extended_palette().secondary.weak.text),
                text_input("New name", &self.rename_input)
                    .on_input(Message::RenameChanged)
                    .on_submit(Message::RenameConfirm)
                    .width(Length::Fixed(300.0))
                    .padding(6),
                row![
                    button(text("Cancel"))
                        .on_press(Message::CancelRename)
                        .padding([6, 14])
                        .style(button::secondary),
                    button(text("Rename"))
                        .on_press(Message::RenameConfirm)
                        .padding([6, 14])
                        .style(button::primary),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(16),
        )
        .width(380)
        .padding(20)
        .style(container::rounded_box)
        .into()
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let title = text("pi-notes").size(18).font(iced::Font::MONOSPACE);
        let project = text(format!("project: {}", notes::project_dir_name(&self.cwd)))
            .size(12)
            .color(self.theme.extended_palette().secondary.weak.text);

        let title_input = text_input("Note title (optional)", &self.title)
            .on_input(Message::TitleChanged)
            .width(Length::Fixed(260.0))
            .padding(6);

        // Saving requires a loaded session; without one there is nothing to
        // save, so the button is disabled.
        let save = button(text("Save note"))
            .on_press_maybe(self.session.as_ref().map(|_| Message::SaveNote))
            .padding([6, 14])
            .style(button::primary);
        let refresh = button(text("Refresh"))
            .on_press(Message::RefreshSession)
            .padding([6, 14])
            .style(button::secondary);

        row![title, project, Space::new().width(Length::Fill), title_input, refresh, save]
            .spacing(10)
            .padding([10, 12])
            .align_y(iced::Alignment::Center)
            .into()
    }

    fn tree_pane(&self) -> Element<'_, Message> {
        let header = container(
            text("Stored notes")
.font(iced::Font::MONOSPACE)
                .size(14)
                ,
        )
        .padding([8, 8]);

        let filter_input = text_input("Filter notes (regex)", &self.filter)
            .on_input(Message::FilterChanged)
            .padding(6)
            .width(Length::Fill);
        let clear_filter = button(text("Clear")
            .size(12)
            .color(if self.filter.is_empty() {
                self.theme.extended_palette().secondary.weak.text
            } else {
                self.theme.extended_palette().primary.strong.text
            }))
        .on_press(Message::FilterChanged(String::new()))
        .padding([4, 10])
        .style(button::secondary);

        let filter_error: Option<Element<'_, Message>> = if self.filter_error {
            Some(
                text("Invalid regex")
                    .size(11)
                    .color(self.theme.extended_palette().danger.strong.text)
                    .into(),
            )
        } else {
            None
        };

        let mut col = column![].spacing(6).padding([4, 8]);
        for row_ in self.displayed_rows() {
            let indent = Space::new().width(Length::Fixed(row_.depth as f32 * 16.0));
            let label = if row_.is_dir {
                format!("{} {}", if row_.expanded { "▼" } else { "▶" }, row_.name)
            } else {
                row_.name.clone()
            };
            let is_selected = self.selected_note.as_ref() == Some(&row_.path);
            let style = if is_selected {
                button::primary
            } else {
                button::text
            };
            let btn = button(text(label).size(13))
                .on_press(Message::NodeClicked(row_.path.clone()))
                .style(style);
            col = col.push(row![indent, btn].spacing(4));
        }

        container(
            column![
                header,
                container(
                    row![filter_input, clear_filter]
                        .spacing(4)
                        .align_y(iced::Alignment::Center),
                ),
                filter_error,
                scrollable(col).height(Length::Fill)
            ].padding(10)
            .spacing(6),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(container::bordered_box)
        .into()
    }

    /// The rows to show in the notes tree. With an active (valid) regex filter
    /// this re-flattens the tree, keeping ancestor directories so the hierarchy
    /// is preserved; otherwise the normal expanded rows are used.
    fn displayed_rows(&self) -> Vec<tree::Row> {
        let trimmed = self.filter.trim();
        if trimmed.is_empty() {
            return self.rows.clone();
        }
        match regex::Regex::new(trimmed) {
            Ok(re) => {
                let mut rows = Vec::new();
                tree::flatten_filtered(&self.tree_root, &|node| self.note_matches(node, &re), &mut rows);
                rows
            }
            // Invalid regex: fall back to the full tree rather than hiding notes.
            Err(_) => self.rows.clone(),
        }
    }

    /// Whether a note matches the filter: its file name matches, or (for a
    /// note file) its content matches.
    fn note_matches(&self, node: &notes::TreeNode, re: &regex::Regex) -> bool {
        if re.is_match(&node.name) {
            return true;
        }
        if node.is_dir {
            return false;
        }
        std::fs::read_to_string(&node.path)
            .map(|content| re.is_match(&content))
            .unwrap_or(false)
    }

    fn viewer_pane(&self) -> Element<'_, Message> {
        let header = if self.viewing_note {
            row![
                text("Note").size(14).font(iced::Font::MONOSPACE),
                button(text("Rename").size(12))
                    .on_press(Message::RenameNote)
                    .padding([4, 12])
                    .style(button::secondary),
                button(text("Edit").size(12))
                    .on_press(Message::EditCurrent)
                    .padding([4, 12])
                    .style(button::secondary),
                    button(text("Remove").size(12))
                        .on_press(Message::RemoveNote)
                        .padding([4, 12])
                        .style(button::danger),
                    Space::new().width(Length::Fill),
                    text(&self.source_label)
                        .size(11)
                        .color(self.theme.extended_palette().secondary.weak.text)
                ].padding(10)
                .spacing(10)
                .align_y(iced::Alignment::Center)
        } else {
            row![
                text("Last message").size(14).font(iced::Font::MONOSPACE),
                Space::new().width(Length::Fill),
                text(&self.source_label)
                    .size(11)
                    .color(self.theme.extended_palette().secondary.weak.text)
            ].padding(10)
            .align_y(iced::Alignment::Center)
        };

        let content = self.markdown_view();
        let body = scrollable(container(content).padding(14).width(Length::Fill))
            .height(Length::Fill);

        container(column![header, rule::horizontal(1), body])
            .height(Length::Fill)
            .width(Length::Fill)
            .style(container::bordered_box)
            .into()
    }

    fn markdown_view(&self) -> Element<'_, Message> {
        if self.viewing_note {
            // Notes already contain both sections as headings.
            return match &self.note_source {
                Some(source) => self.render_markdown(source),
                None => text("Nothing to display.").into(),
            };
        }

        // No session loaded: show a static placeholder instead of empty panes.
        if self.session.is_none() {
            return self.no_session_view();
        }

        // Thinking block first (collapsible, styled as a quotation), then the
        // message underneath.
        column![self.thinking_section(), self.message_section()]
            .spacing(14)
            .into()
    }

    /// Static placeholder shown in the viewer when no session is loaded.
    fn no_session_view(&self) -> Element<'_, Message> {
        let title = text("No session loaded")
            .size(18)
            .font(iced::Font::MONOSPACE)
            .color(self.theme.extended_palette().primary.strong.text);
        let body = text(
            "Launch pi-notes from inside pi so PI_SESSION_FILE is set,\nor pass --session <file> to open a session file directly.\n\nUse the notes tree on the left to browse stored notes.",
        )
        .size(13)
        .color(self.theme.extended_palette().secondary.weak.text);

        container(column![title, Space::new().height(8), body].spacing(6).padding(20))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .into()
    }

    fn thinking_section(&self) -> Element<'_, Message> {
        let arrow = match self.thinking_state {
            ThinkingState::Expanded => "▼",
            ThinkingState::Collapsed => "▶",
        };
        let toggle = button(text(format!("{arrow} Thinking"))
            .size(13)
            .font(iced::Font::MONOSPACE))
        .on_press(Message::ToggleThinking)
        .style(button::text);

        if self.thinking_state == ThinkingState::Collapsed {
            return toggle.into();
        }

        let content = self.render_markdown(&self.thinking_source);
        let quote = container(content)
            .width(Length::Fill)
            .padding([10, 14])
            .style(|theme: &Theme| container::Style {
                background: Some(
                    theme.extended_palette().secondary.weak.color.into(),
                ),
                border: iced::Border {
                    color: theme.extended_palette().secondary.strong.color,
                    width: 2.0,
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

        column![row![toggle, Space::new().width(Length::Fill)], quote]
            .spacing(6)
            .into()
    }

    fn message_section(&self) -> Element<'_, Message> {
        self.render_markdown(&self.message_source)
    }

    /// Renders markdown with the pi-mdview renderer (the engine behind the
    /// pi-mdview viewer). The document is re-wrapped to the actual pane width
    /// via `responsive`, and memoised per width by [`MarkdownRenderer`].
    fn render_markdown<'a>(&'a self, source: &'a str) -> Element<'a, Message> {
        let markdown = &self.markdown;
        let theme = markdown.theme;
        let font = fonts::monospace();

        responsive(move |size| {
            let columns = columns_for_width(size.width);
            let lines = markdown.lines(source, columns);
            let spans = pi_mdview::document_spans(&lines, &theme, font, None);

            rich_text(spans)
                .font(font)
                .size(FONT_SIZE)
                .line_height(LineHeight::Relative(LINE_HEIGHT_RATIO))
                .width(Length::Shrink)
                .wrapping(Wrapping::None)
                .color(pi_mdview::widget::rgb(theme.text))
                .on_link_click(Message::LinkClicked)
                .into()
        })
        .width(Length::Fill)
        .height(Length::Shrink)
        .into()
    }
}

/// Overlay a confirmation dialog on top of the base content, dimming the rest
/// of the window. Clicking outside the dialog (or pressing its Cancel button)
/// sends `on_blur` to dismiss it. Follows the iced `modal` example.
fn confirm_modal<'a, Message>(
    base: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    on_blur: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    stack![
        base.into(),
        opaque(
            mouse_area(center(opaque(content)).style(|_theme| container::Style {
                background: Some(
                    Color {
                        a: 0.6,
                        ..Color::BLACK
                    }
                    .into(),
                ),
                ..container::Style::default()
            }))
            .on_press(on_blur)
        )
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_the_monospace_grid() {
        // At the default 14px font and 0.6 em advance, roughly one column per
        // 8.4 px, clamped to a sane range.
        assert_eq!(columns_for_width(0.0), 20);
        assert_eq!(columns_for_width(100_000.0), 400);
        assert!(columns_for_width(840.0) >= 99);
        assert!(columns_for_width(840.0) > columns_for_width(420.0));
    }

    #[test]
    fn markdown_renderer_caches_and_matches_pi_mdview() {
        let renderer = MarkdownRenderer::new();
        let source = "# heading\n\nsome **bold** text";
        let lines = renderer.lines(source, 40);
        assert_eq!(
            pi_mdview::plain_lines(&lines),
            pi_mdview::plain_lines(&render_mdview(source, 40))
        );
        // A second call hits the cache and yields identical output.
        assert_eq!(renderer.lines(source, 40), lines);
    }

    fn render_mdview(source: &str, columns: usize) -> Vec<MdLine> {
        Renderer::new(MarkdownTheme::pi_dark())
            .options(RenderOptions {
                padding_x: 0,
                padding_y: 0,
                pad_to_width: false,
                render_latex: true,
            })
            .render(source, columns)
    }
}
