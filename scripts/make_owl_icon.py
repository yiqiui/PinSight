# -*- coding: utf-8 -*-
"""Draw the PinSight owl logo as PNG/ICO assets (v4).

Composition: the owl's face IS a screenshot selection.  A rounded rectangle
(the "face") framed by four viewfinder corner brackets -- amber on the left
pair, cyan on the right -- with two eyes inside: the left a focus cross in
amber, the right a plain dot in cyan.  A small triangular beak below centre.
No round head, no ear tufts, no wings: nothing left of the generic cute-owl
template.  The whole mark reads as "a region being captured" first and an
owl second.
"""
import os

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
ASSETS = os.path.normpath(os.path.join(HERE, "..", "assets"))

# palette ---------------------------------------------------------------
PLATE_TOP = (24, 28, 36)        # dark plate, slight vertical shade
PLATE_BOT = (13, 16, 22)
FACE = (237, 233, 224)          # cream face (the "selection")
AMBER = (0xFB, 0xBF, 0x24)      # Accent::Amber
CYAN = (0x22, 0xD3, 0xEE)       # Accent::Cyan
PUPIL = (16, 18, 24)

S = 1024                        # master canvas (supersample)


def rounded_plate():
    """Rounded-square plate with a subtle vertical shade."""
    r = int(S * 0.225)
    plate = Image.new("RGBA", (S, S), PLATE_TOP + (255,))
    grad = Image.new("L", (1, S))
    for y in range(S):
        t = (y / S - 0.35) / 0.65
        grad.putpixel((0, y), int(255 * min(1.0, max(0.0, t))))
    dark = Image.new("RGBA", (S, S), PLATE_BOT + (255,))
    plate = Image.composite(dark, plate, grad.resize((S, S)))
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, S - 1, S - 1], radius=r, fill=255)
    out = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    out.paste(plate, (0, 0), mask)
    return out


def face_layer():
    """The selection-rectangle face with corner brackets and eyes."""
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    # face: a rounded rectangle, wider than tall, slightly above centre
    fx0, fy0, fx1, fy1 = S * 0.16, S * 0.24, S * 0.84, S * 0.76
    d.rounded_rectangle([fx0, fy0, fx1, fy1], radius=S * 0.09, fill=FACE + (255,))

    # corner brackets: amber on the left pair, cyan on the right pair,
    # drawn outside the face with a gap -- the classic marquee look.
    bw = S * 0.045          # bracket stroke width
    leg = S * 0.14          # bracket leg length
    gap = S * 0.028
    pairs = (
        (fx0 - gap, fy0 - gap, -1, AMBER),   # top-left
        (fx0 - gap, fy1 + gap, -1, AMBER),   # bottom-left
        (fx1 + gap, fy0 - gap, 1, CYAN),     # top-right
        (fx1 + gap, fy1 + gap, 1, CYAN),     # bottom-right
    )
    for bx, by, side, colour in pairs:
        # horizontal leg points inward toward the face
        x_in = bx + side * leg
        d.line([(bx, by), (x_in, by)], fill=colour + (255,), width=int(bw))
        # vertical leg points inward vertically too (toward the face)
        y_in = by + (leg if by > fy0 else -leg)
        d.line([(bx, by), (bx, y_in)], fill=colour + (255,), width=int(bw))

    # eyes: left = focus cross in amber, right = dot in cyan
    ey = (fy0 + fy1) / 2 - S * 0.02
    ex_l, ex_r = cx_left, cx_right = fx0 + (fx1 - fx0) * 0.30, fx0 + (fx1 - fx0) * 0.70
    eye_r = S * 0.088
    # left eye: amber ring + cross pupil
    d.ellipse([ex_l - eye_r, ey - eye_r, ex_l + eye_r, ey + eye_r],
              outline=AMBER + (255,), width=int(S * 0.024))
    arm = eye_r * 0.62
    pw = max(4, int(S * 0.018))
    d.line([(ex_l - arm, ey), (ex_l + arm, ey)], fill=PUPIL + (255,), width=pw)
    d.line([(ex_l, ey - arm), (ex_l, ey + arm)], fill=PUPIL + (255,), width=pw)
    # right eye: cyan ring + round pupil
    d.ellipse([ex_r - eye_r, ey - eye_r, ex_r + eye_r, ey + eye_r],
              outline=CYAN + (255,), width=int(S * 0.024))
    pupil_r = eye_r * 0.42
    d.ellipse([ex_r - pupil_r, ey - pupil_r, ex_r + pupil_r, ey + pupil_r],
              fill=PUPIL + (255,))

    # beak: small triangle below the eyes
    cx = (fx0 + fx1) / 2
    by0 = ey + eye_r + S * 0.035
    d.polygon([(cx - S * 0.030, by0), (cx + S * 0.030, by0), (cx, by0 + S * 0.055)],
              fill=(0xB4, 0x53, 0x09, 255))
    return layer


def build():
    img = rounded_plate()
    img.alpha_composite(face_layer())

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
