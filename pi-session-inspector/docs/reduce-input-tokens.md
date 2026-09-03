# Reducing LLM input-token usage — agent instructions

> **Goal:** keep model **input tokens** (the non-cached prompt tokens billed per
> request) low. The dominant lever is *not* the conversation, it's **how much
> output the model produces per turn**, because freshly-generated output is
> re-sent as uncached input on the very next request.
>
> Measured in the `rm-otel-dash` sessions: every time a turn emitted a large
> output, the following request's input spiked by roughly that same amount
> (e.g. a 6.5K-output turn → ~6.7K input on the next request; a 1.3K-output
> turn → ~1.5K input on the next request).

---

## Ready-to-paste prompt rules

Paste this block into the task, the agent's instructions, or `AGENTS.md`:

```markdown
## Token-efficiency rules (keep input & context small)

- **Never rewrite a whole file.** Make small, targeted edits:
  - one `sed -i` replacement for a single change, or
  - a short in-place `python` script that changes only the lines you need,
  - or a structured edit tool call that inserts/removes just the changed region.
  Do **not** re-emit a whole file through a `cat > ... <<'EOF'` heredoc to change
  a few lines.
- **Don't re-paste unchanged content.** Anchor edits to line numbers or unique
  strings; say "replace X with Y at line N" instead of reprinting the file.
- **Read narrowly.** Use `grep -n`, `sed -n 'A,Bp'`, `head`/`tail` to inspect
  only the ranges you need. Avoid dumping whole large files into context.
- **Split big changes.** If a section genuinely must be rewritten, do it in a few
  small steps (one function / one hunk at a time), not one giant write.
- **Keep thinking/tool output short.** Long reasoning and oversized commands are
  re-sent as input on the next turn. Prefer concise reasoning and the smallest
  correct command.
- **Remember the budget:** every output token is paid for twice — once as
  output, once as the next request's input. The smallest correct diff is the
  cheapest overall.
```

---

## Why these help (the mechanism)

Per request, the provider bills:

- **output** — tokens you generate this turn, and
- **input** — the prompt, minus what matched the prompt cache.

On any request, the portion of the *previous* turn you just generated (thinking,
text, tool calls) is fresh and **not yet in the cache**, so it is re-sent as
**input** on the next turn. Therefore:

```
input[next]  ≈  input[cached base] + (output[prev] not yet cached) + small delta
```

So a turn that emits **more output tokens** (a whole-file heredoc, a long
thinking block, an oversized command) directly inflates the *next* request's
input. Keeping each turn's output small is the single biggest lever.

### Observed in the real sessions

| Session | `src/ui.rs` edits | outcome |
|---|---|---|
| v2 | 2 × **17KB `cat >` full rewrites** (5–6.5K output each) | next-request input spikes to ~6.7K |
| v8 | many small `sed -i` / short `python` patches | next-request input stays ~200–700 |

Full-file rewrites also grow the cached context, but they are **not** hard cache
resets; they just make every following request pay to re-send the big block.
