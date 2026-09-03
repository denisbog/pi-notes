//! pi-notes — a Rust / Iced desktop app that displays the last pi assistant
//! message and its thinking block as rendered Markdown, lets you store the
//! message as a note (in a directory hierarchy that mirrors pi's session
//! files) and browse previously stored notes through a file tree.
//!
//! It is designed to be launched from inside pi as a `/notes` command (see
//! `extension/notes.ts`). The current session is located via the
//! `PI_SESSION_FILE` environment variable that pi sets, falling back to the
//! most recently modified session file under `~/.pi/agent/sessions/`.

mod app;
mod editor;
mod notes;
mod session;
mod symbols;
mod tree;

use app::App;

pub fn main() -> iced::Result {
    App::run()
}
