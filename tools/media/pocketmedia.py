#!/usr/bin/env python3
"""pocketmedia: small PNG stills and animated GIF clips from simulator pictures.

The simulator saves 320x240 PNG pictures (frame-00000.png, ...). This tool
turns them into small files for the documentation:

    still IN.png OUT.png [--scale N]          a small PNG (palette when it can)
    gif OUT.gif --dir D --from A --to B ...   an animated GIF89a clip
    gif OUT.gif FILE.png FILE.png ...         the same from a list of files
    info FILE ...                             size, colours, frames, length

Python standard library only (no Pillow, no ffmpeg). Run it as
"python3 -I pocketmedia.py ...".

Pixels are kept as plain ints 0xRRGGBB in a flat list, row by row.
"""

import argparse
import os
import struct
import sys
import zlib
from array import array
from collections import Counter
from fractions import Fraction

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

# array('I') is four bytes on every usual platform; the fast pixel
# conversions below rely on that and fall back to a loop otherwise.
_FAST = array("I").itemsize == 4
_LITTLE = sys.byteorder == "little"


class MediaError(Exception):
    """A clear, user-facing problem with an input file or an option."""


class Image:
    """A picture: width, height and a flat list of 0xRRGGBB ints."""

    __slots__ = ("width", "height", "pixels")

    def __init__(self, width, height, pixels):
        if width < 1 or height < 1:
            raise MediaError(f"picture size {width}x{height} is empty")
        if len(pixels) != width * height:
            raise MediaError(
                f"picture {width}x{height} needs {width * height} pixels, got {len(pixels)}"
            )
        self.width = width
        self.height = height
        self.pixels = pixels

    def __eq__(self, other):
        return (
            isinstance(other, Image)
            and self.width == other.width
            and self.height == other.height
            and self.pixels == other.pixels
        )

    def __repr__(self):
        return f"Image({self.width}x{self.height})"


# ---------------------------------------------------------------------------
# Pixel conversions


def rgb_bytes_to_ints(raw):
    """b'RGBRGB...' -> [0xRRGGBB, ...]."""
    n = len(raw) // 3
    if not _FAST:
        return [(raw[3 * i] << 16) | (raw[3 * i + 1] << 8) | raw[3 * i + 2] for i in range(n)]
    buf = bytearray(4 * n)
    # Slice assignment runs in C; the byte order decides where R, G, B go
    # inside the native 32-bit word.
    if _LITTLE:
        buf[0::4] = raw[2::3]
        buf[1::4] = raw[1::3]
        buf[2::4] = raw[0::3]
    else:
        buf[1::4] = raw[0::3]
        buf[2::4] = raw[1::3]
        buf[3::4] = raw[2::3]
    words = array("I")
    words.frombytes(bytes(buf))
    return words.tolist()


def ints_to_rgb_bytes(pixels):
    """[0xRRGGBB, ...] -> b'RGBRGB...'."""
    if not _FAST:
        out = bytearray()
        for p in pixels:
            out += bytes(((p >> 16) & 255, (p >> 8) & 255, p & 255))
        return bytes(out)
    buf = array("I", pixels).tobytes()
    out = bytearray(3 * len(pixels))
    if _LITTLE:
        out[0::3] = buf[2::4]
        out[1::3] = buf[1::4]
        out[2::3] = buf[0::4]
    else:
        out[0::3] = buf[1::4]
        out[1::3] = buf[2::4]
        out[2::3] = buf[3::4]
    return bytes(out)


def scale_image(image, n):
    """Whole-number nearest-neighbour scale: every pixel becomes an n x n block."""
    if n == 1:
        return image
    if n < 1:
        raise MediaError(f"scale must be 1 or more, not {n}")
    w = image.width
    px = image.pixels
    out = []
    for y in range(image.height):
        row = px[y * w:(y + 1) * w]
        wide = [p for p in row for _ in range(n)]
        for _ in range(n):
            out.extend(wide)
    return Image(w * n, image.height * n, out)


# ---------------------------------------------------------------------------
# PNG reading

_COLOUR_TYPES = {0: ("grey", 1), 2: ("RGB", 3), 3: ("palette", 1), 6: ("RGBA", 4)}


def _paeth(a, b, c):
    p = a + b - c
    pa = abs(p - a)
    pb = abs(p - b)
    pc = abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    if pb <= pc:
        return b
    return c


