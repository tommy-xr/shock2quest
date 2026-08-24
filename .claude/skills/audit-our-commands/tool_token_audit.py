#!/usr/bin/env python3
"""Audit Claude Code transcripts: token cost of tool results, grouped by command category.

Usage: tool_token_audit.py [--days N] [--top N] [--samples] DIR [DIR...]
Scans *.jsonl session files (including subagent files) in each DIR.
Token estimate: chars/4 (close enough for ranking; matches rtk's heuristic).
"""
import json, sys, os, re, glob, time, argparse
from collections import defaultdict

def strip_prefixes(c):
    # peel off env assignments, `cd X &&`, `VAR=...;`, timeout, leading parens
    prev = None
    while prev != c:
        prev = c
        c = re.sub(r"^\(", "", c).strip()
        c = re.sub(r"^cd\s+(?:\"[^\"]*\"|'[^']*'|\S+)\s*(?:&&|;)\s*", "", c)
        c = re.sub(r"^[A-Za-z_][A-Za-z0-9_]*=(?:\"[^\"]*\"|'[^']*'|\S*)\s*(?:&&|;)?\s*", "", c)
        c = re.sub(r"^(?:timeout\s+\d+\s+|command\s+|exec\s+|sudo\s+)", "", c)
    return c

def categorize(tool, inp):
    if tool == "Read":
        p = ((inp or {}).get("file_path") or "").lower()
        if re.search(r"\.(png|jpe?g|gif|webp|bmp)$", p):
            return "[Read image]", p
        return "[Read text]", p
    if tool != "Bash":
        return f"[{tool}]", None
    cmd = (inp or {}).get("command", "") or ""
    c = strip_prefixes(cmd.strip())
    # debug runtime HTTP
    m = re.search(r"curl[^|;&]*?/v1/([a-zA-Z_/]+)", c)
    if m:
        return f"curl /v1/{m.group(1).rstrip('/')}", cmd
    if "curl" in c:
        return "curl (other)", cmd
    m = re.match(r"cargo\s+(dq|dbgr|dv|dr|bn|dbgc)\b\s*(\S*)", c)
    if m:
        sub = m.group(2) if m.group(1) in ("dq", "bn") else ""
        return f"cargo {m.group(1)} {sub}".strip(), cmd
    m = re.match(r"(?:RUSTFLAGS=\S*\s+)?cargo\s+(check|build|test|clippy|fmt|run)\b", c)
    if m:
        return f"cargo {m.group(1)}", cmd
    m = re.match(r"(?:cd\s+\S+\s*(?:&&|;)\s*)?npm\s+(?:run\s+)?(\S+)", c)
    if m:
        return f"npm {m.group(1)}", cmd
    m = re.match(r"(git|gh)\s+(\S+)", c)
    if m:
        return f"{m.group(1)} {m.group(2)}", cmd
    first = c.split()[0] if c.split() else "(empty)"
    return first, cmd

def content_len(content):
    if isinstance(content, str):
        return len(content)
    if isinstance(content, list):
        n = 0
        for b in content:
            if isinstance(b, dict):
                if b.get("type") == "text":
                    n += len(b.get("text", ""))
                elif b.get("type") == "image":
                    n += 1600 * 4  # rough: ~1600 tokens per screenshot-sized image
        return n
    return 0

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+")
    ap.add_argument("--days", type=float, default=30)
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument("--samples", action="store_true", help="print biggest single results per top category")
    args = ap.parse_args()

    cutoff = time.time() - args.days * 86400
    files = []
    for d in args.dirs:
        for f in glob.glob(os.path.join(d, "**", "*.jsonl"), recursive=True):
            if "/memory/" in f:
                continue
            if os.path.getmtime(f) >= cutoff:
                files.append(f)

    stats = defaultdict(lambda: {"count": 0, "chars": 0, "max": 0, "max_cmd": "", "max_file": ""})
    total_chars = 0
    nfiles = 0
    for f in files:
        nfiles += 1
        pending = {}  # tool_use_id -> (category, cmd)
        try:
            with open(f, "r", errors="replace") as fh:
                for line in fh:
                    try:
                        e = json.loads(line)
                    except Exception:
                        continue
                    msg = e.get("message") or {}
                    content = msg.get("content")
                    if not isinstance(content, list):
                        continue
                    for b in content:
                        if not isinstance(b, dict):
                            continue
                        if b.get("type") == "tool_use":
                            cat, cmd = categorize(b.get("name", "?"), b.get("input"))
                            pending[b.get("id")] = (cat, cmd)
                        elif b.get("type") == "tool_result":
                            cat, cmd = pending.pop(b.get("tool_use_id"), ("[unmatched]", None))
                            n = content_len(b.get("content"))
                            s = stats[cat]
                            s["count"] += 1
                            s["chars"] += n
                            total_chars += n
                            if n > s["max"]:
                                s["max"], s["max_cmd"], s["max_file"] = n, (cmd or "")[:200], f
        except Exception as ex:
            print(f"warn: {f}: {ex}", file=sys.stderr)

    rows = sorted(stats.items(), key=lambda kv: -kv[1]["chars"])
    est_total = total_chars // 4
    print(f"# {nfiles} session files (last {args.days:g} days), total tool-result ~{est_total/1e6:.1f}M est-tokens\n")
    print(f"{'category':<42} {'calls':>6} {'est-tok':>10} {'%':>5} {'avg':>7} {'max':>8}")
    for cat, s in rows[: args.top]:
        tok = s["chars"] // 4
        print(f"{cat:<42} {s['count']:>6} {tok:>10,} {100*s['chars']/max(total_chars,1):>4.1f}% {tok//max(s['count'],1):>7,} {s['max']//4:>8,}")
    if args.samples:
        print("\n## biggest single result per top-10 category")
        for cat, s in rows[:10]:
            print(f"\n### {cat}  (max {s['max']//4:,} est-tok)\n  cmd: {s['max_cmd']}\n  file: {s['max_file']}")

if __name__ == "__main__":
    main()
