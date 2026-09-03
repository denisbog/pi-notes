//! Reading and parsing pi session files (JSONL).
//!
//! A session file lives at `~/.pi/agent/sessions/--<cwd>--/<timestamp>_<id>.jsonl`
//! and is a sequence of JSON lines. Each `message` entry wraps an AgentMessage
//! whose `content` is an array of typed blocks (`text`, `thinking`,
//! `toolCall`, `image`, ...). We scan the file for the last assistant message
//! and pull out the concatenated `text` blocks (the reply) and `thinking`
//! blocks (the reasoning).
//!
//! The session to display is always the one of the **current running pi
//! instance**: pi exports it through the `PI_SESSION_FILE` environment
//! variable (inherited by any process pi spawns, including our `/notes`
//! command). We rely on that variable alone and never guess by scanning
//! session files on disk.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// The parts of a session we care about.
#[derive(Debug, Clone, Default)]
pub struct Session {
    /// Absolute path to the session file.
    pub path: PathBuf,
    /// The working directory recorded in the session header (if any).
    pub cwd: Option<String>,
    /// Concatenated `text` content blocks of the last assistant message on
    /// the active path.
    pub message: String,
    /// Concatenated `thinking` content blocks of the last assistant message on
    /// the active path.
    pub thinking: String,
    /// Role of the last message found, for status display.
    pub last_role: Option<String>,
    /// Entry id of the leaf used (the pi tree position that was displayed).
    pub leaf_id: Option<String>,
    /// Timestamp of the leaf entry, for display.
    pub leaf_timestamp: Option<String>,
}

/// A parsed session entry. Only the fields needed to walk the tree and extract
/// assistant text/thinking are kept.
#[derive(Debug, Clone)]
struct Entry {
    id: String,
    parent_id: Option<String>,
    timestamp: String,
    role: Option<String>,
    text: String,
    thinking: String,
}

/// The session file path pi exports through the environment variable. This is
/// the session of the **current running pi instance** — pi sets `PI_SESSION_FILE`
/// on every process it spawns, so the `/notes` command inherits it.
pub fn env_session_file() -> Option<PathBuf> {
    std::env::var_os("PI_SESSION_FILE")
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// Resolve the session file of the current running pi instance.
///
/// This is intentionally just the `PI_SESSION_FILE` environment variable — we
/// never scan the session directory or fall back to the most recently updated
/// file, because that could select a different pi instance's session.
pub fn resolve_session_file() -> Option<PathBuf> {
    env_session_file()
}

/// The base agent data directory (`~/.pi/agent` or the directory that holds
/// the sessions folder containing the active session file).
pub fn agent_root() -> PathBuf {
    if let Some(file) = env_session_file() {
        // <agent>/sessions/<project>/<file> -> <agent>
        let mut dir = file.parent(); // project dir
        if let Some(d) = dir {
            dir = d.parent(); // sessions dir
        }
        if let Some(d) = dir {
            dir = d.parent(); // agent dir
        }
        if let Some(agent) = dir {
            return agent.to_path_buf();
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".pi").join("agent")
}

/// Extract the `text` and `thinking` content blocks of an assistant message
/// from its `message` JSON object.
fn extract_blocks(msg: &Value) -> (String, String) {
    let mut text = String::new();
    let mut thinking = String::new();
    if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                        text.push_str(t);
                        text.push('\n');
                    }
                }
                Some("thinking") => {
                    if let Some(t) = block.get("thinking").and_then(|t| t.as_str()) {
                        thinking.push_str(t);
                        thinking.push('\n');
                    }
                }
                _ => {}
            }
        }
    }
    (text, thinking)
}