def _unfilter(raw, height, stride, bpp, name):
    """Undo the per-row PNG filters. raw has one filter byte before each row."""
    out = bytearray(height * stride)
    prev = bytearray(stride)  # the row "above" the first row is all zero
    pos = 0
    for y in range(height):
        ftype = raw[pos]
        line = bytearray(raw[pos + 1:pos + 1 + stride])
        pos += stride + 1
        if ftype == 0:
            pass
        elif ftype == 1:  # Sub: add the byte one pixel to the left
            for i in range(bpp, stride):
                line[i] = (line[i] + line[i - bpp]) & 255
        elif ftype == 2:  # Up: add the byte above
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif ftype == 3:  # Average: floor((left + up) / 2), left is 0 at the edge
            for i in range(bpp):
                line[i] = (line[i] + (prev[i] >> 1)) & 255
            for i in range(bpp, stride):
                line[i] = (line[i] + ((line[i - bpp] + prev[i]) >> 1)) & 255
        elif ftype == 4:  # Paeth: left and upper-left are 0 at the edge
            for i in range(bpp):
                line[i] = (line[i] + prev[i]) & 255
            for i in range(bpp, stride):
                line[i] = (line[i] + _paeth(line[i - bpp], prev[i], prev[i - bpp])) & 255
        else:
            raise MediaError(f"{name}: row {y} has unknown filter type {ftype}")
        out[y * stride:(y + 1) * stride] = line
        prev = line
    return bytes(out)


def decode_png(data, name="picture"):
    """Decode PNG bytes into an Image. Accepts 8-bit grey, RGB, palette, RGBA."""
    if data[:8] != PNG_SIGNATURE:
        raise MediaError(f"{name}: not a PNG file (bad signature)")
    pos = 8
    ihdr = None
    plte = None
    idat = []
    while True:
        if pos + 12 > len(data):
            raise MediaError(f"{name}: file ends before the IEND chunk")
        length, ctype = struct.unpack(">I4s", data[pos:pos + 8])
        end = pos + 8 + length
        if end + 4 > len(data):
            raise MediaError(f"{name}: chunk {ctype!r} is cut short")
        body = data[pos + 8:end]
        (crc,) = struct.unpack(">I", data[end:end + 4])
        if zlib.crc32(ctype + body) != crc:
            raise MediaError(f"{name}: bad CRC in chunk {ctype.decode('latin-1')}")
        pos = end + 4
        if ctype == b"IHDR":
            ihdr = body
        elif ctype == b"PLTE":
            plte = body
        elif ctype == b"IDAT":
            idat.append(body)
        elif ctype == b"IEND":
            break
        elif not ctype[0] & 0x20:
            # Upper-case first letter = critical chunk we do not understand.
            raise MediaError(f"{name}: unknown critical chunk {ctype.decode('latin-1')}")
    if ihdr is None or len(ihdr) != 13:
        raise MediaError(f"{name}: missing or bad IHDR chunk")
    w, h, depth, colour, comp, filt, interlace = struct.unpack(">IIBBBBB", ihdr)
    if colour not in _COLOUR_TYPES:
        kinds = {4: "grey with alpha"}
        raise MediaError(
            f"{name}: colour type {colour} ({kinds.get(colour, 'unknown')}) is not supported; "
            "only 8-bit grey (0), RGB (2), palette (3) and RGBA (6)"
        )
    if depth != 8:
        raise MediaError(f"{name}: bit depth {depth} is not supported; only 8 bits per sample")
    if interlace != 0:
        raise MediaError(f"{name}: interlaced PNG is not supported")
    if comp != 0 or filt != 0:
        raise MediaError(f"{name}: unknown compression or filter method")
    if w < 1 or h < 1:
        raise MediaError(f"{name}: empty picture {w}x{h}")
    if not idat:
        raise MediaError(f"{name}: no IDAT chunk")
    try:
        raw = zlib.decompress(b"".join(idat))
    except zlib.error as e:
        raise MediaError(f"{name}: broken compressed data ({e})") from None
    bpp = _COLOUR_TYPES[colour][1]
    stride = w * bpp
    if len(raw) != h * (stride + 1):
        raise MediaError(
            f"{name}: picture data is {len(raw)} bytes, expected {h * (stride + 1)}"
        )
    flat = _unfilter(raw, h, stride, bpp, name)
    n = w * h
    if colour == 2:
        pixels = rgb_bytes_to_ints(flat)
    elif colour == 0:
        rgb = bytearray(3 * n)
        rgb[0::3] = flat
        rgb[1::3] = flat
        rgb[2::3] = flat
        pixels = rgb_bytes_to_ints(rgb)
    elif colour == 3:
        if plte is None or len(plte) % 3 or not plte:
            raise MediaError(f"{name}: palette picture without a valid PLTE chunk")
        pal = rgb_bytes_to_ints(plte)
        if max(flat) >= len(pal):
            raise MediaError(f"{name}: pixel index {max(flat)} is past the palette's end")
        pixels = [pal[i] for i in flat]
    else:  # RGBA
        alpha = flat[3::4]
        rgb = bytearray(3 * n)
        rgb[0::3] = flat[0::4]
        rgb[1::3] = flat[1::4]
        rgb[2::3] = flat[2::4]
        if alpha.count(255) != n:
            # GIF has no partial transparency: put see-through pixels on black.
            print(f"note: {name}: has see-through pixels, put on black", file=sys.stderr)
            for i in range(n):
                a = alpha[i]
                if a != 255:
                    for j in range(3 * i, 3 * i + 3):
                        rgb[j] = (rgb[j] * a + 127) // 255
        pixels = rgb_bytes_to_ints(rgb)
    return Image(w, h, pixels)


