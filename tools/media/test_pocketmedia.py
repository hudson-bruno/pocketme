"""Tests for pocketmedia.py.

There is no picture library on this PC, so these tests carry their own,
separately written readers (a GIF decoder with LZW, sub-rectangles,
transparency, disposal and colour tables; a PNG reader with CRC and zlib
checks) and their own PNG writer for test inputs. Every file the tool
writes is decoded again here and compared pixel by pixel.

Run from this folder:  python3 -I -m unittest -v
The real simulator pictures are looked for in ../../.local/media-work/probe
(or the folder in POCKETMEDIA_SAMPLES); those tests are skipped without it.
"""

import contextlib
import io
import os
import random
import shutil
import struct
import sys
import tempfile
import unittest
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import pocketmedia as pm  # noqa: E402

SAMPLES = os.environ.get(
    "POCKETMEDIA_SAMPLES", os.path.join(HERE, "..", "..", ".local", "media-work", "probe")
)


def sample_files():
    if not os.path.isdir(SAMPLES):
        return []
    return sorted(
        os.path.join(SAMPLES, f) for f in os.listdir(SAMPLES) if f.endswith(".png")
    )


# ---------------------------------------------------------------------------
# Independent helpers: CRC-32, Adler-32


def _crc_table():
    table = []
    for n in range(256):
        c = n
        for _ in range(8):
            c = (0xEDB88320 ^ (c >> 1)) if c & 1 else (c >> 1)
        table.append(c)
    return table


CRC_TABLE = _crc_table()


def crc32(data):
    c = 0xFFFFFFFF
    for b in data:
        c = CRC_TABLE[(c ^ b) & 0xFF] ^ (c >> 8)
    return c ^ 0xFFFFFFFF


def adler32(data):
    a, b = 1, 0
    for x in data:
        a = (a + x) % 65521
        b = (b + a) % 65521
    return (b << 16) | a


def rgb(r, g, b):
    return (r << 16) | (g << 8) | b


