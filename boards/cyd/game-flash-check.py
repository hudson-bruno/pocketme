#!/usr/bin/env python3
"""Checks the game flash windows of a built firmware (README "Game flash
parts", feature `game-flash`).

usage: game-flash-check.py <elf> [<game files>]   a build WITH game-flash
       game-flash-check.py <elf> --absent          a build WITHOUT it

<game files> is the folder games/build-chip.sh wrote (default:
boards/cyd/target/chip-games/files, or CYD_GAME_FILES when set, as
build.rs reads it). The window numbers are read from
crates/game-api/src/file.rs (game_api::layout, the only place they are
written) and compared with the ELF.

With game-flash it fails (exit 1) when
  - `.game_flash_code` is not at layout::FLASH_CODE_ADDR with
    layout::FLASH_CODE_SIZE bytes, or `.text` does not start right where
    it ends (espflash would make a second flash code segment, and the
    bootloader maps only one);
  - `.game_flash_ro` is not at layout::FLASH_RO_ADDR with
    layout::FLASH_RO_SIZE bytes, or `.rodata_merge`/`.rodata` do not
    start right where it ends (the same for flash data);
  - the last word of a window is not its end mark ("GFCW", "GFRW");
  - a part in <game files>/flash/<NAME>/ (CODE.BIN, RO.BIN) is not in the
    window byte for byte at its `_game_flash_{code,ro}_<NAME>_start`, or
    the ELF holds a part that has no files there;
  - a byte of a window outside its parts and its end mark is not zero.
With --absent it fails when any `.game_flash_*` section or
`_game_flash_*` symbol is in the ELF.

The last line is the summary and ends `ok` or `FAILED`. Reads files only
(pure Python, no tools).
"""

import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
FILE_RS = os.path.join(HERE, "..", "..", "crates", "game-api", "src", "file.rs")
END_MARKS = {"code": b"GFCW", "ro": b"GFRW"}


def layout_consts():
    """FLASH_CODE_ADDR/SIZE and FLASH_RO_ADDR/SIZE from game_api::layout."""
    text = open(FILE_RS, encoding="utf-8").read()
    out = {}
    for name in ("FLASH_CODE_ADDR", "FLASH_CODE_SIZE", "FLASH_RO_ADDR", "FLASH_RO_SIZE"):
        m = re.search(r"pub const %s: u32 = (0x[0-9A-Fa-f_]+|[0-9_]+);" % name, text)
        if not m:
            sys.exit("game-flash-check: %s not found in %s" % (name, FILE_RS))
        out[name] = int(m.group(1).replace("_", ""), 0)
    return out


class Elf:
    """Just enough of a 32-bit little-endian ELF: sections and symbols."""

    def __init__(self, path):
        self.data = open(path, "rb").read()
        d = self.data
        if d[:4] != b"\x7fELF" or d[4] != 1 or d[5] != 1:
            sys.exit("game-flash-check: %s is not a 32-bit little-endian ELF" % path)
        shoff, = struct.unpack_from("<I", d, 0x20)
        shentsize, shnum, shstrndx = struct.unpack_from("<HHH", d, 0x2E)
        raw = []
        for i in range(shnum):
            raw.append(struct.unpack_from("<IIIIIIIIII", d, shoff + i * shentsize))
        names = raw[shstrndx]
        self.sections = {}
        self.by_index = []
        for s in raw:
            name = self._str(names[4], s[0])
            sec = {"name": name, "type": s[1], "addr": s[3], "offset": s[4], "size": s[5],
                   "link": s[6], "entsize": s[9]}
            self.by_index.append(sec)
            self.sections.setdefault(name, sec)
        self.symbols = {}
        for sec in self.by_index:
            if sec["type"] != 2:  # SHT_SYMTAB
                continue
            strtab = self.by_index[sec["link"]]
            for at in range(sec["offset"], sec["offset"] + sec["size"], 16):
                st_name, value, _size, _info, _other, _shndx = struct.unpack_from("<IIIBBH", d, at)
                if st_name:
                    self.symbols.setdefault(self._str(strtab["offset"], st_name), value)

    def _str(self, table, at):
        start = table + at
        end = self.data.index(b"\0", start)
        return self.data[start:end].decode("utf-8", "replace")

    def bytes_of(self, sec):
        if sec["type"] == 8:  # SHT_NOBITS
            return b"\0" * sec["size"]
        return self.data[sec["offset"]:sec["offset"] + sec["size"]]


def game_files_dir():
    env = os.environ.get("CYD_GAME_FILES")
    return env if env else os.path.join(HERE, "target", "chip-games", "files")


