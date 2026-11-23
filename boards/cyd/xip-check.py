#!/usr/bin/env python3
"""Checks the flash code probe of a firmware built with the measuring
feature `xip-probe` (src/xip_probe.rs, README "Flash code probe").

usage: xip-check.py <elf>            a build WITH xip-probe
       xip-check.py <elf> --absent   a build WITHOUT it: no trace of it

It fails (exit 1) when
  1. a symbol is missing: _cyd_xip_probe_start, _cyd_xip_probe_end,
     cyd_xip_probe_run, cyd_xip_probe_run_quiet, CYD_XIP_PROBE_LAYOUT,
     _cyd_hot_start;
  2. the block is not 131072 bytes or does not start on a cache line;
  3. the block (with its stop `ret.n`) is not inside the section .text
     and the flash code range, or not before the hot block, or .text_hot
     does not start where .text ends (one flash code segment);
  4. its bytes are not 4096 times the probe line, then `0d f0`;
  5. its disassembly holds anything but addi.n, bnez.n, ret.n and nop.n,
     not exactly 4097 ret.n, or a bnez.n whose target is not its own
     address + 4 in its own line (so no literal load, no call, no jump out
     of the block: it can reach nothing outside itself);
  6. another function symbol starts inside the block;
  7. a trampoline is not in RAM, or is not, instruction by instruction
     with its operands (TRAMP, TRAMP_QUIET): entry, three moves, [rsil
     a9, 5,] rsr.ccount, callx0 a4, rsr.ccount, [wsr.ps, rsync,] move,
     sub a2, a7, a6, retw (only the callx0 between the two counter reads;
     no literal, no other call);
  8. the layout table is not "XIPP", 4096 lines, 32 bytes a line, 512
     lines a way, start lines inside the block, lines x bytes = the
     block's size;
  9. a trampoline is called from the hot block or from the second core's
     code (pocketme_cyd::core2), or from nowhere;
 10. no function loads the block's address or the layout table's address
     (the firmware would not use what this script checks).
It prints each start offset's entry address and first cache set, the
cache sets of the first core's own step code (FIRST_CORE: what a part
competes with in the first core's cache), and the cache sets of the hot
block's parts (the second core's code, in its own cache with two cores),
to read the board's A2 runs against.

With --absent it fails when any of those symbols (but _cyd_hot_start) or
the probe's log text is in the ELF.

The last line is a summary ending `ok` or `FAILED`. It reuses
hot-check.py's way of reading the ELF (the esp toolchain's
xtensa-esp32-elf-nm, -objdump, -readelf and -c++filt on PATH: . ~/export-esp.sh).
It never opens the serial port.
"""
import argparse
import os
import re
import shutil
import struct
import subprocess
import sys

LINE = 32
LINES = 4096
SET_LINES = 512
BLOCK = LINE * LINES
FLASH = (0x400C2000, 0x40C00000)  # as hot-check.py
IRAM = (0x40070000, 0x400C0000)
PROBE_LINE = bytes([0x0B, 0x22, 0xCC, 0x02, 0x0D, 0xF0]) + bytes([0x3D, 0xF0]) * 13
STOP = bytes([0x0D, 0xF0])
MAGIC = 0x50504958  # "XIPP" little-endian
SYMS = ["_cyd_xip_probe_start", "_cyd_xip_probe_end", "cyd_xip_probe_run",
        "cyd_xip_probe_run_quiet", "CYD_XIP_PROBE_LAYOUT"]
TEXTS = [b"xip a1", b"xip: block", b"MEASURING BUILD: xip-probe"]
# The trampolines, instruction by instruction as objdump prints them.
TRAMP = ["entry a1, 32", "or a5, a0, a0", "or a4, a2, a2", "or a2, a3, a3",
         "rsr.ccount a6", "callx0 a4", "rsr.ccount a7",
         "or a0, a5, a5", "sub a2, a7, a6", "retw"]
TRAMP_QUIET = TRAMP[:4] + ["rsil a9, 5"] + TRAMP[4:7] + ["wsr.ps a9", "rsync"] + TRAMP[7:]
# The first core's own code around the probe's part (the console's step,
# the hand-over, the probe's frame hook): demangled-name pieces. Their
# cache sets are printed, as these are the lines a part competes with in
# the FIRST core's cache (the hot block is the second core's, in its own
# cache, with two cores).
FIRST_CORE = ["<pocketme_cyd::app::App>::step", "<pocketme_cyd::app::App>::link_bytes", "SwapSender",
              "pocketme_cyd::core2::join", "pocketme_cyd::xip_probe::"]


