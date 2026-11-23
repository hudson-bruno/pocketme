#!/usr/bin/env python3
"""Turns a game's tile picture, drawn as text, into the card file ART.BIN.

  make_art.py SOURCE OUT.BIN [--png PREVIEW.png]   make the card file
  make_art.py --check SOURCE                       only check the source
  make_art.py --show ART.BIN PREVIEW.png           a picture of a card file

No outside Python packages. The preview is a PNG, 4 times bigger, in the
colours the panel really shows (RGB565, as the console sends them).

THE SOURCE (games/<game>/art.txt): plain text a person can edit, or a
script can write.

    # A comment, to the end of the line. Blank lines are skipped.
    size 36 36            the picture's width and height (the console
                          takes 36 x 36 today: console-core theme::TILE)
    colour . 0B0F1A       a letter and its colour as RRGGBB; at most 16
    colour a F7D154
    pixels                then one line per pixel row, one letter per
    ....aaaa....          pixel (exactly `width` letters on each line,
    ...                   exactly `height` lines)

A letter is any one printable character except a space or '#'. The
colours are numbered in the order they are written (0 first).

THE CARD FILE (ART.BIN, version 1), every number little-endian:

    offset  size  what
    0       3     the letters "ART"
    3       1     the format version: 1
    4       1     width in pixels  (the console takes 36 today)
    5       1     height in pixels (36)
    6       1     how many colours: 1 to 16
    7       1     bits per pixel: 4
    8       2*n   the colours, RGB565 (5 bits red, 6 green, 5 blue)
    8+2n    ...   the pixels, row by row from the top; each row is
                  ceil(width / 2) bytes; two pixels per byte, the left
                  one in the high 4 bits; each pixel is a colour number
                  (below n). A spare half byte at the end of a row is 0.

The file is exactly 8 + 2n + height * ceil(width / 2) bytes; nothing may
follow. The console reads it only when the file's size is exactly that,
and shows the old letter tile for anything else (crates/console-core,
src/art.rs). 36 x 36 with 16 colours: 8 + 32 + 648 = 688 bytes.
"""
import struct
import sys
import zlib

MAGIC = b"ART"
VERSION = 1
BITS = 4
MAX_COLOURS = 16


class ArtError(Exception):
    pass


def rgb565(hex6):
    """0xRRGGBB to RGB565, as console-core's theme::rgb does."""
    r, g, b = (hex6 >> 16) & 0xFF, (hex6 >> 8) & 0xFF, hex6 & 0xFF
    return ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)


def rgb888(c565):
    """RGB565 to 8-bit red, green, blue (the low bits repeated)."""
    r, g, b = (c565 >> 11) & 0x1F, (c565 >> 5) & 0x3F, c565 & 0x1F
    return (r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)


def parse(text, name="source"):
    """Reads the text source. Returns (width, height, colours565, rows of
    colour numbers)."""
    size = None
    letters = {}
    colours = []
    rows = []
    in_pixels = False
    for no, raw in enumerate(text.splitlines(), 1):
        where = f"{name}:{no}"
        line = raw.split("#", 1)[0]  # '#' is never a letter: a comment to the end
        if in_pixels:
            if not line.strip():
                continue
            row = line.strip()
            if len(row) != size[0]:
                raise ArtError(f"{where}: {len(row)} pixels in this row, want {size[0]}")
            try:
                rows.append([letters[ch] for ch in row])
            except KeyError as e:
                raise ArtError(f"{where}: letter {e} has no colour line")
            continue
        words = line.split()
        if not words:
            continue
        if words[0] == "size" and len(words) == 3:
            w, h = int(words[1]), int(words[2])
            if not (1 <= w <= 255 and 1 <= h <= 255):
                raise ArtError(f"{where}: size must be 1 to 255")
            size = (w, h)
        elif words[0] == "colour" and len(words) == 3:
            ch, hx = words[1], words[2]
            if len(ch) != 1 or ch == "#":
                raise ArtError(f"{where}: a colour letter is one character, not '#'")
            if ch in letters:
                raise ArtError(f"{where}: letter {ch!r} given twice")
            if len(hx) != 6:
                raise ArtError(f"{where}: colour must be RRGGBB")
            try:
                value = int(hx, 16)
            except ValueError:
                raise ArtError(f"{where}: colour must be RRGGBB")
            if len(colours) >= MAX_COLOURS:
                raise ArtError(f"{where}: more than {MAX_COLOURS} colours")
            letters[ch] = len(colours)
            colours.append(rgb565(value))
        elif words[0] == "pixels" and len(words) == 1:
            if size is None:
                raise ArtError(f"{where}: 'size' must come before 'pixels'")
            if not colours:
                raise ArtError(f"{where}: no colours before 'pixels'")
            in_pixels = True
        else:
            raise ArtError(f"{where}: cannot read this line: {line.strip()!r}")
    if not in_pixels:
        raise ArtError(f"{name}: no 'pixels' line")
    if len(rows) != size[1]:
        raise ArtError(f"{name}: {len(rows)} pixel rows, want {size[1]}")
    return size[0], size[1], colours, rows


