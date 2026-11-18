# -*- coding: utf-8 -*-
"""Generate the PinSight app icon set (v8 — paper-cut mantis, imported artwork).

v8 replaces the procedural drawing with a hand-picked paper-cut mantis
artwork (green mantis holding a dashed screenshot selection, camera pendant,
viewfinder brackets) supplied as `scripts/icon_artwork_src.png`.  The script
extracts the green cut-paper marks from the source (chroma-keyed on green
dominance), composites them onto an off-white rounded plate and exports
assets/icon.png, icon.ico, tray.ico.

The plate is now LIGHT (luminance ~240) with DARK green marks — the inverse
of v6/v7 — so src/app.rs glyph() gained an invert branch keyed on the plate's
border luminance to keep the White tray style working.

Outputs: assets/icon.png, icon.ico, tray.ico.

tray.ico stays in the legacy BMP/DIB format because the runtime reads it via
CreateIconFromResourceEx, which rejects PIL's PNG-compressed ICO entries.
"""

import os
import struct

from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
ASSETS = os.path.normpath(os.path.join(HERE, "..", "assets"))
SRC = os.path.join(HERE, "icon_artwork_src.png")

S = 1024  # master canvas (supersample); every size is box-filtered from this

PLATE_TOP = (246, 248, 249)   # off-white paper, top
PLATE_BOT = (233, 237, 239)   # slightly darker at the bottom
PLATE_EDGE = (214, 219, 222)  # thin plate border


def extract_marks():
    """Chroma-key the green cut-paper artwork off its light background.

    Returns (rgba_image, alpha_mask).  Green dominance (G - max(R, B)) drives
    the mask; the soft ramp keeps the anti-aliased paper edges.
    """
    im = Image.open(SRC).convert("RGB")
    r, g, b = im.split()
    d1 = ImageChops.subtract(g, r)
    d2 = ImageChops.subtract(g, b)
    diff = ImageChops.lighter(d1, d2)
    # 0 below 10, 255 above 34, linear ramp between
    mask = diff.point(lambda v: 0 if v < 10 else (255 if v > 34 else int((v - 10) / 24 * 255)))
    bbox = mask.getbbox()
    im = im.crop(bbox)
    mask = mask.crop(bbox)
    return im, mask


def rounded_plate():
    """Off-white rounded-square plate with a soft gradient and thin border."""
    r = int(S * 0.225)
    g = Image.new("RGBA", (S, S), PLATE_TOP)
    mask = Image.new("L", (S, S))
    dm = ImageDraw.Draw(mask)
    for y in range(S):
        dm.line([(0, y), (S, y)], fill=int(255 * (y / S)))
    plate = Image.composite(Image.new("RGBA", (S, S), PLATE_BOT), g, mask)
    rim = max(2, int(S * 0.010))
    ImageDraw.Draw(plate).rounded_rectangle(
        [rim, rim, S - 1 - rim, S - 1 - rim], radius=r, outline=PLATE_EDGE, width=rim)
    clip = Image.new("L", (S, S), 0)
    ImageDraw.Draw(clip).rounded_rectangle([0, 0, S - 1, S - 1], radius=r, fill=255)
    out = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    out.paste(plate, (0, 0), clip)
    return out


def master():
    img = rounded_plate()
    art, mask = extract_marks()
    # fit the artwork into ~78% of the plate, centred
    target = int(S * 0.78)
    s = min(target / art.width, target / art.height)
    art = art.resize((max(1, int(art.width * s)), max(1, int(art.height * s))), Image.LANCZOS)
    mask = mask.resize(art.size, Image.LANCZOS)
    x = (S - art.width) // 2
    y = (S - art.height) // 2
    # faint paper-cut drop shadow so the marks sit "on" the plate
    sh = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    sh_mask = mask.filter(ImageFilter.GaussianBlur(S * 0.008))
    sh.paste((24, 62, 52, 70), (x + int(S * 0.012), y + int(S * 0.014)), sh_mask)
    img = Image.alpha_composite(img, sh)
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    layer.paste(art, (x, y), mask)
    return Image.alpha_composite(img, layer)


def master_small():
    """Solid-silhouette variant for tray-sized icons (<= 32 px).

    The paper-cut interior texture dissolves into noise at that scale, so the
    small variant fills the (dilated) silhouette with one flat green and keeps
    only the outline: plate + mantis + selection frame, still readable at 16px.
    """
    img = rounded_plate()
    art, mask = extract_marks()
    target = int(S * 0.80)
    s = min(target / art.width, target / art.height)
    art = art.resize((max(1, int(art.width * s)), max(1, int(art.height * s))), Image.LANCZOS)
    mask = mask.resize(art.size, Image.LANCZOS)
    # dilate + fill holes: silhouette only, one flat green
    solid = mask.filter(ImageFilter.MaxFilter(15)).filter(ImageFilter.MinFilter(7))
    solid = solid.point(lambda v: 255 if v > 128 else 0)
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    layer.paste((22, 104, 86, 255), (0, 0), solid)
    x = (S - art.width) // 2
    y = (S - art.height) // 2
    img.paste(layer, (x, y), layer)
    return img


def dib_entry(img):
    """One classic (BMP/DIB) icon image for `CreateIconFromResourceEx`.

    PIL writes PNG-compressed ICO entries by default, which
    `CreateIconFromResourceEx` rejects — hence the hand-assembled DIB.
    """
    w, h = img.size
    rgba = img.convert("RGBA")
    pixels = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r_, g_, b_, a = rgba.getpixel((x, y))
            pixels += bytes((b_, g_, r_, a))
    mask = bytes(((w + 31) // 32) * 4 * h)
    header = struct.pack(
        "<IiiHHIIiiII",
        40, w, h * 2, 1, 32, 0,
        len(pixels) + len(mask), 0, 0, 0, 0,
    )
    return header + bytes(pixels) + mask


def write_ico(path, images):
    entries = []
    offset = 6 + 16 * len(images)
    data = bytearray()
    for img in images:
        entry = dib_entry(img)
        w, h = img.size
        entries.append(struct.pack(
            "<BBBBHHII",
            w if w < 256 else 0, h if h < 256 else 0,
            0, 0, 1, 32, len(entry), offset,
        ))
        data += entry
        offset += len(entry)
    header = struct.pack("<HHH", 0, 1, len(images))
    with open(path, "wb") as f:
        f.write(header + b"".join(entries) + bytes(data))


def build():
    m = master()
    ms = master_small()
    icon_sizes = [16, 24, 32, 48, 64, 128, 256]
    tray_sizes = [16, 20, 24, 32]

    def at(s):
        return m.resize((s, s), Image.LANCZOS)

    def at_small(s):
        return ms.resize((s, s), Image.LANCZOS)

    m.resize((512, 512), Image.LANCZOS).save(os.path.join(ASSETS, "icon.png"))
    # dedicated source for the White tray glyph: the solid silhouette keeps
    # its shape when thresholded, the textured artwork would turn to noise
    ms.resize((64, 64), Image.LANCZOS).save(os.path.join(ASSETS, "icon_glyph.png"))
    # small sizes use the solid-silhouette variant, the rest the full artwork
    write_ico(os.path.join(ASSETS, "icon.ico"),
              [at_small(16), at_small(24), at_small(32)]
              + [at(s) for s in (48, 64, 128, 256)])
    write_ico(os.path.join(ASSETS, "tray.ico"),
              [at_small(s) for s in tray_sizes])
    print("icon.png / icon.ico / tray.ico written to", ASSETS)


if __name__ == "__main__":
    build()