def tool(name):
    path = shutil.which("xtensa-esp32-elf-" + name)
    if not path:
        sys.exit(f"xip-check: xtensa-esp32-elf-{name} not on PATH (. ~/export-esp.sh)")
    return path


def run(*args, stdin=None):
    return subprocess.run(args, input=stdin, capture_output=True, text=True, check=True).stdout


def inside(a, rng):
    return rng[0] <= a < rng[1]


def sets(lo, hi):
    """The cache sets the bytes lo..hi touch, as text."""
    if hi <= lo:
        return "none"
    first, last = lo // LINE % SET_LINES, (hi - 1) // LINE % SET_LINES
    n = (hi - 1) // LINE - lo // LINE + 1
    return f"{first}..{last} ({n} lines{', wraps' if n > SET_LINES or last < first else ''})"


def disasm(elf, lo, hi):
    """(address, mnemonic, operands) of lo..hi."""
    out = []
    for row in run(tool("objdump"), "-d", f"--start-address={lo:#x}", f"--stop-address={hi:#x}", elf).splitlines():
        m = re.match(r"\s*([0-9a-f]+):\s+[0-9a-f]+\s+(\S+)\s*(.*)", row)
        if m:
            out.append((int(m[1], 16), m[2], m[3]))
    return out


def main():
    ap = argparse.ArgumentParser(description="Checks the xip-probe block of a pocketme-cyd ELF.")
    ap.add_argument("elf")
    ap.add_argument("--absent", action="store_true", help="the build must have no probe at all")
    args = ap.parse_args()
    elf = args.elf
    name = os.path.basename(elf)
    faults = []

    syms, funcs = {}, {}  # name -> address; start -> (mangled name, size)
    for row in run(tool("nm"), "-S", elf).splitlines():
        cols = row.split()
        if len(cols) >= 3:
            syms[cols[-1]] = int(cols[0], 16)
        if len(cols) == 4 and cols[2] in "tTwW":
            a, size = int(cols[0], 16), int(cols[1], 16)
            if size and (a not in funcs or funcs[a][1] < size):
                funcs[a] = (cols[3], size)
    raw = open(elf, "rb").read()

    if args.absent:
        for s in SYMS:
            if s in syms:
                faults.append(f"symbol {s} is in the ELF")
        for t in TEXTS:
            if t in raw:
                faults.append(f"the text {t.decode()!r} is in the ELF")
        if "_cyd_hot_start" not in syms:
            faults.append("no _cyd_hot_start (not a pocketme-cyd build with the hot block?)")
        for f in faults:
            print("xip-check: " + f)
        print(f"xip-check: {name} (--absent): no probe symbol, no probe text: " + ("FAILED" if faults else "ok"))
        sys.exit(1 if faults else 0)

    # 1. Symbols.
    missing = [s for s in SYMS + ["_cyd_hot_start", "_cyd_hot_window", "_cyd_hot_end"] if s not in syms]
    if missing:
        print(f"xip-check: missing symbols: {', '.join(missing)} (built without xip-probe?)")
        print(f"xip-check: {name}: FAILED")
        sys.exit(1)
    start, end = syms["_cyd_xip_probe_start"], syms["_cyd_xip_probe_end"]
    hot, hot_window, hot_end = syms["_cyd_hot_start"], syms["_cyd_hot_window"], syms["_cyd_hot_end"]
    tramps = {"cyd_xip_probe_run": syms["cyd_xip_probe_run"],
              "cyd_xip_probe_run_quiet": syms["cyd_xip_probe_run_quiet"]}
    table_at = syms["CYD_XIP_PROBE_LAYOUT"]

    # 2. Size and alignment.
    if end - start != BLOCK:
        faults.append(f"the block is {end - start} bytes, not {BLOCK}")
    if start % LINE:
        faults.append(f"the block starts at {start:#x}, not on a {LINE}-byte cache line")

    # Sections (name -> address, file offset, size).
    sec = {}
    for row in run(tool("readelf"), "-S", "-W", elf).splitlines():
        m = re.match(r"\s*\[\s*\d+\]\s+(\S+)\s+\S+\s+([0-9a-f]{8})\s+([0-9a-f]+)\s+([0-9a-f]+)", row)
        if m:
            sec[m[1]] = (int(m[2], 16), int(m[3], 16), int(m[4], 16))

    def section_of(a):
        for n, (addr, _, size) in sec.items():
            if addr <= a < addr + size and n not in (".rwtext_hot", ".rodata_merge"):
                return n
        return None

    def read(a, n):
        s = section_of(a)
        if s is None:
            return None
        addr, off, size = sec[s]
        if a + n > addr + size:
            return None
        return raw[off + a - addr: off + a - addr + n]

    # 3. Placement.
    if ".text" not in sec or ".text_hot" not in sec:
        faults.append("no .text or .text_hot section")
    else:
        t_addr, _, t_size = sec[".text"]
        if not (t_addr <= start and end + len(STOP) <= t_addr + t_size):
            faults.append(f"the block {start:#x}..{end + len(STOP):#x} is not inside .text {t_addr:#x}..{t_addr + t_size:#x}")
        if sec[".text_hot"][0] != t_addr + t_size:
            faults.append(f".text_hot starts at {sec['.text_hot'][0]:#x}, not where .text ends ({t_addr + t_size:#x})")
    if not (inside(start, FLASH) and inside(end + len(STOP) - 1, FLASH)):
        faults.append(f"the block {start:#x}..{end:#x} is not in the flash code range")
    if end + len(STOP) > hot:
        faults.append(f"the block ends at {end + len(STOP):#x}, after the hot block's start {hot:#x}")

    # 4. Bytes.
    data = read(start, BLOCK + len(STOP))
    if data is None:
        faults.append("the block's bytes are not in the file")
    elif data != PROBE_LINE * LINES + STOP:
        bad = next(i for i in range(len(data)) if data[i] != (PROBE_LINE * LINES + STOP)[i])
        faults.append(f"the block's bytes differ from the probe line first at {start + bad:#x}")

    # 5. Meaning.
    dis = disasm(elf, start, end + len(STOP))
    kinds, rets = {}, 0
    for a, op, arg in dis:
        kinds[op] = kinds.get(op, 0) + 1
        if op == "ret.n":
            rets += 1
        elif op == "bnez.n":
            t = re.search(r"a2, ([0-9a-f]+)", arg)
            target = int(t[1], 16) if t else None
            if target != a + 4 or target // LINE != a // LINE:
                faults.append(f"bnez.n at {a:#x} goes to {arg}, not its own address + 4 in its line")
        elif op not in ("addi.n", "nop.n"):
            faults.append(f"{op} {arg} at {a:#x} in the block (only addi.n, bnez.n, ret.n, nop.n belong there)")
    if rets != LINES + 1:
        faults.append(f"the block has {rets} ret.n, not {LINES + 1}")
    if kinds.get("addi.n") != LINES or kinds.get("bnez.n") != LINES or kinds.get("nop.n") != 13 * LINES:
        faults.append(f"instruction counts {kinds} are not {LINES} addi.n, {LINES} bnez.n, {13 * LINES} nop.n")

    # 6. Nothing else starts inside.
    for s, a in syms.items():
        if start < a < end and s != "_cyd_xip_probe_start":
            faults.append(f"symbol {s} at {a:#x} is inside the block")
    for a, (m, _) in funcs.items():
        if start < a < end:
            faults.append(f"function {m} starts at {a:#x}, inside the block")

    # 7. Trampolines: every instruction with its operands (so a swapped
    # subtraction, another call register or a lower mask level fails too).
    for t, a in tramps.items():
        if not inside(a, IRAM):
            faults.append(f"{t} is at {a:#x}, not in RAM")
        size = funcs.get(a, (None, 0))[1]
        seq = [" ".join(f"{op} {arg}".split()) for _, op, arg in disasm(elf, a, a + size)] if size else []
        # A trailing nop.n is alignment, not code.
        while seq and seq[-1] == "nop.n":
            seq.pop()
        want = TRAMP_QUIET if t.endswith("_quiet") else TRAMP
        if seq != want:
            faults.append(f"{t}: instructions {seq}, want {want}")

    # 8. The layout table.
    table = read(table_at, 32)
    if table is None:
        faults.append(f"the layout table at {table_at:#x} is not in the file")
        words = []
    else:
        words = list(struct.unpack("<8I", table))
        magic, lines, line, set_lines, starts = words[0], words[1], words[2], words[3], words[4:]
        if magic != MAGIC or lines != LINES or line != LINE or set_lines != SET_LINES:
            faults.append(f"layout table {[hex(w) for w in words[:4]]}: want XIPP, {LINES}, {LINE}, {SET_LINES}")
        if lines * line != end - start:
            faults.append(f"layout lines x bytes = {lines * line}, the block is {end - start}")
        for s in starts:
            if s >= LINES:
                faults.append(f"start line {s} is outside the block")

    # 9 and 10. Who uses the trampolines, the block's and the table's addresses.
    wanted = {**{v: k for k, v in tramps.items()}, start: "_cyd_xip_probe_start", table_at: "CYD_XIP_PROBE_LAYOUT"}
    users = {k: set() for k in wanted.values()}
    names = {}
    mangled = [m for m, _ in funcs.values()]
    demangled = run(tool("c++filt"), stdin="\n".join(mangled) + "\n").splitlines()
    clean = {m: re.sub(r"\[[0-9a-f]+\]", "", d) for m, d in zip(mangled, demangled)}
    ranges = sorted((a, a + size, m) for a, (m, size) in funcs.items())
    lo_all = min(a for a, _, _ in ranges)
    hi_all = max(b for _, b, _ in ranges)
    import bisect
    starts_sorted = [r[0] for r in ranges]

    def func_at(a):
        i = bisect.bisect_right(starts_sorted, a) - 1
        if i >= 0 and ranges[i][0] <= a < ranges[i][1]:
            return ranges[i]
        return None

    for sname in (".text", ".rwtext", ".text_hot", ".rwtext.wifi"):
        if sname not in sec or sec[sname][2] == 0:
            continue
        addr, _, size = sec[sname]
        lo, hi = max(addr, lo_all), min(addr + size, hi_all)
        for a, op, arg in disasm(elf, lo, hi):
            target = None
            if op == "l32r":
                t = re.search(r"\(([0-9a-f]{8})", arg)
                if t:
                    target = int(t[1], 16)
            elif op.startswith("call") and not op.startswith("callx"):
                t = re.match(r"([0-9a-f]{8})", arg)
                if t:
                    target = int(t[1], 16)
            if target in wanted:
                f = func_at(a)
                if f is None or inside(f[0], (start, end)):
                    continue  # a literal pool read as code, or the block itself
                users[wanted[target]].add(f[0])
                names[f[0]] = clean[f[2]]
    for t in tramps:
        if not users[t]:
            faults.append(f"nothing calls {t}")
        for f in sorted(users[t]):
            if inside(f, (hot, hot_end)):
                faults.append(f"{names[f]} at {f:#x} (in the hot block) calls {t}")
            if "pocketme_cyd::core2" in names[f]:
                faults.append(f"{names[f]} (the second core's code) calls {t}")
    for s in ("_cyd_xip_probe_start", "CYD_XIP_PROBE_LAYOUT"):
        if not users[s]:
            faults.append(f"no function loads the address of {s}")

    # Report.
    print(f"xip-check: block {start:#x}..{end:#x} ({end - start} bytes, {(end - start) // LINE} lines), sets {sets(start, end)}")
    for i, s in enumerate(words[4:] if words else []):
        e = start + s * LINE
        print(f"xip-check: start {i} ({s * LINE // 1024} KB): line {s}, entry {e:#x}, first set {e // LINE % SET_LINES}")
    for a, b, m in ranges:
        d = clean[m]
        if any(p in d for p in FIRST_CORE) and not inside(a, (hot, hot_end)):
            print(f"xip-check: first core {d} at {a:#x} ({b - a} bytes) sets {sets(a, b)}")
    print(f"xip-check: hot block present part {hot:#x}..{hot_window:#x} sets {sets(hot, hot_window)} (second core)")
    print(f"xip-check: hot block window part {hot_window:#x}..{hot_end:#x} sets {sets(hot_window, hot_end)} (second core)")
    for t, a in tramps.items():
        callers = ", ".join(sorted(names[f] for f in users[t])) or "none"
        print(f"xip-check: {t} at {a:#x} ({funcs.get(a, ('', 0))[1]} bytes), called by: {callers}")
    for s in ("_cyd_xip_probe_start", "CYD_XIP_PROBE_LAYOUT"):
        print(f"xip-check: address of {s} loaded by: " + (", ".join(sorted(names[f] for f in users[s])) or "none"))
    for f in faults:
        print("xip-check: " + f)
    print(f"xip-check: {name}: block {end - start} bytes at {start:#x}, {hot - end - len(STOP)} bytes before the hot block, "
          f"{rets} ret.n, layout {'ok' if words and words[0] == MAGIC else 'bad'}: " + ("FAILED" if faults else "ok"))
    sys.exit(1 if faults else 0)


main()
