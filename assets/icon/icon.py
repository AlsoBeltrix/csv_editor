"""The CSV Editor icon: a green document with a folded corner, a white grid, one amber cell.

This file is the editable source. Run it to regenerate every committed asset next to it:

    python assets/icon/icon.py

Outputs: csv-editor.svg (vector, for browsers), csv-editor-<size>.png (16 to 512),
csv-editor.ico (16 to 256, every size rendered on its own, not downscaled), favicon.ico (16, 32,
48) and contact-sheet.png (every size on light and dark, for review). Builds consume the committed
files and never run this script; Python with Pillow is the only tool it needs.

The geometry lives in a unit square. Sizes of 24 px and under use the simplified form: a 2x2 grid
with thicker lines, which is what still reads at favicon size. Rendering supersamples 8x and
downscales with Lanczos.
"""
from pathlib import Path

from PIL import Image, ImageDraw

GREEN = (31, 140, 76)
GREEN_LIGHT = (84, 190, 122)
WHITE = (255, 255, 255)
AMBER = (255, 184, 28)

SUPERSAMPLE = 8
PNG_SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
FAVICON_SIZES = [16, 32, 48]
SMALL = 24  # at this size and under, the simplified form

# Geometry (unit square). The page is narrower than tall, like paper.
MARGIN_Y = 0.06
MARGIN_X = 0.14
FOLD = 0.26
GRID_INSET_X = 0.10
GRID_TOP = 0.34
GRID_BOTTOM_INSET = 0.10


def geometry(small):
    """Shapes as (kind, color, points-or-box) in unit coordinates."""
    x0, y0, x1, y1 = MARGIN_X, MARGIN_Y, 1 - MARGIN_X, 1 - MARGIN_Y
    shapes = [
        ("polygon", GREEN, [(x0, y0), (x1 - FOLD, y0), (x1, y0 + FOLD), (x1, y1), (x0, y1)]),
        ("polygon", GREEN_LIGHT, [(x1 - FOLD, y0), (x1 - FOLD, y0 + FOLD), (x1, y0 + FOLD)]),
    ]
    gx0, gy0, gx1, gy1 = x0 + GRID_INSET_X, y0 + GRID_TOP, x1 - GRID_INSET_X, y1 - GRID_BOTTOM_INSET
    cols = rows = 2 if small else 3
    line = 0.06 if small else 0.035
    cw, rh = (gx1 - gx0) / cols, (gy1 - gy0) / rows
    shapes.append(("rect", WHITE, (gx0, gy0, gx1, gy1)))
    hc, hr = (1, 0) if small else (1, 1)
    shapes.append(("rect", AMBER, (gx0 + hc * cw, gy0 + hr * rh, gx0 + (hc + 1) * cw, gy0 + (hr + 1) * rh)))
    for i in range(1, cols):
        x = gx0 + i * cw
        shapes.append(("rect", GREEN, (x - line / 2, gy0, x + line / 2, gy1)))
    for j in range(1, rows):
        y = gy0 + j * rh
        shapes.append(("rect", GREEN, (gx0, y - line / 2, gx1, y + line / 2)))
    return shapes


def render(size):
    s = size * SUPERSAMPLE
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    for kind, color, geom in geometry(size <= SMALL):
        if kind == "polygon":
            d.polygon([(x * s, y * s) for x, y in geom], fill=color)
        else:
            x0, y0, x1, y1 = geom
            d.rectangle([x0 * s, y0 * s, x1 * s, y1 * s], fill=color)
    return img.resize((size, size), Image.LANCZOS)


def svg():
    """The full-size form as SVG; a browser scales it to any size."""
    def rgb(c):
        return "#%02x%02x%02x" % c

    parts = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256">']
    for kind, color, geom in geometry(False):
        if kind == "polygon":
            pts = " ".join(f"{x * 256:.1f},{y * 256:.1f}" for x, y in geom)
            parts.append(f'  <polygon fill="{rgb(color)}" points="{pts}"/>')
        else:
            x0, y0, x1, y1 = (v * 256 for v in geom)
            parts.append(f'  <rect fill="{rgb(color)}" x="{x0:.1f}" y="{y0:.1f}" width="{x1 - x0:.1f}" height="{y1 - y0:.1f}"/>')
    parts.append("</svg>\n")
    return "\n".join(parts)


def write_ico(path, sizes):
    # Largest first: Pillow drops any requested size larger than the first image.
    images = [render(s) for s in sorted(sizes, reverse=True)]
    images[0].save(path, format="ICO", sizes=[(s, s) for s in sizes], append_images=images[1:])


def contact_sheet(path):
    pad, label = 24, 28
    sizes = PNG_SIZES[:-1]
    row_h = sizes[-1] + 40
    width = pad * 4 + sum(sizes) + pad * (len(sizes) - 1)
    img = Image.new("RGB", (width, pad + 2 * (label + row_h) + pad), (236, 236, 236))
    d = ImageDraw.Draw(img)
    y = pad
    for bg, name in [((255, 255, 255), "light"), ((32, 33, 36), "dark")]:
        d.rectangle([pad, y, width - pad, y + label + row_h], fill=bg)
        d.text((pad + 8, y + 6), f"CSV Editor icon on {name}", fill=(90, 90, 90) if name == "light" else (200, 200, 200))
        x = pad * 2
        for size in sizes:
            ic = render(size)
            img.paste(ic, (x, y + label + (row_h - 40 - size) // 2 + 10), ic)
            d.text((x, y + label + row_h - 30), str(size), fill=(120, 120, 120))
            x += size + pad
        y += label + row_h
    img.save(path)


def main():
    here = Path(__file__).resolve().parent
    for size in PNG_SIZES:
        render(size).save(here / f"csv-editor-{size}.png")
    (here / "csv-editor.svg").write_text(svg(), encoding="utf-8", newline="\n")
    write_ico(here / "csv-editor.ico", ICO_SIZES)
    write_ico(here / "favicon.ico", FAVICON_SIZES)
    contact_sheet(here / "contact-sheet.png")
    for name in ["csv-editor.ico", "favicon.ico"]:
        with Image.open(here / name) as ico:
            print(name, sorted(ico.info["sizes"]))
    print("wrote", len(PNG_SIZES), "PNGs, SVG, 2 ICOs, contact sheet in", here)


if __name__ == "__main__":
    main()
