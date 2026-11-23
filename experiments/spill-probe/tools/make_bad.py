#!/usr/bin/env python3
"""Makes the two bad probe files from SPILL.BIN (experiments/spill-probe).

  make_bad.py SLOTS_X SPILL_BIN OUT_DIR

Writes, each with a correct CRC-32, so that only the sizes are wrong:

SPILLBAD.BIN  SPILL with its zeroed size (bss) raised so that its data
              meets its spill by one word (4 bytes). The console must
              refuse it before it writes anything: "GAME TOO BIG FOR
              SLOTS" (game_loader::LoadError::CodeMeetsData).
SPILLBIG.BIN  SPILL with its code made one word longer than the code slot
              and the whole spill together (4 zero bytes put after the
              code, so the file still holds what the header says). The
              console must refuse it before it writes anything: "GAME
              CODE TOO BIG" (LoadError::CodeTooBig).

SLOTS_X is the slots.x that games/build-chip.sh made beside SPILL.BIN
(from crates/game-api's layout): every slot number comes from there, none
is written in this file. Test files only: never on a normal card.
"""
import os
import re
import struct
import sys
import zlib

NAMES = ("GAME_CODE_ORIGIN", "GAME_CODE_LENGTH", "GAME_DATA_ORIGIN",
         "GAME_DATA_LENGTH", "GAME_SPILL_ORIGIN", "GAME_SPILL_LENGTH")
# The header: eight little-endian 32-bit numbers, then the CRC-32
# (crates/game-api/src/file.rs, GameHeader; games/chip/pack.py).
HEAD = struct.Struct("<8I")
MAGIC = int.from_bytes(b"GAME", "little")


def slots(path):
    """The six slot numbers of a slots.x, by name."""
    found = {}
    for line in open(path):
        m = re.match(r"\s*(GAME_\w+)\s*=\s*(0x[0-9a-fA-F]+)\s*;", line)
        if m:
            found[m.group(1)] = int(m.group(2), 16)
    missing = [n for n in NAMES if n not in found]
    if missing:
        sys.exit(f"make_bad: {path} has no {', '.join(missing)}")
    return found


def pack(fields, payload):
    head = HEAD.pack(*fields)
    crc = zlib.crc32(head + payload) & 0xFFFF_FFFF
    return head + struct.pack("<I", crc) + payload


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    s = slots(sys.argv[1])
    with open(sys.argv[2], "rb") as f:
        game = f.read()
    out = sys.argv[3]

    fields = list(HEAD.unpack_from(game))
    magic, _version, code_addr, code, data_addr, data, bss, entry = fields
    payload = game[HEAD.size + 4:]
    (crc,) = struct.unpack_from("<I", game, HEAD.size)
    if magic != MAGIC or len(payload) != code + data:
        sys.exit("make_bad: SPILL_BIN is not a whole game file")
    if zlib.crc32(game[:HEAD.size] + payload) & 0xFFFF_FFFF != crc:
        sys.exit("make_bad: SPILL_BIN's CRC-32 is wrong")
    if code_addr != s["GAME_CODE_ORIGIN"] or data_addr != s["GAME_DATA_ORIGIN"]:
        sys.exit("make_bad: SPILL_BIN was built for other slots than SLOTS_X")
    spill = code - s["GAME_CODE_LENGTH"]
    if spill <= 0:
        sys.exit("make_bad: SPILL_BIN has no code past the code slot")
    if spill + data + bss > s["GAME_DATA_LENGTH"]:
        sys.exit("make_bad: SPILL_BIN itself does not fit")

    # SPILLBAD: spill + data + bss is one word more than the data slot.
    bad = list(fields)
    bad[6] = s["GAME_DATA_LENGTH"] - spill - data + 4
    assert spill + data + bad[6] == s["GAME_DATA_LENGTH"] + 4
    with open(os.path.join(out, "SPILLBAD.BIN"), "wb") as f:
        f.write(pack(bad, payload))

    # SPILLBIG: code one word more than the code slot and the spill.
    big = list(fields)
    big[3] = s["GAME_CODE_LENGTH"] + s["GAME_SPILL_LENGTH"] + 4
    grow = big[3] - code
    big_payload = payload[:code] + bytes(grow) + payload[code:]
    with open(os.path.join(out, "SPILLBIG.BIN"), "wb") as f:
        f.write(pack(big, big_payload))

    print(f"make_bad: SPILLBAD.BIN code {code} (spill {spill}), data {data}, "
          f"bss {bss} -> {bad[6]}: spill + data + bss = "
          f"{spill + data + bad[6]}, data slot {s['GAME_DATA_LENGTH']}")
    print(f"make_bad: SPILLBIG.BIN code {code} -> {big[3]} "
          f"(code slot {s['GAME_CODE_LENGTH']} + spill "
          f"{s['GAME_SPILL_LENGTH']} + 4)")


if __name__ == "__main__":
    main()
