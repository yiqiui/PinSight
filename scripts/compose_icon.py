# -*- coding: utf-8 -*-
"""Compose the final PinSight icon set from the generated owl artwork.

The AI-generated eagle-owl (flat vector, dark rounded plate) is used as-is;
four viewfinder corner brackets -- amber on the left pair, cyan on the right
pair -- are drawn on top in the plate corners to give the capture-tool
signature.  Output: assets/icon.png, icon.ico, tray.ico.
"""
import os

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, ".."))
ASSETS = os.path.join(ROOT, "assets")
SRC = os.path.join(ROOT, "icon_gen",
                   "Minimalist_flat_vector_app_ico_2026-10-31T10-28-26.png")

AMBER = (0xFB, 0xBF, 0x24)
CYAN = (0x22, 0xD3, 0xEE)
S = 1024


def brackets(img):
    """Draw the four viewfinder brackets in the plate corners."""
    d = ImageDraw.Draw(img)
    bw = int(S * 0.040)     # stroke width
    leg = int(S * 0.115)    # leg length
    inset = int(S * 0.055)  # distance from the plate corner
    corners = (
        # (x, y, dx, dy, colour) -- legs point inward along both axes
        (inset, inset, 1, 1, AMBER),               # top-left
        (S - 1 - inset, inset, -1, 1, CYAN),       # top-right
        (inset, S - 1 - inset, 1, -1, AMBER),      # bottom-left
        (S - 1 - inset, S - 1 - inset, -1, -1, CYAN),  # bottom-right
    )
    for x, y, dx, dy, colour in corners:
        d.line([(x, y), (x + dx * leg, y)], fill=colour + (255,), width=bw)
        d.line([(x, y), (x, y + dy * leg)], fill=colour + (255,), width=bw)
    return img


def build():
    img = Image.open(SRC).convert("RGBA")
    if img.size != (S, S):
        img = img.resize((S, S), Image.LANCZOS)
    img = brackets(img)

    png = img.resize((256, 256), Image.LANCZOS)
    png.save(os.path.join(ASSETS, "icon.png"))

    img.save(os.path.join(ASSETS, "icon.ico"),
             sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64),
                    (128, 128), (256, 256)])
    img.save(os.path.join(ASSETS, "tray.ico"),
             sizes=[(16, 16), (20, 20), (24, 24), (32, 32)])
    print("icon.png / icon.ico / tray.ico written")


if __name__ == "__main__":
    build()
