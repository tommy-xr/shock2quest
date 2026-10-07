#!/usr/bin/env python3
"""Summarize Node TAP without treating skips, TODOs, or interrupted logs as passes."""
import argparse
import json
import re
from pathlib import Path


def summarize(text):
    counts = {key: int(value) for key, value in re.findall(
        r"^# (tests|suites|pass|fail|cancelled|skipped|todo) (\d+)$", text, re.M)}
    duration = re.search(r"^# duration_ms ([\d.]+)$", text, re.M)
    plan = re.search(r"^1\.\.(\d+)$", text, re.M)
    verdicts = re.findall(r"^shock2-sdk tests: (PASS|FAIL)(.*)$", text, re.M)
    verdict = verdicts[-1][0] if verdicts else None
    cases = []
    pattern = r"^([ \t]*)(not ok|ok) (\d+) - (.*)$"
    matches = list(re.finditer(pattern, text, re.M))
    for index, match in enumerate(matches):
        indent, result, number, name = match.groups()
        directive = re.search(r" # (SKIP|TODO)(?:\s+(.*))?$", name)
        status = directive[1].lower() if directive else ("failed" if result == "not ok" else "passed")
        case = {"number": int(number), "name": name[:directive.start()] if directive else name,
                "status": status, "indent": len(indent)}
        if directive and directive[2]:
            case["reason"] = directive[2]
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        block = text[match.end():end]
        prefix = re.escape(indent)
        elapsed = re.search(rf"^{prefix}  duration_ms: ([\d.]+)$", block, re.M)
        location = re.search(rf"^{prefix}  location: (.+)$", block, re.M)
        if elapsed:
            case["duration_ms"] = float(elapsed[1])
        if location:
            case["location"] = location[1].strip("'\"")
        cases.append(case)
    # Node may be run directly, without the SDK wrapper. Its completed plan and
    # summary are useful evidence, but the caller must still retain exit status.
    complete = bool(plan) and all(k in counts for k in ("tests", "pass", "fail", "cancelled", "skipped", "todo"))
    consistent = complete and sum(counts[k] for k in ("pass", "fail", "cancelled", "skipped", "todo")) == counts["tests"]
    return {
        "complete_summary": complete,
        "consistent_counts": consistent,
        "counts": counts,
        "planned_top_level_cases": int(plan[1]) if plan else None,
        "duration_ms": float(duration[1]) if duration else None,
        "wrapper_verdict": verdict,
        "passing_summary": bool(consistent and counts["pass"] > 0 and counts["fail"] == 0 and counts["cancelled"] == 0 and verdict != "FAIL"),
        "failures": [c for c in cases if c["status"] == "failed"],
        "skips": [c for c in cases if c["status"] == "skip"],
        "todos": [c for c in cases if c["status"] == "todo"],
        "slowest": sorted((c for c in cases if "duration_ms" in c and c["indent"] == 0), key=lambda c: c["duration_ms"], reverse=True)[:20],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = summarize(args.log.read_text(errors="replace"))
    payload = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(payload)
        print(json.dumps({"report": str(args.output), "complete_summary": report["complete_summary"], "counts": report["counts"]}))
    else:
        print(payload, end="")


if __name__ == "__main__":
    main()
