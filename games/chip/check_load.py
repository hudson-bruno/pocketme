#!/usr/bin/env python3
"""Loads a game file as the console does and checks the result.

  check_load.py ELF BIN SLOTS_X [--flash CODE.BIN RO.BIN]

The console reads the header of BIN (crates/game-api/src/file.rs), copies
the code to the code slot and the data to the data slot, each as one
block, and sets the next `bss` bytes to zero. This script does the same
in a model of the chip's memory, then compares every byte the linker
placed (ELF: each section that takes memory, at its run-time address)
with what the model holds. A difference means the game would run with
wrong bytes: a starting value moved, or a variable not set to zero.

Code past the code slot (the spill, stage 1) runs on into the TOP of the
data slot, seen from the instruction bus in reverse word order. The
model keeps one byte per piece of real memory, so spill code and data
share it as they do on the chip: a data or zero byte that lands on a
byte of code fails ("data over code"). This is the PC's own check of the
linker's ASSERT in game.x and of the loader's rule
(game_api::layout::fits), done a second way.

SLOTS_X is the slots.x build-chip.sh made from crates/game-api's layout:
every number comes from there, none is written in this file. The mirror
rule (instruction word I in the spill is data word
DATA_END - 4 - (I - SPILL_ORIGIN)) is game_api::layout::sram1_iram_to_dram,
kept in step by hand; it is quoted from ESP-IDF and was seen to hold on
the CYD (2026-11-22). slots.x also names the data words of the spill's first and
last word (GAME_SPILL_FIRST_DATA_WORD, _LAST_), written by that rule;
this script stops when its own copy gives others.

Why it exists: the linker may leave a small gap before a section that
needs 8-byte alignment. In memory that gap is there; in the file it is
not, so everything after it would land too early. Nothing else noticed.
(Added by the second reader, step 2. Called by games/build-chip.sh.)

A "GAMF" file (a game with a flash part): the payload starts after the
32-byte extension, and the ELF's .flash_text and .flash_rodata are not
loaded by the console but held by the firmware. They are left out of the
slot comparison; instead the extension must name exactly their
addresses, sizes and CRC-32s, the whole-file CRC must cover the
extension, and with --flash the two files the firmware embeds must be
those sections' bytes.
"""
import zlib
import re
import struct
import sys

SHF_ALLOC = 0x2
SHT_NOBITS = 8
NAMES = ("GAME_CODE_ORIGIN", "GAME_CODE_LENGTH", "GAME_DATA_ORIGIN",
         "GAME_DATA_LENGTH", "GAME_SPILL_ORIGIN", "GAME_SPILL_LENGTH",
         "GAME_SPILL_FIRST_DATA_WORD", "GAME_SPILL_LAST_DATA_WORD")


def slots(path):
    """The slot numbers of a slots.x (NAMES), by name."""
    found = {}
    for line in open(path):
        m = re.match(r"\s*(GAME_\w+)\s*=\s*(0x[0-9a-fA-F]+)\s*;", line)
        if m:
            found[m.group(1)] = int(m.group(2), 16)
    missing = [n for n in NAMES if n not in found]
    if missing:
        sys.exit(f"check_load: {path} has no {', '.join(missing)}")
    return found


