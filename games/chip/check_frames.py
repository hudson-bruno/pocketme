#!/usr/bin/env python3
"""Reads the stack frames of a linked game and stops on one too big.

  check_frames.py ELF NAME TABLE

Disassembles ELF with xtensa-esp32-elf-objdump -d -C and reads the first
instruction of each function. On the ESP32 (windowed calls) that is
`entry a1, N` (objdump may print `sp` for a1, and N in hex): the function
takes N bytes of stack for its own frame. A frame over 32760 bytes does
not fit in `entry`; the compiler then writes `entry a1, 32`, loads the
rest into a register (movi or l32r) and moves the stack pointer
(`movsp a1, aX`). Both forms are read. A function whose frame cannot be
read counts as too big.

Writes every function's frame, biggest first, to TABLE, prints the three
biggest, and ends with 1 when a frame is over its limit (LIMITS below).
Called by games/build-chip.sh after each game is linked; its self-test is
chip/test_check_frames.py (no chip tools needed), which build-chip.sh
runs first. Standard library only.

What it does not see: frames add up along a chain of calls (update ->
draw -> fill ...), and this script reads each frame alone. Passing it does
not prove that a game fits the console's stack.
"""
import re
import subprocess
import sys

# The limits, in bytes. ESTIMATES, to be confirmed on the board.
# Where they come from: the console's stack is 29,200 bytes, and the
# console itself had used 16,752 of it at its deepest before any game ran
# (both measured on the CYD earlier), which leaves about 12,400 for a
# game's deepest chain of calls. The limits are a guard against one known
# mistake (the game's whole state built inside the frame of the update
# call: a 10.8 KB state made that frame 25.8 KB, found on the board on
# 2026-11-22; game_kit::rt::restart now builds it in a frame of its own,
# given back before the first update), not a proof that a game fits.
# Any function whose demangled name contains RESTART_NAME (the state is
# built there, once, before the game's own calls run).
RESTART_NAME = "game_kit::rt::restart"
RESTART_LIMIT = 16384
# Every other function.
OTHER_LIMIT = 4096
# Instructions after `entry` searched for the big-frame form.
BIG_FRAME_WINDOW = 6

OBJDUMP = "xtensa-esp32-elf-objdump"

LABEL = re.compile(r"^([0-9a-f]+) <(.*)>:\s*$")
INSN = re.compile(r"^\s*([0-9a-f]+):\s+(?:[0-9a-f]+ ?)+\s+(\S+)\s*(.*)$")


def number(text):
    """An immediate as objdump prints it: 48, -20 or 0x2a90."""
    text = text.strip()
    return int(text, 16) if text.lower().startswith(("0x", "-0x")) else int(text)


def is_sp(reg):
    return reg in ("a1", "sp")


def frame_of(insns):
    """The frame of a function from its first instructions (list of
    (mnemonic, operands)): bytes, or None when it cannot be read, or
    False when the function has no `entry` (not a windowed function:
    literal data, or a function called with call0)."""
    if not insns or insns[0][0] != "entry":
        return False
    ops = [o.strip() for o in insns[0][1].split(",")]
    if len(ops) != 2 or not is_sp(ops[0]):
        return None
    try:
        size = number(ops[1])
    except ValueError:
        return None
    # The big-frame form: entry a1, 32; movi/l32r aX, REST (perhaps
    # movi then addmi); sub aX, a1, aX; movsp a1, aX. Any other write to
    # aX before the movsp leaves the frame unread (None), never smaller.
    loads = {}
    for mnem, operands in insns[1:1 + BIG_FRAME_WINDOW]:
        parts = [o.strip() for o in operands.split(",")]
        if mnem in ("movi", "movi.n") and len(parts) == 2:
            try:
                loads[parts[0]] = number(parts[1])
            except ValueError:
                loads.pop(parts[0], None)
        elif mnem in ("addmi", "addi", "addi.n") and len(parts) == 3:
            # movi and addmi together make a value too big for movi alone.
            try:
                base = loads.get(parts[1])
                if base is None:
                    raise ValueError
                loads[parts[0]] = base + number(parts[2])
            except ValueError:
                loads.pop(parts[0], None)
        elif mnem == "l32r" and parts:
            m = re.search(r"\((?:0x)?([0-9a-f]+)", operands)
            if m:
                loads[parts[0]] = int(m.group(1), 16)
            else:
                loads.pop(parts[0], None)
        elif mnem == "movsp" and len(parts) == 2 and is_sp(parts[0]):
            rest = loads.get(parts[1])
            return None if rest is None else size + rest
        elif mnem == "sub" and len(parts) == 3 and parts[0] == parts[2] and is_sp(parts[1]):
            pass  # sub aX, a1, aX: aX still names the amount moved
        elif parts and parts[0] in loads:
            # Anything else that may write aX (or only reads it: then
            # the frame is left unread, on the safe side).
            loads.pop(parts[0])
    return size


def frames(text):
    """(frame, address, name) of each function in an objdump -d text, in
    the order found; frame is None when it cannot be read. Functions
    without `entry` are left out."""
    out = []
    current = None
    insns = []

    def close():
        if current is not None:
            f = frame_of(insns)
            if f is not False:
                out.append((f, current[0], current[1]))

    for line in text.splitlines():
        m = LABEL.match(line)
        if m:
            close()
            current = (int(m.group(1), 16), m.group(2))
            insns = []
            continue
        if current is None or len(insns) > BIG_FRAME_WINDOW:
            continue
        m = INSN.match(line)
        if m:
            insns.append((m.group(2), m.group(3)))
    close()
    return out


def limit_of(name):
    return RESTART_LIMIT if RESTART_NAME in name else OTHER_LIMIT


def over(found):
    """The entries of `found` over their limit (unreadable ones too)."""
    return [(f, a, n) for f, a, n in found if f is None or f > limit_of(n)]


def biggest_first(found):
    return sorted(found, key=lambda e: (-(1 << 30) if e[0] is None else -e[0], e[1]))


def table(found):
    lines = ["# frame (bytes), limit, address, function; biggest first",
             f"# limits: {RESTART_LIMIT} for {RESTART_NAME}, {OTHER_LIMIT} for every other function (estimates)"]
    for f, a, n in biggest_first(found):
        size = "?" if f is None else str(f)
        mark = "  TOO BIG" if f is None or f > limit_of(n) else ""
        lines.append(f"{size:>6} {limit_of(n):>6} {a:08x} {n}{mark}")
    return "\n".join(lines) + "\n"


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    elf, name, out = sys.argv[1:]
    try:
        text = subprocess.run([OBJDUMP, "-d", "-C", elf], check=True,
                              capture_output=True, text=True).stdout
    except (OSError, subprocess.CalledProcessError) as e:
        sys.exit(f"check_frames: {name}: {OBJDUMP} failed: {e}")
    found = frames(text)
    if not found:
        sys.exit(f"check_frames: {name}: no function with `entry` found in {elf}")
    with open(out, "w") as f:
        f.write(table(found))
    top = "; ".join(f"{'?' if f is None else f} {n}" for f, _a, n in biggest_first(found)[:3])
    print(f"{name:<8} biggest stack frames: {top}")
    sys.stdout.flush()
    bad = over(found)
    for f, a, n in bad:
        what = "cannot be read" if f is None else f"is {f} bytes"
        print(f"build-chip: {name}: STACK FRAME TOO BIG: the frame of {n} "
              f"(at {a:#x}) {what}, the limit is {limit_of(n)} "
              f"(games/chip/check_frames.py; table in {out})", file=sys.stderr)
    if bad:
        sys.exit(1)


if __name__ == "__main__":
    main()