def encode(width, height, colours, rows):
    """The ART.BIN bytes."""
    out = bytearray(MAGIC)
    out += bytes([VERSION, width, height, len(colours), BITS])
    for c in colours:
        out += struct.pack("<H", c)
    for row in rows:
        padded = row + [0] * (len(row) % 2)
        for i in range(0, len(padded), 2):
            out.append((padded[i] << 4) | padded[i + 1])
    return bytes(out)


def decode(data):
    """Reads ART.BIN bytes with the same checks as the console (any size,
    though). Returns (width, height, colours565, rows)."""
    if len(data) < 8:
        raise ArtError("shorter than the 8-byte header")
    if data[:3] != MAGIC:
        raise ArtError("does not start with ART")
    version, w, h, n, bits = data[3:8]
    if version != VERSION:
        raise ArtError(f"version {version}, this tool reads {VERSION}")
    if bits != BITS or not 1 <= n <= MAX_COLOURS or w == 0 or h == 0:
        raise ArtError("bad header")
    row_bytes = (w + 1) // 2
    want = 8 + 2 * n + h * row_bytes
    if len(data) != want:
        raise ArtError(f"{len(data)} bytes, the header says {want}")
    colours = list(struct.unpack_from(f"<{n}H", data, 8))
    rows = []
    at = 8 + 2 * n
    for _ in range(h):
        row = []
        for i in range(w):
            byte = data[at + i // 2]
            v = byte >> 4 if i % 2 == 0 else byte & 0xF
            if v >= n:
                raise ArtError(f"pixel colour {v}, but only {n} colours")
            row.append(v)
        rows.append(row)
        at += row_bytes
    return w, h, colours, rows


def png(path, width, height, colours, rows, scale=4):
    """Writes a preview PNG (8-bit RGB, no outside packages)."""
    raw = bytearray()
    for row in rows:
        line = bytearray()
        for v in row:
            line += bytes(rgb888(colours[v])) * scale
        for _ in range(scale):
            raw.append(0)  # filter: none
            raw += line

    def chunk(kind, body):
        crc = zlib.crc32(kind + body) & 0xFFFFFFFF
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", crc)

    head = struct.pack(">IIBBBBB", width * scale, height * scale, 8, 2, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", head))
        f.write(chunk(b"IDAT", zlib.compress(bytes(raw), 9)))
        f.write(chunk(b"IEND", b""))


def main(argv):
    try:
        if len(argv) == 2 and argv[0] == "--check":
            with open(argv[1], encoding="utf-8") as f:
                w, h, colours, _ = parse(f.read(), argv[1])
            print(f"make_art: {argv[1]}: ok, {w} x {h}, {len(colours)} colours")
            return 0
        if len(argv) == 3 and argv[0] == "--show":
            with open(argv[1], "rb") as f:
                art = decode(f.read())
            png(argv[2], *art)
            return 0
        if len(argv) in (2, 4) and not argv[0].startswith("--"):
            src, out = argv[0], argv[1]
            with open(src, encoding="utf-8") as f:
                art = parse(f.read(), src)
            data = encode(*art)
            assert decode(data) == art
            with open(out, "wb") as f:
                f.write(data)
            if len(argv) == 4:
                if argv[2] != "--png":
                    sys.exit(__doc__)
                png(argv[3], *art)
            return 0
    except (ArtError, OSError, ValueError) as e:
        print(f"make_art: {e}", file=sys.stderr)
        return 1
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
