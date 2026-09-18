/**
 * Shared helpers for the pi-notes TUI extensions (`session-notes.ts` and
 * `notes-viewer.ts`).
 *
 * This file lives in a subdirectory so pi's extension discovery does not try
 * to load it as an extension (only `extensions/*.ts` and
 * `extensions/<dir>/index.ts` are auto-loaded).
 */

import { getAgentDir, getMarkdownTheme } from "@earendil-works/pi-coding-agent";
import { Markdown, truncateToWidth, visibleWidth } from "@earendil-works/pi-tui";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// ---------------------------------------------------------------------------
// Notes storage (mirrors pi-notes/src/notes.rs)
// ---------------------------------------------------------------------------

/** `/home/denis/llm` -> `--home-denis-llm--` (pi's session dir convention). */
export function projectDirName(cwd: string): string {
  // Normalize Windows paths to POSIX form so the same logic works everywhere:
  // backslashes become slashes and the drive letter prefix is dropped
  // ("C:\\Users\\denis\\llm" -> "/Users/denis/llm").
  const normalized = cwd.replace(/\\/g, "/").replace(/^[A-Za-z]:/, "");
  const stripped = normalized.replace(/^\/+/, "").replace(/\/+$/, "");
  const joined = stripped.replace(/\//g, "-");
  // Sanitize anything else that would be unsafe in a directory name, so a
  // Windows path can never produce an invalid or hostile folder name.
  const safe = joined.replace(/[^A-Za-z0-9\-_. ]/g, "_").trim().replace(/\.+$/, "");
  return `--${safe || "root"}--`;
}

/** Sanitize a user-provided name into a safe file stem. */
export function sanitizeStem(title: string): string {
  let out = "";
  for (const ch of title) {
    if (/[A-Za-z0-9\-_. ]/.test(ch)) out += ch;
    else out += "_";
  }
  const trimmed = out.trim().replace(/\.+$/, "").replace(/ /g, "_");
  return trimmed.length > 0 ? trimmed : "note";
}

export function timestampCompact(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return (
    `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}` +
    `-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`
  );
}

/** Root directory where all notes live. */
export function notesRoot(): string {
  return path.join(getAgentDir(), "notes");
}

/** Per-project notes directory used by `/note`. */
export function notesDir(cwd: string): string {
  return path.join(notesRoot(), projectDirName(cwd));
}

/** Write a note and return the file path. */
export function saveNote(cwd: string, title: string, content: string): string {
  const dir = notesDir(cwd);
  fs.mkdirSync(dir, { recursive: true });
  const stem = title.trim() ? `${timestampCompact()}_${sanitizeStem(title)}` : timestampCompact();
  const file = path.join(dir, `${stem}.md`);
  fs.writeFileSync(file, content, "utf8");
  return file;
}

// ---------------------------------------------------------------------------
// Small display helpers (shared by both pickers)
// ---------------------------------------------------------------------------

/** Replace the home directory prefix with `~`. */
export function shortenPath(p: string): string {
  const home = os.homedir();
  return p.startsWith(home) ? `~${p.slice(home.length)}` : p;
}

/** Compact relative age, e.g. `now`, `12m`, `3d`, `2w`. */
export function formatAge(mtimeMs: number): string {
  if (!mtimeMs) return "";
  const diff = Date.now() - mtimeMs;
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  if (days < 30) return `${Math.floor(days / 7)}w`;
  if (days < 365) return `${Math.floor(days / 30)}mo`;
  return `${Math.floor(days / 365)}y`;
}

/** Pad a (possibly ANSI-styled) line to exactly `width` visible columns. */
export function padTo(line: string, width: number): string {
  const current = visibleWidth(line);
  if (current >= width) return truncateToWidth(line, width, "");
  return line + " ".repeat(width - current);
}

/**
 * Quote a path for a shell command string.
 *
 * herdr parses the `pane run` command string with the host shell: Bash on
 * POSIX, cmd.exe on Windows. Windows backs the path with backslashes and
 * cmd.exe only recognizes double quotes, so the quoting must differ per
 * platform or the path (quotes and all) is passed to nvim literally and fails.
 */
export function shellQuote(value: string): string {
  if (process.platform === "win32") {
    // cmd.exe: double quotes; a doubled quote is the escape for a literal one.
    return `"${value.replace(/"/g, '""')}"`;
  }
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/** Synchronously sleep (Node allows Atomics.wait on the main thread). */
export function sleepSync(ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

// ---------------------------------------------------------------------------
// Markdown preview (the "peek view" shared by both extensions)
// ---------------------------------------------------------------------------

/** Render Markdown to terminal lines at the given width. */
export function renderMarkdownLines(text: string, width: number): string[] {
  try {
    return new Markdown(text, 1, 0, getMarkdownTheme()).render(width);
  } catch {
    return text.split("\n");
  }
}

/** Cache of rendered Markdown lines, keyed by content id + width. */
export class MarkdownCache {
  private cache?: { key: string; width: number; lines: string[] };

  lines(key: string, text: string, width: number): string[] {
    if (this.cache && this.cache.key === key && this.cache.width === width) {
      return this.cache.lines;
    }
    const lines = renderMarkdownLines(text, width);
    this.cache = { key, width, lines };
    return lines;
  }

  clear(): void {
    this.cache = undefined;
  }
}

/** The visible slice of a preview at `scroll`, `height` rows tall. */
export function slicePreview(lines: string[], scroll: number, height: number): string[] {
  return lines.slice(scroll, scroll + height);
}

/** `lines 1-18/60` label for the current preview position. */
export function lineRangeLabel(total: number, scroll: number, visible: number): string {
  if (total <= 0) return "lines 0-0/0";
  const from = Math.max(1, Math.min(scroll + 1, total));
  const to = Math.min(scroll + visible, total);
  return `lines ${from}-${to}/${total}`;
}
