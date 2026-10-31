# -*- coding: utf-8 -*-
"""Draw the PinSight owl logo as PNG/ICO assets.

Geometry-only owl: rounded-square dark plate, cream owl head, two big eyes
drawn as camera focus rings with an amber (left) and cyan (right) iris --
the amber/cyan pair echoes the accent colours.  Ear tufts, a small beak and
two wing wedges so the bird still reads at 16 px tray size.
"""
import os

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
ASSETS = os.path.normpath(os.path.join(HERE, "..", "assets"))

# palette ---------------------------------------------------------------
PLATE_TOP = (24, 28, 36)        # dark plate, slight vertical shade
PLATE_BOT = (13, 16, 22)
HEAD = (237, 233, 224)          # cream head
HEAD_DIM = (216, 210, 197)      # wing shade
EYE_RING = (250, 250, 250)      # focus ring white
AMBER = (0xFB, 0xBF, 0x24)      # left iris  -- Accent::Amber
CYAN = (0x22, 0xD3, 0xEE)       # right iris -- Accent::Cyan
PUPIL = (16, 18, 24)
BEAK = (0xFB, 0xBF, 0x24)       # beak reuses the amber accent

S = 1024                        # master canvas (supersample)


def rounded_plate():
    """Rounded-square plate with a subtle vertical shade."""
    r = int(S * 0.225)
    plate = Image.new("RGBA", (S, S), PLATE_TOP + (255,))
    # darker towards the bottom through a gradient mask
    grad = Image.new("L", (1, S))
    for y in range(S):
        t = (y / S - 0.35) / 0.65
        grad.putpixel((0, y), int(255 * min(1.0, max(0.0, t))))
    grad = grad.resize((S, S))
    dark = Image.new("RGBA", (S, S), PLATE_BOT + (255,))
    plate = Image.composite(dark, plate, grad)
    # punch through a rounded-rect mask
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, S - 1, S - 1], radius=r, fill=255)
    out = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    out.paste(plate, (0, 0), mask)
    return out


def head_layer():
    """Cream owl: wings behind the head, head circle, ear tufts."""
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    cx = S / 2
    # wings (behind the head)
    d.polygon([(cx - S * 0.28, S * 0.36), (cx - S * 0.42, S * 0.66),
               (cx - S * 0.28, S * 0.72)], fill=HEAD_DIM + (255,))
    d.polygon([(cx + S * 0.28, S * 0.36), (cx + S * 0.42, S * 0.66),
               (cx + S * 0.28, S * 0.72)], fill=HEAD_DIM + (255,))
    # head
    d.ellipse([cx - S * 0.30, S * 0.24, cx + S * 0.30, S * 0.84], fill=HEAD + (255,))
    # ear tufts
    d.polygon([(cx - S * 0.26, S * 0.32), (cx - S * 0.08, S * 0.22),
               (cx - S * 0.19, S * 0.40)], fill=HEAD + (255,))
    d.polygon([(cx + S * 0.26, S * 0.32), (cx + S * 0.08, S * 0.22),
               (cx + S * 0.19, S * 0.40)], fill=HEAD + (255,))
    return layer


def eyes_layer():
    """Two focus-ring eyes: white ring, iris, black pupil."""
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    cx = S / 2
    eye_r = S * 0.115
    ring_w = max(3, int(S * 0.022))
    for sign, iris in ((-1, AMBER), (1, CYAN)):
        ex = cx + sign * S * 0.135
        ey = S * 0.49
        d.ellipse([ex - eye_r, ey - eye_r, ex + eye_r, ey + eye_r],
                  outline=EYE_RING + (255,), width=ring_w)
        iris_r = eye_r * 0.62
        d.ellipse([ex - iris_r, ey - iris_r, ex + iris_r, ey + iris_r],
                  fill=iris + (255,))
        pupil_r = iris_r * 0.45
        d.ellipse([ex - pupil_r, ey - pupil_r, ex + pupil_r, ey + pupil_r],
                  fill=PUPIL + (255,))
    return layer


def beak_layer():
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    cx = S / 2
    d.polygon([(cx - S * 0.035, S * 0.60), (cx + S * 0.035, S * 0.60),
               (cx, S * 0.67)], fill=BEAK + (255,))
    return layer


def build():
    img = rounded_plate()
    for layer in (head_layer(), eyes_layer(), beak_layer()):
        img.alpha_composite(layer)

    png = img.resize((256, 256), Image.LANCZOS)
    png.save(os.path.join(ASSETS, "icon.png"))

    img.save(os.path.join(ASSETS, "icon.ico"),
             sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64),
                    (128, 128), (256, 256)])

    img.save(os.path.join(ASSETS, "tray.ico"),
             sizes=[(16, 16), (20, 20), (24, 24), (32, 32)])
    print("icon.png / icon.ico / tray.ico written to", ASSETS)


if __name__ == "__main__":
    build()
