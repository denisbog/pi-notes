/**
 * report-view — render the `pi-session-inspector` usage report *inside* pi.
 *
 * Command:
 *   /report-view [session-file]
 *
 * This is the in-terminal counterpart of the `/report` extension: instead of
 * writing a self-contained HTML page and opening it in a browser, it asks
 * `pi-session-inspector --json <tmp>` for the exact data model the HTML report
 * embeds (aggregate stats + one entry per assistant request with usage,
 * thinking, assistant text, tool calls and results), then renders that model
 * with pi's TUI components.
 *
 * Layout (Session Tree-style chrome):
 *
 *   ────────────────────────────────────────────────────────────────
 *     pi Session Report  <name>                         #12/27
 *     ↑/↓ move · enter detail · n/N flagged · [/] session · h/s/f
 *     toggle · y copy · r reload · esc close
 *   ────────────────────────────────────────────────────────────────
 *     input 33.0K  cached 329.7K  output 11.6K  requests 20  …
 *     <histogram: one column per request, log scale>
 *     <cache-loss timeline + large-request list>
 *   ────────────────────────────────────────────────────────────────
 *     request list (left)        │  selected request detail (right)
 *   ────────────────────────────────────────────────────────────────
 *     status / position
 *
 * The histogram mirrors the HTML page: bar height is log-scaled fresh (non
 * cached) input tokens, colour-coded amber for large input and red where the
 * prompt cache was lost, with a ▲ marker row for cache-loss positions.
 *
 * Install: copy to ~/.pi/agent/extensions/report-view.ts (together with the
 * `notes-shared/` helper directory) and run /reload inside pi.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { DynamicBorder, copyToClipboard, getMarkdownTheme } from "@earendil-works/pi-coding-agent";
import {
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
import { lineRangeLabel, padTo, shortenPath } from "./notes-shared/shared.ts";

// ---------------------------------------------------------------------------
// Data model (mirrors pi-session-inspector/src/html.rs::session_json)
// ---------------------------------------------------------------------------

interface ToolCallItem {
  name: string;
  args: unknown;
  results: string[];
}

interface RequestItem {
  idx: number;
  time: string;
  model: string | null;
  stop: string;
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  reasoning: number;
  total: number;
  cost: number;
  invalidated: boolean;
  lost: number;
  text: string;
  thinking: string;
  toolCalls: ToolCallItem[];
}

interface SessionData {
  sessionId: string;
  file: string;
  project: string;
  started: string;
  durationSecs: number;
  name: string | null;
  input: number;
  cached: number;
  output: number;
  reasoning: number;
  total: number;
  cost: number;
  invalidations: number;
  lostTokens: number;
  lostCost: number;
  contextEnd: number;
  requests: RequestItem[];
}

interface ReportData {
  sessions: SessionData[];
}

// ---------------------------------------------------------------------------
// Formatting helpers (kept identical to the HTML report)
// ---------------------------------------------------------------------------

function fmt(n: number | null | undefined): string {
  if (n == null) return "0";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

function fmtCost(n: number | null | undefined): string {
  if (n == null) return "0";
  if (n >= 0.01) return `$${n.toFixed(4)}`;
  return `$${n.toFixed(6)}`;
}

function fmtDuration(secs: number): string {
  if (!secs || secs <= 0) return "—";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  const parts: string[] = [];
  if (h) parts.push(`${h}h`);
  if (h || m) parts.push(`${m}m`);
  parts.push(`${s}s`);
  return parts.join(" ");
}

/** `2026-08-07T16:42:11.000Z` -> `16:42:11` (local time). */
function shortTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function safeJson(v: unknown): string {
  if (typeof v === "string") return v;
  try {
    return JSON.stringify(v, null, 2) ?? String(v);
  } catch {
    return String(v);
  }
}