def read_png(path):
    try:
        with open(path, "rb") as f:
            data = f.read()
    except OSError as e:
        raise MediaError(f"cannot read {path}: {e.strerror}") from None
    return decode_png(data, path)


# ---------------------------------------------------------------------------
# PNG writing


def _png_chunk(ctype, body):
    return struct.pack(">I", len(body)) + ctype + body + struct.pack(">I", zlib.crc32(ctype + body))


def _filter_rows(flat, height, stride, bpp, ftype):
    """Apply one PNG filter type to every row; returns the bytes to compress."""
    out = bytearray()
    prev = bytes(stride)
    for y in range(height):
        line = flat[y * stride:(y + 1) * stride]
        out.append(ftype)
        if ftype == 0:
            out += line
        else:
            f = bytearray(stride)
            for i in range(stride):
                a = line[i - bpp] if i >= bpp else 0
                b = prev[i]
                if ftype == 1:
                    f[i] = (line[i] - a) & 255
                elif ftype == 2:
                    f[i] = (line[i] - b) & 255
                elif ftype == 3:
                    f[i] = (line[i] - ((a + b) >> 1)) & 255
                else:
                    c = prev[i - bpp] if i >= bpp else 0
                    f[i] = (line[i] - _paeth(a, b, c)) & 255
            out += f
        prev = line
    return bytes(out)


def encode_png(image):
    """A small PNG: palette (type 3) for at most 256 colours, else RGB (type 2)."""
    w, h, px = image.width, image.height, image.pixels
    colours = set(px)
    if len(colours) <= 256:
        palette = sorted(colours)
        index = {c: i for i, c in enumerate(palette)}
        flat = bytes(map(index.__getitem__, px))
        colour, bpp = 3, 1
        # The PNG specification advises filter "none" for palette pictures.
        filters = (0,)
    else:
        palette = None
        flat = ints_to_rgb_bytes(px)
        colour, bpp = 2, 3
        filters = (0, 1, 2, 3, 4)
    best = None
    for ftype in filters:
        z = zlib.compress(_filter_rows(flat, h, w * bpp, bpp, ftype), 9)
        if best is None or len(z) < len(best):
            best = z
    out = bytearray(PNG_SIGNATURE)
    out += _png_chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, colour, 0, 0, 0))
    if palette is not None:
        out += _png_chunk(b"PLTE", ints_to_rgb_bytes(palette))
    out += _png_chunk(b"IDAT", best)
    out += _png_chunk(b"IEND", b"")
    return bytes(out)


def write_file(path, data):
    try:
        with open(path, "wb") as f:
            f.write(data)
    except OSError as e:
        raise MediaError(f"cannot write {path}: {e.strerror}") from None


# ---------------------------------------------------------------------------
# Colour reduction (only for a GIF frame with more than 256 colours)

_MERGE_POOL = 512  # drop low bits until at most this many colours remain


