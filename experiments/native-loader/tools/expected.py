#!/usr/bin/env python3
"""What the test game must report, worked out a second way.

The game (game/src/main.rs) runs on the chip. This script does the same
sums on the PC in plain Python. The console compares what the game reports
with the numbers in console/src/checks.rs, and those numbers were copied
from this script's output. Run it again whenever the game's sums change:

    python3 tools/expected.py
"""
import struct

M = 0xFFFFFFFF
PRIMES = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53]
SQUARES = [10000, 12321, 14884, 17689, 20736, 24025, 27556, 31329]
HELLO = b"hello from the game slot"
FRAMES = 300


def rotl(x, n):
    return ((x << n) | (x >> (32 - n))) & M


def bonus(kind):
    return {0: 10, 1: 250, 2: 7, 3: 4000, 4: 99, 5: 123456, 6: 31, 7: 8}.get(kind, 0)


def dispatch(kind, x):
    if kind == 0:
        return (x * 3) & M
    if kind == 1:
        return (x + 100) & M  # echo_sum on the console
    if kind == 2:
        return rotl(x, 5) ^ 0xA5A5
    if kind == 3:
        return SQUARES[x & 7]
    if kind == 4:
        return x // 7
    if kind == 5:
        return bin(x).count("1")
    if kind == 6:
        return PRIMES[x & 15]
    if kind == 7:
        return (x - 9) & M
    return 0


def trunc_div(a, b):
    """Division that rounds towards zero, as Rust's does."""
    q = abs(a) // abs(b)
    return q if (a < 0) == (b < 0) else -q


seed, divisor = 1_000_000 + 7, 1_000_000 + 3
cube = seed**3
assert cube < 2**63
quotient, remainder = divmod(cube, divisor)
signed = trunc_div(-cube, divisor) & 0xFFFFFFFFFFFFFFFF

score, hash_, trail = 1000, 0x12345678, [0] * 256
for frame in range(FRAMES):
    step = dispatch(frame % 10, frame)
    gain = bonus(frame % 9)
    score = (score + gain) & M
    trail[frame & 255] = PRIMES[frame & 15]
    hash_ = ((rotl(hash_, 5) ^ step) + gain) & M

rows = [
    (1, 1000, ".data arrives with its value (SCORE)"),
    (2, 0, ".bss is zero (UPDATES)"),
    (3, 0, ".bss block is all zero (TRAIL)"),
    (4, 42, "echo_sum(40, 2)"),
    (5, sum(PRIMES), "byte table summed"),
    (6, sum(SQUARES), "16-bit table summed"),
    (7, quotient & M, "64-bit quotient, low half"),
    (8, quotient >> 32, "64-bit quotient, high half"),
    (9, remainder, "64-bit remainder"),
    (10, signed & M, "signed 64-bit quotient, low half"),
    (11, struct.unpack("<I", struct.pack("<f", 16 * 1.5 + 0.25))[0], "float 24.25 as bits"),
    (12, 1, "64-bit clock read twice, in order"),
    (13, sum(HELLO[:24]), "memcpy in ROM: bytes copied and summed"),
    (20, FRAMES, "update calls counted in .bss"),
    (21, score, "SCORE after all frames (const match)"),
    (22, sum(trail), "TRAIL after all frames (byte writes)"),
    (23, hash_, "HASH after all frames (jump-table match)"),
]
for ident, value, what in rows:
    print(f"    Check {{ id: {ident}, expected: {value:#010x}, what: \"{what}\" }},")
