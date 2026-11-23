#!/usr/bin/env python3
"""Looks for data in a game's code slot that code could read bytewise.

    tools/code-slot-audit.py game/target/images/HELLO.elf [more.elf ...]

The code slot is executable RAM, which stops the program when it is read
in pieces smaller than a 32-bit word. So a game may keep only two things
there: instructions, and the "literals" (32-bit constants and addresses)
that the instruction `l32r` loads as whole words.

This reads the disassembly (Xtensa objdump must be on the PATH) and
checks, for every ELF given:

  1. every word that an `l32r` loads, and that is an address in the code
     slot, is the address of an instruction (a function, or a place the
     code jumps to), never of a literal. An address of a literal held in
     a register is the one way code could go on to read literals in
     pieces;
  2. no symbol in the code slot is a data object;
  3. how many instructions read 8 or 16 bits at all (they are fine as
     long as they read the data slot; the board run proves they do, since
     one that read the code slot would have stopped the program).

Worked out on the PC from the ELF file. It cannot prove that a pointer
computed at run time never points into the code slot.
"""

import re
import subprocess
import sys

CODE = range(0x4009_0000, 0x400A_0000)
DATA = range(0x3FFF_4000, 0x4000_0000)


def run(*command):
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout


def audit(elf) -> bool:
    disassembly = run("xtensa-esp32-elf-objdump", "-d", elf)
    instructions = {}  # address -> text
    for line in disassembly.splitlines():
        found = re.match(r"^([0-9a-f]{8}):\s+[0-9a-f]+\s+(\S+)\s*(.*)$", line)
        if found:
            instructions[int(found.group(1), 16)] = (found.group(2), found.group(3))

    literal_addresses = set()
    loads = []
    for address, (name, operands) in instructions.items():
        if name == "l32r":
            # "a8, 40090004 <...> (3fff4000 <...>)": pool address, then value
            found = re.search(r",\s*([0-9a-f]{8})\b.*?\(([0-9a-f]+)", operands)
            if found:
                literal_addresses.add(int(found.group(1), 16))
                loads.append((address, int(found.group(1), 16), int(found.group(2), 16)))

    into_code = [(at, value) for at, _, value in loads if value in CODE]
    bad = [(at, value) for at, value in into_code if value in literal_addresses or value not in instructions]
    kinds = {"code slot": len(into_code), "data slot": 0, "ROM": 0, "plain number": 0}
    for _, _, value in loads:
        if value in DATA:
            kinds["data slot"] += 1
        elif 0x4000_0000 <= value < 0x4006_0000:
            kinds["ROM"] += 1
        elif value not in CODE:
            kinds["plain number"] += 1

    symbols = run("xtensa-esp32-elf-readelf", "-s", "-W", elf)
    objects = [line for line in symbols.splitlines() if re.search(r"\b4009[0-9a-f]{4}\b.*\bOBJECT\b", line)]
    narrow = sum(name in ("l8ui", "l16ui", "l16si", "s8i", "s16i") for name, _ in instructions.values())

    print(f"== {elf}")
    print(f"  {len(instructions)} instructions, {len(literal_addresses)} literals, {len(loads)} l32r loads:")
    print("   ", ", ".join(f"{count} hold a {kind}" for kind, count in kinds.items()))
    print(f"  literals that are code slot addresses but not the address of an instruction: {len(bad)}")
    for at, value in bad:
        print(f"    l32r at {at:#010x} loads {value:#010x}")
    print(f"  data objects in the code slot: {len(objects)}")
    print(f"  instructions that read or write 8 or 16 bits: {narrow} (they must only ever be given data slot addresses)")
    ok = not bad and not objects
    print("  RESULT:", "nothing in the code slot that code is set up to read in pieces" if ok else "LOOK AT THIS")
    return ok


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    return 0 if all([audit(elf) for elf in sys.argv[1:]]) else 1


if __name__ == "__main__":
    sys.exit(main())