def reduce_colours(pixels, limit=256):
    """Map the pixels onto at most `limit` colours.

    Step 1 drops low bits of every channel (grouping close colours, each group
    becomes the weighted average of its members) until at most _MERGE_POOL
    colours are left. Step 2 merges the least used colour into its nearest
    neighbour until `limit` remain. Returns (new pixels, short note) or
    (pixels, None) when nothing had to change.
    """
    counts = Counter(pixels)
    if len(counts) <= limit:
        return pixels, None
    bits = 0
    while True:
        keep = ((0xFF << bits) & 0xFF) * 0x010101
        groups = {}
        for c in counts:
            groups.setdefault(c & keep, []).append(c)
        if len(groups) <= _MERGE_POOL:
            break
        bits += 1
    # One entry per group: [r_sum, g_sum, b_sum, count]
    reps = []
    owner = {}
    for members in groups.values():
        gi = len(reps)
        r = g = b = n = 0
        for c in members:
            k = counts[c]
            r += (c >> 16) * k
            g += ((c >> 8) & 255) * k
            b += (c & 255) * k
            n += k
            owner[c] = gi
        reps.append([r, g, b, n])

    def colour_of(e):
        n = e[3]
        return (
            ((e[0] + n // 2) // n) << 16 | ((e[1] + n // 2) // n) << 8 | ((e[2] + n // 2) // n)
        )

    alive = list(range(len(reps)))
    parent = list(range(len(reps)))
    merged = 0
    while len(alive) > limit:
        small = min(alive, key=lambda i: reps[i][3])
        cs = colour_of(reps[small])
        sr, sg, sb = cs >> 16, (cs >> 8) & 255, cs & 255
        best = None
        best_d = None
        for j in alive:
            if j == small:
                continue
            cj = colour_of(reps[j])
            dr = sr - (cj >> 16)
            dg = sg - ((cj >> 8) & 255)
            db = sb - (cj & 255)
            d = dr * dr + dg * dg + db * db
            if best_d is None or d < best_d:
                best, best_d = j, d
        target = reps[best]
        for k in range(4):
            target[k] += reps[small][k]
        parent[small] = best
        alive.remove(small)
        merged += 1

    def root(i):
        while parent[i] != i:
            i = parent[i]
        return i

    final = {c: colour_of(reps[root(gi)]) for c, gi in owner.items()}
    how = [f"merged {merged} nearest"]
    if bits:
        how.insert(0, f"dropped {bits} low bit{'s' if bits != 1 else ''} per channel")
    note = f"{len(counts)} colours, cut to {len(set(final.values()))} ({', '.join(how)})"
    return [final[p] for p in pixels], note


# ---------------------------------------------------------------------------
# GIF writing


def lzw_encode(indices, min_size):
    """GIF LZW: variable code size from min_size+1 up to 12 bits, codes packed
    least significant bit first. Returns the packed bytes (not yet split into
    sub-blocks). indices must all be below 1 << min_size."""
    clear = 1 << min_size
    eoi = clear + 1
    out = bytearray()
    acc = clear  # the stream starts with a clear code
    shift = min_size + 1
    size = min_size + 1
    next_code = eoi + 1
    table = {}
    get = table.get
    it = iter(indices)
    try:
        prefix = next(it)
    except StopIteration:
        raise MediaError("cannot encode an empty frame") from None
    for k in it:
        key = (prefix << 8) | k  # k < 256 and codes < 4096, so the key is unique
        code = get(key)
        if code is not None:
            prefix = code
            continue
        acc |= prefix << shift
        shift += size
        while shift >= 8:
            out.append(acc & 255)
            acc >>= 8
            shift -= 8
        if next_code == 4096:
            # Table full: send a clear code (still 12 bits wide) and start over.
            acc |= clear << shift
            shift += size
            table.clear()
            next_code = eoi + 1
            size = min_size + 1
        else:
            # Trap: the code size grows when the code about to be added no
            # longer fits, i.e. one step "late" compared with the decoder's view.
            if next_code >= (1 << size):
                size += 1
            table[key] = next_code
            next_code += 1
        prefix = k
    acc |= prefix << shift
    shift += size
    # Trap: after the last code the decoder adds one more entry, and if that
    # fills the current width it reads the end code one bit wider.
    if next_code >= (1 << size) and size < 12:
        size += 1
    acc |= eoi << shift
    shift += size
    while shift > 0:
        out.append(acc & 255)
        acc >>= 8
        shift -= 8
    return bytes(out)


def _sub_blocks(data):
    out = bytearray()
    for i in range(0, len(data), 255):
        chunk = data[i:i + 255]
        out.append(len(chunk))
        out += chunk
    out.append(0)  # block terminator
    return bytes(out)


def _table_bits(entries):
    """Bits for a colour table with this many entries (1..8)."""
    bits = 1
    while (1 << bits) < entries:
        bits += 1
    return bits


def _table_bytes(palette, bits):
    data = ints_to_rgb_bytes(palette)
    return data + bytes(3 * (1 << bits) - len(data))


def _image_data(indices, bits):
    """Minimum code size byte + LZW sub-blocks. The minimum code size is never below 2."""
    min_size = max(2, bits)
    return bytes((min_size,)) + _sub_blocks(lzw_encode(indices, min_size))


def changed_box(prev, cur, w, h):
    """(x0, y0, x1, y1), inclusive, around every pixel that differs; None when equal."""
    rows = [y for y in range(h) if prev[y * w:(y + 1) * w] != cur[y * w:(y + 1) * w]]
    if not rows:
        return None
    x0, x1 = w, -1
    for y in rows:
        base = y * w
        x = 0
        while x < x0 and prev[base + x] == cur[base + x]:
            x += 1
        if x < x0:
            x0 = x
        x = w - 1
        while x > x1 and prev[base + x] == cur[base + x]:
            x -= 1
        if x > x1:
            x1 = x
    return x0, rows[0], x1, rows[-1]


def _region(px, w, box):
    x0, y0, x1, y1 = box
    out = []
    for y in range(y0, y1 + 1):
        out.extend(px[y * w + x0:y * w + x1 + 1])
    return out


def frame_delays(count, every, fps):
    """Delay in hundredths of a second for each of `count` frames, each lasting
    every/fps seconds. Rounding errors are carried forward, so the delays add up
    to the real length (rounded once)."""
    step = Fraction(100 * every, fps)
    ends = [int((step * i) + Fraction(1, 2)) for i in range(count + 1)]  # round half up
    return [ends[i + 1] - ends[i] for i in range(count)]


class GifResult:
    def __init__(self, data, frames_in, frames_written, length_cs, notes):
        self.data = data
        self.frames_in = frames_in
        self.frames_written = frames_written
        self.length_cs = length_cs
        self.notes = notes

    def summary(self):
        return (
            f"frames in {self.frames_in}, frames written {self.frames_written}, "
            f"{len(self.data)} bytes, {self.length_cs / 100:.2f} s"
        )


def encode_gif(frames, delays, scale=1, loop=0, hold_last=0, warn=None):
    """Encode an animated GIF89a.

    frames: a sequence of Image objects or PNG file paths (paths are read
    twice, so a long clip never sits in memory at once).
    delays: hundredths of a second per input frame.
    loop: 0 = for ever, N = N extra plays, None = no loop extension.
    warn: called with each note (defaults to printing on stderr).
    """
    if warn is None:
        def warn(msg):
            print(f"note: {msg}", file=sys.stderr)
    if len(frames) != len(delays):
        raise MediaError("one delay is needed per frame")
    if not frames:
        raise MediaError("no frames to encode")
    if any(d < 0 for d in delays):
        raise MediaError("delays cannot be negative")
    notes = []

    def note(msg):
        notes.append(msg)
        warn(msg)

    def load(i):
        f = frames[i]
        return f if isinstance(f, Image) else read_png(f)

    def label(i):
        f = frames[i]
        return os.path.basename(f) if isinstance(f, str) else f"frame {i}"

    # Pass 1: size check and the colours of the whole clip.
    union = set()
    width = height = None
    for i in range(len(frames)):
        img = load(i)
        if width is None:
            width, height = img.width, img.height
        elif (img.width, img.height) != (width, height):
            raise MediaError(
                f"{label(i)} is {img.width}x{img.height}, the first frame is {width}x{height}"
            )
        if len(union) <= 255:
            union.update(img.pixels)
    # A global table needs one index left free for "transparent".
    use_global = len(union) <= 255
    W, H = width * scale, height * scale

    if use_global:
        gpal = sorted(union)
        gbits = _table_bits(len(gpal) + 1)
        gindex = {c: i for i, c in enumerate(gpal)}
        gtrans = len(gpal)

    # Pass 2: encode. Each entry of `blocks` is [delay, x, y, w, h, transparent
    # index or None, local palette or None, local bits, image data].
    blocks = []
    prev_orig = None
    shown = None  # what the viewer sees after the last written frame
    for i in range(len(frames)):
        img = load(i)
        delay = delays[i]
        if prev_orig is not None and img.pixels == prev_orig:
            blocks[-1][0] += delay  # no change: the previous frame stays longer
            continue
        prev_orig = img.pixels
        cur = scale_image(img, scale).pixels
        if not use_global:
            cur, msg = reduce_colours(cur)
            if msg:
                note(f"{label(i)}: {msg}")
        if shown is None:
            box = (0, 0, W - 1, H - 1)
        else:
            box = changed_box(shown, cur, W, H)
            if box is None:  # equal after colour reduction
                blocks[-1][0] += delay
                continue
        region = _region(cur, W, box)
        before = _region(shown, W, box) if shown is not None else None
        candidates = []
        if use_global:
            opaque = list(map(gindex.__getitem__, region))
            candidates.append((None, None, gbits, _image_data(opaque, gbits)))
            if before is not None:
                see = [gtrans if a == b else gindex[a] for a, b in zip(region, before)]
                candidates.append((gtrans, None, gbits, _image_data(see, gbits)))
        else:
            pal = sorted(set(region))
            bits = _table_bits(len(pal))
            idx = {c: j for j, c in enumerate(pal)}
            opaque = list(map(idx.__getitem__, region))
            candidates.append((None, pal, bits, _image_data(opaque, bits)))
            if before is not None:
                changed = sorted({a for a, b in zip(region, before) if a != b})
                if len(changed) <= 255:
                    t = len(changed)
                    tbits = _table_bits(t + 1)
                    tidx = {c: j for j, c in enumerate(changed)}
                    see = [t if a == b else tidx[a] for a, b in zip(region, before)]
                    candidates.append((t, changed, tbits, _image_data(see, tbits)))

        def cost(c):
            return len(c[3]) + (3 * (1 << c[2]) if c[1] is not None else 0)

        trans, pal, bits, data = min(candidates, key=cost)
        x0, y0, x1, y1 = box
        blocks.append([delay, x0, y0, x1 - x0 + 1, y1 - y0 + 1, trans, pal, bits, data])
        shown = cur

    blocks[-1][0] += hold_last
    short = sum(1 for b in blocks if b[0] < 2)
    if short:
        before_cs = sum(b[0] for b in blocks)
        for b in blocks:
            b[0] = max(b[0], 2)
        after_cs = sum(b[0] for b in blocks)
        note(
            f"{short} frame(s) were shorter than 2/100 s, which browsers show as 10/100 s; "
            f"raised to 2/100 s, so the clip lasts {after_cs / 100:.2f} s instead of "
            f"{before_cs / 100:.2f} s (at 60 fps use --every 2)"
        )
    for b in blocks:
        if b[0] > 0xFFFF:
            raise MediaError(f"a frame lasts {b[0] / 100:.2f} s, more than a GIF can hold (655.35 s)")

    out = bytearray(b"GIF89a")
    packed = 0x70  # colour resolution field: 8 bits per primary
    if use_global:
        packed |= 0x80 | (gbits - 1)
    out += struct.pack("<HHBBB", W, H, packed, 0, 0)
    if use_global:
        out += _table_bytes(gpal, gbits)
    if loop is not None:
        out += b"\x21\xff\x0bNETSCAPE2.0\x03\x01" + struct.pack("<H", loop) + b"\x00"
    for delay, x, y, w, h, trans, pal, bits, data in blocks:
        # Graphic control extension: disposal 1 (leave in place) in bits 2-4.
        gce = (1 << 2) | (1 if trans is not None else 0)
        out += struct.pack("<BBBBHBB", 0x21, 0xF9, 4, gce, delay, trans or 0, 0)
        desc = 0
        if pal is not None:
            desc = 0x80 | (bits - 1)
        out += struct.pack("<BHHHHB", 0x2C, x, y, w, h, desc)
        if pal is not None:
            out += _table_bytes(pal, bits)
        out += data
    out += b"\x3b"
    length = sum(b[0] for b in blocks)
    return GifResult(bytes(out), len(frames), len(blocks), length, notes)


# ---------------------------------------------------------------------------
# GIF reading (for "info"; the tests carry their own separate reader)


def lzw_decode(data, min_size):
    """Decode GIF LZW bytes into a list of indices."""
    clear = 1 << min_size
    eoi = clear + 1
    base = [bytes((i,)) for i in range(clear)] + [b"", b""]
    table = list(base)
    size = min_size + 1
    out = bytearray()
    prev = None
    acc = 0
    nbits = 0
    pos = 0
    n = len(data)
    while True:
        while nbits < size:
            if pos >= n:
                return out  # no end code: stop at the end of the data, as viewers do
            acc |= data[pos] << nbits
            pos += 1
            nbits += 8
        code = acc & ((1 << size) - 1)
        acc >>= size
        nbits -= size
        if code == clear:
            table = list(base)
            size = min_size + 1
            prev = None
            continue
        if code == eoi:
            return out
        if prev is None:
            if code >= clear:
                raise MediaError("LZW data starts with an unknown code")
            entry = table[code]
        else:
            if code < len(table):
                entry = table[code]
                new = table[prev] + entry[:1]
            elif code == len(table):
                entry = table[prev] + table[prev][:1]
                new = entry
            else:
                raise MediaError("LZW data uses a code that does not exist yet")
            if len(table) < 4096:
                table.append(new)
        out += entry
        if len(table) == (1 << size) and size < 12:
            size += 1
        prev = code


def gif_summary(data, name="file"):
    """Width, height, frame count, length (cs), colour count and loop of a GIF."""
    if data[:6] not in (b"GIF87a", b"GIF89a"):
        raise MediaError(f"{name}: not a GIF file")
    w, h, packed, _bg, _aspect = struct.unpack("<HHBBB", data[6:13])
    pos = 13
    gtable = None
    if packed & 0x80:
        size = 3 * (1 << ((packed & 7) + 1))
        gtable = rgb_bytes_to_ints(data[pos:pos + size])
        pos += size

    def blocks_at(p):
        chunks = []
        while True:
            k = data[p]
            p += 1
            if k == 0:
                return b"".join(chunks), p
            chunks.append(data[p:p + k])
            p += k

    frames = 0
    length = 0
    loop = None
    trans = None
    delay = 0
    colours = set()
    try:
        while True:
            b = data[pos]
            pos += 1
            if b == 0x3B:
                break
            if b == 0x21:
                label = data[pos]
                pos += 1
                body, pos = blocks_at(pos)
                if label == 0xF9 and len(body) >= 4:
                    delay = body[1] | (body[2] << 8)
                    trans = body[3] if body[0] & 1 else None
                elif label == 0xFF and body[:11] == b"NETSCAPE2.0" and len(body) >= 14:
                    loop = body[12] | (body[13] << 8)
            elif b == 0x2C:
                _x, _y, fw, fh, desc = struct.unpack("<HHHHB", data[pos:pos + 9])
                pos += 9
                table = gtable
                if desc & 0x80:
                    size = 3 * (1 << ((desc & 7) + 1))
                    table = rgb_bytes_to_ints(data[pos:pos + size])
                    pos += size
                min_size = data[pos]
                pos += 1
                body, pos = blocks_at(pos)
                used = set(lzw_decode(body, min_size)[:fw * fh])
                used.discard(trans)
                if table is None:
                    raise MediaError(f"{name}: a frame has no colour table")
                colours.update(table[i] for i in used if i < len(table))
                frames += 1
                length += delay
                trans = None
                delay = 0
            else:
                raise MediaError(f"{name}: unknown block 0x{b:02x} at byte {pos - 1}")
    except IndexError:
        raise MediaError(f"{name}: file is cut short") from None
    return {"width": w, "height": h, "frames": frames, "length_cs": length,
            "colours": len(colours), "loop": loop}


# ---------------------------------------------------------------------------
# Command line


def _frame_files(args):
    if args.dir is not None:
        if args.files:
            raise MediaError("give either --dir or a list of files, not both")
        if args.start is None or args.stop is None:
            raise MediaError("--dir needs --from and --to")
        if args.stop < args.start:
            raise MediaError("--to must not be before --from")
        files = [
            os.path.join(args.dir, f"frame-{n:05d}.png")
            for n in range(args.start, args.stop + 1, args.every)
        ]
    else:
        if not args.files:
            raise MediaError("no frames: give --dir with --from/--to, or a list of PNG files")
        if args.start is not None or args.stop is not None:
            raise MediaError("--from and --to only go with --dir")
        files = list(args.files)
    for f in files:
        if not os.path.isfile(f):
            raise MediaError(f"missing frame file: {f}")
    return files


def _positive(text):
    try:
        v = int(text)
    except ValueError:
        raise argparse.ArgumentTypeError(f"not a whole number: {text}") from None
    if v < 1:
        raise argparse.ArgumentTypeError(f"must be 1 or more: {text}")
    return v


def _not_negative(text):
    try:
        v = int(text)
    except ValueError:
        raise argparse.ArgumentTypeError(f"not a whole number: {text}") from None
    if v < 0:
        raise argparse.ArgumentTypeError(f"must be 0 or more: {text}")
    return v


def _loop(text):
    try:
        v = int(text)
    except ValueError:
        raise argparse.ArgumentTypeError(f"not a whole number: {text}") from None
    if not -1 <= v <= 0xFFFF:
        raise argparse.ArgumentTypeError("loop must be -1 (play once), 0 (for ever) or 1..65535")
    return v


def build_parser():
    p = argparse.ArgumentParser(
        prog="pocketmedia.py",
        description="Small PNG stills and animated GIF clips from simulator pictures.",
    )
    sub = p.add_subparsers(dest="command", required=True)

    s = sub.add_parser("still", help="write a picture as a small PNG")
    s.add_argument("input")
    s.add_argument("output")
    s.add_argument("--scale", type=_positive, default=1, help="whole-number scale (sharp pixels)")

    g = sub.add_parser("gif", help="write an animated GIF from frames")
    g.add_argument("output")
    g.add_argument("files", nargs="*", help="PNG files in order (instead of --dir)")
    g.add_argument("--dir", help="folder with frame-NNNNN.png files")
    g.add_argument("--from", dest="start", type=_not_negative, help="first frame number")
    g.add_argument("--to", dest="stop", type=_not_negative, help="last frame number (included)")
    g.add_argument("--every", type=_positive, default=1, help="take every K-th frame (default 1)")
    g.add_argument("--fps", type=_positive, default=30, help="simulator frame rate (default 30)")
    g.add_argument("--scale", type=_positive, default=1, help="whole-number scale (sharp pixels)")
    g.add_argument("--loop", type=_loop, default=0,
                   help="0 = for ever (default), N = N more plays, -1 = no loop extension")
    g.add_argument("--hold-last", type=_not_negative, default=0, metavar="CS",
                   help="add CS hundredths of a second to the last frame")

    i = sub.add_parser("info", help="print size, colours, frames and length")
    i.add_argument("files", nargs="+")
    return p


def _info(path):
    try:
        with open(path, "rb") as f:
            data = f.read()
    except OSError as e:
        raise MediaError(f"cannot read {path}: {e.strerror}") from None
    if data[:8] == PNG_SIGNATURE:
        img = decode_png(data, path)
        colour = data[25]
        return (f"{path}: PNG {img.width}x{img.height}, {_COLOUR_TYPES[colour][0]}, "
                f"{len(data)} bytes, {len(set(img.pixels))} colours")
    if data[:3] == b"GIF":
        s = gif_summary(data, path)
        loop = "no loop" if s["loop"] is None else (
            "loops for ever" if s["loop"] == 0 else f"loops {s['loop']} more times")
        return (f"{path}: GIF {s['width']}x{s['height']}, {len(data)} bytes, "
                f"{s['colours']} colours, {s['frames']} frames, "
                f"{s['length_cs'] / 100:.2f} s, {loop}")
    raise MediaError(f"{path}: neither PNG nor GIF")


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        if args.command == "still":
            img = scale_image(read_png(args.input), args.scale)
            data = encode_png(img)
            write_file(args.output, data)
            kind = "palette" if data[25] == 3 else "RGB"
            print(f"{args.output}: {img.width}x{img.height}, {len(set(img.pixels))} colours, "
                  f"{kind} PNG, {len(data)} bytes")
        elif args.command == "gif":
            files = _frame_files(args)
            delays = frame_delays(len(files), args.every, args.fps)
            loop = None if args.loop == -1 else args.loop
            result = encode_gif(files, delays, scale=args.scale, loop=loop,
                                hold_last=args.hold_last)
            write_file(args.output, result.data)
            print(result.summary())
        else:
            for path in args.files:
                print(_info(path))
    except MediaError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
