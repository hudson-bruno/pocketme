#!/usr/bin/env python3
"""Packs one linked game into the file the console loads (GAME.BIN).

  pack.py PAYLOAD CODE DATA BSS ENTRY OUT CODE_ADDR DATA_ADDR
          [--flash FCODE_ADDR CODE.BIN FRO_ADDR RO.BIN]

PAYLOAD  the game's bytes as `objcopy -O binary` lays them out: the code,
         then the data, one after the other (the linker script games/chip/
         game.x makes them follow each other).
CODE     bytes of code (the .text section). Since stage 1 it may be more
         than the code slot: the rest runs on into the top of the data
         slot (the spill). Nothing changes here: the header simply says
         that many bytes of code, and the console's loader puts the part
         past the slot where it belongs (game_api::layout::fits is the
         rule; build-chip.sh and game.x refuse a game that breaks it).
DATA     bytes of constants and starting values (.rodata + .data).
BSS      bytes the console sets to zero after the data (.bss).
ENTRY    the address of game_entry, in hex.
OUT      the file to write.
CODE_ADDR, DATA_ADDR
         where the two slots start, in hex: the numbers the game was
         linked for (build-chip.sh reads them from the slots.x it made
         from crates/game-api's layout; this file knows no slot address).

The file is the header of crates/game-api/src/file.rs (GameHeader): nine
little-endian 32-bit numbers (the word GAME, the version, where the code
goes and its size, where the data goes and its size, the zeroed size, the
entry, and a CRC-32 of all that came before plus the payload), then the
payload. Called by games/build-chip.sh.

--flash (a game with a flash part, games/<name>/flash.list): the file
starts with "GAMF" instead, and the 32-byte extension of game_api's
FlashPartHeader follows the header: its size (32), the code half's
address, size and CRC-32 (CODE.BIN, the bytes of .flash_text), the data
half's (RO.BIN, .flash_rodata; size 0 and CRC 0 when empty), and a
reserved 0. The CRC in the header then covers bytes 0..32, the extension
and the payload. The halves themselves are not in the file: they go into
the firmware. Without --flash the file is byte for byte what it was.
"""
import struct
import sys
import zlib

MAGIC = int.from_bytes(b"GAME", "little")
MAGIC_FLASH = int.from_bytes(b"GAMF", "little")
# The game contract's version (game_api::API_VERSION).
API_VERSION = 3


def main():
    args = sys.argv[1:]
    flash = None
    if len(args) == 13 and args[8] == "--flash":
        flash = args[9:]
        args = args[:8]
    if len(args) != 8:
        sys.exit(__doc__)
    payload_path, code, data, bss, entry, out, code_addr, data_addr = args
    code, data, bss, entry = int(code), int(data), int(bss), int(entry, 16)
    code_addr, data_addr = int(code_addr, 16), int(data_addr, 16)
    with open(payload_path, "rb") as f:
        payload = f.read()
    if len(payload) != code + data:
        sys.exit(f"pack: the payload has {len(payload)} bytes, "
                 f"but code + data is {code + data}")
    for name, v in (("code", code), ("data", data), ("bss", bss)):
        if v % 4:
            sys.exit(f"pack: the {name} size {v} is not a whole number of words")
    if not code_addr <= entry < code_addr + code:
        sys.exit(f"pack: game_entry {entry:#x} is not inside the code")
    ext = b""
    if flash is not None:
        fcode_addr, fro_addr = int(flash[0], 16), int(flash[2], 16)
        fcode = open(flash[1], "rb").read()
        fro = open(flash[3], "rb").read()
        if not fcode or len(fcode) % 4 or len(fro) % 4 or fcode_addr % 4 or fro_addr % 4:
            sys.exit(f"pack: the flash halves ({len(fcode)} and {len(fro)} bytes) must be "
                     "whole words, and the code half not empty")
        crc_of = lambda b: zlib.crc32(b) & 0xFFFF_FFFF if b else 0  # noqa: E731
        ext = struct.pack("<8I", 32, fcode_addr, len(fcode), crc_of(fcode),
                          fro_addr, len(fro), crc_of(fro), 0)
    head = struct.pack("<8I", MAGIC_FLASH if ext else MAGIC, API_VERSION, code_addr, code,
                       data_addr, data, bss, entry)
    crc = zlib.crc32(head + ext + payload) & 0xFFFF_FFFF
    with open(out, "wb") as f:
        f.write(head + struct.pack("<I", crc) + ext + payload)


main()
