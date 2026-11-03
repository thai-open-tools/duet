"""Draws the brand pictures for the installer and the README from duet's own palette and icon.

    python installer/make-art.py

Writes, in light and dark:
  installer/art/wizard-<w>.png      the tall panel on Setup's Welcome and Finished pages
  installer/art/small-<n>.png       the square in the top right corner of the other pages
  docs/brand/banner-<theme>.png     the README header (1280x400)
  docs/brand/social-preview.png     GitHub's link preview (1280x640, dark)
Every picture is drawn once at a large size and shrunk, so the edges stay smooth.
"""
import importlib.util
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ART = ROOT / "installer" / "art"
BRAND = ROOT / "docs" / "brand"
FONTS = Path("C:/Windows/Fonts")

# crates/duet-app/src/palette.rs
DARK = dict(
    bg=0x0E1116, panel=0x151A21, panel2=0x1A2029, panel3=0x202734, line=0x2A3140, text=0xD7DDE7,
    muted=0x8B94A3, dim=0x5D6674, accent=0xE0AA4A, dele=0xE26471, ins=0x4DB986, mod=0xD9A53C,
    moved=0x6E9FE8,
)
LIGHT = dict(
    bg=0xEEF1F5, panel=0xFFFFFF, panel2=0xF5F7FA, panel3=0xECEFF4, line=0xD5DBE4, text=0x232830,
    muted=0x66707E, dim=0x98A1AE, accent=0x9A6A12, dele=0xC23D52, ins=0x1E8A5C, mod=0xA5770F,
    moved=0x35639F,
)
GLOW = 0xE0AA4A  # the icon's amber, in both themes


def rgb(hex_, a=255):
    return ((hex_ >> 16) & 255, (hex_ >> 8) & 255, hex_ & 255, a)


def mix(base, over, t):
    """`over` laid on `base` at opacity t, as one opaque colour (ImageDraw replaces pixels, it never blends)."""
    a, b = rgb(base), rgb(over)
    return tuple(round(x + (y - x) * t) for x, y in zip(a[:3], b[:3])) + (255,)


def font(name, size):
    return ImageFont.truetype(str(FONTS / name), size)


