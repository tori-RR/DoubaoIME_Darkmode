import struct
from io import BytesIO
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    raise SystemExit("need Pillow")

root = Path(__file__).resolve().parents[1] / "src-tauri" / "icons"
src_path = root / "icon-source.png"
ICO_SIZES = [(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)]
SMALL_MAX = 32
BLACK_SUM = 16
PAD_RATIO = 0.08

TRANSPARENT = (0, 0, 0, 0)
PANEL = (22, 24, 28, 255)
BOX = (37, 38, 42, 255)
HIGH = (85, 88, 93, 255)
TEXT = (196, 192, 188, 255)
YELLOW = (236, 170, 22, 255)

FONT = {
    "1": (".#.", "##.", ".#.", ".#.", ".#.", "###"),
    "2": ("##.", "..#", ".#.", "#..", "#..", "###"),
    "a": ("...", ".##", "#.#", "###", "#.#", "#.#"),
    "d": ("..#", "..#", ".##", "#.#", "#.#", ".##"),
    "k": ("#..", "#..", "#.#", "##.", "#.#", "#.#"),
    "r": ("...", "##.", "#.#", "#..", "#..", "#.."),
}


def new_canvas(size: int) -> Image.Image:
    return Image.new("RGBA", (size, size), TRANSPARENT)


def put(px, x: int, y: int, color, w: int, h: int) -> None:
    if 0 <= x < w and 0 <= y < h:
        px[x, y] = color


def fill_round(px, x0: int, y0: int, x1: int, y1: int, color, w: int, h: int, cut: int = 1) -> None:
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            skip = False
            for dx, dy in ((x - x0, y - y0), (x1 - x, y - y0), (x - x0, y1 - y), (x1 - x, y1 - y)):
                if dx < cut and dy < cut and (cut - 1 - dx) + (cut - 1 - dy) >= (cut - 1):
                    skip = True
                    break
            if not skip:
                put(px, x, y, color, w, h)


def blit_glyph(px, ch: str, ox: int, oy: int, color, w: int, h: int) -> int:
    rows = FONT[ch]
    for y, row in enumerate(rows):
        for x, bit in enumerate(row):
            if bit in "1#":
                put(px, ox + x, oy + y, color, w, h)
    return len(rows[0])


def blit_text(px, text: str, ox: int, oy: int, color, w: int, h: int, gap: int = 1) -> None:
    x = ox
    for ch in text:
        x += blit_glyph(px, ch, x, oy, color, w, h) + gap


def draw_16() -> Image.Image:
    im = new_canvas(16)
    px = im.load()
    fill_round(px, 1, 1, 9, 6, BOX, 16, 16, cut=1)
    for x, y in ((3, 3), (3, 4), (4, 4), (5, 3), (5, 4), (6, 4)):
        put(px, x, y, TEXT, 16, 16)
    put(px, 8, 2, YELLOW, 16, 16)
    put(px, 8, 3, YELLOW, 16, 16)
    put(px, 8, 4, YELLOW, 16, 16)
    put(px, 8, 5, YELLOW, 16, 16)
    fill_round(px, 1, 8, 14, 14, PANEL, 16, 16, cut=1)
    fill_round(px, 2, 9, 9, 13, HIGH, 16, 16, cut=1)
    return im


def draw_32() -> Image.Image:
    im = new_canvas(32)
    px = im.load()
    fill_round(px, 1, 3, 20, 14, BOX, 32, 32, cut=2)
    blit_text(px, "dark", 3, 6, TEXT, 32, 32, gap=1)
    for y in range(5, 13):
        put(px, 18, y, YELLOW, 32, 32)
    fill_round(px, 1, 16, 30, 28, PANEL, 32, 32, cut=2)
    fill_round(px, 2, 18, 18, 26, HIGH, 32, 32, cut=2)
    blit_text(px, "1dark", 3, 19, TEXT, 32, 32, gap=1)
    blit_text(px, "2", 23, 19, TEXT, 32, 32, gap=1)
    return im


def knock_out_black(im: Image.Image) -> Image.Image:
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, _a = px[x, y]
            if r + g + b <= BLACK_SUM:
                px[x, y] = TRANSPARENT
    return im


def content_bbox(im: Image.Image) -> tuple[int, int, int, int]:
    px = im.load()
    w, h = im.size
    x0, y0, x1, y1 = w, h, -1, -1
    for y in range(h):
        for x in range(w):
            if px[x, y][3] > 0:
                if x < x0:
                    x0 = x
                if y < y0:
                    y0 = y
                if x > x1:
                    x1 = x
                if y > y1:
                    y1 = y
    if x1 < 0:
        raise SystemExit("no opaque pixels")
    return x0, y0, x1 + 1, y1 + 1


def square_pad(im: Image.Image) -> Image.Image:
    x0, y0, x1, y1 = content_bbox(im)
    cropped = im.crop((x0, y0, x1, y1))
    cw, ch = cropped.size
    pad = max(4, int(max(cw, ch) * PAD_RATIO))
    side = max(cw, ch) + pad * 2
    canvas = Image.new("RGBA", (side, side), TRANSPARENT)
    canvas.paste(cropped, ((side - cw) // 2, (side - ch) // 2), cropped)
    return canvas


def scale_nearest(im: Image.Image, size: int) -> Image.Image:
    return im.resize((size, size), Image.Resampling.NEAREST)


def scale_source(im: Image.Image, size: int) -> Image.Image:
    return im.resize((size, size), Image.Resampling.LANCZOS)


def icon_for(size: int, sprite16: Image.Image, sprite32: Image.Image, art: Image.Image) -> Image.Image:
    if size < 32:
        return scale_nearest(sprite16, size)
    if size <= SMALL_MAX:
        return scale_nearest(sprite32, size)
    return scale_source(art, size)


def save_ico(path: Path, images: list[Image.Image]) -> None:
    blobs = []
    for im in images:
        buf = BytesIO()
        im.convert("RGBA").save(buf, format="PNG")
        w, h = im.size
        blobs.append((w, h, buf.getvalue()))
    offset = 6 + 16 * len(blobs)
    entries = b""
    data = b""
    for w, h, blob in blobs:
        entries += struct.pack(
            "<BBBBHHII",
            0 if w >= 256 else w,
            0 if h >= 256 else h,
            0,
            0,
            0,
            32,
            len(blob),
            offset,
        )
        data += blob
        offset += len(blob)
    path.write_bytes(struct.pack("<HHH", 0, 1, len(blobs)) + entries + data)


def main() -> None:
    if not src_path.exists():
        raise SystemExit(f"missing {src_path}")
    art = square_pad(knock_out_black(Image.open(src_path)))
    sprite16 = draw_16()
    sprite32 = draw_32()
    frames = [icon_for(size, sprite16, sprite32, art) for size, _ in ICO_SIZES]
    icon_for(32, sprite16, sprite32, art).save(root / "32x32.png")
    icon_for(128, sprite16, sprite32, art).save(root / "128x128.png")
    icon_for(256, sprite16, sprite32, art).save(root / "128x128@2x.png")
    save_ico(root / "icon.ico", frames)
    sprite32.save(root / "icon-pixel-32.png")
    print(root)
    print(f"small <= {SMALL_MAX}px pixel sprites; large from {src_path.name}")


if __name__ == "__main__":
    main()
