/**
 * notes-viewer — browse and read the Markdown notes saved under
 * `~/.pi/agent/notes/` without leaving pi.
 *
 * Command:
 *   /notes-view   Open the notes browser.
 *
 * Structure mirrors pi's built-in Session Tree (`/tree`): a title line, a
 * wrapping key-hint line, and a `Type to search:` line. All actions are on
 * ctrl+ keys so nothing needs to be typed:
 *
 *   ↑/↓ move · ←/→ fold · pgup/pgdn page · ctrl+←/→ branch · ctrl+x copy ·
 *   ctrl+r rename · ctrl+d delete · ctrl+s scope · ctrl+o cycle · ctrl+e edit
 *
 * Enter opens the selected note (focus moves to the content pane; ↑/↓ scroll,
 * Esc returns). ctrl+e / the editor shortcut opens nvim in a new herdr tab;
 * that tab is closed again when pi quits.
 *
 * Install: copy to ~/.pi/agent/extensions/notes-viewer.ts.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { DynamicBorder, copyToClipboard, getAgentDir, getMarkdownTheme } from "@earendil-works/pi-coding-agent";
import {
  Input,
  Key,
  Markdown,
  matchesKey,
  truncateToWidth,
  visibleWidth,
  wrapTextWithAnsi,
} from "@earendil-works/pi-tui";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** `/home/denis/llm` -> `--home-denis-llm--` (pi's session dir convention). */
function projectDirName(cwd: string): string {
  const stripped = cwd.replace(/^\/+/, "").replace(/\/+$/, "");
  return `--${stripped.replace(/\//g, "-")}--`;
}

function sanitizeStem(title: string): string {
  let out = "";
  for (const ch of title) {
    if (/[A-Za-z0-9\-_. ]/.test(ch)) out += ch;
    else out += "_";
  }
  const trimmed = out.trim().replace(/\.+$/, "").replace(/ /g, "_");
  return trimmed.length > 0 ? trimmed : "note";
}

function notesRoot(): string {
  return path.join(getAgentDir(), "notes");
}

function shorten(p: string): string {
  const home = os.homedir();
  return p.startsWith(home) ? `~${p.slice(home.length)}` : p;
}

function formatAge(mtimeMs: number): string {
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

interface NoteNode {
  name: string;
  path: string;
  isDir: boolean;
  project: string;
  mtimeMs: number;
  children: NoteNode[];
}

/** Recursively scan `root` for directories and `.md` files. */
function scanNotes(root: string, project = ""): NoteNode[] {
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(root, { withFileTypes: true });
  } catch {
    return [];
  }

  const dirs: NoteNode[] = [];
  const files: NoteNode[] = [];
  for (const entry of entries) {
    if (entry.name.startsWith(".")) continue;
    const full = path.join(root, entry.name);
    if (entry.isDirectory()) {
      const childProject = project || entry.name;
      dirs.push({
        name: entry.name,
        path: full,
        isDir: true,
        project: childProject,
        mtimeMs: 0,
        children: scanNotes(full, childProject),
      });
    } else if (entry.isFile() && entry.name.toLowerCase().endsWith(".md")) {
      let mtimeMs = 0;
      try {
        mtimeMs = fs.statSync(full).mtimeMs;
      } catch {
        /* ignore */
      }
      files.push({
        name: entry.name,
        path: full,
        isDir: false,
        project: project || path.basename(root),
        mtimeMs,
        children: [],
      });
    }
  }
  dirs.sort((a, b) => a.name.localeCompare(b.name));
  files.sort((a, b) => b.mtimeMs - a.mtimeMs || a.name.localeCompare(b.name));
  return [...dirs, ...files];
}

function countFiles(nodes: NoteNode[]): number {
  let n = 0;
  for (const node of nodes) n += node.isDir ? countFiles(node.children) : 1;
  return n;
}

/** Pad a (possibly ANSI-styled) line to exactly `width` visible columns. */
function padTo(line: string, width: number): string {
  const current = visibleWidth(line);
  if (current >= width) return truncateToWidth(line, width, "");
  return line + " ".repeat(width - current);
}

