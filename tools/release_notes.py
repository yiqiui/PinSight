#!/usr/bin/env python3
"""Build the GitHub Release notes for one version from CHANGELOG.md.

Every release must describe what it actually changed: which features were
added and which problems were fixed. This script extracts that version's
section from CHANGELOG.md, produces a compact summary plus the full text in
a <details> block, and FAILS if the version has no section at all -- so a
version bump without release notes can never be published.

Usage (called by .github/workflows/release.yml):

    python3 tools/release_notes.py \
        --changelog CHANGELOG.md --version 0.1.1 \
        --commit <sha> --repo yiqiui/PinSight --output notes.md
"""

from __future__ import annotations

import argparse
import datetime as dt
import re
import sys
from pathlib import Path

# Keep a Changelog order; anything else found is appended afterwards.
CATEGORY_ORDER = ["Added", "Changed", "Fixed", "Docs"]
# How many entry titles to show per category in the compact summary.
SUMMARY_LIMIT = 8


def extract_section(lines: list[str], version: str) -> list[str]:
    """Return the body of `## [version] ...` up to the next `## [` header."""
    start = None
    for i, line in enumerate(lines):
        if re.match(rf"^## \[{re.escape(version)}\]", line):
            start = i + 1
            break
    if start is None:
        return []
    end = len(lines)
    for j in range(start, len(lines)):
        if lines[j].startswith("## ["):
            end = j
            break
    body = lines[start:end]
    # Keep a Changelog keeps `[x.y.z]: url` link definitions at the end of the
    # file; they are not prose and would show up raw inside the notes.
    body = [line for line in body if not re.match(r"^\[[^\]]+\]:\s", line)]
    while body and not body[0].strip():
        body.pop(0)
    while body and not body[-1].strip():
        body.pop()
    return body


def parse(body: list[str]) -> tuple[list[str], dict[str, list[str]]]:
    """Split a section into its intro paragraph and per-category bullet lines."""
    intro: list[str] = []
    categories: dict[str, list[str]] = {}
    current: str | None = None
    for line in body:
        if line.startswith("### "):
            current = line[4:].strip()
            categories.setdefault(current, [])
        elif current is None:
            intro.append(line)
        else:
            categories[current].append(line)
    while intro and not intro[0].strip():
        intro.pop(0)
    while intro and not intro[-1].strip():
        intro.pop()
    return intro, categories


def bullets(lines: list[str]) -> list[str]:
    """Collect top-level `- ...` entries (continuation lines are appended)."""
    out: list[str] = []
    for line in lines:
        if line.startswith("- "):
            out.append(line[2:].strip())
        elif out and line.strip():
            out[-1] += " " + line.strip()
    return out


def title_of(entry: str) -> str:
    """`- **Name**: description` -> `Name`; otherwise a truncated first line."""
    m = re.match(r"\*\*(.+?)\*\*", entry)
    if m:
        return m.group(1)
    text = re.sub(r"\s+", " ", entry).strip()
    return text[:90] + ("..." if len(text) > 90 else "")


def build(args: argparse.Namespace) -> str:
    lines = Path(args.changelog).read_text(encoding="utf-8").split("\n")
    body = extract_section(lines, args.version)
    if not body:
        print(
            f"::error::CHANGELOG.md has no '## [{args.version}]' section; "
            "add release notes before publishing this version",
            file=sys.stderr,
        )
        sys.exit(1)

    intro, categories = parse(body)
    order = [c for c in CATEGORY_ORDER if c in categories]
    order += [c for c in categories if c not in CATEGORY_ORDER]

    counts = {c: len(bullets(categories[c])) for c in order}
    total = sum(counts.values())
    if total == 0:
        print(
            f"::error::CHANGELOG section for {args.version} has no bullet entries",
            file=sys.stderr,
        )
        sys.exit(1)

    stamp = args.timestamp or dt.datetime.now(dt.timezone.utc).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    repo = args.repo
    changelog_url = f"https://github.com/{repo}/blob/{args.commit}/CHANGELOG.md"

    out: list[str] = [
        f"PinSight v{args.version} — built from `main` at `{args.commit}` on {stamp}.",
        "",
        "Packages for the current version are replaced in place on every push; "
        "bump `version` in Cargo.toml to open a new Release.",
        "",
        "---",
        "",
        f"## What changed in {args.version}",
        "",
    ]
    if intro:
        out += [line for line in intro if line.strip()] + [""]

    summary = ", ".join(f"**{counts[c]} {c.lower()}**" for c in order)
    out += [f"**{total} entries** — {summary}.", ""]

    for c in order:
        entries = bullets(categories[c])
        out += [f"### {c}", ""]
        for e in entries[:SUMMARY_LIMIT]:
            out.append(f"- {title_of(e)}")
        if len(entries) > SUMMARY_LIMIT:
            out.append(f"- _…and {len(entries) - SUMMARY_LIMIT} more_")
        out.append("")

    out += [
        "<details>",
        f"<summary>Full changelog for {args.version} (click to expand)</summary>",
        "",
    ]
    out += body
    out += [
        "",
        "</details>",
        "",
        "---",
        "",
        f"[Full CHANGELOG.md]({changelog_url})",
        "",
    ]
    return "\n".join(out)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--changelog", default="CHANGELOG.md")
    ap.add_argument("--version", required=True)
    ap.add_argument("--commit", required=True)
    ap.add_argument("--repo", required=True)
    ap.add_argument("--timestamp", default="")
    ap.add_argument("--output", required=True)
    args = ap.parse_args()
    notes = build(args)
    Path(args.output).write_text(notes, encoding="utf-8", newline="\n")
    print(f"wrote {args.output}: {len(notes)} chars")


if __name__ == "__main__":
    main()
