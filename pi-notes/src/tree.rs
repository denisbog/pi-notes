//! Flattening a [`notes::TreeNode`] into a visible, indented row list based
//! on which directories are expanded. Used by the notes browser panel.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::notes::TreeNode;

/// One visible row in the tree.
#[derive(Debug, Clone)]
pub struct Row {
    pub depth: usize,
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub expanded: bool,
}

/// Produce the list of visible rows given the expanded-set of directory paths.
pub fn flatten<'a>(node: &'a TreeNode, expanded: &HashSet<PathBuf>, rows: &mut Vec<Row>) {
    flatten_at(node, expanded, 0, rows);
}

fn flatten_at<'a>(
    node: &'a TreeNode,
    expanded: &HashSet<PathBuf>,
    depth: usize,
    rows: &mut Vec<Row>,
) {
    if node.is_dir {
        let is_expanded = depth == 0 || expanded.contains(&node.path);
        rows.push(Row {
            depth,
            name: node.name.clone(),
            path: node.path.clone(),
            is_dir: true,
            expanded: is_expanded,
        });
        if is_expanded {
            for child in &node.children {
                flatten_at(child, expanded, depth + 1, rows);
            }
        }
    } else {
        rows.push(Row {
            depth,
            name: node.name.clone(),
            path: node.path.clone(),
            is_dir: false,
            expanded: false,
        });
    }
}

/// Produce rows matching `matches`, keeping every ancestor directory so the
/// tree hierarchy is preserved while filtering. Directories that contain a
/// match (even if they don't match themselves) are shown expanded so the
/// matching descendants are visible; the manual `expanded` set is ignored.
///
/// The predicate receives the whole [`TreeNode`], so it may match on more than
/// the name (e.g. the note's file content).
pub fn flatten_filtered<'a>(
    node: &'a TreeNode,
    matches: &dyn Fn(&TreeNode) -> bool,
    rows: &mut Vec<Row>,
) {
    flatten_filtered_at(node, matches, 0, rows);
}

fn flatten_filtered_at<'a>(
    node: &'a TreeNode,
    matches: &dyn Fn(&TreeNode) -> bool,
    depth: usize,
    rows: &mut Vec<Row>,
) {
    if node.is_dir {
        // Drop whole subtrees that contain no match anywhere.
        if !contains_match(node, matches) {
            return;
        }
        rows.push(Row {
            depth,
            name: node.name.clone(),
            path: node.path.clone(),
            is_dir: true,
            expanded: true,
        });
        for child in &node.children {
            flatten_filtered_at(child, matches, depth + 1, rows);
        }
    } else if matches(node) {
        rows.push(Row {
            depth,
            name: node.name.clone(),
            path: node.path.clone(),
            is_dir: false,
            expanded: false,
        });
    }
}

fn contains_match(node: &TreeNode, matches: &dyn Fn(&TreeNode) -> bool) -> bool {
    if matches(node) {
        return true;
    }
    node.children.iter().any(|c| contains_match(c, matches))
}