/** POSIX single-quote escaping for a path passed to a shell. */
function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/** Synchronously sleep (Node allows Atomics.wait on the main thread). */
function sleepSync(ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

// ---------------------------------------------------------------------------
// Viewer component
// ---------------------------------------------------------------------------

const HELP_TEXT =
  "↑/↓ move · ←/→ fold · pgup/pgdn page · ctrl+←/→ branch · ctrl+x copy · " +
  "ctrl+r rename · ctrl+d delete · ctrl+s scope · ctrl+e edit";

interface FlatNote {
  node: NoteNode;
  depth: number;
  isLast: boolean;
  gutters: boolean[];
}

interface StatusMessage {
  text: string;
  type: "info" | "error" | "muted";
}

class NotesViewer {
  private roots: NoteNode[] = [];
  private collapsed = new Set<string>();
  private visible: FlatNote[] = [];
  private cursor = 0;
  private scroll = 0;

  private selectedPath: string | undefined;
  private content = "";
  private previewScroll = 0;
  private readonly contentCache = new Map<string, string>();
  private readonly parentPath = new Map<string, string>();
  private readonly childPaths = new Map<string, string[]>();

  private scopeAll: boolean;
  private pendingDelete: string | undefined;
  private focus: "list" | "content" = "list";
  private pendingRefresh = false;
  private readonly onEditorTab?: (tabId: string) => void;

  private search = "";
  private renameInput: Input | null = null;
  private renameTarget: NoteNode | null = null;

  private status: StatusMessage | undefined;
  private statusTimer: ReturnType<typeof setTimeout> | undefined;

  private mdCache?: { path: string; width: number; lines: string[] };

  private cachedWidth?: number;
  private cachedLines?: string[];

  constructor(
    private readonly tui: any,
    private readonly theme: any,
    private readonly cwd: string,
    private readonly done: (value: null) => void,
    options?: { onEditorTab?: (tabId: string) => void },
  ) {
    this.onEditorTab = options?.onEditorTab;
    this.scopeAll = true;
    this.reload();
    this.focusFirstFile();
  }

  // --- layout ---------------------------------------------------------------

  private width(): number {
    return this.tui.terminal?.columns ?? (typeof this.tui.width === "number" ? this.tui.width : 80);
  }

  private rows(): number {
    return this.tui.terminal?.rows ?? (typeof this.tui.height === "number" ? this.tui.height : 24);
  }

  private helpLines(): string[] {
    return wrapTextWithAnsi(`  ${HELP_TEXT}`, Math.max(10, this.width())).map((line) =>
      this.theme.fg("muted", line),
    );
  }

  private contentHeight(): number {
    return Math.max(4, this.rows() - (8 + this.helpLines().length));
  }

  private entryRows(): number {
    return Math.max(1, this.contentHeight() - 1);
  }

  private layout(): { listWidth: number; previewWidth: number } {
    const width = this.width();
    const listWidth = Math.max(14, Math.min(46, Math.min(width - 2, Math.round(width * 0.36))));
    return { listWidth, previewWidth: Math.max(1, width - listWidth - 1) };
  }

  // --- data -----------------------------------------------------------------

  private reload(): void {
    this.contentCache.clear();
    this.mdCache = undefined;
    this.content = "";
    this.roots = scanNotes(notesRoot());
    this.parentPath.clear();
    this.childPaths.clear();
    const index = (nodes: NoteNode[], parent?: string) => {
      for (const node of nodes) {
        if (parent) this.parentPath.set(node.path, parent);
        if (node.isDir && node.children.length) {
          this.childPaths.set(
            node.path,
            node.children.map((c) => c.path),
          );
        }
        if (node.isDir) index(node.children, node.path);
      }
    };
    index(this.roots);
    this.rebuild();
  }

  private scopedRoots(): NoteNode[] {
    if (this.scopeAll) return this.roots;
    const wanted = projectDirName(this.cwd);
    return this.roots.filter((r) => r.name === wanted);
  }

  private query(): string {
    return this.search.trim().toLowerCase();
  }

  private rebuild(): void {
    const q = this.query();
    const roots = this.scopedRoots();
    if (q) {
      const matches: FlatNote[] = [];
      const walk = (node: NoteNode, depth: number, isLast: boolean, gutters: boolean[]) => {
        if (!node.isDir) {
          const haystack = `${node.name} ${node.project}`.toLowerCase();
          if (haystack.includes(q) || this.fileContent(node.path).toLowerCase().includes(q)) {
            matches.push({ node, depth, isLast, gutters });
          }
          return;
        }
        node.children.forEach((child, i) =>
          walk(child, depth + 1, i === node.children.length - 1, [...gutters, !isLast]),
        );
      };
      roots.forEach((root, i) => walk(root, 0, i === roots.length - 1, []));
      this.visible = matches;
    } else {
      const out: FlatNote[] = [];
      const walk = (node: NoteNode, depth: number, isLast: boolean, gutters: boolean[]) => {
        out.push({ node, depth, isLast, gutters });
        if (node.isDir && !this.collapsed.has(node.path)) {
          node.children.forEach((child, i) =>
            walk(child, depth + 1, i === node.children.length - 1, [...gutters, !isLast]),
          );
        }
      };
      roots.forEach((root, i) => walk(root, 0, i === roots.length - 1, []));
      this.visible = out;
    }

    if (this.cursor >= this.visible.length) this.cursor = Math.max(0, this.visible.length - 1);
    this.syncPreview();
    this.invalidate();
    this.tui.requestRender();
  }

  private current(): FlatNote | undefined {
    return this.visible[this.cursor];
  }

  private syncPreview(): void {
    const node = this.current()?.node;
    if (node && !node.isDir) this.loadFile(node);
  }

  /** Read a note's text (capped) for content search; cached per path. */
  private fileContent(filePath: string): string {
    const cached = this.contentCache.get(filePath);
    if (cached !== undefined) return cached;
    let text = "";
    try {
      const fd = fs.openSync(filePath, "r");
      const buffer = Buffer.alloc(64 * 1024);
      const read = fs.readSync(fd, buffer, 0, buffer.length, 0);
      fs.closeSync(fd);
      text = buffer.subarray(0, read).toString("utf8");
    } catch {
      text = "";
    }
    this.contentCache.set(filePath, text);
    return text;
  }

  private loadFile(node: NoteNode): void {
    if (node.path === this.selectedPath && this.content !== "") return;
    try {
      this.content = fs.readFileSync(node.path, "utf8");
    } catch (error: any) {
      this.content = `# Could not read note\n\n${error?.message ?? error}`;
    }
    this.selectedPath = node.path;
    this.previewScroll = 0;
    this.mdCache = undefined;
  }

  private focusFirstFile(): void {
    const index = this.visible.findIndex((v) => !v.node.isDir);
    if (index >= 0) {
      this.cursor = index;
      this.ensureVisible();
      this.syncPreview();
      this.invalidate();
      this.tui.requestRender();
    }
  }

  private focusPath(target: string): void {
    const index = this.visible.findIndex((v) => v.node.path === target);
    if (index >= 0) {
      this.cursor = index;
      this.ensureVisible();
      this.syncPreview();
      this.invalidate();
    }
  }

  // --- navigation -----------------------------------------------------------

  private move(delta: number): void {
    if (this.visible.length === 0) return;
    const count = this.visible.length;
    this.cursor = (this.cursor + delta + count) % count;
    this.ensureVisible();
    this.syncPreview();
    this.invalidate();
    this.tui.requestRender();
  }

  private page(delta: number): void {
    this.move(delta * Math.max(1, this.entryRows()));
  }

  private ensureVisible(): void {
    const height = this.entryRows();
    if (this.cursor < this.scroll) this.scroll = this.cursor;
    if (this.cursor >= this.scroll + height) this.scroll = this.cursor - height + 1;
    if (this.scroll < 0) this.scroll = 0;
  }

  private branchJump(direction: "up" | "down"): void {
    const node = this.current()?.node;
    if (!node) return;
    const target = direction === "up" ? this.parentPath.get(node.path) : this.childPaths.get(node.path)?.[0];
    if (!target) return;
    const index = this.visible.findIndex((v) => v.node.path === target);
    if (index >= 0) {
      this.cursor = index;
      this.ensureVisible();
      this.syncPreview();
      this.invalidate();
      this.tui.requestRender();
    }
  }

  private toggleDir(node: NoteNode): void {
    if (this.collapsed.has(node.path)) this.collapsed.delete(node.path);
    else this.collapsed.add(node.path);
    this.rebuild();
  }

  // --- actions --------------------------------------------------------------

  private setStatus(text: string, type: StatusMessage["type"] = "info"): void {
    this.status = { text, type };
    if (this.statusTimer) clearTimeout(this.statusTimer);
    if (type !== "error") {
      this.statusTimer = setTimeout(() => {
        this.status = undefined;
        this.statusTimer = undefined;
        this.invalidate();
        this.tui.requestRender();
      }, 4000);
    }
    this.invalidate();
    this.tui.requestRender();
  }

  private error(text: string): void {
    this.setStatus(text, "error");
  }

  private toggleScope(): void {
    this.scopeAll = !this.scopeAll;
    this.cursor = 0;
    this.scroll = 0;
    this.rebuild();
    this.setStatus(this.scopeAll ? "Scope: all projects" : "Scope: current project");
  }

  private copySelected(): void {
    const node = this.current()?.node;
    if (!node || node.isDir) {
      this.error("Select a note to copy.");
      return;
    }
    const text = this.fileContent(node.path) || node.path;
    copyToClipboard(text)
      .then(() => this.setStatus(`Copied ${node.name}`))
      .catch(() => this.error("Copy failed"));
  }

  private beginRename(): void {
    const node = this.current()?.node;
    if (!node || node.isDir) {
      this.error("Select a note file to rename.");
      return;
    }
    const input = new Input({ placeholder: "new name" });
    input.setValue(node.name.replace(/\.md$/i, ""));
    this.renameInput = input;
    this.renameTarget = node;
    this.invalidate();
    this.tui.requestRender();
  }

  private commitRename(): void {
    const node = this.renameTarget;
    const input = this.renameInput;
    if (!node || !input) return;
    const value = input.getValue().trim();
    this.renameInput = null;
    this.renameTarget = null;
    if (!value) {
      this.setStatus("Rename cancelled");
      return;
    }
    const stem = sanitizeStem(value.replace(/\.md$/i, ""));
    const target = path.join(path.dirname(node.path), `${stem}.md`);
    if (target === node.path) {
      this.setStatus("Name unchanged");
      return;
    }
    if (fs.existsSync(target)) {
      this.error(`${path.basename(target)} already exists.`);
      return;
    }
    try {
      fs.renameSync(node.path, target);
    } catch (error: any) {
      this.error(`Rename failed: ${error?.message ?? error}`);
      return;
    }
    this.selectedPath = target;
    this.content = "";
    this.reload();
    this.focusPath(target);
    this.setStatus(`Renamed to ${path.basename(target)}`);
  }

  private cancelRename(): void {
    this.renameInput = null;
    this.renameTarget = null;
    this.invalidate();
    this.tui.requestRender();
  }

  private requestDelete(): void {
    const node = this.current()?.node;
    if (!node || node.isDir) {
      this.error("Select a note file to delete.");
      return;
    }
    this.pendingDelete = node.path;
    this.invalidate();
    this.tui.requestRender();
  }

  private confirmDelete(): void {
    const target = this.pendingDelete;
    this.pendingDelete = undefined;
    if (!target) return;
    try {
      fs.unlinkSync(target);
    } catch (error: any) {
      this.error(`Delete failed: ${error?.message ?? error}`);
      return;
    }
    if (this.selectedPath === target) {
      this.selectedPath = undefined;
      this.content = "";
      this.mdCache = undefined;
    }
    this.contentCache.delete(target);
    this.cursor = Math.max(0, this.cursor - 1);
    this.reload();
    this.setStatus(`Deleted ${path.basename(target)}`);
  }

  /** Open the selected note in nvim inside a new herdr tab. */
  private openInEditor(): void {
    const node = this.current()?.node;
    if (!node || node.isDir) {
      this.error("Select a note file to edit.");
      return;
    }
    if (process.env.HERDR_ENV !== "1" && !process.env.HERDR_SOCKET_PATH) {
      this.error("herdr is not available in this session.");
      return;
    }
    const herdr = process.env.HERDR_BIN_PATH || "herdr";
    try {
      const createArgs = [
        "tab",
        "create",
        "--cwd",
        path.dirname(node.path),
        "--label",
        `nvim ${path.basename(node.path)}`,
        "--focus",
      ];
      if (process.env.HERDR_WORKSPACE_ID) {
        createArgs.push("--workspace", process.env.HERDR_WORKSPACE_ID);
      }
      const raw = execFileSync(herdr, createArgs, { encoding: "utf8", timeout: 5000 });
      const created = JSON.parse(raw);
      const paneId = created?.result?.root_pane?.pane_id;
      const tabId = created?.result?.tab?.tab_id;
      if (!paneId) throw new Error("herdr did not return a pane id");
      if (tabId) this.onEditorTab?.(tabId);

      const command = `nvim ${shellQuote(node.path)}`;
      let lastError: unknown;
      for (let attempt = 0; attempt < 5; attempt++) {
        try {
          execFileSync(herdr, ["pane", "run", paneId, command], { encoding: "utf8", timeout: 5000 });
          lastError = undefined;
          break;
        } catch (error) {
          lastError = error;
          sleepSync(150);
        }
      }
      if (lastError) throw lastError;
      this.pendingRefresh = true;
      this.setStatus(`Opened ${path.basename(node.path)} in nvim (new herdr tab)`);
    } catch (error: any) {
      const detail = error?.stderr?.toString?.() || error?.message || String(error);
      this.error(`Failed to open nvim: ${String(detail).trim()}`);
    }
  }

  /** Re-read the selected note from disk (after editing it elsewhere). */
  private refreshSelected(): void {
    const node = this.current()?.node;
    if (!node || node.isDir) return;
    this.mdCache = undefined;
    this.content = "";
    this.selectedPath = undefined;
    this.loadFile(node);
    this.invalidate();
    this.tui.requestRender();
  }

  // --- input ----------------------------------------------------------------

  private appendSearch(text: string): void {
    this.search += text;
    this.cursor = 0;
    this.scroll = 0;
    this.rebuild();
    this.tui.requestRender();
  }

  handleInput(data: string): void {
    if (this.pendingRefresh) {
      this.pendingRefresh = false;
      this.refreshSelected();
    }

    if (this.renameInput) {
      if (matchesKey(data, Key.enter)) return this.commitRename();
      if (matchesKey(data, Key.escape)) return this.cancelRename();
      this.renameInput.handleInput(data);
      this.invalidate();
      this.tui.requestRender();
      return;
    }

    if (this.pendingDelete) {
      if (matchesKey(data, Key.enter) || matchesKey(data, Key.ctrl("d"))) return this.confirmDelete();
      if (matchesKey(data, Key.escape)) {
        this.pendingDelete = undefined;
        this.invalidate();
        this.tui.requestRender();
        return;
      }
    }

    // Global ctrl+ actions.
    if (matchesKey(data, Key.ctrl("e"))) return this.openInEditor();
    if (matchesKey(data, Key.ctrl("x"))) return this.copySelected();
    if (matchesKey(data, Key.ctrl("r"))) return this.beginRename();
    if (matchesKey(data, Key.ctrl("d"))) return this.requestDelete();
    if (matchesKey(data, Key.ctrl("s")) || matchesKey(data, Key.ctrl("o"))) return this.toggleScope();

    if (matchesKey(data, Key.escape)) {
      if (this.search) {
        this.search = "";
        this.cursor = 0;
        this.rebuild();
        return;
      }
      if (this.focus === "content") {
        this.focus = "list";
        this.invalidate();
        this.tui.requestRender();
        return;
      }
      this.done(null);
      return;
    }

    if (this.focus === "content") {
      if (matchesKey(data, Key.up)) return this.scrollPreview(-1);
      if (matchesKey(data, Key.down)) return this.scrollPreview(1);
      if (matchesKey(data, Key.pageUp)) return this.scrollPreview(-this.entryRows());
      if (matchesKey(data, Key.pageDown)) return this.scrollPreview(this.entryRows());
      if (matchesKey(data, Key.home)) return this.scrollPreview(-Number.MAX_SAFE_INTEGER);
      if (matchesKey(data, Key.end)) return this.scrollPreview(Number.MAX_SAFE_INTEGER);
      if (matchesKey(data, Key.enter)) {
        this.focus = "list";
        this.invalidate();
        this.tui.requestRender();
        return;
      }
    } else {
      if (matchesKey(data, Key.up)) return this.move(-1);
      if (matchesKey(data, Key.down)) return this.move(1);
      if (matchesKey(data, Key.pageUp)) return this.page(-1);
      if (matchesKey(data, Key.pageDown)) return this.page(1);
      if (matchesKey(data, Key.left)) {
        const node = this.current()?.node;
        if (node?.isDir) this.toggleDir(node);
        return;
      }
      if (matchesKey(data, Key.right) || matchesKey(data, Key.tab)) {
        const node = this.current()?.node;
        if (node?.isDir) this.toggleDir(node);
        return;
      }
      if (matchesKey(data, Key.enter)) {
        const node = this.current()?.node;
        if (node?.isDir) {
          this.toggleDir(node);
        } else if (node) {
          this.loadFile(node);
          this.previewScroll = 0;
          this.focus = "content";
          this.setStatus(`Viewing ${path.basename(node.path)} — ↑↓ scroll · esc back`);
        }
        return;
      }
    }

    if (matchesKey(data, Key.backspace)) {
      if (this.search.length > 0) {
        this.search = this.search.slice(0, -1);
        this.cursor = 0;
        this.scroll = 0;
        this.rebuild();
        this.tui.requestRender();
      }
      return;
    }

    const hasControlChars = [...data].some((ch) => {
      const code = ch.charCodeAt(0);
      return code < 32 || code === 0x7f || (code >= 0x80 && code <= 0x9f);
    });
    if (!hasControlChars && data.length > 0) this.appendSearch(data);
  }

  invalidate(): void {
    this.cachedWidth = undefined;
    this.cachedLines = undefined;
  }

  // --- render ---------------------------------------------------------------

  render(width: number): string[] {
    if (this.cachedWidth === width && this.cachedLines) return this.cachedLines;
    const theme = this.theme;
    const border = (s: string) => theme.fg("border", s);
    const lines: string[] = [];

    lines.push("");
    lines.push(...new DynamicBorder(border).render(width));

    const scopeLabel = this.scopeAll ? "All projects" : "Current project";
    const focusBadge = theme.fg("accent", this.focus === "content" ? "[content] " : "[list] ");
    const total = countFiles(this.scopedRoots());
    const left =
      focusBadge + theme.bold("Stored Notes") + theme.fg("muted", `  ${scopeLabel}`);
    const selectedName = this.selectedPath ? path.basename(this.selectedPath) : "";
    const right = theme.fg(
      selectedName ? "accent" : "muted",
      `${selectedName ? selectedName + "  " : ""}${total} note${total === 1 ? "" : "s"}`,
    );
    const gap = Math.max(1, width - visibleWidth(left) - visibleWidth(right));
    lines.push(truncateToWidth(left + " ".repeat(gap) + right, width));

    lines.push(...this.helpLines());

    if (this.renameInput) {
      const prefix = `  ${theme.fg("muted", "Rename note:")} `;
      const inputLine = this.renameInput.render(Math.max(8, width - visibleWidth(prefix)))[0] ?? "";
      lines.push(truncateToWidth(prefix + inputLine, width));
    } else {
      const query = this.search ? ` ${theme.fg("accent", this.search)}` : "";
      lines.push(truncateToWidth(`  ${theme.fg("muted", "Type to search:")}${query}`, width));
    }

    lines.push(...new DynamicBorder(border).render(width));
    lines.push("");

    const rows = this.entryRows();
    const { listWidth, previewWidth } = this.layout();
    const listLines = this.renderList(listWidth, rows);
    const previewLines = this.renderPreview(previewWidth, rows);
    const divider = theme.fg("border", "│");
    for (let i = 0; i < rows; i++) {
      lines.push(
        padTo(listLines[i] ?? "", listWidth) + divider + truncateToWidth(previewLines[i] ?? "", previewWidth, ""),
      );
    }

    if (this.pendingDelete) {
      lines.push(
        truncateToWidth(
          theme.fg(
            "error",
            `  Delete ${path.basename(this.pendingDelete)}? enter/ctrl+d confirm · esc cancel`,
          ),
          width,
        ),
      );
    } else if (this.status) {
      const color = this.status.type === "error" ? "error" : this.status.type === "muted" ? "muted" : "accent";
      lines.push(truncateToWidth(theme.fg(color, `  ${this.status.text}`), width));
    } else {
      const pos = this.visible.length ? `  (${this.cursor + 1}/${this.visible.length})` : "  (0/0)";
      const name = this.selectedPath ? ` · ${path.basename(this.selectedPath)}` : "";
      lines.push(truncateToWidth(theme.fg("muted", pos + name), width));
    }

    lines.push("");
    lines.push(...new DynamicBorder(border).render(width));

    this.cachedWidth = width;
    this.cachedLines = lines;
    return lines;
  }

  private renderList(width: number, height: number): string[] {
    const theme = this.theme;
    const out: string[] = [];
    if (this.visible.length === 0) {
      const root = notesRoot();
      const message = fs.existsSync(root)
        ? "  No notes match. Save one with /note."
        : `  No notes yet at ${shorten(root)}.`;
      out.push(theme.fg("muted", truncateToWidth(message, width, "…")));
      return out;
    }

    for (let i = this.scroll; i < Math.min(this.scroll + height, this.visible.length); i++) {
      const flat = this.visible[i];
      const node = flat.node;
      const isCursor = i === this.cursor;
      const isCurrent = !node.isDir && node.path === this.selectedPath;

      const gutters = flat.gutters.map((g) => (g ? "│  " : "   ")).join("");
      const connector = flat.depth === 0 ? "" : flat.isLast ? "└─ " : "├─ ";
      const icon = node.isDir
        ? this.collapsed.has(node.path)
          ? "▸ "
          : "▾ "
        : isCurrent
          ? theme.fg("accent", "• ")
          : "  ";
      const cursor = isCursor ? theme.fg("accent", "› ") : "  ";

      const nameColor = node.isDir ? "accent" : isCurrent ? "accent" : undefined;
      const age = node.isDir ? "" : theme.fg("dim", `  ${formatAge(node.mtimeMs)}`);
      const available = Math.max(8, width - 2 - visibleWidth(gutters) - 4 - visibleWidth(age));
      const name = truncateToWidth(node.name, available, "…");

      let line =
        cursor +
        theme.fg("dim", gutters) +
        connector +
        icon +
        (nameColor ? theme.fg(nameColor, name) : name) +
        age;
      if (isCursor) line = theme.bg("selectedBg", line);
      out.push(truncateToWidth(line, width));
    }
    return out;
  }

  private mdLines(width: number): string[] {
    if (!this.content) return [this.theme.fg("muted", "No note selected.")];
    if (this.mdCache && this.mdCache.path === this.selectedPath && this.mdCache.width === width) {
      return this.mdCache.lines;
    }
    let lines: string[];
    try {
      const md = new Markdown(this.content, 1, 0, getMarkdownTheme());
      lines = md.render(width);
    } catch {
      lines = this.content.split("\n");
    }
    this.mdCache = { path: this.selectedPath ?? "", width, lines };
    return lines;
  }

  private renderPreview(width: number, height: number): string[] {
    const theme = this.theme;
    const all = this.mdLines(width);
    const out: string[] = [];
    if (this.previewScroll > 0) {
      out.push(theme.fg("dim", `  ↑ ${this.previewScroll} more line(s) above`));
    }
    for (let i = this.previewScroll; i < Math.min(this.previewScroll + height, all.length); i++) {
      out.push(truncateToWidth(all[i] ?? "", width, ""));
    }
    const below = all.length - (this.previewScroll + height);
    if (below > 0) out.push(theme.fg("dim", `  ↓ ${below} more line(s) below`));
    while (out.length < height) out.push("");
    return out.slice(0, height);
  }

  private scrollPreview(delta: number): void {
    const width = this.layout().previewWidth;
    const total = this.mdLines(width).length;
    const max = Math.max(0, total - this.entryRows());
    this.previewScroll = Math.max(0, Math.min(max, this.previewScroll + delta));
    this.invalidate();
    this.tui.requestRender();
  }
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export default function (pi: ExtensionAPI) {
  // herdr tabs opened for editing; closed when pi exits so they don't linger.
  const editorTabs = new Set<string>();

  pi.on("session_shutdown", async (event) => {
    if (event.reason !== "quit" || editorTabs.size === 0) return;
    closeHerdrTabs(editorTabs);
    editorTabs.clear();
  });

  pi.registerCommand("notes-view", {
    description:
      "Browse, preview, rename, delete and edit stored Markdown notes (ctrl+r/d/s/e, type to search)",
    handler: async (_args, ctx) => {
      if (ctx.mode !== "tui") {
        ctx.ui.notify("The notes viewer needs interactive (TUI) mode.", "error");
        return;
      }

      await ctx.ui.custom<null>((tui, theme, _kb, done) => {
        return new NotesViewer(tui, theme, ctx.cwd, done, {
          onEditorTab: (tabId) => editorTabs.add(tabId),
        }) as any;
      });
    },
  });
}

/** Close herdr tabs previously opened for editing (best effort). */
function closeHerdrTabs(tabs: Iterable<string>): void {
  if (process.env.HERDR_ENV !== "1" && !process.env.HERDR_SOCKET_PATH) return;
  const herdr = process.env.HERDR_BIN_PATH || "herdr";
  for (const tab of tabs) {
    try {
      execFileSync(herdr, ["tab", "close", tab], { stdio: "ignore", timeout: 2000 });
    } catch {
      /* tab already gone */
    }
  }
}
