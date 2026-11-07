#!/usr/bin/env python3
"""Print the CHANGELOG.md section for a release tag; fail if there is none or it is empty."""

import sys
from pathlib import Path


def section(text, tag):
    lines, found = [], False
    for line in text.splitlines():
        if line.startswith("## "):
            if found:
                break
            found = line[3:].strip() == tag
            continue
        if found:
            lines.append(line)
    return "\n".join(lines).strip() if found else None


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: release_notes.py TAG")
    tag = sys.argv[1]
    path = Path(__file__).resolve().parents[1] / "CHANGELOG.md"
    notes = section(path.read_text(encoding="utf-8"), tag)
    if not notes:
        raise SystemExit(f"CHANGELOG.md has no notes under '## {tag}'")
    print(notes)


if __name__ == "__main__":
    main()