def main(argv):
    if len(argv) < 2 or len(argv) > 3:
        sys.exit(__doc__)
    elf = Elf(argv[1])
    name = os.path.basename(argv[1])
    faults = []

    if len(argv) == 3 and argv[2] == "--absent":
        secs = sorted(n for n in elf.sections if n.startswith(".game_flash"))
        syms = sorted(n for n in elf.symbols if n.startswith("_game_flash"))
        if secs:
            faults.append("sections %s are in a build without game-flash" % ", ".join(secs))
        if syms:
            faults.append("%d _game_flash symbols in a build without game-flash (first %s)"
                          % (len(syms), syms[0]))
        for f in faults:
            print("FAULT: " + f)
        print("game-flash-check: %s (--absent): %s" % (
            name, "FAILED" if faults else "no game flash section or symbol: ok"))
        return 1 if faults else 0

    files = argv[2] if len(argv) == 3 else game_files_dir()
    lay = layout_consts()
    parts_dir = os.path.join(files, "flash")
    want_parts = sorted(
        n for n in (os.listdir(parts_dir) if os.path.isdir(parts_dir) else [])
        if os.path.isfile(os.path.join(parts_dir, n, "CODE.BIN")))
    summary = []
    for kind, addr_key, size_key, follow in (
            ("code", "FLASH_CODE_ADDR", "FLASH_CODE_SIZE", (".text",)),
            ("ro", "FLASH_RO_ADDR", "FLASH_RO_SIZE", (".rodata_merge", ".rodata"))):
        secname = ".game_flash_" + kind
        sec = elf.sections.get(secname)
        addr, size = lay[addr_key], lay[size_key]
        if sec is None:
            faults.append("no section %s" % secname)
            continue
        if sec["addr"] != addr or sec["size"] != size:
            faults.append("%s is %#x + %#x, game_api::layout says %#x + %#x"
                          % (secname, sec["addr"], sec["size"], addr, size))
        for f in follow:
            nxt = elf.sections.get(f)
            if nxt is None:
                faults.append("no section %s" % f)
            elif nxt["addr"] != sec["addr"] + sec["size"]:
                faults.append("%s starts at %#x, not right after %s (%#x): two flash segments"
                              % (f, nxt["addr"], secname, sec["addr"] + sec["size"]))
        body = bytearray(elf.bytes_of(sec))
        if bytes(body[-4:]) != END_MARKS[kind]:
            faults.append("%s does not end with its end mark %r (found %r)"
                          % (secname, END_MARKS[kind], bytes(body[-4:])))
        body[-4:] = b"\0\0\0\0"
        used = 0
        held = sorted(
            s[len("_game_flash_%s_" % kind):-len("_start")] for s in elf.symbols
            if s.startswith("_game_flash_%s_" % kind) and s.endswith("_start"))
        for game in held:
            if game not in want_parts:
                faults.append("%s holds a part of %s, which has no files in %s"
                              % (secname, game, parts_dir))
        for game in want_parts:
            path = os.path.join(parts_dir, game, "CODE.BIN" if kind == "code" else "RO.BIN")
            want = open(path, "rb").read() if os.path.isfile(path) else b""
            start = elf.symbols.get("_game_flash_%s_%s_start" % (kind, game))
            if not want:
                if start is not None:
                    faults.append("%s holds a %s part of %s, but %s is empty" % (secname, kind, game, path))
                continue
            if start is None:
                faults.append("%s holds no %s part of %s (%s has %d bytes)"
                              % (secname, kind, game, path, len(want)))
                continue
            off = start - sec["addr"]
            got = bytes(body[off:off + len(want)])
            if off < 0 or got != want:
                faults.append("%s part of %s at %#x is not %s byte for byte" % (kind, game, start, path))
            body[max(off, 0):max(off, 0) + len(want)] = b"\0" * len(want)
            used += len(want)
            summary.append("%s %s %#x+%d" % (game, kind, start, len(want)))
        nonzero = sum(1 for b in body if b)
        if nonzero:
            faults.append("%s: %d bytes outside the parts and the end mark are not zero"
                          % (secname, nonzero))
        summary.append("%s window %#x+%d (%d used)" % (kind, addr, size, used))

    loaded = 0
    d = elf.data
    phoff, = struct.unpack_from("<I", d, 0x1C)
    phentsize, phnum = struct.unpack_from("<HH", d, 0x2A)
    for i in range(phnum):
        p_type, _off, _va, _pa, filesz, _memsz, _flags, _align = struct.unpack_from(
            "<IIIIIIII", d, phoff + i * phentsize)
        if p_type == 1:
            loaded += filesz
    summary.append("loaded bytes %d" % loaded)
    for f in faults:
        print("FAULT: " + f)
    print("game-flash-check: %s: %s: %s" % (name, "; ".join(summary), "FAILED" if faults else "ok"))
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
