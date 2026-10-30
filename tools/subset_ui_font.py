#!/usr/bin/env python3
"""Rebuild the embedded UI font subsets for PinSight.

Why: the default (Auto) face used to memory-map the full ~19 MB Microsoft
YaHei. The UI mostly shows strings from the i18n table (seven languages), so a
subset of those glyphs is far smaller and removes the dependency on a system
font being installed.

YaHei alone is not enough, though: it carries no Hangul (the Korean UI would
be blank) and no dingbat symbols such as ✓ ✗ ↔. So the script splits the
required glyph set across several source faces and writes one small subset per
face:

    assets/pinsight_ui.ttf      Latin + CJK + kana   (YaHei)
    assets/pinsight_ui_ko.ttf   Hangul                (Malgun Gothic)
    assets/pinsight_ui_sym.ttf  symbols               (MS Gothic)

fonts.rs installs all of them as a fallback chain, so a glyph missing from one
face is picked up from the next. A system font is still mapped as the last
resort: the user can type arbitrary text (a Chinese save path, for example)
that no subset can predict.

Re-run this after adding or editing user-visible strings — every src/*.rs
string literal is scanned automatically:

    python tools/subset_ui_font.py

Requires: pip install fonttools
"""
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src")
ASSETS = os.path.join(ROOT, "assets")

# (output file, source font, font number inside a .ttc).
# Faces are tried in order; the first one that exists wins.
FACES = [
    ("pinsight_ui.ttf", r"C:\Windows\Fonts\msyh.ttc", 0),
    ("pinsight_ui.ttf", "/System/Library/Fonts/PingFang.ttc", 0),
    ("pinsight_ui.ttf", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
    ("pinsight_ui_ko.ttf", r"C:\Windows\Fonts\malgun.ttf", 0),
    ("pinsight_ui_ko.ttf", "/System/Library/Fonts/AppleSDGothicNeo.ttc", 0),
    ("pinsight_ui_ko.ttf", "/usr/share/fonts/opentype/noto/NotoSansKR-Regular.otf", 0),
    ("pinsight_ui_sym.ttf", r"C:\Windows\Fonts\msgothic.ttc", 0),
    ("pinsight_ui_sym.ttf", r"C:\Windows\Fonts\YuGothR.ttc", 0),
    ("pinsight_ui_sym.ttf", "/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0),
]

# Punctuation / symbols the UI draws outside the i18n table.
EXTRA = "…·—–×→←↔✓✗①②③④⑤⑥⑦⑧⑨★☆●○■□▲▼◆◇©®™°±÷≈≠≤≥↓"


def collect_glyphs() -> set:
    chars = set(range(0x20, 0x7F))  # ASCII printable
    for c in EXTRA:
        chars.add(ord(c))

    def harvest(text: str):
        for m in re.finditer(r'"((?:[^"\\]|\\.)*)"', text):
            for ch in m.group(1):
                if ord(ch) >= 0x20:
                    chars.add(ord(ch))

    for name in sorted(os.listdir(SRC)):
        if name.endswith(".rs"):
            try:
                harvest(open(os.path.join(SRC, name), encoding="utf-8").read())
            except OSError:
                pass
    return {c for c in chars if c >= 0x20}


def to_unicodes(cps):
    cps = sorted(cps)
    ranges = []
    s = p = cps[0]
    for cp in cps[1:]:
        if cp == p + 1:
            p = cp
            continue
        ranges.append((s, p))
        s = p = cp
    ranges.append((s, p))
    return ",".join(f"U+{a:04X}-{b:04X}" if a != b else f"U+{a:04X}" for a, b in ranges)


def face_cmap(path: str, number: int) -> dict:
    """Codepoint -> glyph name for one face, or {} if it cannot be read."""
    try:
        from fontTools.ttLib import TTFont

        font = TTFont(path, fontNumber=number, lazy=True)
        return font.getBestCmap()
    except Exception:
        return {}


def pick_sources():
    """First existing candidate per output file."""
    chosen = {}
    for out, path, number in FACES:
        if out in chosen:
            continue
        if os.path.exists(path):
            chosen[out] = (path, number)
    return chosen


def main():
    wanted = collect_glyphs()
    sources = pick_sources()
    if not sources:
        sys.exit("no source CJK font found; edit FACES")

    # Greedily assign each wanted codepoint to the first face that has it, so
    # the faces do not duplicate work and the chain stays short.
    assignments = {out: set() for out in sources}
    cmaps = {out: face_cmap(path, num) for out, (path, num) in sources.items()}
    # The main face goes first so Latin and CJK land there; the auxiliary faces
    # only pick up what is left.
    order = sorted(sources, key=lambda o: (o != "pinsight_ui.ttf", o))
    for cp in sorted(wanted):
        for out in order:
            if cp in cmaps[out]:
                assignments[out].add(cp)
                break

    for out, (path, number) in sources.items():
        cps = assignments[out]
        if not cps:
            continue
        dest = os.path.join(ASSETS, out)
        cmd = [
            sys.executable, "-m", "fontTools.subset", path,
            f"--unicodes={to_unicodes(cps)}",
            "--output-file=" + dest,
            f"--font-number={number}",
            "--layout-features=*",
            "--no-hinting",
            "--recalc-bounds",
        ]
        r = subprocess.run(cmd)
        if r.returncode != 0:
            sys.exit(r.returncode)
        print(f"{out}: {len(cps)} glyphs -> {dest} "
              f"({os.path.getsize(dest)/1024:.1f} KB, from {os.path.basename(path)})")

    # Sanity check: every wanted codepoint must be in some produced subset.
    covered = set()
    for out in assignments:
        dest = os.path.join(ASSETS, out)
        if os.path.exists(dest):
            try:
                from fontTools.ttLib import TTFont

                covered |= set(TTFont(dest, lazy=True).getBestCmap())
            except Exception:
                pass
    missing = sorted(wanted - covered)
    if missing:
        print("WARNING: not covered by any subset: "
              + "".join(chr(c) for c in missing)[:120])
    else:
        print(f"all {len(wanted)} required glyphs covered")


if __name__ == "__main__":
    main()
