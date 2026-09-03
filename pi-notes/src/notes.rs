//! Note storage and the file tree used to browse stored notes.
//!
//! Notes are stored under `~/.pi/agent/notes/<project-dir>/<name>.md` where
//! `<project-dir>` uses the exact same `--<cwd with / replaced by ->--`
//! convention that pi uses for session directories
//! (`~/.pi/agent/sessions/--home-denis-llm--/...`). This keeps notes grouped
//! by project, right next to the sessions that produced them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::session;

/// Root directory where all notes live.
pub fn notes_root() -> PathBuf {
    session::agent_root().join("notes")
}

/// The directory name for a working directory, mirroring pi's convention:
/// `/home/denis/llm` -> `--home-denis-llm--`.
pub fn project_dir_name(cwd: &str) -> String {
    let stripped = cwd.trim_matches('/');
    let normalized = stripped.replace('/', "-");
    format!("--{normalized}--")
}

/// Sanitize a user-provided note title into a safe file stem.
fn sanitize(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for ch in title.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == ' ' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim().trim_end_matches('.').replace(' ', "_");
    if trimmed.is_empty() {
        "note".to_string()
    } else {
        trimmed
    }
}

fn timestamp_compact() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Save a note into `notes_root/<project>/` and return the path written.
pub fn save_note(cwd: &str, title: &str, content: &str) -> Result<PathBuf, String> {
    let root = notes_root();
    let dir = root.join(project_dir_name(cwd));
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;

    let name = if title.trim().is_empty() {
        format!("{}.md", timestamp_compact())
    } else {
        format!("{}_{}.md", timestamp_compact(), sanitize(title))
    };

    let path = dir.join(name);
    std::fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

/// Rename a note file within its directory, preserving its location.
///
/// `new_stem` is the desired base name (with or without a `.md` extension);
/// the result always keeps the `.md` extension. Returns the new path.
pub fn rename_note(path: &Path, new_stem: &str) -> Result<PathBuf, String> {
    let dir = path
        .parent()
        .ok_or_else(|| "note has no parent directory".to_string())?;
    let trimmed = new_stem.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let stem = sanitize(trimmed.trim_end_matches(".md"));
    let new_path = dir.join(format!("{stem}.md"));
    if new_path == path {
        return Ok(path.to_path_buf());
    }
    if new_path.exists() {
        return Err(format!(
            "a note named {} already exists",
            new_path.file_name().unwrap().to_string_lossy()
        ));
    }
    std::fs::rename(path, &new_path)
        .map_err(|e| format!("rename {}: {e}", path.display()))?;
    Ok(new_path)
}

/// Delete a note file from disk.
pub fn delete_note(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("note does not exist: {}", path.display()));
    }
    if path.is_dir() {
        return Err(format!("{} is a directory, not a note", path.display()));
    }
    std::fs::remove_file(path).map_err(|e| format!("remove {}: {e}", path.display()))
}

/// A node in the notes file tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

/// Recursively build a tree of the notes root, including only directories and
/// `.md` files.
pub fn load_tree(root: &Path) -> TreeNode {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "notes".to_string());

    let mut children = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(root) {
        let mut dirs = BTreeMap::new();
        let mut files = BTreeMap::new();
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.insert(
                    entry.file_name().to_string_lossy().to_string(),
                    load_tree(&path),
                );
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                files.insert(
                    entry.file_name().to_string_lossy().to_string(),
                    TreeNode {
                        name: entry.file_name().to_string_lossy().to_string(),
                        path,
                        is_dir: false,
                        children: Vec::new(),
                    },
                );
            }
        }
        children.extend(dirs.into_values());
        children.extend(files.into_values());
    }

    TreeNode {
        name,
        path: root.to_path_buf(),
        is_dir: true,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_dir_mirrors_pi_convention() {
        assert_eq!(project_dir_name("/home/denis/llm"), "--home-denis-llm--");
        assert_eq!(project_dir_name("/a/b c"), "--a-b c--");
    }

    #[test]
    fn saves_and_browses_note() {
        let cwd = "/home/denis/llm";
        let title = "My test note";
        let path = save_note(cwd, title, "# Hello\n\nsome $\\alpha$ note").expect("save");
        assert!(path.exists());
        assert!(path.file_name().unwrap().to_string_lossy().contains("My_test_note"));
        // Notes must live under ~/.pi/agent/notes/<project>/, never under the
        // sessions directory (guards against the agent_root off-by-one bug).
        let notes_root = notes_root();
        assert!(
            path.starts_with(&notes_root),
            "note must be stored under notes root {notes_root:?}, got {path:?}"
        );
        let expected_dir = notes_root.join("--home-denis-llm--");
        assert!(
            path.parent().is_some_and(|p| p == expected_dir),
            "note must be in its project dir {expected_dir:?}, got {:?}",
            path.parent()
        );
        // The tree should contain the project dir and the file.
        let tree = load_tree(&notes_root);
        assert!(tree.is_dir);
        let flattened: Vec<String> = collect_names(&tree, 0, &mut Vec::new());
        assert!(flattened.iter().any(|n| n == "--home-denis-llm--"));
        let _ = std::fs::remove_file(&path);
    }

    fn collect_names(n: &TreeNode, _d: usize, out: &mut Vec<String>) -> Vec<String> {
        out.push(n.name.clone());
        for c in &n.children {
            collect_names(c, 0, out);
        }
        out.clone()
    }
}
