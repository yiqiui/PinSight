#!/usr/bin/env python3
"""Click-through acceptance sweep of the PinSight main window (v2).

Coordinates measured from live screenshots (125% DPI, light theme).
Sidebar pages, all 8 settings tabs, theme segmented control and accent
swatches are clicked; each step asserts the screen actually changed.
"""
from __future__ import annotations

import ctypes
import hashlib
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import shot_ui as ui  # noqa: E402

OUT = Path(__file__).parent.parent / "ui_sweep_out"
OUT.mkdir(exist_ok=True)

# Settings tab strip: logical y for the label row, x per tab (measured).
TABS = [("常规", 163), ("外观", 212), ("截屏", 264), ("贴图", 315),
        ("输出", 366), ("控制", 420), ("更新", 470), ("状态", 520)]
TAB_Y_CANDIDATES = (145, 140, 150)

# Accent swatch row (appearance tab): logical y + x centres, blue first.
SWATCH_Y = 310
SWATCHES = [603, 641, 679, 717, 755, 793, 831]

# Theme segmented control (appearance tab): logical y + option centres.
THEME_Y = 274
THEME_OPTS = {"system": 689, "light": 706, "dark": 724}


def shot(hwnd, name):
    img = ui.grab(hwnd, OUT / f"{name}.png")
    return hashlib.md5(img.tobytes()).hexdigest()[:10], img


def click_tab(hwnd, lx):
    """Tabs are thin; try a few y candidates until the page changes."""
    before, _ = shot(hwnd, "tab_probe")
    for ly in TAB_Y_CANDIDATES:
        ui.click(hwnd, lx, ly)
        time.sleep(0.65)
        after, _ = shot(hwnd, "tab_probe")
        if after != before:
            return True
    return False


def main():
    subprocess.run(["taskkill", "/F", "/IM", "pinsight.exe"], capture_output=True)
    time.sleep(0.6)
    log = open(OUT / "app.log", "wb")
    proc = subprocess.Popen([r"target\release\pinsight.exe"], stdout=log, stderr=log)
    hwnd = None
    for _ in range(40):
        time.sleep(0.25)
        hwnd = ui.find_window("PinSight")
        if hwnd:
            break
    if not hwnd:
        print("FAIL app did not start")
        return 1
    time.sleep(1.2)
    ui.activate(hwnd)
    results = []

    def step(name, ok):
        print(f"{'PASS' if ok else 'FAIL'} {name}")
        results.append(ok)

    # --- sidebar pages -----------------------------------------------------
    prev, _ = shot(hwnd, "initial")
    for name, y in [("page_library", 215), ("page_pins", 275),
                    ("page_gif", 335), ("page_settings", 395)]:
        ui.click(hwnd, 31, y)
        time.sleep(0.75)
        cur, _ = shot(hwnd, name)
        step(name, cur != prev)
        prev = cur
    ui.click(hwnd, 31, 155)  # back to capture
    time.sleep(0.75)

    # --- settings tabs -------------------------------------------------------
    ui.click(hwnd, 31, 395)
    time.sleep(0.75)
    prev, _ = shot(hwnd, "tab_base")
    for label, lx in TABS:
        ok = click_tab(hwnd, lx)
        step(f"tab_{label}", ok)
        prev, _ = shot(hwnd, "tab_base")

    # --- appearance: theme switch --------------------------------------------
    click_tab(hwnd, 212)
    time.sleep(0.4)
    before, _ = shot(hwnd, "theme_before")
    ui.click(hwnd, THEME_OPTS["dark"], THEME_Y)
    time.sleep(0.8)
    after, _ = shot(hwnd, "theme_dark")
    step("theme_dark", after != before)
    ui.click(hwnd, THEME_OPTS["light"], THEME_Y)
    time.sleep(0.8)

    # --- accent swatches: cycle three, expect each to change the screen ------
    for i, sx in enumerate(SWATCHES[:3]):
        before, _ = shot(hwnd, f"sw{i}_before")
        ui.click(hwnd, sx, SWATCH_Y)
        time.sleep(0.7)
        after, _ = shot(hwnd, f"sw{i}_after")
        step(f"accent_swatch_{i}", after != before)
    ui.click(hwnd, SWATCHES[0], SWATCH_Y)  # back to blue
    time.sleep(0.6)

    proc.terminate()

    failed = [i for i, r in enumerate(results) if not r]
    print(f"\n{len(results) - len(failed)}/{len(results)} passed")
    if failed:
        print("failed steps:", failed)
        return 1
    print("ALL PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