/** Build a markdown code fence that cannot be terminated by `text` itself. */
function fence(text: string, lang = ""): string {
  const runs = text.match(/`{3,}/g);
  const n = runs ? Math.max(...runs.map((r) => r.length)) + 1 : 3;
  const f = "`".repeat(Math.max(3, n));
  return `${f}${lang}\n${text}\n${f}`;
}

/** `~/.pi/...` shortening for paths. */
function prettyPath(p: string): string {
  try {
    return shortenPath(p);
  } catch {
    return p;
  }
}

/** Wrap a list of (possibly ANSI-styled) chips onto lines of `width`. */
function wrapItems(items: string[], width: number, indent = 2, sep = "  "): string[] {
  const out: string[] = [];
  let cur = "";
  let curW = 0;
  const limit = Math.max(8, width - indent);
  for (const it of items) {
    if (!it) continue;
    const iw = visibleWidth(it);
    if (curW > 0 && curW + sep.length + iw > limit) {
      out.push(" ".repeat(indent) + cur);
      cur = it;
      curW = iw;
    } else {
      cur = curW === 0 ? it : cur + sep + it;
      curW = curW === 0 ? iw : curW + sep.length + iw;
    }
  }
  if (curW > 0) out.push(" ".repeat(indent) + cur);
  return out.length ? out : [" ".repeat(indent)];
}

const EIGHTHS = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

interface StatusMessage {
  text: string;
  type: "info" | "error";
}

// ---------------------------------------------------------------------------
// Viewer component
// ---------------------------------------------------------------------------

const HELP_TEXT =
  "↑/↓ move · enter detail · ctrl+d/u or pgup/pgdn scroll · n/N flagged · [/] session " +
  "· h/s/f hist/stats/flags · y copy · r reload · esc close";

interface ReportViewerOptions {
  /** Re-run pi-session-inspector and return fresh data (for `r`). */
  reload: () => Promise<ReportData>;
}

class ReportViewer {
  private sessions: SessionData[];
  private sessionIdx = 0;
  private cursor = 0;
  private listScroll = 0;
  private detailScroll = 0;
  private focus: "list" | "detail" = "list";

  private showStats = true;
  private showHist = true;
  private showFlags = true;

  private status?: StatusMessage;
  private reloading = false;
  private closed = false;

  private cachedWidth?: number;
  private cachedLines?: string[];

  private readonly mdTheme = getMarkdownTheme();
  private readonly mdCache = new Map<string, { width: number; lines: string[] }>();
  private thresholds: number[] = [];
  private flagged: number[][] = [];

  private readonly tui: any;
  private readonly theme: any;
  private readonly done: (value: null) => void;
  private readonly opts: ReportViewerOptions;

  constructor(
    tui: any,
    theme: any,
    done: (value: null) => void,
    data: ReportData,
    opts: ReportViewerOptions,
  ) {
    this.tui = tui;
    this.theme = theme;
    this.done = done;
    this.opts = opts;
    this.sessions = data.sessions ?? [];
    this.prepare();
    this.cursor = this.clampCursor(0);
  }

  /** (Re)compute per-session derived data: large-input threshold + flag list. */
  private prepare(): void {
    this.thresholds = this.sessions.map((s) => largeThreshold(s.requests ?? []));
    this.flagged = this.sessions.map((s) => flaggedIndices(s.requests ?? []));
  }

  // --- geometry -------------------------------------------------------------

  private width(): number {
    return this.tui.terminal?.columns ?? (typeof this.tui.width === "number" ? this.tui.width : 80);
  }

  private rows(): number {
    return this.tui.terminal?.rows ?? (typeof this.tui.height === "number" ? this.tui.height : 24);
  }

  private session(): SessionData | undefined {
    return this.sessions[this.sessionIdx];
  }

  private reqs(): RequestItem[] {
    return this.session()?.requests ?? [];
  }

  private threshold(): number {
    return this.thresholds[this.sessionIdx] ?? 500;
  }

  private flagList(): number[] {
    return this.flagged[this.sessionIdx] ?? [];
  }

  private clampCursor(i: number): number {
    const n = this.reqs().length;
    if (n === 0) return 0;
    return Math.max(0, Math.min(n - 1, i));
  }

  // --- status ---------------------------------------------------------------

  private setStatus(text: string, type: "info" | "error" = "info"): void {
    this.status = { text, type };
    this.refresh();
  }

  private refresh(): void {
    if (this.closed) return;
    this.invalidate();
    this.tui.requestRender();
  }

  // --- markdown -------------------------------------------------------------

  private md(key: string, text: string, width: number): string[] {
    const hit = this.mdCache.get(key);
    if (hit && hit.width === width) return hit.lines;
    let lines: string[];
    try {
      lines = new Markdown(text, 0, 0, this.mdTheme).render(width);
    } catch {
      lines = text.split("\n");
    }
    this.mdCache.set(key, { width, lines });
    return lines;
  }

  // --- sections -------------------------------------------------------------

  private helpLines(): string[] {
    return wrapTextWithAnsi(`  ${HELP_TEXT}`, Math.max(10, this.width())).map((l) =>
      this.theme.fg("muted", l),
    );
  }

  private statsLines(width: number): string[] {
    const theme = this.theme;
    const sess = this.session();
    if (!sess) return [];
    const reqs = this.reqs();
    const large = reqs.filter((r) => r.input >= this.threshold()).length;
    const chip = (label: string, value: string, color?: string) =>
      theme.fg("muted", `${label} `) + theme.fg((color as any) ?? "text", value);
    const items = [
      chip("input", fmt(sess.input)),
      chip("cached", fmt(sess.cached)),
      chip("output", fmt(sess.output)),
      sess.reasoning ? chip("reasoning", fmt(sess.reasoning)) : "",
      chip("requests", `${reqs.length}`),
      chip("large input", `${large}`, large ? "warning" : undefined),
      chip("cache lost", `${sess.invalidations}`, sess.invalidations ? "error" : undefined),
      chip("tokens lost", fmt(sess.lostTokens), sess.lostTokens ? "warning" : undefined),
      chip("lost cost", fmtCost(sess.lostCost), sess.lostCost ? "error" : undefined),
      chip("context end", fmt(sess.contextEnd)),
      chip("cost", fmtCost(sess.cost)),
      chip("duration", fmtDuration(sess.durationSecs)),
    ].filter(Boolean);
    return wrapItems(items, width);
  }

  /** Bucket requests into at most `maxCols` columns for the histogram. */
  private buckets(maxCols: number): Array<{ max: number; loss: boolean; large: boolean; first: number; last: number }> {
    const reqs = this.reqs();
    const th = this.threshold();
    const stride = Math.max(1, Math.ceil(reqs.length / Math.max(1, maxCols)));
    const out: Array<{ max: number; loss: boolean; large: boolean; first: number; last: number }> = [];
    for (let i = 0; i < reqs.length; i += stride) {
      const slice = reqs.slice(i, i + stride);
      out.push({
        max: Math.max(...slice.map((r) => r.input)),
        loss: slice.some((r) => r.invalidated),
        large: slice.some((r) => r.input >= th),
        first: slice[0].idx,
        last: slice[slice.length - 1].idx,
      });
    }
    return out;
  }

  private histLines(width: number): string[] {
    const theme = this.theme;
    const reqs = this.reqs();
    if (!reqs.length) return [theme.fg("muted", "  (no requests)")];

    const labelW = 6;
    const avail = Math.max(8, width - labelW - 1);
    const buckets = this.buckets(avail);
    const maxVal = Math.max(1, ...buckets.map((b) => b.max));
    const H = this.rows() < 34 ? 3 : 5;
    const logMax = Math.log(1 + maxVal);

    const chart: string[] = [];
    for (let row = H - 1; row >= 0; row--) {
      let line = row === H - 1 ? fmt(maxVal).padStart(5) + " " : "      ";
      line += theme.fg("border", "│");
      for (const b of buckets) {
        const h = (Math.log(1 + b.max) / logMax) * H * 8;
        const frac = Math.max(0, Math.min(8, h - row * 8));
        const ch = EIGHTHS[Math.round(frac)];
        const color = b.loss ? "error" : b.large ? "warning" : "accent";
        line += theme.fg(color as any, ch);
      }
      chart.push(truncateToWidth(line, width, ""));
    }

    // Cache-loss marker row + axis.
    let markers = " ".repeat(labelW) + theme.fg("border", "│");
    for (const b of buckets) markers += b.loss ? theme.fg("error", "▲") : " ";
    let axis = " ".repeat(labelW) + theme.fg("border", "│");
    axis += theme.fg("dim", `#${buckets[0].first}`);
    const last = `#${buckets[buckets.length - 1].last}`;
    const gap = avail - visibleWidth(`#${buckets[0].first}`) - visibleWidth(last);
    if (gap > 0) axis += " ".repeat(gap) + theme.fg("dim", last);

    const legend =
      "  " +
      theme.fg("accent", "■ normal") +
      "  " +
      theme.fg("warning", "■ large input") +
      "  " +
      theme.fg("error", "■ cache lost (▲ marker)");

    return [
      ...chart,
      truncateToWidth(markers, width, ""),
      truncateToWidth(axis, width, ""),
      truncateToWidth(legend, width, ""),
    ];
  }

  /** Cache-loss events with the gap / drop% / cost the HTML timeline shows. */
  private lossEvents(): Array<{ i: number; r: RequestItem; gap: number | null; prevCr: number; dropPct: number; cost: number }> {
    const sess = this.session();
    const reqs = this.reqs();
    const rate = sess && sess.lostTokens ? sess.lostCost / sess.lostTokens : 0;
    const out: Array<{ i: number; r: RequestItem; gap: number | null; prevCr: number; dropPct: number; cost: number }> = [];
    let prevCr: number | null = null;
    let prevT: number | null = null;
    for (let i = 0; i < reqs.length; i++) {
      const r = reqs[i];
      const t = Date.parse(r.time);
      const gap = prevT != null && !isNaN(t) ? (t - prevT) / 1000 : null;
      let dropPct: number | null = null;
      if (prevCr != null && prevCr > 0) dropPct = ((prevCr - r.cacheRead) / prevCr) * 100;
      if (r.invalidated && dropPct != null && dropPct > 0.01) {
        out.push({ i, r, gap, prevCr: prevCr as number, dropPct, cost: r.lost * rate });
      }
      prevCr = r.cacheRead;
      prevT = isNaN(t) ? null : t;
    }
    return out;
  }

  private flagsLines(width: number): string[] {
    const theme = this.theme;
    const reqs = this.reqs();
    const sess = this.session();
    if (!sess || !reqs.length) return [];
    const out: string[] = [];

    const losses = this.lossEvents();
    if (losses.length) {
      out.push(
        truncateToWidth(
          theme.fg("error", `  Cache-loss timeline (${losses.length} · ${fmtCost(sess.lostCost)})`),
          width,
        ),
      );
      for (const l of losses.slice(0, 5)) {
        const gap = l.gap != null ? `${Math.round(l.gap)}s` : "-";
        const body =
          `${shortTime(l.r.time)} · gap ${gap} · cache ${fmt(l.prevCr)} → ${fmt(l.r.cacheRead)}` +
          ` (${l.dropPct.toFixed(0)}%) · re-sent ${fmt(l.r.input)} · ${fmtCost(l.cost)}`;
        out.push(truncateToWidth(`  ${theme.fg("error", `#${l.r.idx}`)} ${theme.fg("muted", body)}`, width));
      }
      if (losses.length > 5) out.push(theme.fg("muted", `  … ${losses.length - 5} more cache loss(es)`));
    }

    const top = reqs
      .map((r, i) => ({ r, i }))
      .sort((a, b) => b.r.input - a.r.input)
      .slice(0, 5);
    if (top.length) {
      out.push(truncateToWidth(theme.fg("warning", "  Large requests"), width));
      for (const { r } of top) {
        out.push(
          truncateToWidth(
            `  ${theme.fg("warning", `#${r.idx}`)} ${theme.fg("muted", `${shortTime(r.time)} · input ${fmt(r.input)}`)}`,
            width,
          ),
        );
      }
    }
    return out;
  }

  // --- body panes -----------------------------------------------------------

  private renderList(width: number, height: number): string[] {
    const theme = this.theme;
    const reqs = this.reqs();
    if (!reqs.length) return [theme.fg("muted", "  (no requests)")];
    const th = this.threshold();
    const perEntry = 2;
    const visible = Math.max(1, Math.floor(height / perEntry));

    if (this.cursor < this.listScroll) this.listScroll = this.cursor;
    if (this.cursor >= this.listScroll + visible) this.listScroll = this.cursor - visible + 1;
    this.listScroll = Math.max(0, Math.min(this.listScroll, Math.max(0, reqs.length - visible)));

    const out: string[] = [];
    for (let i = this.listScroll; i < Math.min(this.listScroll + visible, reqs.length); i++) {
      const r = reqs[i];
      const selected = i === this.cursor;
      const mark = r.invalidated
        ? theme.fg("error", "⚠")
        : r.input >= th
          ? theme.fg("warning", "▲")
          : " ";
      const model = (r.model ?? "").split("/").pop() ?? "";
      const headAvail = Math.max(4, width - visibleWidth(`›  #123  00:00:00  `));
      let line1 =
        (selected ? theme.fg("accent", "› ") : "  ") +
        mark +
        " " +
        theme.fg("muted", `#${String(r.idx).padStart(3)}`) +
        " " +
        theme.fg(selected ? "text" : "dim", shortTime(r.time)) +
        " " +
        theme.fg("dim", truncateToWidth(model, headAvail, "…"));
      let line2 =
        "    " +
        theme.fg("muted", `in ${fmt(r.input)} · c ${fmt(r.cacheRead)} · out ${fmt(r.output)} · ${fmtCost(r.cost)}`);
      line1 = truncateToWidth(line1, width, "");
      line2 = truncateToWidth(line2, width, "");
      if (selected) {
        line1 = theme.bg("selectedBg", padTo(line1, width));
        line2 = theme.bg("selectedBg", padTo(line2, width));
      }
      out.push(line1, line2);
    }
    while (out.length < height) out.push("");
    return out.slice(0, height);
  }

  private detailLines(width: number): string[] {
    const theme = this.theme;
    const req = this.reqs()[this.cursor];
    if (!req) return [theme.fg("muted", "No request selected.")];

    const th = this.threshold();
    const out: string[] = [];
    const badge = req.invalidated
      ? theme.fg("error", `   CACHE LOST ${fmt(req.lost)}`)
      : req.input >= th
        ? theme.fg("warning", "   LARGE INPUT")
        : "";
    out.push(truncateToWidth(theme.bold(theme.fg("accent", `Request #${req.idx}`)) + badge, width, ""));

    const meta = [req.time, req.model ?? "", req.stop ? `stop: ${req.stop}` : ""].filter(Boolean).join("  ·  ");
    out.push(truncateToWidth(theme.fg("muted", `  ${meta}`), width, ""));

    const chip = (label: string, value: string, color?: string) =>
      theme.fg("muted", `${label} `) + theme.fg((color as any) ?? "text", value);
    out.push(
      ...wrapItems(
        [
          chip("input", fmt(req.input), req.input >= th ? "warning" : undefined),
          chip("cached", fmt(req.cacheRead)),
          req.cacheWrite ? chip("cacheW", fmt(req.cacheWrite)) : "",
          chip("output", fmt(req.output)),
          req.reasoning ? chip("reasoning", fmt(req.reasoning)) : "",
          chip("total", fmt(req.total)),
          chip("cost", fmtCost(req.cost)),
        ].filter(Boolean),
        width,
      ),
    );

    const outChars =
      (req.thinking?.length ?? 0) +
      (req.text?.length ?? 0) +
      (req.toolCalls ?? []).reduce((sum, t) => sum + safeJson(t.args).length, 0);
    out.push(
      truncateToWidth(
        theme.fg("accent", "  next call ") +
          theme.fg(
            "muted",
            `output ${fmt(outChars)} chars / ~${fmt(Math.round(outChars / 4))} tok is re-sent as input on the next request`,
          ),
        width,
        "",
      ),
    );
    out.push("");

    const header = (label: string, chars: number) =>
      truncateToWidth(
        theme.fg("accent", `▌ ${label}`) +
          theme.fg("dim", `  ${fmt(chars)} chars · ~${fmt(Math.round(chars / 4))} tok`),
        width,
        "",
      );

    if (req.thinking) {
      out.push(header("thinking", req.thinking.length));
      for (const l of this.md(`th:${this.sessionIdx}:${req.idx}`, req.thinking, Math.max(4, width - 2))) {
        out.push(truncateToWidth(theme.fg("thinkingText", "│ ") + l, width, ""));
      }
      out.push("");
    }

    if (req.text) {
      out.push(header("assistant text", req.text.length));
      out.push(...this.md(`tx:${this.sessionIdx}:${req.idx}`, req.text, width).map((l) => truncateToWidth(l, width, "")));
      out.push("");
    }

    (req.toolCalls ?? []).forEach((tc, ti) => {
      const args = safeJson(tc.args);
      out.push(header(`tool call · ${tc.name}`, args.length));
      out.push(
        ...this.md(`ta:${this.sessionIdx}:${req.idx}:${ti}`, fence(args, "json"), width).map((l) =>
          truncateToWidth(l, width, ""),
        ),
      );
      const results = (tc.results ?? []).filter((x) => x && x.length);
      if (results.length) {
        const joined = results.join("\n");
        out.push(header("tool result", joined.length));
        out.push(
          ...this.md(`tr:${this.sessionIdx}:${req.idx}:${ti}`, fence(joined), width).map((l) =>
            truncateToWidth(l, width, ""),
          ),
        );
      }
      out.push("");
    });

    if (!req.thinking && !req.text && !(req.toolCalls ?? []).length) {
      out.push(theme.fg("muted", "  (no content)"));
    }
    return out;
  }

  private renderDetail(width: number, height: number): string[] {
    const all = this.detailLines(width);
    this.lastDetailTotal = all.length;
    const max = Math.max(0, all.length - height);
    this.detailScroll = Math.max(0, Math.min(this.detailScroll, max));
    const out = all
      .slice(this.detailScroll, this.detailScroll + height)
      .map((l) => truncateToWidth(l, width, ""));
    while (out.length < height) out.push("");
    return out;
  }

  // --- actions --------------------------------------------------------------

  private move(delta: number): void {
    if (this.focus === "detail") return this.scrollDetail(delta);
    const next = this.clampCursor(this.cursor + delta);
    if (next !== this.cursor) {
      this.cursor = next;
      this.detailScroll = 0;
      this.refresh();
    }
  }

  /** Scroll the selected request's detail pane, keeping it in range. */
  private scrollDetail(delta: number): void {
    this.detailScroll += delta;
    this.refresh();
  }

  private page(delta: number): void {
    const pageSize = Math.max(1, this.bodyHeight() - 1);
    if (this.focus === "detail") return this.scrollDetail(delta * pageSize);
    this.move(delta * Math.max(1, Math.floor(this.bodyHeight() / 2)));
  }

  private jumpFlagged(dir: 1 | -1): void {
    if (this.focus !== "list") return;
    const list = this.flagList();
    if (!list.length) return this.setStatus("No flagged requests (large input / cache loss).");
    const at = dir > 0 ? list.find((i) => i > this.cursor) : [...list].reverse().find((i) => i < this.cursor);
    this.cursor = at ?? (dir > 0 ? list[0] : list[list.length - 1]);
    this.detailScroll = 0;
    this.refresh();
  }

  private switchSession(delta: number): void {
    if (this.sessions.length < 2) return;
    this.sessionIdx = (this.sessionIdx + delta + this.sessions.length) % this.sessions.length;
    this.cursor = this.clampCursor(0);
    this.listScroll = 0;
    this.detailScroll = 0;
    this.mdCache.clear();
    this.refresh();
  }

  private copyDetail(): void {
    const req = this.reqs()[this.cursor];
    if (!req) return;
    const sess = this.session();
    const out: string[] = [];
    out.push(`Request #${req.idx}${req.invalidated ? `  CACHE LOST ${fmt(req.lost)}` : ""}`);
    out.push([req.time, req.model ?? "", req.stop ? `stop: ${req.stop}` : ""].filter(Boolean).join("  ·  "));
    out.push(
      `input ${fmt(req.input)} · cached ${fmt(req.cacheRead)} · output ${fmt(req.output)} · ` +
        `reasoning ${fmt(req.reasoning)} · total ${fmt(req.total)} · ${fmtCost(req.cost)}`,
    );
    if (sess) out.push(`session: ${sess.name ?? sess.sessionId}  (${prettyPath(sess.file)})`);
    if (req.thinking) out.push("", "## thinking", "", req.thinking);
    if (req.text) out.push("", "## assistant text", "", req.text);
    (req.toolCalls ?? []).forEach((tc) => {
      out.push("", `## tool call: ${tc.name}`, "", fence(safeJson(tc.args), "json"));
      const results = (tc.results ?? []).filter((x) => x && x.length);
      if (results.length) out.push("", "### tool result", "", fence(results.join("\n")));
    });
    copyToClipboard(out.join("\n"))
      .then(() => this.setStatus(`Copied request #${req.idx} to clipboard`))
      .catch((e: Error) => this.setStatus(`Copy failed: ${e.message}`, "error"));
  }

  private async doReload(): Promise<void> {
    if (this.reloading) return;
    this.reloading = true;
    this.setStatus("Reloading report…");
    try {
      const data = await this.opts.reload();
      this.sessions = data.sessions ?? [];
      this.sessionIdx = Math.min(this.sessionIdx, Math.max(0, this.sessions.length - 1));
      this.prepare();
      this.cursor = this.clampCursor(0);
      this.listScroll = 0;
      this.detailScroll = 0;
      this.mdCache.clear();
      this.setStatus(`Reloaded · ${new Date().toLocaleTimeString()}`);
    } catch (e) {
      this.setStatus(`Reload failed: ${(e as Error).message}`, "error");
    } finally {
      this.reloading = false;
    }
  }

  // --- input ----------------------------------------------------------------

  handleInput(data: string): void {
    // Toggles / global actions work in either pane.
    if (data === "h") {
      this.showHist = !this.showHist;
      return this.refresh();
    }
    if (data === "s") {
      this.showStats = !this.showStats;
      return this.refresh();
    }
    if (data === "f") {
      this.showFlags = !this.showFlags;
      return this.refresh();
    }
    if (data === "y") return this.copyDetail();
    if (data === "r") return void this.doReload();
    if (data === "[") return this.switchSession(-1);
    if (data === "]") return this.switchSession(1);

    if (matchesKey(data, Key.escape) || data === "q") {
      if (this.focus === "detail") {
        this.focus = "list";
        return this.refresh();
      }
      this.closed = true;
      return this.done(null);
    }

    if (matchesKey(data, Key.enter) || matchesKey(data, Key.tab)) {
      this.focus = this.focus === "list" ? "detail" : "list";
      return this.refresh();
    }

    if (matchesKey(data, Key.up)) return this.move(-1);
    if (matchesKey(data, Key.down)) return this.move(1);
    if (matchesKey(data, Key.pageUp)) return this.page(-1);
    if (matchesKey(data, Key.pageDown)) return this.page(1);

    // Half-page scrolling of the detail pane, available whether or not the
    // detail pane has focus.
    const halfPage = Math.max(1, Math.floor(this.bodyHeight() / 2));
    if (matchesKey(data, Key.ctrl("d"))) return this.scrollDetail(halfPage);
    if (matchesKey(data, Key.ctrl("u"))) return this.scrollDetail(-halfPage);
    if (matchesKey(data, Key.home)) {
      if (this.focus === "detail") {
        this.detailScroll = 0;
        return this.refresh();
      }
      this.cursor = this.clampCursor(0);
      this.detailScroll = 0;
      return this.refresh();
    }
    if (matchesKey(data, Key.end)) {
      if (this.focus === "detail") {
        this.detailScroll = Number.MAX_SAFE_INTEGER;
        return this.refresh();
      }
      this.cursor = this.clampCursor(this.reqs().length - 1);
      this.detailScroll = 0;
      return this.refresh();
    }

    if (this.focus === "list") {
      if (data === "n") return this.jumpFlagged(1);
      if (data === "N") return this.jumpFlagged(-1);
    }
  }

  invalidate(): void {
    // Note: the markdown cache is intentionally kept here (mirroring
    // notes-viewer). It is cleared explicitly on reload, where content can
    // actually change.
    this.cachedWidth = undefined;
    this.cachedLines = undefined;
  }

  // --- render ---------------------------------------------------------------

  private bodyHeight(): number {
    return this.lastBodyHeight ?? 6;
  }
  private lastBodyHeight = 6;
  private lastDetailTotal = 0;

  render(width: number): string[] {
    if (this.cachedWidth === width && this.cachedLines) return this.cachedLines;
    const theme = this.theme;
    const rows = this.rows();
    const border = (s: string) => theme.fg("border", s);

    const help = this.helpLines();
    // Lines that are always present before the body: blank + top border +
    // title + help + border = 4 + help. Plus 2 (divider + bottom border) and
    // 1 footer. Keep >= 4 body rows.
    const minBody = 4;
    const sectionBudget = Math.max(0, rows - (4 + help.length) - 2 - 1 - minBody);

    let showStats = this.showStats;
    let showHist = this.showHist;
    let showFlags = this.showFlags;
    let stats = showStats ? this.statsLines(width) : [];
    let hist = showHist ? this.histLines(width) : [];
    let flags = showFlags ? this.flagsLines(width) : [];
    const sectionCount = () => stats.length + hist.length + flags.length;
    if (sectionCount() > sectionBudget && flags.length) {
      showFlags = false;
      flags = [];
    }
    if (sectionCount() > sectionBudget && hist.length) {
      showHist = false;
      hist = [];
    }
    if (sectionCount() > sectionBudget && stats.length) {
      stats = stats.slice(0, sectionBudget);
    }

    const lines: string[] = [];
    lines.push("");
    lines.push(...new DynamicBorder(border).render(width));

    const sess = this.session();
    const total = this.reqs().length;
    const left =
      theme.bold("pi Session Report") +
      (sess ? theme.fg("muted", `  ${sess.name ?? sess.sessionId}`) : "");
    const right =
      (this.sessions.length > 1 ? theme.fg("muted", `session ${this.sessionIdx + 1}/${this.sessions.length}  `) : "") +
      theme.fg("accent", `#${Math.min(this.cursor + 1, total)}/${total}`);
    const gap = Math.max(1, width - visibleWidth(left) - visibleWidth(right));
    lines.push(truncateToWidth(left + " ".repeat(gap) + right, width, ""));
    lines.push(...help);
    lines.push(...new DynamicBorder(border).render(width));
    if (sess) lines.push(theme.fg("dim", truncateToWidth(`  ${prettyPath(sess.file)}`, width, "…")));
    lines.push(...stats);
    lines.push(...hist);
    lines.push(...flags);
    lines.push(...new DynamicBorder(border).render(width));

    const bodyHeight = Math.max(minBody, rows - lines.length - 2);
    this.lastBodyHeight = bodyHeight;

    const listWidth = Math.max(24, Math.min(52, Math.round(width * 0.42)));
    const detailWidth = Math.max(1, width - listWidth - 1);
    const list = this.renderList(listWidth, bodyHeight);
    const detail = this.renderDetail(detailWidth, bodyHeight);
    const divider = theme.fg("border", "│");
    for (let i = 0; i < bodyHeight; i++) {
      lines.push(padTo(list[i] ?? "", listWidth) + divider + truncateToWidth(detail[i] ?? "", detailWidth, ""));
    }

    lines.push(...new DynamicBorder(border).render(width));

    const large = this.reqs().filter((r) => r.input >= this.threshold()).length;
    const footer = this.status
      ? theme.fg(this.status.type === "error" ? "error" : "accent", `  ${this.status.text}`)
      : this.focus === "detail"
        ? theme.fg("muted", `  ${lineRangeLabel(this.lastDetailTotal, this.detailScroll, this.bodyHeight())} · `) +
          theme.fg("accent", "↑↓/pgup/pgdn scroll · enter/esc back")
        : theme.fg(
            "muted",
            `  ${total} request${total === 1 ? "" : "s"} · ${large} large · ${sess?.invalidations ?? 0} cache loss · ` +
              `${this.flagList().length} flagged`,
          ) +
          (this.lastDetailTotal > this.bodyHeight()
            ? theme.fg("muted", " · ctrl+d/u scroll detail")
            : "");
    lines.push(truncateToWidth(footer, width, ""));

    const finalLines = lines.slice(0, rows);
    this.cachedWidth = width;
    this.cachedLines = finalLines;
    return finalLines;
  }
}

// ---------------------------------------------------------------------------
// Derived-data helpers
// ---------------------------------------------------------------------------

function largeThreshold(reqs: RequestItem[]): number {
  if (!reqs.length) return 500;
  const sorted = reqs.map((r) => r.input).sort((a, b) => a - b);
  const p80 = sorted[Math.floor(sorted.length * 0.8)] ?? 0;
  return Math.max(500, p80);
}

function flaggedIndices(reqs: RequestItem[]): number[] {
  const set = new Set<number>();
  reqs.forEach((r, i) => {
    if (r.invalidated) set.add(i);
  });
  reqs
    .map((r, i) => ({ r, i }))
    .sort((a, b) => b.r.input - a.r.input)
    .slice(0, 5)
    .forEach(({ i }) => set.add(i));
  return [...set].sort((a, b) => a - b);
}

/** Resolve the session file: explicit argument first, else the current one. */
function resolveSessionFile(args: string, ctx: any): string | undefined {
  if (args && args.trim()) {
    const candidate = args.trim().split(/\s+/)[0];
    if (candidate) return candidate;
  }
  return ctx.sessionManager.getSessionFile() ?? undefined;
}

/** Escape a literal path so it can be passed to the `-S` regex selector. */
function regexEscape(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Run pi-session-inspector and parse its JSON report data model. */
async function loadReport(pi: ExtensionAPI, sessionFile: string): Promise<ReportData> {
  const inspector = process.env.PI_SESSION_INSPECTOR_BIN || "pi-session-inspector";
  const tmp = path.join(os.tmpdir(), `pi-report-${process.pid}-${Date.now()}.json`);
  try {
    const result = await pi.exec(inspector, ["--json", tmp, "-S", regexEscape(sessionFile)], {
      timeout: 120_000,
    });
    if (result.code !== 0) {
      const detail = (result.stderr || result.stdout || "").trim() || `exit code ${result.code}`;
      throw new Error(detail);
    }
    return JSON.parse(fs.readFileSync(tmp, "utf8")) as ReportData;
  } finally {
    fs.rmSync(tmp, { force: true });
  }
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export default function (pi: ExtensionAPI) {
  pi.registerCommand("report-view", {
    description:
      "Render the pi session token/cache usage report inside pi (stats, input histogram, cache-loss timeline, request timeline). Usage: /report-view [session-file]",
    handler: async (args, ctx) => {
      if (ctx.mode !== "tui") {
        ctx.ui.notify("The report viewer needs interactive (TUI) mode.", "error");
        return;
      }
      const sessionFile = resolveSessionFile(args, ctx);
      if (!sessionFile) {
        ctx.ui.notify("No session file available to report on.", "error");
        return;
      }

      const load = () => loadReport(pi, sessionFile);
      let data: ReportData;
      try {
        data = await load();
      } catch (e) {
        ctx.ui.notify(`Failed to generate report: ${(e as Error).message}`, "error");
        return;
      }
      if (!data.sessions?.length) {
        ctx.ui.notify("No sessions matched the report filters.", "error");
        return;
      }

      await ctx.ui.custom<null>((tui, theme, _kb, done) => {
        return new ReportViewer(tui, theme, done, data, { reload: load }) as any;
      });
    },
  });
}
