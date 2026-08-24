---
name: audit-our-commands
description: >-
  Audit Claude Code session transcripts to measure which tool calls / commands
  consume the most result tokens, find waste patterns (re-paging files, wide
  greps, full diffs, oversized screenshots, duplicate invocations), and
  recommend optimizations. Use when asked to audit token usage, measure command
  output cost, or find token-efficiency opportunities in recent sessions.
---

# audit-our-commands

Measure where tool-result tokens go across recent Claude Code sessions, then
characterize the waste and recommend fixes, ranked by estimated savings.

## Step 1 — Quantitative pass (cheap, run inline)

Run the bundled analyzer over the relevant transcript directories (transcripts
live under `~/.claude/projects/<escaped-project-path>/`; one dir per checkout
or worktree, so glob broadly):

```bash
python3 .claude/skills/audit-our-commands/tool_token_audit.py \
  --days 30 --top 50 --samples ~/.claude/projects/*<project-pattern>*
```

It joins `tool_use` → `tool_result` in every session JSONL (subagent files
included), estimates tokens as chars/4, strips `cd X &&` / env-var / `timeout`
prefixes, and aggregates by command category — `sed`, `grep`, `git diff`,
`curl /v1/<endpoint>`, `cargo dq <sub>`, `[Read text]`, `[Read image]`, etc.
Output: per-category call count, estimated tokens, share, avg, max, and (with
`--samples`) the biggest single result per top category with its exact command.

**Image caveat:** the analyzer uses a flat ~1600 tokens/image. Real cost is
`ceil(w/28) * ceil(h/28)` tokens (standard tier caps the long edge at 1568 px
≈ 1568 tokens; high-resolution tier at 2576 px ≈ 4784 tokens). Check actual
PNG dimensions (`sips -g pixelWidth -g pixelHeight`) before trusting the flat
estimate — HiDPI capture can silently double dimensions.

## Step 2 — Qualitative pass (delegate to one subagent)

For the top 4–6 categories, spawn ONE agent to extract the largest actual
results from the JSONL (with a small python script — never Read the raw
multi-MB session files directly) and characterize the waste:

- **Paging/re-reading**: `sed -n 'A,Bp'` used as a pager, whole-file Reads,
  repeat reads of the same path within one session, paging self-generated
  diff dumps.
- **Wide greps**: `-A/-B/-C` context, unanchored patterns returning hundreds
  of hits, `grep -n "" file` used as cat.
- **Full diffs**: `git diff` without `--stat` first, `| head -N` with large N.
- **Duplicates**: hash identical `(tool, command)` per session and total the
  rerun token cost.

If a token-filtering proxy like `rtk` is installed, have the agent measure
native-vs-wrapped output on 2–3 sampled commands and judge whether the
filtering would have lost information the session actually needed. Findings
from the 2026-08 audit of this repo: `rtk grep` saved 86–96% on wide greps but
caps output at 200 hits / 80 chars per line (a net loss on small greps — don't
wrap those, and don't trust it when the hit *count* matters); `rtk read -l
aggressive` is a good outline reader (~84%) but its `-m N` mode renumbers
lines; `rtk git diff` drops hunks — never use it as a review artifact.

## Step 3 — Recommend, ranked by estimated tokens/month

For each recommendation give the mechanism (guidance line, wrapper, code fix),
the estimated monthly savings computed from the Step 1 numbers, and the risk
(especially silent truncation). Recurring winners: outline-before-page for
large files, `git diff --stat` first, never re-read what is already in
context, and downscaling images at the capture source.
