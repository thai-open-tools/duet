"""Draws duet's app icon: LocalDiff's amber tile with two arrows, both arrows the same length
over the same x span so the pair is centred both ways (180-degree point symmetry).

Each size is drawn on its own pixel grid, not scaled down from one picture: shaft edges land on
whole pixels and every coordinate is mirrored about the centre, so 16 px and 32 px stay sharp
and centred too. Writes crates/duet-app/assets/duet.ico and duet-512.png.

Run: python scripts/make-icon.py   (needs Pillow)
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

AMBER = (224, 170, 74, 255)
INK = (23, 18, 10, 255)
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
# The header's icon, one picture per Windows display scale (crates/duet-app/src/ui/brand.rs).
HEADER_SIZES = [26, 32, 40, 46, 52, 58, 64, 78, 92]
SS = 16  # supersampling factor

ASSETS = Path(__file__).resolve().parent.parent / "crates" / "duet-app" / "assets"


def snap_centre(c, thick):
    """A centre that puts a `thick`-pixel band's edges on whole pixels."""
    return round(c) if thick % 2 == 0 else int(c) + 0.5


def draw(size):
    """The icon at `size` px. Geometry is in a 512 grid, scaled, rounded, then mirrored."""
    k = size / 512
    small = size <= 24
    inset = max(1, round((8 if small else 20) * k)) if size < 512 else 20
    radius = (80 if small else 92) * k
    thick = max(2, round((64 if small else 48) * k))
    head_h = round((168 if small else 120) * k)
    if head_h % 2 != thick % 2:  # equal parity keeps the head centred on the shaft
        head_h += 1
    head_len = max(2, round((104 if small else 92) * k))
    x0 = round((96 if small else 106) * k)
    x1 = size - x0
    top = snap_centre((160 if small else 200) * k, thick)
    bottom = size - top

    s = lambda v: v * SS  # noqa: E731
    full = (size * SS, size * SS)
    # The rasteriser is not exactly symmetric, so the tile is averaged with itself turned
    # 180 degrees, and the left-pointing arrow IS the right-pointing one turned 180 degrees.
    tile = Image.new("RGBA", full, (0, 0, 0, 0))
    ImageDraw.Draw(tile).rounded_rectangle(
        [s(inset), s(inset), s(size - inset) - 1, s(size - inset) - 1],
        radius=s(radius),
        fill=AMBER,
    )
    tile = Image.blend(tile, tile.rotate(180), 0.5)

    arrow = Image.new("RGBA", full, (0, 0, 0, 0))
    g = ImageDraw.Draw(arrow)
    cap = s(thick) / 4
    half_t, half_h = thick / 2, head_h / 2
    g.rounded_rectangle(
        [s(x0), s(top - half_t), s(x1 - head_len) + cap - 1, s(top + half_t) - 1],
        radius=cap,
        fill=INK,
    )
    g.polygon(
        [(s(x1 - head_len), s(top - half_h)), (s(x1), s(top)), (s(x1 - head_len), s(top + half_h))],
        fill=INK,
    )
    assert bottom == size - top
    arrows = Image.alpha_composite(arrow, arrow.rotate(180))
    big = Image.alpha_composite(tile, arrows)
    return big.resize((size, size), Image.Resampling.BOX)


def ink_box(im):
    px = im.load()
    pts = [
        (x, y)
        for y in range(im.height)
        for x in range(im.width)
        if px[x, y][3] > 128 and sum(px[x, y][:3]) < 300
    ]
    xs, ys = [p[0] for p in pts], [p[1] for p in pts]
    return min(xs), min(ys), max(xs), max(ys)


def check(im):
    """Fails unless the picture equals itself turned 180 degrees (so it is centred)."""
    diff = ImageChops.difference(im, im.rotate(180))
    worst = max(band.getextrema()[1] for band in diff.split())
    l, t, r, b = ink_box(im)
    margins = (l, im.width - 1 - r, t, im.height - 1 - b)
    assert worst <= 2, f"{im.width}px is not point-symmetric (max diff {worst})"
    assert margins[0] == margins[1] and margins[2] == margins[3], f"{im.width}px margins {margins}"
    return margins


def main():
    ASSETS.mkdir(parents=True, exist_ok=True)
    frames = {size: draw(size) for size in SIZES}
    for size, im in frames.items():
        print(f"{size:>3} px  arrow margins left/right/top/bottom = {check(im)}")
    header = ASSETS / "header"
    header.mkdir(exist_ok=True)
    for size in HEADER_SIZES:
        im = draw(size)
        print(f"{size:>3} px  arrow margins left/right/top/bottom = {check(im)}")
        im.save(header / f"duet-{size}.png", optimize=True)
    big = draw(512)
    check(big)
    big.save(ASSETS / "duet-512.png")
    largest = frames[SIZES[-1]]
    largest.save(
        ASSETS / "duet.ico",
        format="ICO",
        sizes=[(s, s) for s in SIZES],
        append_images=[frames[s] for s in SIZES[:-1]],
    )
    # Read every frame back: each must be the picture drawn for its size, not a rescale.
    ico = Image.open(ASSETS / "duet.ico")
    for size in SIZES:
        ico.size = (size, size)
        frame = ico.convert("RGBA")
        diff = ImageChops.difference(frame, frames[size])
        worst = max(band.getextrema()[1] for band in diff.split())
        assert worst <= 2, f"the {size}px frame in duet.ico is not the drawn one ({worst})"
    print("wrote", ASSETS / "duet.ico", "and", ASSETS / "duet-512.png")


if __name__ == "__main__":
    main()
