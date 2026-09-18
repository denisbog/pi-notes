/**
 * session-notes — save selected items from the current pi session as a
 * Markdown note.
 *
 * Command:
 *   /note   Open a filterable list of the current session's entries, styled
 *           like pi's built-in Session Tree (`/tree`): the same title, key
 *           hints and "Type to search:" line, plus the ctrl+key filters
 *           (ctrl+d/t/u/l/a, cycle ctrl+o / ctrl+shift+o).
 *
 *   Navigation: ↑/↓ move · pgup/pgdn (←/→) page · ctrl+←/→ branch ·
 *               ctrl+x copy · shift+l label · shift+t label time
 *   Selection:  Tab toggles the highlighted entry; selected entries stay
 *               visible even when they no longer match the filter/search.
 *   Save:       Enter (or /save) writes the selected entries as one note.
 *
 * Notes are written to the same location the Rust `pi-notes` app uses:
 *
 *   ~/.pi/agent/notes/<project-dir>/<timestamp>_<title>.md
 *
 * where `<project-dir>` mirrors pi's session directory naming
 * (`/home/denis/llm` -> `--home-denis-llm--`).
 *
 * Install: copy to ~/.pi/agent/extensions/session-notes.ts.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { DynamicBorder, copyToClipboard } from "@earendil-works/pi-coding-agent";
import {
  Input,
  Key,
  matchesKey,
  truncateToWidth,
  visibleWidth,
  wrapTextWithAnsi,
} from "@earendil-works/pi-tui";
import {
  MarkdownCache,
  lineRangeLabel,
  padTo,
  saveNote,
  slicePreview,
} from "./notes-shared/shared.ts";

// ---------------------------------------------------------------------------
// Session entry rendering helpers
// ---------------------------------------------------------------------------

type Entry = any;

function contentToText(content: any): string {
  if (content == null) return "";
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((block: any) => {
        if (!block || typeof block !== "object") return "";
        switch (block.type) {
          case "text":
            return block.text ?? "";
          case "image":
            return `_[image ${block.mimeType ?? "image"}]_`;
          case "toolCall":
            return `\`${block.name}\``;
          default:
            return "";
        }
      })
      .filter(Boolean)
      .join("\n\n");
  }
  return String(content);
}

function blocksOf(message: any, type: string): any[] {
  const content = message?.content;
  if (!Array.isArray(content)) return [];
  return content.filter((b: any) => b && b.type === type);
}

function hasTextContent(content: any): boolean {
  if (typeof content === "string") return content.trim().length > 0;
  if (!Array.isArray(content)) return false;
  return content.some((b: any) => b?.type === "text" && String(b.text ?? "").trim().length > 0);
}

function quote(text: string): string {
  return text
    .split("\n")
    .map((line) => `> ${line}`)
    .join("\n");
}

function entryHeadline(entry: Entry): string {
  const oneLine = (s: string) => s.replace(/[\n\t\r]/g, " ").replace(/\s+/g, " ").trim();
  switch (entry.type) {
    case "message": {
      const m = entry.message ?? {};
      if (m.role === "user") return `user: ${oneLine(contentToText(m.content))}`;
      if (m.role === "assistant") {
        const text = oneLine(contentToText(m.content));
        if (text) return `assistant: ${text}`;
        const thinking = blocksOf(m, "thinking")
          .map((b) => b.thinking)
          .join(" ");
        return `assistant: ${thinking ? `(thinking) ${oneLine(thinking)}` : "(no content)"}`;
      }
      if (m.role === "toolResult") return `${m.toolName ?? "tool"}: ${oneLine(contentToText(m.content))}`;
      if (m.role === "bashExecution") return `bash: ${oneLine(m.command ?? "")}`;
      if (m.role === "branchSummary") return `branch summary: ${oneLine(m.summary ?? "")}`;
      if (m.role === "compactionSummary") return `compaction: ${oneLine(m.summary ?? "")}`;
      if (m.role === "custom") return `${m.customType}: ${oneLine(contentToText(m.content))}`;
      return `${m.role ?? "message"}`;
    }
    case "custom_message":
      return `${entry.customType}: ${oneLine(contentToText(entry.content))}`;
    case "compaction":
      return `compaction (${entry.tokensBefore ?? "?"} tokens)`;
    case "branch_summary":
      return `branch summary: ${oneLine(entry.summary ?? "")}`;
    case "model_change":
      return `model: ${entry.provider}/${entry.modelId}`;
    case "thinking_level_change":
      return `thinking: ${entry.thinkingLevel}`;
    case "session_info":
      return `title: ${entry.name ?? ""}`;
    case "custom":
      return `custom: ${entry.customType}`;
    case "label":
      return `label: ${entry.label ?? ""}`;
    default:
      return entry.type;
  }
}

function entrySearchText(entry: Entry, label?: string): string {
  return `${entry.id} ${entry.type} ${label ?? ""} ${entryHeadline(entry)}`.toLowerCase();
}

function entryToMarkdown(entry: Entry): string {
  switch (entry.type) {
    case "message": {
      const m = entry.message ?? {};
      switch (m.role) {
        case "user":
          return `## User\n\n${contentToText(m.content)}\n`;
        case "assistant": {
          const parts: string[] = [];
          const thinking = blocksOf(m, "thinking")
            .map((b) => b.thinking ?? "")
            .filter(Boolean)
            .join("\n\n");
          if (thinking) parts.push(`> **Thinking**\n>\n${quote(thinking)}`);
          const text = blocksOf(m, "text")
            .map((b) => b.text ?? "")
            .filter(Boolean)
            .join("\n\n");
          if (text) parts.push(text);
          const tools = blocksOf(m, "toolCall");
          if (tools.length) {
            parts.push(
              tools
                .map((t) => `**Tool call:** \`${t.name}\`\n\n\`\`\`json\n${JSON.stringify(t.arguments ?? {}, null, 2)}\n\`\`\``)
                .join("\n\n"),
            );
          }
          return `## Assistant\n\n${parts.join("\n\n")}\n`;
        }
        case "toolResult":
          return `## Tool result: ${m.toolName ?? "tool"}\n\n\`\`\`\n${contentToText(m.content)}\n\`\`\`\n`;
        case "bashExecution":
          return `## Bash\n\n\`\`\`sh\n$ ${m.command ?? ""}\n${m.output ?? ""}\n\`\`\`\n`;
        case "branchSummary":
          return `## Branch summary\n\n${m.summary ?? ""}\n`;
        case "compactionSummary":
          return `## Compaction summary\n\n${m.summary ?? ""}\n`;
        case "custom":
          return `## ${m.customType ?? "custom"}\n\n${contentToText(m.content)}\n`;
        default:
          return `## ${m.role ?? "message"}\n\n${contentToText(m.content)}\n`;
      }
    }
    case "custom_message":
      return `## ${entry.customType ?? "custom"}\n\n${contentToText(entry.content)}\n`;
    case "compaction":
      return `## Compaction summary\n\n${entry.summary ?? ""}\n`;
    case "branch_summary":
      return `## Branch summary\n\n${entry.summary ?? ""}\n`;
    case "model_change":
      return `## Model\n\n\`${entry.provider}/${entry.modelId}\`\n`;
    case "thinking_level_change":
      return `## Thinking level\n\n\`${entry.thinkingLevel}\`\n`;
    case "session_info":
      return `## Session title\n\n${entry.name ?? ""}\n`;
    default:
      return "";
  }
}

// ---------------------------------------------------------------------------
// Session Tree look & feel
// ---------------------------------------------------------------------------

const HELP_TEXT =
  "↑/↓ move · ←/→ page · ctrl+←/→ branch · ctrl+x copy · shift+l label · " +
  "shift+t label time · filters ctrl+d/t/u/l/a · cycle ctrl+o/shift+ctrl+o";

type FilterMode = "default" | "no-tools" | "user-only" | "labeled-only" | "all";
const FILTER_MODES: FilterMode[] = ["default", "no-tools", "user-only", "labeled-only", "all"];
const FILTER_LABELS: Record<FilterMode, string> = {
  default: "default",
  "no-tools": "no-tools",
  "user-only": "user",
  "labeled-only": "labeled",
  all: "all",
};

interface FlatNode {
  entry: Entry;
  label?: string;
  labelTimestamp?: string;
  isOnActivePath: boolean;
}

class SessionTreePicker {
  private readonly roots: any[];
  private readonly activeIds: Set<string>;
  private readonly leafId: string | undefined;
  private readonly selected = new Set<string>();
  private readonly pi: any;
  private readonly keybindings: any;
  private readonly allNodes: FlatNode[] = [];
  private readonly parentMap = new Map<string, string>();
  private readonly childrenMap = new Map<string, string[]>();

  private visible: FlatNode[] = [];
  private cursor = 0;
  private scroll = 0;
  private status = "";
  private search = "";
  private filterMode: FilterMode = "default";
  private showLabelTimestamps = false;
  private labelInput: Input | null = null;
  private labelTarget: FlatNode | null = null;
  private peek = false;
  private peekFocus = false;
  private peekScroll = 0;
  private readonly peekCache = new MarkdownCache();

  private cachedWidth?: number;
  private cachedLines?: string[];
  private lastEntryRows = 10;

  constructor(
    private readonly tui: any,
    private readonly theme: any,
    private readonly done: (value: string[] | null) => void,
    roots: any[],
    activeIds: Set<string>,
    leafId: string | undefined,
    pi: any,
    keybindings: any,
  ) {
    this.roots = roots;
    this.activeIds = activeIds;
    this.leafId = leafId;
    this.pi = pi;
    this.keybindings = keybindings;
    this.indexAll();
    this.rebuild();
  }

  /** Flatten the session tree into a depth-first list and record its edges. */
  private indexAll(): void {
    const walk = (node: any, parentId: string | undefined) => {
      const id = node.entry.id;
      if (parentId !== undefined) this.parentMap.set(id, parentId);
      const kids = (node.children ?? []).map((child: any) => child.entry.id);
      if (kids.length) this.childrenMap.set(id, kids);
      this.allNodes.push({
        entry: node.entry,
        label: node.label,
        labelTimestamp: node.labelTimestamp,
        isOnActivePath: this.activeIds.has(id),
      });
      node.children.forEach((child: any) => walk(child, id));
    };
    this.roots.forEach((root) => walk(root, undefined));
  }

  private width(): number {
    return this.tui.terminal?.columns ?? (typeof this.tui.width === "number" ? this.tui.width : 80);
  }

  private rows(): number {
    return this.tui.terminal?.rows ?? (typeof this.tui.height === "number" ? this.tui.height : 24);
  }

  private helpLines(width: number): string[] {
    const wrapped = wrapTextWithAnsi(`  ${HELP_TEXT}`, Math.max(10, width));
    return wrapped.map((line) => this.theme.fg("muted", line));
  }

  private listAreaHeight(): number {
    const helpCount = this.helpLines(this.width()).length;
    return Math.max(4, this.rows() - (8 + helpCount));
  }

  private entryRows(): number {
    return Math.max(1, this.listAreaHeight() - 1);
  }

  private matches(data: string, binding: string, fallbackKey?: string): boolean {
    try {
      if (this.keybindings?.matches?.(data, binding)) return true;
    } catch {
      /* ignore */
    }
    return fallbackKey !== undefined && matchesKey(data, fallbackKey);
  }

  private tokens(): string[] {
    return this.search.trim().toLowerCase().split(/\s+/).filter(Boolean);
  }

  private passesFilter(node: FlatNode): boolean {
    const entry = node.entry;
    const isCurrentLeaf = entry.id === this.leafId;
    if (entry.type === "message" && entry.message?.role === "assistant" && !isCurrentLeaf) {
      const msg = entry.message;
      const isErrorOrAborted = msg.stopReason && msg.stopReason !== "stop" && msg.stopReason !== "toolUse";
      if (!hasTextContent(msg.content) && !isErrorOrAborted) return false;
    }
    const isSettings =
      entry.type === "label" ||
      entry.type === "custom" ||
      entry.type === "model_change" ||
      entry.type === "thinking_level_change" ||
      entry.type === "session_info";
    switch (this.filterMode) {
      case "user-only":
        return entry.type === "message" && entry.message?.role === "user";
      case "no-tools":
        return !isSettings && !(entry.type === "message" && entry.message?.role === "toolResult");
      case "labeled-only":
        return node.label !== undefined;
      case "all":
        return true;
      default:
        return !isSettings;
    }
  }

  private rebuild(): void {
    const tokens = this.tokens();
    this.visible = this.allNodes.filter((node) => {
      if (this.selected.has(node.entry.id)) return true; // selected items stay visible
      if (!this.passesFilter(node)) return false;
      if (tokens.length === 0) return true;
      const text = entrySearchText(node.entry, node.label);
      return tokens.every((token) => text.includes(token));
    });
    if (this.cursor >= this.visible.length) this.cursor = Math.max(0, this.visible.length - 1);
    this.ensureVisible();
    this.invalidate();
  }

  private ensureVisible(): void {
    const height = this.entryRows();
    if (this.cursor < this.scroll) this.scroll = this.cursor;
    if (this.cursor >= this.scroll + height) this.scroll = this.cursor - height + 1;
    if (this.scroll < 0) this.scroll = 0;
  }

  private move(delta: number): void {
    if (this.visible.length === 0) return;
    const count = this.visible.length;
    this.cursor = (this.cursor + delta + count) % count;
    this.ensureVisible();
    this.invalidate();
    this.tui.requestRender();
  }

  private page(delta: number): void {
    this.move(delta * Math.max(1, this.entryRows()));
  }

  /** ctrl+← / ctrl+→ branch jump: parent / first child in the flat list. */
  private branchJump(direction: "up" | "down"): void {
    const id = this.visible[this.cursor]?.entry.id;
    if (!id) return;
    let targetId: string | undefined;
    if (direction === "up") {
      targetId = this.parentMap.get(id);
    } else {
      const children = this.childrenMap.get(id);
      targetId = children?.[0];
    }
    if (!targetId) return;
    const index = this.visible.findIndex((n) => n.entry.id === targetId);
    if (index >= 0) {
      this.cursor = index;
      this.ensureVisible();
      this.invalidate();
      this.tui.requestRender();
    }
  }

  private toggle(): void {
    const node = this.visible[this.cursor];
    if (!node) return;
    const id = node.entry.id;
    if (this.selected.has(id)) this.selected.delete(id);
    else this.selected.add(id);
    this.status = `${this.selected.size} selected`;
    this.rebuild();
    this.tui.requestRender();
  }

  private copySelected(): void {
    const node = this.visible[this.cursor];
    if (!node) return;
    const text = entryToMarkdown(node.entry) || entryHeadline(node.entry);
    copyToClipboard(text)
      .then(() => {
        this.status = "copied to clipboard";
        this.invalidate();
        this.tui.requestRender();
      })
      .catch(() => {
        this.status = "copy failed";
        this.invalidate();
        this.tui.requestRender();
      });
  }

  private beginLabelEdit(): void {
    const node = this.visible[this.cursor];
    if (!node) return;
    const input = new Input({ placeholder: "label (empty to remove)" });
    if (node.label) input.setValue(node.label);
    this.labelInput = input;
    this.labelTarget = node;
    this.invalidate();
    this.tui.requestRender();
  }

  private commitLabel(): void {
    const node = this.labelTarget;
    const input = this.labelInput;
    if (!node || !input) return;
    const value = input.getValue().trim() || undefined;
    try {
      this.pi.setLabel(node.entry.id, value);
    } catch {
      /* ignore */
    }
    node.label = value;
    node.labelTimestamp = value ? new Date().toISOString() : undefined;
    this.status = value ? `label: ${value}` : "label removed";
    this.labelInput = null;
    this.labelTarget = null;
    this.rebuild();
    this.tui.requestRender();
  }

  private cancelLabel(): void {
    this.labelInput = null;
    this.labelTarget = null;
    this.invalidate();
    this.tui.requestRender();
  }

  private setFilter(mode: FilterMode): void {
    this.filterMode = mode;
    this.cursor = 0;
    this.peekScroll = 0;
    this.rebuild();
    this.tui.requestRender();
  }

  /** Peek: preview the highlighted entry's full Markdown content. */
  private togglePeek(): void {
    this.peek = !this.peek;
    this.peekFocus = false;
    this.peekScroll = 0;
    this.invalidate();
    this.tui.requestRender();
  }

  private peekRows(): number {
    return Math.max(1, this.listAreaHeight() - 1);
  }

  private peekLines(width: number): string[] {
    const node = this.visible[this.cursor];
    if (!node) return [this.theme.fg("muted", "  No entry to preview")];
    const markdown = entryToMarkdown(node.entry) || entryHeadline(node.entry);
    return this.peekCache.lines(node.entry.id, markdown, width);
  }

  private scrollPeek(delta: number): void {
    const total = this.peekLines(this.peekLayout().previewWidth).length;
    const max = Math.max(0, total - this.peekRows());
    this.peekScroll = Math.max(0, Math.min(max, this.peekScroll + delta));
    this.invalidate();
    this.tui.requestRender();
  }

  /** Whether peek renders as a right-hand pane (wide) or replaces the list. */
  private peekLayout(): { split: boolean; listWidth: number; previewWidth: number } {
    const width = this.width();
    if (this.peek && width >= 96) {
      const listWidth = Math.max(24, Math.min(46, Math.round(width * 0.36)));
      return { split: true, listWidth, previewWidth: Math.max(20, width - listWidth - 1) };
    }
    return { split: false, listWidth: width, previewWidth: width };
  }

  private peekMove(delta: number): void {
    if (this.visible.length === 0) return;
    const count = this.visible.length;
    this.cursor = (this.cursor + delta + count) % count;
    this.ensureVisible();
    this.peekScroll = 0;
    this.invalidate();
    this.tui.requestRender();
  }

  private peekMoveTo(index: number): void {
    if (this.visible.length === 0) return;
    this.cursor = Math.max(0, Math.min(this.visible.length - 1, index));
    this.ensureVisible();
    this.peekScroll = 0;
    this.invalidate();
    this.tui.requestRender();
  }

  private cycleFilter(direction: 1 | -1): void {
    const index = FILTER_MODES.indexOf(this.filterMode);
    this.setFilter(FILTER_MODES[(index + direction + FILTER_MODES.length) % FILTER_MODES.length]);
  }

  /** Append printable input to the search query; returns false for control keys. */
  private appendSearchInput(data: string): boolean {
    const hasControlChars = [...data].some((ch) => {
      const code = ch.charCodeAt(0);
      return code < 32 || code === 0x7f || (code >= 0x80 && code <= 0x9f);
    });
    if (hasControlChars || data.length === 0) return false;
    this.search += data;
    this.cursor = 0;
    this.peekScroll = 0;
    this.rebuild();
    this.tui.requestRender();
    return true;
  }

  handleInput(data: string): void {
    if (this.status) this.status = "";

    if (this.labelInput) {
      if (matchesKey(data, Key.enter)) return this.commitLabel();
      if (matchesKey(data, Key.escape)) return this.cancelLabel();
      this.labelInput.handleInput(data);
      this.invalidate();
      this.tui.requestRender();
      return;
    }

    // Actions that work in both list and peek mode (so filters stay usable).
    if (this.matches(data, "app.message.copy")) return this.copySelected();
    if (this.matches(data, "app.tree.editLabel")) return this.beginLabelEdit();
    if (this.matches(data, "app.tree.toggleLabelTimestamp")) {
      this.showLabelTimestamps = !this.showLabelTimestamps;
      this.invalidate();
      this.tui.requestRender();
      return;
    }
    if (this.matches(data, "app.tree.filter.default")) return this.setFilter("default");
    if (this.matches(data, "app.tree.filter.noTools")) {
      return this.setFilter(this.filterMode === "no-tools" ? "default" : "no-tools");
    }
    if (this.matches(data, "app.tree.filter.userOnly")) {
      return this.setFilter(this.filterMode === "user-only" ? "default" : "user-only");
    }
    if (this.matches(data, "app.tree.filter.labeledOnly")) {
      return this.setFilter(this.filterMode === "labeled-only" ? "default" : "labeled-only");
    }
    if (this.matches(data, "app.tree.filter.all")) {
      return this.setFilter(this.filterMode === "all" ? "default" : "all");
    }
    if (this.matches(data, "app.tree.filter.cycleForward")) return this.cycleFilter(1);
    if (this.matches(data, "app.tree.filter.cycleBackward")) return this.cycleFilter(-1);

    // Save the note from anywhere (Enter also saves when not peeking).
    if (matchesKey(data, Key.ctrl("w"))) {
      this.done([...this.selected]);
      return;
    }

    if (matchesKey(data, Key.escape)) {
      if (this.search) {
        this.search = "";
        this.cursor = 0;
        this.peekScroll = 0;
        this.rebuild();
        this.tui.requestRender();
        return;
      }
      if (this.peekFocus) {
        this.peekFocus = false;
        this.invalidate();
        this.tui.requestRender();
        return;
      }
      if (this.peek) {
        this.togglePeek();
        return;
      }
      this.done(null);
      return;
    }

    if (matchesKey(data, Key.ctrl("p"))) return this.togglePeek();

    // Peek content focus: Enter opened the preview, arrows now scroll it.
    if (this.peekFocus) {
      if (matchesKey(data, Key.up)) return this.scrollPeek(-1);
      if (matchesKey(data, Key.down)) return this.scrollPeek(1);
      if (matchesKey(data, Key.pageUp)) return this.scrollPeek(-this.peekRows());
      if (matchesKey(data, Key.pageDown)) return this.scrollPeek(this.peekRows());
      if (matchesKey(data, Key.home)) return this.scrollPeek(-Number.MAX_SAFE_INTEGER);
      if (matchesKey(data, Key.end)) return this.scrollPeek(Number.MAX_SAFE_INTEGER);
      if (matchesKey(data, Key.enter)) {
        this.peekFocus = false;
        this.invalidate();
        this.tui.requestRender();
        return;
      }
      if (matchesKey(data, Key.tab)) return this.toggle();
      if (matchesKey(data, Key.backspace)) {
        if (this.search.length > 0) {
          this.search = this.search.slice(0, -1);
          this.cursor = 0;
          this.peekScroll = 0;
          this.rebuild();
          this.tui.requestRender();
        }
        return;
      }
      if (this.appendSearchInput(data)) return;
      return;
    }

    // Peek list focus: arrows move the selection, Enter views the content.
    if (this.peek) {
      if (this.matches(data, "app.tree.foldOrUp")) return this.branchJump("up");
      if (this.matches(data, "app.tree.unfoldOrDown")) return this.branchJump("down");
      if (matchesKey(data, Key.up)) return this.peekMove(-1);
      if (matchesKey(data, Key.down)) return this.peekMove(1);
      if (this.matches(data, "tui.editor.cursorLeft", Key.left)) return this.page(-1);
      if (this.matches(data, "tui.editor.cursorRight", Key.right)) return this.page(1);
      if (matchesKey(data, Key.pageUp)) return this.page(-1);
      if (matchesKey(data, Key.pageDown)) return this.page(1);
      if (matchesKey(data, Key.home)) return this.peekMoveTo(0);
      if (matchesKey(data, Key.end)) return this.peekMoveTo(this.visible.length - 1);
      if (matchesKey(data, Key.tab)) return this.toggle();
      if (matchesKey(data, Key.enter)) {
        this.peekFocus = true;
        this.peekScroll = 0;
        this.invalidate();
        this.tui.requestRender();
        return;
      }
      if (matchesKey(data, Key.backspace)) {
        if (this.search.length > 0) {
          this.search = this.search.slice(0, -1);
          this.cursor = 0;
          this.peekScroll = 0;
          this.rebuild();
          this.tui.requestRender();
        }
        return;
      }
      if (this.appendSearchInput(data)) return;
      return;
    }
    if (this.matches(data, "tui.select.up", Key.up)) return this.move(-1);
    if (this.matches(data, "tui.select.down", Key.down)) return this.move(1);
    if (this.matches(data, "tui.editor.cursorLeft", Key.left) || this.matches(data, "tui.select.pageUp", Key.pageUp)) {
      return this.page(-1);
    }
    if (this.matches(data, "tui.editor.cursorRight", Key.right) || this.matches(data, "tui.select.pageDown", Key.pageDown)) {
      return this.page(1);
    }
    if (this.matches(data, "app.tree.foldOrUp")) return this.branchJump("up");
    if (this.matches(data, "app.tree.unfoldOrDown")) return this.branchJump("down");

    if (matchesKey(data, Key.tab)) return this.toggle();
    if (matchesKey(data, Key.enter)) {
      this.done([...this.selected]);
      return;
    }
    if (matchesKey(data, Key.backspace) || this.matches(data, "tui.editor.deleteCharBackward")) {
      if (this.search.length > 0) {
        this.search = this.search.slice(0, -1);
        this.cursor = 0;
        this.rebuild();
        this.tui.requestRender();
      }
      return;
    }

    this.appendSearchInput(data);
  }

  invalidate(): void {
    this.cachedWidth = undefined;
    this.cachedLines = undefined;
  }

  render(width: number): string[] {
    if (this.cachedWidth === width && this.cachedLines) return this.cachedLines;
    const theme = this.theme;
    const border = (s: string) => theme.fg("border", s);
    const lines: string[] = [];

    lines.push("");
    lines.push(...new DynamicBorder(border).render(width));
    lines.push(truncateToWidth(`  ${theme.bold("Session Tree")}`, width));
    lines.push(...this.helpLines(width));

    if (this.labelInput) {
      const prefix = `  ${theme.fg("muted", "Label (empty to remove):")} `;
      const inputLine = this.labelInput.render(Math.max(8, width - visibleWidth(prefix)))[0] ?? "";
      lines.push(truncateToWidth(prefix + inputLine, width));
    } else {
      const query = this.search ? ` ${theme.fg("accent", this.search)}` : "";
      lines.push(truncateToWidth(`  ${theme.fg("muted", "Type to search:")}${query}`, width));
    }

    lines.push(...new DynamicBorder(border).render(width));
    lines.push("");

    const entryRows = this.entryRows();
    this.lastEntryRows = entryRows;
    const layout = this.peekLayout();
    const listLines: string[] = [];
    if (this.peek && layout.split) {
      const left = this.renderEntryRows(layout.listWidth, entryRows);
      const right = slicePreview(this.peekLines(layout.previewWidth), this.peekScroll, entryRows);
      const divider = theme.fg("border", "│");
      for (let i = 0; i < entryRows; i++) {
        listLines.push(
          padTo(left[i] ?? "", layout.listWidth) +
            divider +
            truncateToWidth(right[i] ?? "", layout.previewWidth, ""),
        );
      }
    } else if (this.peek) {
      const preview = slicePreview(this.peekLines(width), this.peekScroll, entryRows);
      for (const line of preview) listLines.push(truncateToWidth(line, width, ""));
    } else if (this.visible.length === 0) {
      listLines.push(theme.fg("muted", "  No entries"));
    } else {
      listLines.push(...this.renderEntryRows(width, entryRows));
    }
    while (listLines.length < entryRows) listLines.push("");
    lines.push(...listLines);

    const filterSuffix = this.filterMode === "default" ? "" : ` [${FILTER_LABELS[this.filterMode]}]`;
    const pos = this.visible.length ? `  (${this.cursor + 1}/${this.visible.length})${filterSuffix}` : "  (0/0)";
    if (this.peek) {
      const range = lineRangeLabel(
        this.peekLines(layout.previewWidth).length,
        this.peekScroll,
        entryRows,
      );
      const sel = this.selected.size ? ` · ${this.selected.size} selected` : "";
      const hint = this.peekFocus
        ? " · tab select · ↑↓ scroll · esc back · ctrl+w save"
        : " · tab select · enter view · esc close peek · ctrl+w save";
      lines.push(
        truncateToWidth(theme.fg("muted", `  ${range}${sel}`) + theme.fg("accent", hint), width),
      );
    } else if (this.status) {
      lines.push(truncateToWidth(theme.fg("muted", pos) + theme.fg("accent", ` · ${this.status}`), width));
    } else {
      const sel = this.selected.size ? ` · ${this.selected.size} selected` : "";
      lines.push(
        truncateToWidth(theme.fg("muted", pos + sel + " · tab select · enter save · ctrl+p peek"), width),
      );
    }

    lines.push("");
    lines.push(...new DynamicBorder(border).render(width));

    this.cachedWidth = width;
    this.cachedLines = lines;
    return lines;
  }

  private renderEntryRows(width: number, rows: number): string[] {
    const out: string[] = [];
    if (this.visible.length === 0) {
      out.push(this.theme.fg("muted", "  No entries"));
      return out;
    }
    for (let i = this.scroll; i < Math.min(this.scroll + rows, this.visible.length); i++) {
      out.push(this.renderRow(this.visible[i], i === this.cursor, width));
    }
    return out;
  }

  private renderRow(node: FlatNode, isCursor: boolean, width: number): string {
    const theme = this.theme;
    const box = this.selected.has(node.entry.id) ? "[x] " : "[ ] ";
    const pathMarker = node.isOnActivePath ? theme.fg("accent", "• ") : "";
    const label = node.label ? theme.fg("warning", `[${node.label}] `) : "";
    const labelTime =
      this.showLabelTimestamps && node.label && node.labelTimestamp
        ? theme.fg("muted", `${formatLabelTime(node.labelTimestamp)} `)
        : "";
    const cursor = isCursor ? theme.fg("accent", "› ") : "  ";

    const fixedWidth = visibleWidth(pathMarker) + box.length + visibleWidth(label) + visibleWidth(labelTime);
    const available = Math.max(8, width - 2 - fixedWidth);
    const headline = truncateToWidth(entryHeadline(node.entry), available, "…");

    let line = cursor + pathMarker + box + label + labelTime + headline;
    line = truncateToWidth(line, width);
    if (isCursor) line = theme.bg("selectedBg", line);
    return line;
  }
}

function formatLabelTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  const diff = Date.now() - date.getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d`;
  return `${Math.floor(days / 30)}mo`;
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export default function (pi: ExtensionAPI) {
  pi.registerCommand("note", {
    description:
      "Save selected session entries as a Markdown note (Session Tree-style list with filters and type-to-search)",
    handler: async (_args, ctx) => {
      if (ctx.mode !== "tui") {
        ctx.ui.notify("The session note picker needs interactive (TUI) mode.", "error");
        return;
      }

      const entries = ctx.sessionManager.getEntries();
      if (entries.length === 0) {
        ctx.ui.notify("The current session has no entries to save.", "warning");
        return;
      }

      const roots = ctx.sessionManager.getTree();
      const activeIds = new Set<string>(ctx.sessionManager.getBranch().map((e: Entry) => e.id));
      const leafId = ctx.sessionManager.getLeafId();

      const selectedIds = await ctx.ui.custom<string[] | null>((tui, theme, keybindings, done) => {
        return new SessionTreePicker(tui, theme, done, roots, activeIds, leafId, pi, keybindings) as any;
      });

      if (!selectedIds || selectedIds.length === 0) {
        ctx.ui.notify("No entries selected.", "info");
        return;
      }

      // Order selected entries the way they appear in the tree.
      const order = new Map<string, number>();
      let index = 0;
      const walk = (node: any) => {
        order.set(node.entry.id, index++);
        node.children.forEach(walk);
      };
      roots.forEach(walk);
      const chosen = [...selectedIds].sort(
        (a, b) => (order.get(a) ?? 0) - (order.get(b) ?? 0),
      );

      const entryById = new Map<string, Entry>();
      for (const entry of entries) entryById.set(entry.id, entry);

      const firstUser = chosen
        .map((id) => entryById.get(id))
        .find((e) => e?.type === "message" && e.message?.role === "user");
      const defaultTitle =
        contentToText(firstUser?.message?.content)
          .split("\n")[0]
          .slice(0, 60)
          .trim() || "note";

      const title = await ctx.ui.input("Note title", defaultTitle);
      if (title === undefined) {
        ctx.ui.notify("Note cancelled — nothing saved.", "info");
        return;
      }
      const noteTitle = title.trim() || defaultTitle;

      const header = [
        `# ${noteTitle}`,
        "",
        `_Saved from pi session \`${ctx.sessionManager.getSessionId()}\` on ${new Date().toLocaleString()}_`,
        "",
        "---",
        "",
      ].join("\n");

      const body = chosen
        .map((id) => entryById.get(id))
        .filter(Boolean)
        .map((entry) => entryToMarkdown(entry as Entry))
        .filter(Boolean)
        .join("\n---\n\n");

      const content = `${header}${body}\n`;
      try {
        const file = saveNote(ctx.cwd, noteTitle, content);
        ctx.ui.notify(`Saved ${chosen.length} entr${chosen.length === 1 ? "y" : "ies"} to ${file}`, "info");
      } catch (error: any) {
        ctx.ui.notify(`Failed to save note: ${error?.message ?? error}`, "error");
      }
    },
  });
}
