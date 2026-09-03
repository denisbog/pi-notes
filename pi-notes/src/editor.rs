//! Opening files in the user's system editor.

use std::path::Path;
use std::process::Command;

/// Launch the user's system editor on `path`.
///
/// Returns `Ok(true)` when the editor ran to completion (the caller should
/// reload the file afterwards) and `Ok(false)` when the editor was launched
/// detached (e.g. the `xdg-open` fallback, which returns immediately).
pub fn open_in_editor(path: &Path) -> Result<bool, String> {
    let editor = resolve_editor();
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("xdg-open").to_string();
    let args: Vec<&str> = parts.collect();

    if program == "xdg-open" {
        // Opens the default application without waiting; no auto-reload.
        Command::new(&program)
            .args(&args)
            .arg(path)
            .spawn()
            .map_err(|e| format!("failed to launch {program}: {e}"))?;
        return Ok(false);
    }

    // Wait for the editor to close so we can reload the edited file.
    let status = Command::new(&program)
        .args(&args)
        .arg(path)
        .status()
        .map_err(|e| format!("failed to launch {program}: {e}"))?;

    if !status.success() {
        return Err(format!("editor {program} exited with {status}"));
    }

    Ok(true)
}

/// Resolve the system editor: `$VISUAL`, then `$EDITOR`, then the first known
/// editor on `PATH`, falling back to `xdg-open`.
fn resolve_editor() -> String {
    for var in ["VISUAL", "EDITOR"] {
        if let Ok(value) = std::env::var(var) {
            if !value.trim().is_empty() {
                return value;
            }
        }
    }

    for name in [
        "code", "code-insiders", "zed", "subl", "gedit", "kate", "kwrite",
        "mousepad", "helix", "hx", "nvim", "vim", "vi", "nano", "micro",
    ] {
        if is_in_path(name) {
            return name.to_string();
        }
    }

    "xdg-open".to_string()
}

fn is_in_path(name: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).any(|dir| dir.join(name).exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn opens_editor_and_waits_for_changes() {
        let dir = std::env::temp_dir().join(format!("pinotes-ed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.md");
        std::fs::write(&file, "hello\n").unwrap();

        // A tiny "editor" that appends a line to the given file and exits.
        let script = dir.join("fake-editor.sh");
        std::fs::write(&script, "#!/bin/sh\necho appended >> \"$1\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let script_path = script.to_string_lossy().to_string();

        let old = std::env::var("EDITOR").ok();
        std::env::set_var("EDITOR", &script_path);

        let waited = open_in_editor(&file).expect("open editor");
        assert!(waited, "should have waited for the editor to close");
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains("appended"), "editor change not reflected");

        // restore env
        match old {
            Some(v) => std::env::set_var("EDITOR", v),
            None => std::env::remove_var("EDITOR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