# ---------------------------------------------------------------------------
# Independent PNG writer (for test inputs) and reader (for checking output)


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def filter_row(ftype, row, prev, bpp):
    out = []
    for i, x in enumerate(row):
        a = row[i - bpp] if i >= bpp else 0
        b = prev[i]
        c = prev[i - bpp] if i >= bpp else 0
        pred = [0, a, b, (a + b) // 2, paeth(a, b, c)][ftype]
        out.append((x - pred) % 256)
    return bytes(out)


def make_png(w, h, colour_type, sample_rows, filters, plte=None, depth=8, interlace=0,
             extra_chunks=()):
    """sample_rows: list of bytes, one per row, already in the colour type's layout."""
    bpp = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[colour_type]

    def chunk(t, body):
        return struct.pack(">I", len(body)) + t + body + struct.pack(">I", crc32(t + body))

    raw = bytearray()
    prev = bytes(len(sample_rows[0]))
    for y, row in enumerate(sample_rows):
        f = filters[y % len(filters)]
        raw.append(f)
        raw += filter_row(f, row, prev, bpp)
        prev = row
    out = b"\x89PNG\r\n\x1a\n"
    out += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, colour_type, 0, 0, interlace))
    for t, body in extra_chunks:
        out += chunk(t, body)
    if plte is not None:
        out += chunk(b"PLTE", plte)
    z = zlib.compress(bytes(raw))
    out += chunk(b"IDAT", z[:len(z) // 2]) + chunk(b"IDAT", z[len(z) // 2:])  # two IDATs
    out += chunk(b"IEND", b"")
    return out


def rgb_png(w, h, pixels, filters=(0,)):
    rows = []
    for y in range(h):
        row = bytearray()
        for p in pixels[y * w:(y + 1) * w]:
            row += bytes(((p >> 16) & 255, (p >> 8) & 255, p & 255))
        rows.append(bytes(row))
    return make_png(w, h, 2, rows, filters)


def check_and_read_png(test, data):
    """Strict reading of a PNG the tool wrote: signature, CRCs, zlib stream,
    then the pixels. Returns (width, height, colour type, pixels)."""
    test.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
    p = 8
    chunks = []
    while p < len(data):
        (n,) = struct.unpack(">I", data[p:p + 4])
        t = data[p + 4:p + 8]
        body = data[p + 8:p + 8 + n]
        (c,) = struct.unpack(">I", data[p + 8 + n:p + 12 + n])
        test.assertEqual(crc32(t + body), c, f"CRC of {t!r}")
        chunks.append((t, body))
        p += 12 + n
    test.assertEqual(p, len(data), "bytes after IEND")
    names = [t for t, _ in chunks]
    test.assertEqual(names[0], b"IHDR")
    test.assertEqual(names[-1], b"IEND")
    w, h, depth, ct, comp, filt, inter = struct.unpack(">IIBBBBB", chunks[0][1])
    test.assertEqual((depth, comp, filt, inter), (8, 0, 0, 0))
    z = b"".join(b for t, b in chunks if t == b"IDAT")
    # zlib header: deflate, window ok, check bits make it a multiple of 31
    test.assertEqual(z[0] & 0x0F, 8)
    test.assertLessEqual(z[0] >> 4, 7)
    test.assertEqual(((z[0] << 8) | z[1]) % 31, 0)
    test.assertFalse(z[1] & 0x20, "no preset dictionary")
    d = zlib.decompressobj(-15)  # raw deflate, so the Adler-32 is checked by hand
    raw = d.decompress(z[2:])
    test.assertTrue(d.eof)
    test.assertEqual(len(d.unused_data), 4)
    test.assertEqual(struct.unpack(">I", d.unused_data)[0], adler32(raw))
    bpp = {2: 3, 3: 1}[ct]
    stride = w * bpp
    test.assertEqual(len(raw), h * (stride + 1))
    prev = [0] * stride
    flat = []
    for y in range(h):
        f = raw[y * (stride + 1)]
        row = list(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = row[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            row[i] = (row[i] + [0, a, b, (a + b) // 2, paeth(a, b, c)][f]) % 256
        flat += row
        prev = row
    if ct == 2:
        pixels = [rgb(*flat[i:i + 3]) for i in range(0, len(flat), 3)]
    else:
        plte = dict(chunks)[b"PLTE"]
        test.assertEqual(len(plte) % 3, 0)
        pal = [rgb(*plte[i:i + 3]) for i in range(0, len(plte), 3)]
        test.assertLessEqual(len(pal), 256)
        pixels = [pal[i] for i in flat]
    return w, h, ct, pixels


# ---------------------------------------------------------------------------
# Independent GIF reader


def gif_lzw_decode(data, min_size, stats=None):
    """Prefix/suffix table decoder. Strict: needs the end code, and nothing but
    zero padding may follow it."""
    clear = 1 << min_size
    eoi = clear + 1
    prefix = [None] * 4096
    suffix = list(range(clear)) + [0] * (4096 - clear)
    first = list(range(clear)) + [0] * (4096 - clear)

    def string(code):
        s = []
        while code is not None:
            s.append(suffix[code])
            code = prefix[code]
        s.reverse()
        return s

    out = []
    bitpos = 0
    size = min_size + 1
    next_free = eoi + 1
    old = None
    clears = 0
    while True:
        byte = bitpos >> 3
        if byte >= len(data) or bitpos + size > 8 * len(data):
            raise AssertionError("LZW data ended without an end code")
        code = (int.from_bytes(data[byte:byte + 3], "little") >> (bitpos & 7)) & ((1 << size) - 1)
        bitpos += size
        if code == clear:
            clears += 1
            size = min_size + 1
            next_free = eoi + 1
            old = None
            continue
        if code == eoi:
            break
        if old is None:
            assert code < clear, "first code after clear must be a plain index"
            out.append(code)
            old = code
            continue
        if code < next_free:
            s = string(code)
            new_first = s[0]
        elif code == next_free:
            s = string(old)
            s.append(s[0])
            new_first = s[0]
        else:
            raise AssertionError(f"code {code} is past the table ({next_free})")
        out.extend(s)
        if next_free < 4096:
            prefix[next_free] = old
            suffix[next_free] = new_first
            first[next_free] = first[old]
            next_free += 1
            if next_free == (1 << size) and size < 12:
                size += 1
        old = code
    # Only zero bits may follow the end code, and no extra bytes.
    assert (bitpos + 7) // 8 == len(data), "bytes after the end code"
    if bitpos & 7:
        assert data[-1] >> (bitpos & 7) == 0, "non-zero padding after the end code"
    if stats is not None:
        stats["clears"] = stats.get("clears", 0) + clears
    return out


def read_gif(data):
    """Full decode with compositing. Returns a dict with the header facts and a
    list of frames; each frame has the canvas after it, its delay and its block facts."""
    assert data[:6] == b"GIF89a", data[:6]
    w, h, packed, bg, aspect = struct.unpack("<HHBBB", data[6:13])
    p = 13
    gct = None
    if packed & 0x80:
        n = 2 << (packed & 7)
        gct = [rgb(*data[p + 3 * i:p + 3 * i + 3]) for i in range(n)]
        p += 3 * n

    def subblocks(p):
        out = b""
        while True:
            n = data[p]
            p += 1
            if n == 0:
                return out, p
            out += data[p:p + n]
            assert len(data[p:p + n]) == n, "sub-block cut short"
            p += n

    canvas = [None] * (w * h)
    frames = []
    loop = None
    gce = None
    stats = {}
    while True:
        b = data[p]
        p += 1
        if b == 0x3B:
            assert p == len(data), "bytes after the trailer"
            break
        if b == 0x21:
            label = data[p]
            p += 1
            if label == 0xF9:
                assert data[p] == 4
                flags, delay, trans = struct.unpack("<BHB", data[p + 1:p + 5])
                assert data[p + 5] == 0
                p += 6
                gce = {"disposal": (flags >> 2) & 7, "delay": delay,
                       "trans": trans if flags & 1 else None}
            else:
                body, p = subblocks(p)
                if label == 0xFF and body[:11] == b"NETSCAPE2.0":
                    assert body[11] == 1
                    loop = body[12] | (body[13] << 8)
            continue
        assert b == 0x2C, f"unexpected block 0x{b:02x}"
        x, y, fw, fh, flags = struct.unpack("<HHHHB", data[p:p + 9])
        p += 9
        assert not flags & 0x40, "interlaced"
        assert x + fw <= w and y + fh <= h, "frame outside the screen"
        table = gct
        local = None
        if flags & 0x80:
            n = 2 << (flags & 7)
            local = [rgb(*data[p + 3 * i:p + 3 * i + 3]) for i in range(n)]
            table = local
            p += 3 * n
        assert table is not None
        min_size = data[p]
        p += 1
        assert 2 <= min_size <= 8
        body, p = subblocks(p)
        idx = gif_lzw_decode(body, min_size, stats)
        assert len(idx) == fw * fh, (len(idx), fw * fh)
        g = gce or {"disposal": 0, "delay": 0, "trans": None}
        saved = list(canvas)
        for j, i in enumerate(idx):
            if i == g["trans"]:
                continue
            assert i < len(table)
            canvas[(y + j // fw) * w + x + j % fw] = table[i]
        frames.append({"pixels": list(canvas), "delay": g["delay"], "disposal": g["disposal"],
                       "trans": g["trans"], "box": (x, y, fw, fh), "local": local,
                       "min_size": min_size})
        if g["disposal"] == 2:
            for yy in range(y, y + fh):
                for xx in range(x, x + fw):
                    canvas[yy * w + xx] = gct[bg] if gct else None
        elif g["disposal"] == 3:
            canvas = saved
        gce = None
    return {"width": w, "height": h, "packed": packed, "gct": gct, "loop": loop,
            "frames": frames, "clears": stats.get("clears", 0)}


def scale_ref(w, h, pixels, n):
    out = []
    for y in range(h * n):
        for x in range(w * n):
            out.append(pixels[(y // n) * w + x // n])
    return out


def merged_reference(images, delays):
    """What the clip should show: consecutive equal pictures become one."""
    out = []
    for img, d in zip(images, delays):
        if out and out[-1][0] == img.pixels:
            out[-1][1] += d
        else:
            out.append([img.pixels, d])
    return out


# ---------------------------------------------------------------------------
# Test cases


class PngReadingTests(unittest.TestCase):
    def setUp(self):
        self.rnd = random.Random(1)

    def random_rgb(self, w, h, colours=None):
        if colours:
            pool = [self.rnd.randrange(1 << 24) for _ in range(colours)]
            return [self.rnd.choice(pool) for _ in range(w * h)]
        return [self.rnd.randrange(1 << 24) for _ in range(w * h)]

    def test_every_filter_type_alone_and_mixed(self):
        w, h = 17, 9
        px = self.random_rgb(w, h)
        for filters in ([0], [1], [2], [3], [4], [0, 1, 2, 3, 4], [4, 3, 2, 1]):
            img = pm.decode_png(rgb_png(w, h, px, filters))
            self.assertEqual((img.width, img.height), (w, h))
            self.assertEqual(img.pixels, px, f"filters {filters}")

    def test_grey_palette_and_rgba(self):
        w, h = 5, 4
        grey = [self.rnd.randrange(256) for _ in range(w * h)]
        rows = [bytes(grey[y * w:(y + 1) * w]) for y in range(h)]
        img = pm.decode_png(make_png(w, h, 0, rows, [0, 1, 2, 3, 4]))
        self.assertEqual(img.pixels, [rgb(g, g, g) for g in grey])

        pal = [self.rnd.randrange(1 << 24) for _ in range(7)]
        plte = b"".join(bytes(((c >> 16) & 255, (c >> 8) & 255, c & 255)) for c in pal)
        idx = [self.rnd.randrange(7) for _ in range(w * h)]
        rows = [bytes(idx[y * w:(y + 1) * w]) for y in range(h)]
        img = pm.decode_png(make_png(w, h, 3, rows, [4, 0, 1], plte=plte))
        self.assertEqual(img.pixels, [pal[i] for i in idx])

        px = self.random_rgb(w, h)
        rows = []
        for y in range(h):
            row = bytearray()
            for p in px[y * w:(y + 1) * w]:
                row += bytes(((p >> 16) & 255, (p >> 8) & 255, p & 255, 255))
            rows.append(bytes(row))
        img = pm.decode_png(make_png(w, h, 6, rows, [3, 2]))
        self.assertEqual(img.pixels, px)

    def test_rgba_with_see_through_pixels_goes_on_black(self):
        rows = [bytes((200, 100, 50, 128, 10, 20, 30, 0))]
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            img = pm.decode_png(make_png(2, 1, 6, rows, [0]))
        self.assertEqual(img.pixels, [rgb(100, 50, 25), 0])
        self.assertIn("see-through", err.getvalue())

    def test_refusals_are_clear(self):
        one = [bytes(3)]
        cases = [
            (make_png(1, 1, 2, [bytes(6)], [0], depth=16), "bit depth 16"),
            (make_png(1, 1, 2, one, [0], interlace=1), "interlaced"),
            (make_png(1, 1, 4, [bytes(2)], [0]), "colour type 4"),
            (make_png(1, 1, 2, one, [0], extra_chunks=[(b"ABCD", b"")]), "unknown critical"),
            (b"GIF89a" + bytes(20), "not a PNG"),
            (make_png(1, 1, 3, [bytes(1)], [0]), "PLTE"),
        ]
        for data, words in cases:
            with self.assertRaises(pm.MediaError) as cm:
                pm.decode_png(data, "x.png")
            self.assertIn(words, str(cm.exception))
            self.assertIn("x.png", str(cm.exception))
        bad = bytearray(make_png(1, 1, 2, one, [0]))
        bad[30] ^= 1  # inside the IHDR CRC
        with self.assertRaises(pm.MediaError) as cm:
            pm.decode_png(bytes(bad), "x.png")
        self.assertIn("bad CRC", str(cm.exception))

    def test_ancillary_chunks_are_skipped(self):
        data = make_png(1, 1, 2, [bytes((1, 2, 3))], [0], extra_chunks=[(b"tEXt", b"a\0b")])
        self.assertEqual(pm.decode_png(data).pixels, [rgb(1, 2, 3)])

    @unittest.skipUnless(sample_files(), "no simulator pictures found")
    def test_simulator_pictures_match_an_independent_read(self):
        for path in sample_files():
            with open(path, "rb") as f:
                data = f.read()
            img = pm.decode_png(data, path)
            self.assertEqual((img.width, img.height), (320, 240))
            self.assertEqual(data[25], 2, "simulator pictures are RGB")
            w, h, ct, px = check_and_read_png(self, data)
            self.assertEqual(img.pixels, px)


class PngWritingTests(unittest.TestCase):
    def setUp(self):
        self.rnd = random.Random(2)

    def roundtrip(self, img):
        data = pm.encode_png(img)
        w, h, ct, px = check_and_read_png(self, data)
        self.assertEqual((w, h), (img.width, img.height))
        self.assertEqual(px, img.pixels)
        self.assertEqual(pm.decode_png(data).pixels, img.pixels)
        return ct, data

    def test_palette_when_256_colours_or_fewer(self):
        for n in (1, 2, 255, 256):
            pool = [self.rnd.randrange(1 << 24) for _ in range(n)]
            px = pool + [self.rnd.choice(pool) for _ in range(40 * 30 - n)]
            ct, _ = self.roundtrip(pm.Image(40, 30, px))
            self.assertEqual(ct, 3, f"{n} colours")

    def test_rgb_when_more_than_256_colours(self):
        px = list(range(257)) + [0] * (30 * 30 - 257)
        ct, _ = self.roundtrip(pm.Image(30, 30, px))
        self.assertEqual(ct, 2)
        px = [self.rnd.randrange(1 << 24) for _ in range(23 * 12)]
        ct, _ = self.roundtrip(pm.Image(23, 12, px))
        self.assertEqual(ct, 2)

    def test_scaled_stills_through_the_command_line(self):
        px = [self.rnd.choice((0xFF0000, 0x00FF00, 0x123456)) for _ in range(7 * 5)]
        with work_dir() as d:
            src = os.path.join(d, "in.png")
            with open(src, "wb") as f:
                f.write(rgb_png(7, 5, px))
            for n in (1, 2, 3):
                out = os.path.join(d, f"out{n}.png")
                with contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(pm.main(["still", src, out, "--scale", str(n)]), 0)
                with open(out, "rb") as f:
                    w, h, ct, got = check_and_read_png(self, f.read())
                self.assertEqual((w, h, ct), (7 * n, 5 * n, 3))
                self.assertEqual(got, scale_ref(7, 5, px, n))

    @unittest.skipUnless(sample_files(), "no simulator pictures found")
    def test_simulator_pictures_become_small_palette_pngs(self):
        for path in sample_files():
            img = pm.read_png(path)
            ct, data = self.roundtrip(img)
            self.assertEqual(ct, 3)
            self.assertLess(len(data), os.path.getsize(path) // 10)
            ct, data = self.roundtrip(pm.scale_image(img, 2))
            self.assertEqual(ct, 3)


class LzwTests(unittest.TestCase):
    def test_many_lengths_and_alphabets(self):
        # Sweeping lengths makes the end code fall on every code-size boundary.
        rnd = random.Random(3)
        for min_size in (2, 3, 4, 8):
            for n in list(range(1, 300)) + [511, 512, 513, 1023, 1024, 1025]:
                alphabet = 1 << min_size
                idx = [rnd.randrange(alphabet) for _ in range(n)]
                self.assertEqual(gif_lzw_decode(pm.lzw_encode(idx, min_size), min_size), idx)
                self.assertEqual(pm.lzw_decode(pm.lzw_encode(idx, min_size), min_size), bytes(idx))

    def test_runs_that_fill_the_table_several_times(self):
        rnd = random.Random(4)
        idx = [rnd.randrange(256) for _ in range(60000)]
        stats = {}
        self.assertEqual(gif_lzw_decode(pm.lzw_encode(idx, 8), 8, stats), idx)
        self.assertGreaterEqual(stats["clears"], 1 + 10)  # the first clear plus resets
        # A long single-colour run grows codes one step at a time: also crosses 4096.
        idx = [1] * 9_000_000
        stats = {}
        self.assertEqual(gif_lzw_decode(pm.lzw_encode(idx, 2), 2, stats), idx)
        self.assertGreaterEqual(stats["clears"], 2)


class GifTests(unittest.TestCase):
    def setUp(self):
        self.rnd = random.Random(5)

    def encode(self, images, delays, **kw):
        notes = []
        res = pm.encode_gif(images, delays, warn=notes.append, **kw)
        return res, read_gif(res.data), notes

    def assert_clip(self, images, delays, scale=1, **kw):
        res, gif, notes = self.encode(images, delays, scale=scale, **kw)
        ref = merged_reference(images, delays)
        w, h = images[0].width, images[0].height
        self.assertEqual((gif["width"], gif["height"]), (w * scale, h * scale))
        self.assertEqual(len(gif["frames"]), len(ref))
        self.assertEqual(res.frames_written, len(ref))
        self.assertEqual(res.frames_in, len(images))
        for k, (fr, (px, d)) in enumerate(zip(gif["frames"], ref)):
            want = px if scale == 1 else scale_ref(w, h, px, scale)
            self.assertEqual(fr["pixels"], want, f"frame {k}")
            self.assertEqual(fr["disposal"], 1)
            if k < len(ref) - 1:
                self.assertEqual(fr["delay"], max(d, 2))
        self.assertEqual(gif["frames"][0]["box"], (0, 0, w * scale, h * scale))
        return res, gif, notes

    def random_clip(self, w, h, frames, colours, changes):
        pool = [self.rnd.randrange(1 << 24) for _ in range(colours)]
        px = [self.rnd.choice(pool) for _ in range(w * h)]
        images = [pm.Image(w, h, list(px))]
        for _ in range(frames - 1):
            px = list(px)
            for _ in range(changes):
                px[self.rnd.randrange(w * h)] = self.rnd.choice(pool)
            images.append(pm.Image(w, h, px))
        return images

    def test_random_pictures(self):
        images = self.random_clip(37, 23, 8, 40, 30)
        res, gif, notes = self.assert_clip(images, [5] * 8)
        self.assertIsNotNone(gif["gct"])
        self.assertEqual(notes, [])
        # fully random frames, every pixel new each time
        images = [pm.Image(19, 13, [self.rnd.randrange(100) for _ in range(19 * 13)])
                  for _ in range(5)]
        self.assert_clip(images, [3, 4, 3, 4, 3])

    def test_one_colour_pictures(self):
        images = [pm.Image(10, 10, [0x336699] * 100) for _ in range(4)]
        res, gif, _ = self.assert_clip(images, [10, 10, 10, 10])
        self.assertEqual(len(gif["frames"]), 1)
        self.assertEqual(gif["frames"][0]["delay"], 40)
        self.assertEqual(gif["frames"][0]["min_size"], 2)
        # two plain colours, the whole screen flips
        images = [pm.Image(8, 4, [c] * 32) for c in (0, 0xFFFFFF, 0, 0xFFFFFF)]
        self.assert_clip(images, [7] * 4)

    def test_exactly_255_colours_use_a_global_table(self):
        pool = list(range(0, 255 * 3, 3))
        images = []
        for k in range(3):
            px = [pool[(i * 7 + k * 31) % 255] for i in range(32 * 16)]
            images.append(pm.Image(32, 16, px))
        res, gif, notes = self.assert_clip(images, [4, 4, 4])
        self.assertIsNotNone(gif["gct"])
        self.assertEqual(len(gif["gct"]), 256)
        self.assertTrue(all(f["local"] is None for f in gif["frames"]))
        self.assertEqual(notes, [])

    def test_exactly_256_colours_use_local_tables_without_loss(self):
        pool = [rgb(i, 255 - i, (i * 37) & 255) for i in range(256)]
        px = [pool[i % 256] for i in range(32 * 16)]
        images = [pm.Image(32, 16, px)]
        px2 = list(px)
        px2[5] = pool[9]
        px2[300] = pool[200]
        images.append(pm.Image(32, 16, px2))
        res, gif, notes = self.assert_clip(images, [10, 10])
        self.assertIsNone(gif["gct"])
        self.assertEqual(len(gif["frames"][0]["local"]), 256)
        self.assertIsNone(gif["frames"][0]["trans"])
        self.assertEqual(notes, [])
        # 256 colours in the clip but fewer in each frame: still exact
        a = pm.Image(16, 16, [pool[i] for i in range(128)] * 2)
        b = pm.Image(16, 16, [pool[128 + i] for i in range(128)] * 2)
        self.assert_clip([a, b, a], [5, 5, 5])

    def test_257_colours_in_one_frame_are_reduced_and_said(self):
        pool = [rgb(i % 256, (i * 3) % 256, 255 if i >= 256 else 0) for i in range(257)]
        px = [pool[i % 257] for i in range(32 * 16)]
        images = [pm.Image(32, 16, px), pm.Image(32, 16, [pool[0]] * (32 * 16))]
        res, gif, notes = self.encode(images, [10, 10])
        self.assertEqual(len(notes), 1)
        self.assertIn("257 colours", notes[0])
        f0 = gif["frames"][0]["pixels"]
        self.assertLessEqual(len(set(f0)), 256)
        for got, want in zip(f0, px):
            for s in (16, 8, 0):
                self.assertLessEqual(abs(((got >> s) & 255) - ((want >> s) & 255)), 8)
        self.assertEqual(gif["frames"][1]["pixels"], [pool[0]] * (32 * 16))

    def test_a_frame_with_many_colours_is_reduced_close_to_the_original(self):
        px = [self.rnd.randrange(1 << 24) for _ in range(64 * 48)]
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            res = pm.encode_gif([pm.Image(64, 48, px)], [10])
        self.assertIn("cut to", err.getvalue())
        gif = read_gif(res.data)
        self.assertLessEqual(len(set(gif["frames"][0]["pixels"])), 256)

    def test_long_run_passes_4096_codes_several_times(self):
        images = self.random_clip(160, 120, 3, 200, 5000)
        res, gif, _ = self.assert_clip(images, [10, 10, 10])
        self.assertGreaterEqual(gif["clears"], 3 + 5)

    def test_unchanged_frames_are_merged(self):
        a = pm.Image(6, 5, [1] * 30)
        b = pm.Image(6, 5, [1] * 29 + [2])
        res, gif, _ = self.assert_clip([a, a, b, b, b, a], [3, 4, 3, 4, 3, 5])
        self.assertEqual([f["delay"] for f in gif["frames"]], [7, 10, 5])
        self.assertEqual(res.length_cs, 22)

    def test_one_changed_pixel_gives_a_one_pixel_frame(self):
        px = [self.rnd.choice((10, 20, 30)) for _ in range(40 * 30)]
        px2 = list(px)
        px2[17 * 40 + 23] = 0xABCDEF
        res, gif, _ = self.assert_clip([pm.Image(40, 30, px), pm.Image(40, 30, px2)], [5, 5])
        self.assertEqual(gif["frames"][1]["box"], (23, 17, 1, 1))

    def test_transparency_is_used_when_it_is_smaller(self):
        # A noisy picture where the two corners change: the box is the whole
        # screen, and writing the unchanged middle as "transparent" is far smaller.
        px = [self.rnd.randrange(60) for _ in range(64 * 64)]
        px2 = list(px)
        px2[0] = 1000
        px2[-1] = 1001
        res, gif, _ = self.assert_clip([pm.Image(64, 64, px), pm.Image(64, 64, px2)], [5, 5])
        self.assertEqual(gif["frames"][1]["box"], (0, 0, 64, 64))
        self.assertIsNotNone(gif["frames"][1]["trans"])
        # and in a clip with local tables
        # 195 old colours + 61 new ones: 256 in the clip (no global table),
        # 256 in the second frame (no reduction)
        px3 = [(i * 7) % 195 for i in range(64 * 64)]
        px4 = list(px3)
        for i in range(60):
            px4[i] = 1000 + i
        px4[-1] = 2000
        res, gif, _ = self.assert_clip([pm.Image(64, 64, px3), pm.Image(64, 64, px4)], [5, 5])
        self.assertIsNone(gif["gct"])
        self.assertIsNotNone(gif["frames"][1]["trans"])
        self.assertLessEqual(len(gif["frames"][1]["local"]), 64)

    def test_scale_2_and_3(self):
        images = self.random_clip(13, 9, 4, 20, 6)
        for n in (2, 3):
            self.assert_clip(images, [5] * 4, scale=n)

    def test_loop_and_hold_last(self):
        a = pm.Image(2, 2, [0, 1, 2, 3])
        b = pm.Image(2, 2, [3, 2, 1, 0])
        res, gif, _ = self.encode([a, b], [10, 10], hold_last=150)
        self.assertEqual(gif["loop"], 0)
        self.assertEqual([f["delay"] for f in gif["frames"]], [10, 160])
        self.assertEqual(res.length_cs, 170)
        res, gif, _ = self.encode([a, b], [10, 10], loop=3)
        self.assertEqual(gif["loop"], 3)
        res, gif, _ = self.encode([a, b], [10, 10], loop=None)
        self.assertIsNone(gif["loop"])
        self.assertNotIn(b"NETSCAPE", res.data)

    def test_delays_keep_the_real_length(self):
        self.assertEqual(pm.frame_delays(6, 1, 30), [3, 4, 3, 3, 4, 3])
        self.assertEqual(sum(pm.frame_delays(150, 1, 30)), 500)
        self.assertEqual(sum(pm.frame_delays(75, 2, 60)), 250)
        self.assertEqual(set(pm.frame_delays(30, 2, 60)), {3, 4})
        self.assertEqual(sum(pm.frame_delays(7, 3, 30)), 70)
        self.assertEqual(pm.frame_delays(3, 1, 60), [2, 1, 2])

    def test_too_short_delays_are_raised_and_said(self):
        images = [pm.Image(1, 2, [k, 0]) for k in range(3)]
        res, gif, notes = self.encode(images, pm.frame_delays(3, 1, 60))
        self.assertEqual([f["delay"] for f in gif["frames"]], [2, 2, 2])
        self.assertEqual(len(notes), 1)
        self.assertIn("--every 2", notes[0])

    def test_sizes_must_match(self):
        with self.assertRaises(pm.MediaError):
            pm.encode_gif([pm.Image(2, 2, [0] * 4), pm.Image(3, 2, [0] * 6)], [1, 1])

    def test_byte_layout_by_hand(self):
        R, G, B = 0xFF0000, 0x00FF00, 0x0000FF
        a = pm.Image(2, 2, [R, G, B, R])
        b = pm.Image(2, 2, [R, G, B, G])
        data = pm.encode_gif([a, b], [10, 25], warn=lambda m: None).data
        want = bytearray()
        want += b"GIF89a"
        # logical screen: 2x2; global table, 8-bit colour resolution, 4 entries
        # (3 colours + 1 kept free for transparent); background 0; aspect 0
        want += bytes((2, 0, 2, 0, 0x80 | 0x70 | 0x01, 0, 0))
        want += bytes((0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 0, 0))  # sorted: B, G, R, spare
        want += b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00"  # loop for ever
        # frame 1: graphic control: size 4, disposal 1, no transparency, 10 cs
        want += bytes((0x21, 0xF9, 4, 0x04, 10, 0, 0, 0))
        # image descriptor at 0,0, 2x2, no local table
        want += bytes((0x2C, 0, 0, 0, 0, 2, 0, 2, 0, 0))
        # indices R G B R = 2 1 0 2; min code size 2; codes (bits, LSB first):
        # clear 4 (3), 2 (3), 1 (3), 0 (3), 2 (4: table reached 8), end 5 (4)
        want += bytes((2, 3, 0x54, 0x20, 0x05, 0))
        # frame 2: only the bottom-right pixel changed; 25 cs
        want += bytes((0x21, 0xF9, 4, 0x04, 25, 0, 0, 0))
        want += bytes((0x2C, 1, 0, 1, 0, 1, 0, 1, 0, 0))
        # index 1 (G): clear 4 (3), 1 (3), end 5 (3)
        want += bytes((2, 2, 0x4C, 0x01, 0))
        want += b"\x3b"
        self.assertEqual(data.hex(" "), bytes(want).hex(" "))

    def test_sub_blocks_are_at_most_255_bytes(self):
        images = self.random_clip(64, 64, 1, 200, 0)
        data = pm.encode_gif(images, [5], warn=lambda m: None).data
        # walk the image data of the single frame
        p = data.index(b"\x2c", 13 + 3 * 256 + 19)
        p += 10
        self.assertEqual(data[p], 8)
        p += 1
        sizes = []
        while data[p]:
            sizes.append(data[p])
            p += 1 + data[p]
        self.assertTrue(all(s == 255 for s in sizes[:-1]))
        self.assertGreater(len(sizes), 3)
        self.assertEqual(data[p + 1:], b"\x3b")

    @unittest.skipUnless(sample_files(), "no simulator pictures found")
    def test_real_simulator_pictures(self):
        files = sample_files()
        images = [pm.read_png(f) for f in files]
        self.assert_clip(images, [10] * len(images))
        self.assert_clip(images[:2], [10, 10], scale=2)
        # straight from the files, as the command line does
        res = pm.encode_gif(files, [10] * len(files), warn=lambda m: None)
        gif = read_gif(res.data)
        ref = merged_reference(images, [10] * len(images))
        self.assertEqual([f["pixels"] for f in gif["frames"]], [p for p, _ in ref])
        self.assertLess(len(res.data), sum(os.path.getsize(f) for f in files) // 10)


class CommandLineTests(unittest.TestCase):
    def test_gif_from_a_frame_folder_and_info(self):
        rnd = random.Random(6)
        with work_dir() as d:
            px = [rnd.randrange(5) for _ in range(12 * 8)]
            images = []
            for n in range(10):
                px = list(px)
                px[rnd.randrange(96)] = rnd.randrange(5)
                images.append(px)
                with open(os.path.join(d, f"frame-{n:05d}.png"), "wb") as f:
                    f.write(rgb_png(12, 8, px))
            out = os.path.join(d, "clip.gif")
            text = io.StringIO()
            with contextlib.redirect_stdout(text):
                code = pm.main(["gif", out, "--dir", d, "--from", "2", "--to", "8",
                                "--every", "3", "--fps", "30", "--scale", "2",
                                "--hold-last", "50"])
            self.assertEqual(code, 0)
            line = text.getvalue().strip()
            self.assertRegex(line, r"^frames in 3, frames written [123], \d+ bytes, 0\.80 s$")
            with open(out, "rb") as f:
                gif = read_gif(f.read())
            ref = [images[2], images[5], images[8]]
            got = [fr["pixels"] for fr in gif["frames"]]
            want = []
            for p in ref:
                s = scale_ref(12, 8, p, 2)
                if not want or want[-1] != s:
                    want.append(s)
            self.assertEqual(got, want)

            text = io.StringIO()
            with contextlib.redirect_stdout(text):
                self.assertEqual(pm.main(["info", out, os.path.join(d, "frame-00000.png")]), 0)
            lines = text.getvalue().splitlines()
            self.assertIn("GIF 24x16", lines[0])
            self.assertIn("0.80 s", lines[0])
            self.assertIn("loops for ever", lines[0])
            self.assertIn(f"{len(set(images[2]) | set(images[5]) | set(images[8]))} colours",
                          lines[0])
            self.assertIn("PNG 12x8", lines[1])

            # an explicit list of files works the same way
            text = io.StringIO()
            files = [os.path.join(d, f"frame-{n:05d}.png") for n in (2, 5, 8)]
            with contextlib.redirect_stdout(text):
                self.assertEqual(pm.main(["gif", out, *files, "--every", "3"]), 0)
            self.assertIn("frames in 3", text.getvalue())

    def test_missing_frame_names_the_file(self):
        with work_dir() as d:
            with open(os.path.join(d, "frame-00000.png"), "wb") as f:
                f.write(rgb_png(1, 1, [0]))
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                code = pm.main(["gif", os.path.join(d, "x.gif"), "--dir", d,
                                "--from", "0", "--to", "1"])
            self.assertEqual(code, 1)
            self.assertIn("frame-00001.png", err.getvalue())
            self.assertFalse(os.path.exists(os.path.join(d, "x.gif")))

    def test_bad_input_is_an_error_not_a_crash(self):
        with work_dir() as d:
            p = os.path.join(d, "bad.png")
            with open(p, "wb") as f:
                f.write(make_png(1, 1, 2, [bytes(6)], [0], depth=16))
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                self.assertEqual(pm.main(["still", p, os.path.join(d, "o.png")]), 1)
            self.assertIn("bit depth 16", err.getvalue())


@contextlib.contextmanager
def work_dir():
    """A temporary folder inside this tool's folder, removed afterwards."""
    d = tempfile.mkdtemp(prefix=".test-", dir=HERE)
    try:
        yield d
    finally:
        shutil.rmtree(d)


if __name__ == "__main__":
    unittest.main()