def load_icon():
    sys.dont_write_bytecode = True  # no __pycache__ next to scripts/make-icon.py
    spec = importlib.util.spec_from_file_location("make_icon", ROOT / "scripts" / "make-icon.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.draw


ICON = load_icon()


def gradient(size, top, bottom):
    w, h = size
    im = Image.new("RGBA", size)
    px = ImageDraw.Draw(im)
    for y in range(h):
        t = y / max(1, h - 1)
        c = tuple(round(a + (b - a) * t) for a, b in zip(rgb(top), rgb(bottom)))
        px.line([(0, y), (w, y)], fill=c)
    return im


def glow(im, centre, radius, strength):
    layer = Image.new("RGBA", im.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    cx, cy = centre
    d.ellipse([cx - radius, cy - radius, cx + radius, cy + radius], fill=rgb(GLOW, strength))
    layer = layer.filter(ImageFilter.GaussianBlur(radius * 0.45))
    return Image.alpha_composite(im, layer)


# One pane of the illustration: (indent, length, kind) per row; kind marks the changed rows.
LEFT_ROWS = [(0, .62, None), (1, .48, None), (1, .70, "del"), (1, .40, None), (0, .22, None),
             (0, .58, None), (1, .66, "mod"), (1, .36, None), (0, .50, "moved"), (0, .30, None)]
RIGHT_ROWS = [(0, .62, None), (1, .48, None), (1, .54, "ins"), (1, .40, None), (0, .22, None),
              (0, .50, "moved"), (0, .58, None), (1, .72, "mod"), (1, .36, None), (0, .30, None)]


def pane(d, box, rows, p, s):
    """A rounded editor pane with code-like bars; changed rows get their diff tint."""
    x0, y0, x1, y1 = box
    d.rounded_rectangle(box, radius=round(14 * s), fill=rgb(p["panel"]), outline=rgb(p["line"]),
                        width=max(1, round(2 * s)))
    # title strip with three dots
    d.rounded_rectangle([x0, y0, x1, y0 + round(30 * s)], radius=round(14 * s), fill=rgb(p["panel3"]))
    d.rectangle([x0, y0 + round(16 * s), x1, y0 + round(30 * s)], fill=rgb(p["panel3"]))
    for i in range(3):
        cx = x0 + round((18 + i * 16) * s)
        cy = y0 + round(15 * s)
        r = round(4 * s)
        d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=rgb(p["dim"]))
    top = y0 + round(46 * s)
    row_h = (y1 - top - round(12 * s)) / len(rows)
    centres = []
    for i, (indent, length, kind) in enumerate(rows):
        ry = top + i * row_h
        if kind:
            colour = {"del": p["dele"], "ins": p["ins"], "mod": p["mod"], "moved": p["moved"]}[kind]
            d.rectangle([x0 + round(2 * s), ry, x1 - round(2 * s), ry + row_h - round(2 * s)],
                        fill=mix(p["panel"], colour, 0.16 if p is DARK else 0.13))
            d.rectangle([x0 + round(2 * s), ry, x0 + round(6 * s), ry + row_h - round(2 * s)],
                        fill=rgb(colour))
        bar_x = x0 + round((18 + indent * 18) * s)
        bar_w = (x1 - x0 - round(36 * s)) * length
        bar_h = max(2, round(row_h * 0.30))
        by = ry + (row_h - bar_h) / 2 - round(1 * s)
        colour = mix(p["panel"], p["text"], 0.72) if kind else rgb(p["dim"])
        d.rounded_rectangle([bar_x, by, bar_x + bar_w, by + bar_h], radius=bar_h / 2, fill=colour)
        centres.append(ry + row_h / 2)
    return centres


def link(d, a, b, colour, s):
    """A soft S-curve from a row on the left pane to its partner on the right."""
    (ax, ay), (bx, by) = a, b
    pts = []
    for i in range(41):
        t = i / 40
        x = ax + (bx - ax) * t
        y = ay + (by - ay) * (3 * t * t - 2 * t * t * t)
        pts.append((x, y))
    d.line(pts, fill=colour, width=max(1, round(3 * s)), joint="curve")


def illustration(im, box, p, s):
    d = ImageDraw.Draw(im)
    x0, y0, x1, y1 = box
    gap = round(34 * s)
    w = (x1 - x0 - gap) / 2
    left = (x0, y0, x0 + w, y1)
    right = (x1 - w, y0, x1, y1)
    lc = pane(d, left, LEFT_ROWS, p, s)
    rc = pane(d, right, RIGHT_ROWS, p, s)
    for (li, ri, kind) in [(2, 2, "del"), (6, 7, "mod"), (8, 5, "moved")]:
        colour = {"del": p["dele"], "mod": p["mod"], "moved": p["moved"]}[kind]
        link(d, (left[2], lc[li]), (right[0], rc[ri]), rgb(colour), s)


def wordmark(d, xy, size, p, anchor="mm"):
    d.text(xy, "duet", font=font("seguisb.ttf", size), fill=rgb(p["text"]), anchor=anchor)


def wizard(p, dark):
    """The tall Welcome/Finished panel, drawn at 2x the 250% size, aspect 164:314."""
    W, H = 1010, 1930
    s = W / 505
    im = gradient((W, H), p["panel"], p["bg"])
    im = glow(im, (W // 2, round(300 * s)), round(170 * s), 70 if dark else 28)
    icon = ICON(512).resize((round(190 * s),) * 2, Image.Resampling.LANCZOS)
    im.alpha_composite(icon, (W // 2 - icon.width // 2, round(205 * s)))
    d = ImageDraw.Draw(im)
    wordmark(d, (W // 2, round(470 * s)), round(104 * s), p)
    d.text((W // 2, round(545 * s)), "compare folders & files", font=font("segoeui.ttf", round(30 * s)),
           fill=rgb(p["muted"]), anchor="mm")
    illustration(im, (round(40 * s), round(625 * s), W - round(40 * s), round(880 * s)), p, s * 0.82)
    d.text((W // 2, round(925 * s)), "offline  ·  fast  ·  open source", font=font("segoeui.ttf", round(22 * s)),
           fill=rgb(p["dim"]), anchor="mm")
    return im


def banner(p, dark, size=(1280, 400), tagline=True):
    W, H = size[0] * 2, size[1] * 2
    s = W / 1280
    im = gradient((W, H), p["panel"], p["bg"])
    if dark:
        im = glow(im, (round(170 * s), H // 2), round(170 * s), 60)
    icon = ICON(512).resize((round(140 * s),) * 2, Image.Resampling.LANCZOS)
    im.alpha_composite(icon, (round(100 * s), H // 2 - icon.height // 2))
    d = ImageDraw.Draw(im)
    wordmark(d, (round(270 * s), H // 2 - round(34 * s)), round(110 * s), p, anchor="lm")
    if tagline:
        d.text((round(276 * s), H // 2 + round(44 * s)), "Folders, text, tables, pictures and bytes.",
               font=font("segoeui.ttf", round(26 * s)), fill=rgb(p["muted"]), anchor="lm")
        d.text((round(276 * s), H // 2 + round(80 * s)), "Fast, offline, and at home in Thai.",
               font=font("segoeui.ttf", round(26 * s)), fill=rgb(p["muted"]), anchor="lm")
    box_h = min(H - round(80 * s), round(300 * s))
    illustration(im, (round(840 * s), (H - box_h) // 2, W - round(70 * s), (H + box_h) // 2), p, s * 0.95)
    return im.resize(size, Image.Resampling.LANCZOS)


def main():
    ART.mkdir(parents=True, exist_ok=True)
    BRAND.mkdir(parents=True, exist_ok=True)
    wizard_sizes = [(202, 386), (269, 515), (336, 643), (404, 772), (505, 965)]
    small_sizes = [58, 77, 97, 116, 124, 143, 159]
    for name, p, dark in [("light", LIGHT, False), ("dark", DARK, True)]:
        big = wizard(p, dark)
        for w, h in wizard_sizes:
            big.resize((w, h), Image.Resampling.LANCZOS).save(ART / f"wizard-{name}-{w}.png", optimize=True)
        banner(p, dark).save(BRAND / f"banner-{name}.png", optimize=True)
    icon = ICON(512)
    for n in small_sizes:
        # a little air around the tile, like the built-in image
        pad = round(n * 0.06)
        tile = icon.resize((n - 2 * pad,) * 2, Image.Resampling.LANCZOS)
        canvas = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        canvas.alpha_composite(tile, (pad, pad))
        canvas.save(ART / f"small-{n}.png", optimize=True)
    banner(DARK, True, size=(1280, 640)).save(BRAND / "social-preview.png", optimize=True)
    icon.resize((256, 256), Image.Resampling.LANCZOS).save(BRAND / "icon-256.png", optimize=True)
    print("art written to", ART, "and", BRAND)


if __name__ == "__main__":
    main()
