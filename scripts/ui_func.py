#!/usr/bin/env python3
"""Functional-level UI checks: tiles, hotkey recorder, GIF inputs, sliders."""
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


def shot(hwnd, name):
    img = ui.grab(hwnd, OUT / f"{name}.png")
    return hashlib.md5(img.tobytes()).hexdigest()[:10], img


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
    time.sleep(1.2)
    ui.activate(hwnd)
    results = []

    def step(name, ok):
        print(f"{'PASS' if ok else 'FAIL'} {name}")
        results.append(ok)

    def verify_click(name, lx, ly, expect_change=True):
        before, _ = shot(hwnd, f"{name}_before")
        ui.click(hwnd, lx, ly)
        time.sleep(0.8)
        after, _ = shot(hwnd, f"{name}_after")
        step(name, (after != before) == expect_change)

    # --- capture page: 录屏/停止 tile toggles its label ----------------------
    # The record tile sits in the second row of the action grid. From the tile
    # layout: two columns, row height 84 + gap; grid starts ~y 300.
    verify_click("tile_record_toggle", 260, 520)
    # click again to restore (it may have opened the overlay; Esc via window)
    ui.click(hwnd, 260, 520)
    time.sleep(0.8)

    # --- hotkey recorder: click 录制 on the first hotkey row -----------------
    ui.click(hwnd, 31, 395)   # settings
    time.sleep(0.7)
    ui.click(hwnd, 420, 145)  # 控制 tab
    time.sleep(0.7)
    # First row's 录制 button — the row y measured earlier: 652 logical.
    verify_click("hotkey_record_arm", 790, 652)
    # armed → the row now shows the press prompt; Esc cancels
    ui.send_key_combo(0x1B)
    time.sleep(0.5)

    # --- output tab: quality slider moves ------------------------------------
    ui.click(hwnd, 366, 145)  # 输出 tab
    time.sleep(0.7)
    verify_click("output_slider_area", 700, 300)

    proc.terminate()

    failed = [i for i, r in enumerate(results) if not r]
    print(f"\n{len(results) - len(failed)}/{len(results)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