def sections(elf):
    """(name, address, bytes or None for zeroed, size) of each section
    that takes memory."""
    if elf[:4] != b"\x7fELF" or elf[4] != 1 or elf[5] != 1:
        sys.exit("check_load: not a 32-bit little-endian ELF file")
    shoff, = struct.unpack_from("<I", elf, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", elf, 0x2E)
    heads = [struct.unpack_from("<10I", elf, shoff + i * shentsize) for i in range(shnum)]
    names = heads[shstrndx]
    out = []
    for h in heads:
        name_off, kind, flags, addr, off, size = h[:6]
        if not flags & SHF_ALLOC or size == 0:
            continue
        end = elf.index(b"\0", names[4] + name_off)
        name = elf[names[4] + name_off:end].decode()
        body = None if kind == SHT_NOBITS else elf[off:off + size]
        out.append((name, addr, body, size))
    return out


FLASH_SECTIONS = (".flash_text", ".flash_rodata")


def check_flash(what, game, secs, files):
    """The extension of a GAMF file against the ELF (and the files the
    firmware embeds). Returns where the payload starts."""
    head = struct.unpack_from("<9I", game, 0)
    if len(game) < 68:
        sys.exit(f"check_load: {what}: cut inside the flash extension")
    ext = struct.unpack_from("<8I", game, 36)
    crc = zlib.crc32(game[:32] + game[36:]) & 0xFFFF_FFFF
    if crc != head[8]:
        sys.exit(f"check_load: {what}: the CRC does not cover 0..32 + the extension + payload")
    by = {n: (a, b) for n, a, b, _s in secs}
    if ".flash_text" not in by:
        sys.exit(f"check_load: {what}: a GAMF file, but the ELF has no .flash_text")
    code_addr, code = by[".flash_text"]
    ro_addr, ro = by.get(".flash_rodata", (ext[4], b""))
    crc_of = lambda b: zlib.crc32(b) & 0xFFFF_FFFF if b else 0  # noqa: E731
    want = (32, code_addr, len(code), crc_of(code), ro_addr, len(ro), crc_of(ro), 0)
    if ext != want:
        sys.exit(f"check_load: {what}: the flash extension {[hex(x) for x in ext]} "
                 f"does not name the ELF's flash sections {[hex(x) for x in want]}")
    if files:
        for path, body in zip(files, (code, ro)):
            if open(path, "rb").read() != body:
                sys.exit(f"check_load: {what}: {path} is not the ELF's flash section")
    return 68


def main():
    args = sys.argv[1:]
    files = None
    if len(args) == 6 and args[3] == "--flash":
        files = args[4:]
        args = args[:3]
    if len(args) != 3:
        sys.exit(__doc__)
    sys.argv[1:] = args
    elf = open(sys.argv[1], "rb").read()
    game = open(sys.argv[2], "rb").read()
    s = slots(sys.argv[3])
    what = sys.argv[2]
    code_org, code_len = s["GAME_CODE_ORIGIN"], s["GAME_CODE_LENGTH"]
    data_org, data_len = s["GAME_DATA_ORIGIN"], s["GAME_DATA_LENGTH"]
    spill_org, spill_len = s["GAME_SPILL_ORIGIN"], s["GAME_SPILL_LENGTH"]
    data_end = data_org + data_len
    if spill_org != code_org + code_len or spill_len > data_len:
        sys.exit(f"check_load: {sys.argv[3]}: the spill must follow the code "
                 "slot and be no bigger than the data slot")

    def place(addr):
        """The data-bus address of a spill byte (reverse word order, the
        bytes inside a word in order); any other address as it is."""
        if spill_org <= addr < spill_org + spill_len:
            word = (addr - spill_org) & ~3
            return data_end - 4 - word + (addr & 3)
        return addr

    # The copy of the rule must give the words game_api::layout gives.
    first, last = s["GAME_SPILL_FIRST_DATA_WORD"], s["GAME_SPILL_LAST_DATA_WORD"]
    if spill_len and (place(spill_org) != first
                      or place(spill_org + spill_len - 4) != last):
        sys.exit(f"check_load: {sys.argv[3]}: the spill's first and last word "
                 f"are data words {first:#x} and {last:#x}, but place() gives "
                 f"{place(spill_org):#x} and {place(spill_org + spill_len - 4):#x}: "
                 "its mirror rule differs from "
                 "game_api::layout::sram1_iram_to_dram")

    (magic, _version, code_addr, code_size, data_addr, data_size, bss_size,
     _entry, _crc) = struct.unpack_from("<9I", game, 0)
    secs = sections(elf)
    start = 36
    if magic == int.from_bytes(b"GAMF", "little"):
        start = check_flash(what, game, secs, files)
        secs = [x for x in secs if x[0] not in FLASH_SECTIONS]
    elif any(x[0] in FLASH_SECTIONS for x in secs) or files:
        sys.exit(f"check_load: {what}: the ELF has a flash part but the file is not GAMF")
    if code_addr != code_org or data_addr != data_org:
        sys.exit(f"check_load: {what}: built for other slots than {sys.argv[3]}")
    if code_size > code_len + spill_len:
        sys.exit(f"check_load: {what}: {code_size} bytes of code are over the "
                 f"code slot and the spill ({code_len + spill_len})")
    payload = game[start:]
    # The console's view: address -> byte, for every byte it writes, in
    # the order it writes them: code, data, zeroes.
    memory = {}
    code_bytes = set()
    for i, b in enumerate(payload[:code_size]):
        a = place(code_addr + i)
        memory[a] = b
        code_bytes.add(a)

    def put(at, b, kind):
        if at >= data_end:
            sys.exit(f"check_load: {what}: {kind} byte at {at:#x} is past the data slot")
        if at in code_bytes:
            sys.exit(f"check_load: {what}: data over code: a {kind} byte lands at "
                     f"{at:#x}, on code past the code slot "
                     f"(spill {max(0, code_size - code_len)} bytes, "
                     f"data {data_size} + zeroed {bss_size}, room {data_len})")
        memory[at] = b

    for i, b in enumerate(payload[code_size:code_size + data_size]):
        put(data_addr + i, b, "data")
    for i in range(bss_size):
        put(data_addr + data_size + i, 0, "zeroed")
    bad = 0
    for name, addr, body, size in secs:
        want = body if body is not None else bytes(size)
        for i in range(size):
            got = memory.get(place(addr + i))
            if got != want[i]:
                print(f"check_load: {what}: {name} byte at {addr + i:#x} "
                      f"should be {want[i]:#04x}, the console would load "
                      f"{'nothing' if got is None else hex(got)}", file=sys.stderr)
                bad += 1
                break
    if bad:
        sys.exit(f"check_load: {what}: {bad} section(s) would load wrongly")


main()