/// Parse a session file and return the assistant message + thinking at the
/// position pi is currently displaying.
///
/// Session files are trees (`id`/`parentId`). The pi `/tree` command lets the
/// user navigate to any previous message, so the displayed position is not
/// always the last entry in the file. `leaf_id` is the id of the entry pi is
/// currently showing (from `ctx.sessionManager.getLeafId()`); when `None`, the
/// last entry in file order is used, which is pi's default leaf when a session
/// is opened.
///
/// We then walk the active path from that leaf up to the root and, over the
/// assistant messages on that path, take the most recent non-empty `text`
/// block (the message) and the most recent non-empty `thinking` block (the
/// reasoning — often attached to later tool-use turns).
pub fn load_session(path: &Path, leaf_id: Option<&str>) -> Result<Session, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;

    let mut session = Session {
        path: path.to_path_buf(),
        ..Default::default()
    };

    let mut entries: Vec<Entry> = Vec::new();
    let mut by_id: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: Value = serde_json::from_str(line)
            .map_err(|e| format!("parse line in {}: {e}", path.display()))?;

        match entry.get("type").and_then(|t| t.as_str()) {
            Some("session") => {
                session.cwd = entry
                    .get("cwd")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string());
            }
            Some("message") => {
                let Some(msg) = entry.get("message") else {
                    continue;
                };
                let role = msg
                    .get("role")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string());
                let (text, thinking) =
                    if role.as_deref() == Some("assistant") {
                        extract_blocks(msg)
                    } else {
                        (String::new(), String::new())
                    };
                let e = Entry {
                    id: entry
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    parent_id: entry
                        .get("parentId")
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string()),
                    timestamp: entry
                        .get("timestamp")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    role,
                    text,
                    thinking,
                };
                by_id.insert(e.id.clone(), entries.len());
                entries.push(e);
            }
            // Non-message entries (model_change, compaction, branch_summary,
            // label, ...) still participate in the tree, so record them too.
            _ => {
                let id = entry
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or_default()
                    .to_string();
                if !id.is_empty() {
                    let e = Entry {
                        id: id.clone(),
                        parent_id: entry
                            .get("parentId")
                            .and_then(|p| p.as_str())
                            .map(|s| s.to_string()),
                        timestamp: entry
                            .get("timestamp")
                            .and_then(|t| t.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        role: None,
                        text: String::new(),
                        thinking: String::new(),
                    };
                    by_id.insert(id, entries.len());
                    entries.push(e);
                }
            }
        }
    }

    // Resolve the leaf: the explicitly selected entry if it exists, otherwise
    // the last entry in file order (pi's default leaf on open).
    let leaf = match leaf_id.and_then(|id| by_id.get(id)) {
        Some(&idx) => idx,
        None => entries.len().saturating_sub(1),
    };
    if let Some(entry) = entries.get(leaf) {
        session.leaf_id = Some(entry.id.clone());
        session.leaf_timestamp = Some(entry.timestamp.clone());
        session.last_role = entry.role.clone();
    }

    // Walk the active path from the leaf up to the root, and collect the
    // most recent non-empty message/thinking among the assistant messages on
    // it ("most recent" = closest to the leaf, i.e. the position pi is
    // currently displaying). Message and thinking are tracked independently,
    // since thinking is often attached to a later tool-use turn.
    let mut current = leaf;
    loop {
        let Some(entry) = entries.get(current) else {
            break;
        };
        if entry.role.as_deref() == Some("assistant") {
            if session.thinking.is_empty() && !entry.thinking.trim().is_empty() {
                session.thinking = entry.thinking.clone();
            }
            if session.message.is_empty() && !entry.text.trim().is_empty() {
                session.message = entry.text.clone();
            }
        }
        match &entry.parent_id {
            Some(pid) => match by_id.get(pid) {
                Some(&idx) => current = idx,
                None => break,
            },
            None => break,
        }
    }

    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_last_assistant_message_and_thinking() {
        // Point at the live session to prove real-world parsing works.
        let path = std::env::var("PI_SESSION_FILE").ok().map(PathBuf::from);
        let path = path.or_else(|| {
            Some(PathBuf::from(
                "/home/denis/.pi/agent/sessions/--home-denis-llm--/2026-09-03T10-42-50-503Z_01a066dd-3e07-7411-9ec2-e06ba4c61e11.jsonl",
            ))
        });
        let s = load_session(&path.expect("session path"), None).expect("load session");
        assert!(!s.message.trim().is_empty(), "message should not be empty");
        assert!(
            !s.thinking.trim().is_empty(),
            "thinking should not be empty"
        );
        assert_eq!(s.last_role.as_deref(), Some("assistant"));
    }

    #[test]
    fn follows_the_selected_branch_leaf() {
        // Synthetic session with a branch: entries a-b-c-d then a second
        // branch b-e-f. The file-last entry is f (branch), but if the user
        // selected entry d in pi (via /tree), we must show d's reply.
        let path = std::env::temp_dir().join(format!("pinotes-sess-{}", std::process::id()));
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"version\":3,\"id\":\"s1\",\"timestamp\":\"t0\",\"cwd\":\"/tmp\"}\n",
                "{\"type\":\"message\",\"id\":\"a\",\"parentId\":null,\"timestamp\":\"t1\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"first\"}]}}\n",
                "{\"type\":\"message\",\"id\":\"b\",\"parentId\":\"a\",\"timestamp\":\"t2\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"think A\"},{\"type\":\"text\",\"text\":\"reply A\"}]}}\n",
                "{\"type\":\"message\",\"id\":\"c\",\"parentId\":\"b\",\"timestamp\":\"t3\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"second\"}]}}\n",
                "{\"type\":\"message\",\"id\":\"d\",\"parentId\":\"c\",\"timestamp\":\"t4\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"think B\"},{\"type\":\"text\",\"text\":\"reply B\"}]}}\n",
                "{\"type\":\"message\",\"id\":\"e\",\"parentId\":\"b\",\"timestamp\":\"t5\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"branch prompt\"}]}}\n",
                "{\"type\":\"message\",\"id\":\"f\",\"parentId\":\"e\",\"timestamp\":\"t6\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"think C\"},{\"type\":\"text\",\"text\":\"reply C\"}]}}\n",
            ),
        )
        .unwrap();

        // No leaf -> last entry in file order (f): the default pi position.
        let s = load_session(&path, None).unwrap();
        assert!(s.message.contains("reply C"));
        assert!(s.thinking.contains("think C"));
        assert_eq!(s.leaf_id.as_deref(), Some("f"));

        // User selected entry d in /tree -> show that path's reply.
        let s = load_session(&path, Some("d")).unwrap();
        assert!(s.message.contains("reply B"));
        assert!(!s.message.contains("reply C"));
        assert!(s.thinking.contains("think B"));
        assert_eq!(s.leaf_id.as_deref(), Some("d"));

        // Unknown leaf falls back to the default (last entry).
        let s = load_session(&path, Some("nope")).unwrap();
        assert!(s.message.contains("reply C"));

        let _ = std::fs::remove_file(&path);
    }
}
