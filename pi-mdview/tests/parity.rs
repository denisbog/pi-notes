//! Parity tests: `pi-mdview`'s renderer must produce the same lines as pi's own
//! TUI markdown renderer (`@earendil-works/pi-tui`).
//!
//! Two layers:
//! 1. `matches_pi_golden_files` compares against checked-in golden files, so the
//!    test works without Node or pi installed.
//! 2. `matches_live_pi_renderer` (skipped when Node/pi-tui are unavailable)
//!    re-runs pi's renderer through `tools/pi_render.mjs` and compares again.
//!
//! Regenerate the golden files with `tools/update-golden.sh`.

use std::path::PathBuf;
use std::process::Command;

use pi_mdview::{plain_lines, render_markdown};

const WIDTHS: &[usize] = &[40, 60, 80, 100];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_path() -> PathBuf {
    manifest_dir().join("tools/parity.md")
}

fn fixture_source() -> String {
    std::fs::read_to_string(fixture_path()).expect("read tools/parity.md")
}

fn golden_path(width: usize) -> PathBuf {
    manifest_dir().join(format!("tools/golden/pi-{width}.txt"))
}

fn assert_lines_eq(width: usize, expected: &[String], actual: &[String]) {
    let mut differences = Vec::new();
    for index in 0..expected.len().max(actual.len()) {
        let expected_line = expected
            .get(index)
            .map(String::as_str)
            .unwrap_or("<missing>");
        let actual_line = actual.get(index).map(String::as_str).unwrap_or("<missing>");
        if expected_line != actual_line {
            differences.push(index);
        }
    }
    if differences.is_empty() {
        return;
    }

    let mut report = format!(
        "render mismatch at width {width}: {} of {} lines differ\n",
        differences.len(),
        expected.len().max(actual.len())
    );
    for index in differences.iter().take(10) {
        report.push_str(&format!(
            "  line {index}:\n    pi  : {:?}\n    ours: {:?}\n",
            expected
                .get(*index)
                .map(String::as_str)
                .unwrap_or("<missing>"),
            actual
                .get(*index)
                .map(String::as_str)
                .unwrap_or("<missing>"),
        ));
    }
    panic!("{report}");
}

#[test]
fn matches_pi_golden_files() {
    let source = fixture_source();
    for &width in WIDTHS {
        let golden = std::fs::read_to_string(golden_path(width))
            .unwrap_or_else(|error| panic!("read {}: {error}", golden_path(width).display()));
        let expected: Vec<String> = golden.lines().map(str::to_string).collect();
        let actual = plain_lines(&render_markdown(&source, width));
        assert_lines_eq(width, &expected, &actual);
    }
}

#[test]
fn matches_live_pi_renderer() {
    let Some(tui_path) = find_pi_tui() else {
        eprintln!("skipping: pi-tui not found (set PI_TUI_PATH to enable)");
        return;
    };
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: node not available");
        return;
    }

    let script = manifest_dir().join("tools/pi_render.mjs");
    let source = fixture_source();

    for &width in WIDTHS {
        let output = Command::new("node")
            .arg(&script)
            .arg(width.to_string())
            .arg(fixture_path())
            .env("PI_TUI_PATH", &tui_path)
            .output()
            .expect("run pi renderer");

        if !output.status.success() {
            panic!(
                "pi_render.mjs failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("parse pi renderer JSON");
        let expected: Vec<String> = json
            .as_array()
            .expect("array of lines")
            .iter()
            .map(|value| value.as_str().unwrap_or_default().to_string())
            .collect();

        let actual = plain_lines(&render_markdown(&source, width));
        assert_lines_eq(width, &expected, &actual);
    }
}

/// Locates the installed `@earendil-works/pi-tui` package.
fn find_pi_tui() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PI_TUI_PATH") {
        let path = PathBuf::from(path);
        if path.join("dist/index.js").exists() {
            return Some(path);
        }
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        // nvm installs
        let nvm_versions = home.join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(&nvm_versions) {
            for entry in entries.flatten() {
                roots.push(entry.path().join("lib/node_modules"));
            }
        }
        roots.push(home.join(".local/lib/node_modules"));
        roots.push(home.join(".local/share/pnpm/global/5/node_modules"));
    }
    roots.push(PathBuf::from("/usr/lib/node_modules"));
    roots.push(PathBuf::from("/usr/local/lib/node_modules"));

    for root in roots {
        let candidate =
            root.join("@earendil-works/pi-coding-agent/node_modules/@earendil-works/pi-tui");
        if candidate.join("dist/index.js").exists() {
            return Some(candidate);
        }
        let candidate = root.join("@earendil-works/pi-tui");
        if candidate.join("dist/index.js").exists() {
            return Some(candidate);
        }
    }
    None
}
