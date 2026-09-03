// pi-notes extension: pi commands around the current pi session.
//
// Commands:
//   /notes  [session-file]  Open the pi-notes GUI (Rust / Iced) at the message
//                           pi is currently displaying (the /tree selection).
//   /report [session-file]  Generate an HTML usage report for the session with
//                           `pi-session-inspector --html`, stored next to the
//                           notes files and overwritten on every run (reports
//                           are per session).
//
// Install:  copy this file to ~/.pi/agent/extensions/notes.ts
//           and make sure the `pi-notes` and `pi-session-inspector` binaries
//           are on PATH (both installed via `cargo install`).
//
// Usage:    /notes  [path-to-session.jsonl]
//           /report [path-to-session.jsonl]

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export default function (pi) {
  pi.registerCommand("notes", {
    description:
      "Open pi-notes: view the current pi message + thinking as Markdown and browse/store notes. Usage: /notes [session-file]",
    handler: async (args, ctx) => {
      // Path to the compiled binary; override with PI_NOTES_BIN if needed.
      const bin = process.env.PI_NOTES_BIN || "pi-notes";

      const childArgs = [];

      // 1. Explicit session file wins: /notes /path/to/session.jsonl
      const sessionFile = resolveSessionFile(args, ctx);
      if (sessionFile) {
        childArgs.push("--session", sessionFile);
      }

      // Pass the id of the message pi is currently displaying (the /tree
      // selection, or the current leaf). It is not always the latest entry,
      // so pi-notes must follow the active branch path.
      const leafId = ctx.sessionManager.getLeafId();
      if (leafId) {
        childArgs.push("--leaf", leafId);
      }

      const child = spawn(bin, childArgs, {
        detached: true,
        stdio: "ignore",
        env: process.env,
      });

      child.on("error", (err) => {
        ctx.ui.notify(
          `Failed to launch pi-notes (${bin}): ${err.message}. ` +
            "Build it with `cargo build --release` and install to PATH.",
          "error"
        );
      });

      // Don't keep pi waiting on the GUI process.
      child.unref();
    },
  });

  pi.registerCommand("report", {
    description:
      "Generate an HTML usage report for the current pi session with pi-session-inspector, stored next to the notes files (overwrites per session). Usage: /report [session-file]",
    handler: async (args, ctx) => {
      // Installed via `cargo install` (package pi-session-inspector);
      // override the binary path with PI_SESSION_INSPECTOR_BIN if needed.
      const inspector = process.env.PI_SESSION_INSPECTOR_BIN || "pi-session-inspector";

      // 1. Session to report on: explicit arg, else the current pi session.
      const sessionFile = resolveSessionFile(args, ctx);
      if (!sessionFile) {
        ctx.ui.notify("No session file available to report on.", "error");
        return;
      }

      // 2. Store next to the notes files:
      //      <agent>/sessions/<project-dir>/<file>.jsonl
      //      <agent>/notes/<project-dir>/usage-report.html
      //    The project dir (pi's `--<cwd>--` encoding) is the session file's
      //    parent folder name, so the report lands beside the session's notes.
      const sessionDir = path.dirname(sessionFile); //  .../agent/sessions/<project-dir>
      const sessionsDir = path.dirname(sessionDir); // .../agent/sessions
      const agentDir = path.dirname(sessionsDir); //   .../agent
      const projectDir = path.basename(sessionDir); // --home-denis-llm--
      const notesDir = path.join(agentDir, "notes", projectDir);
      fs.mkdirSync(notesDir, { recursive: true });

      // Reports are per session: use a fixed name so each run overwrites the
      // previous report instead of accumulating files.
      const reportPath = path.join(notesDir, "usage-report.html");

      // 3. Generate the self-contained HTML report for exactly this session.
      ctx.ui.notify(`Generating HTML usage report for ${sessionFile}...`, "info");
      const result = await pi.exec(inspector, ["--html", reportPath, "-S", sessionFile], {
        timeout: 120_000,
      });

      if (result.code === 0) {
        ctx.ui.notify(`HTML usage report written to ${reportPath}`, "success");
        openInBrowser(reportPath, ctx);
      } else {
        const detail =
          (result.stderr || result.stdout || "").trim() || `exit code ${result.code}`;
        ctx.ui.notify(`Failed to generate report (${inspector}): ${detail}`, "error");
      }
    },
  });
}

/** Resolve the session file: explicit argument first, else the current pi
 *  session file. */
function resolveSessionFile(args, ctx) {
  if (args && args.trim()) {
    const candidate = args.trim().split(/\s+/)[0];
    if (candidate) return candidate;
  }
  return ctx.sessionManager.getSessionFile();
}

/** Open a file in the system browser without blocking pi. */
function openInBrowser(filePath, ctx) {
  const opener =
    process.platform === "darwin"
      ? "open"
      : process.platform === "win32"
        ? "cmd"
        : "xdg-open";
  const args = process.platform === "win32" ? ["/c", "start", "", filePath] : [filePath];

  const child = spawn(opener, args, { detached: true, stdio: "ignore" });
  child.on("error", (err) => {
    ctx.ui.notify(`Could not open report in browser (${opener}): ${err.message}`, "error");
  });
  child.unref();
}
