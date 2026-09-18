#!/usr/bin/env node
// Renders markdown with pi's own TUI markdown renderer (`@earendil-works/pi-tui`)
// using an identity theme, so the output is the pure *structure* pi produces:
// line breaks, list markers, borders, spacing. Used as the parity oracle for
// `pi-mdview`'s Rust renderer.
//
// Usage: node tools/pi_render.mjs <width> <file.md>
// Prints a JSON array of lines.

import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { join } from "node:path";

// Make pi emit OSC 8 hyperlinks (hidden in a non-TTY), so links render as the
// link text only, exactly like a real terminal with hyperlink support.
process.env.PI_HYPERLINKS = "1";

const width = Number(process.argv[2] ?? 80);
const file = process.argv[3];
const markdown = file ? readFileSync(file, "utf8") : readFileSync(0, "utf8");

// Locate pi-tui next to the installed pi coding agent.
const candidates = [process.env.PI_TUI_PATH].filter(Boolean);

let tui = null;
for (const candidate of candidates) {
  try {
    tui = await import(pathToFileURL(join(candidate, "dist", "index.js")));
    break;
  } catch {
    // try next
  }
}
if (!tui) {
  console.error(
    "pi-tui not found. Set PI_TUI_PATH to the pi-tui package directory.",
  );
  process.exit(2);
}

const identity = (s) => s;
const theme = {
  heading: identity,
  link: identity,
  linkUrl: identity,
  code: identity,
  codeBlock: identity,
  codeBlockBorder: identity,
  quote: identity,
  quoteBorder: identity,
  hr: identity,
  listBullet: identity,
  bold: identity,
  italic: identity,
  underline: identity,
  strikethrough: identity,
  highlightCode: (code) => code.split("\n"),
};

const md = new tui.Markdown(markdown.replace(/\t/g, "   "), 0, 0, theme, undefined, {});
const lines = md
  .render(width)
  // Strip OSC 8 hyperlinks and any SGR codes contributed by the renderer.
  .map((line) =>
    line
      .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "")
      .replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, ""),
  );
process.stdout.write(JSON.stringify(lines, null, 1));
