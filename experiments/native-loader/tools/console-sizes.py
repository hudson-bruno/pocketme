#!/usr/bin/env python3
"""Says how much of the chip's RAM a console build uses and leaves.

    tools/console-sizes.py console/target/builds/plain.elf [more.elf ...]

Reads the section table and a few linker symbols from the ELF file (with
the Xtensa readelf and nm, which must be on the PATH: `. ~/export-esp.sh`)
and prints, for each of the three blocks of RAM, who has what.

Everything here is worked out from the file on the PC. Nothing is
measured on the board.
"""

import subprocess
import sys

EXEC_START, EXEC_END = 0x4008_0000, 0x400A_0000  # executable RAM, 128 KB
DRAM_END = 0x3FFE_0000  # end of the first block of data RAM
DRAM2_START, DRAM2_END = 0x3FFE_7E30, 0x4000_0000  # second block of data RAM


def sections(elf):
    out = subprocess.run(["xtensa-esp32-elf-readelf", "-S", "-W", elf], check=True, capture_output=True, text=True).stdout
    found = {}
    for line in out.splitlines():
        parts = line.replace("[", " ").replace("]", " ").split()
        if len(parts) >= 6 and parts[0].isdigit() and parts[1].startswith("."):
            found[parts[1]] = (int(parts[3], 16), int(parts[5], 16))
    return found


def symbols(elf):
    out = subprocess.run(["xtensa-esp32-elf-nm", elf], check=True, capture_output=True, text=True).stdout
    return {parts[2]: int(parts[0], 16) for parts in (line.split() for line in out.splitlines()) if len(parts) == 3}


def report(elf):
    sec, sym = sections(elf), symbols(elf)
    size = lambda name: sec.get(name, (0, 0))[1]
    code_slot, code_slot_size = sec[".game_code"]
    data_slot, data_slot_size = sec[".game_data"]
    rwtext_end = sym["_rwtext_end"]
    stack_low, stack_top = sym["_stack_end_cpu0"], sym["_stack_start_cpu0"]
    dram_start = sec[".data"][0]
    heap = size(".dram2_uninit")

    print(f"== {elf}")
    print(f"  flash: code {size('.text')}, constants {size('.rodata') + size('.rodata.wifi')}")
    print(f"  executable RAM {EXEC_START:#x}..{EXEC_END:#x} ({EXEC_END - EXEC_START} bytes):")
    print(f"    console: vectors {size('.vectors')} + its own RAM code {size('.rwtext')} + the radio's RAM code {size('.rwtext.wifi')}"
          f" = {rwtext_end - EXEC_START} bytes, ends at {rwtext_end:#x}")
    print(f"    free between the console and the code slot: {code_slot - rwtext_end} bytes")
    print(f"    game code slot {code_slot:#x}..{code_slot + code_slot_size:#x}: {code_slot_size} bytes")
    print(f"  data RAM, first block, {dram_start:#x}..{DRAM_END:#x} ({DRAM_END - dram_start} bytes for this build"
          f"{'; the 64 KB below it are kept for the radio ROM code' if dram_start > 0x3FFB0000 else ''}):")
    print(f"    console: .data {size('.data') + size('.data.wifi')}, .bss {size('.bss')}, .noinit {size('.noinit')}"
          f" = {stack_low - dram_start} bytes")
    print(f"    main stack: all the rest, {stack_top - stack_low} bytes")
    print(f"  data RAM, second block, {DRAM2_START:#x}..{DRAM2_END:#x} ({DRAM2_END - DRAM2_START} bytes):")
    print(f"    console heap {heap} bytes, game data slot {data_slot:#x}..{data_slot + data_slot_size:#x}: {data_slot_size} bytes,"
          f" free between them: {data_slot - (DRAM2_START + heap)} bytes")


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    for elf in sys.argv[1:]:
        report(elf)
    return 0


if __name__ == "__main__":
    sys.exit(main())
