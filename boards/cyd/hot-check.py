#!/usr/bin/env python3
"""Checks a built firmware against hot-block.list (README "The timed paths
and the flash cache").

usage: hot-check.py <elf> --cores two|one

It fails (exit 1) when
  - a part of the hot block is empty or smaller than its floor (the same
    numbers as HOT_FLOORS in build.rs, read from there);
  - a `must` line of this core mode matches no function in the ELF (a
    renamed or newly inlined function: the list is out of date);
  - a function is not where it belongs, judged by its real address, one
    line per function: a matched `present` or `window` line in its part of
    the block (`.text_hot`), a `ram` line in the block's RAM part, and
    every instance of the screen's `present` and `send` (and their
    closures) in the window part whether a line names it or not;
  - code that runs in the timed paths reaches flash code outside the
    block: the walk starts at every function in the block and at the RAM
    functions of the timed paths (RAM_ROOTS below), follows direct calls
    (call0/4/8/12) and function addresses loaded with L32R (the callx
    targets) through RAM, and fails on every flash function it reaches
    outside the block that is not a cold path (COLD below: panics and
    formatting, matched exactly against demangled names) or a start-up
    only function (ONCE below);
  - a function of the block loads a literal (L32R) from outside the block
    (the linker shared a constant with other code, so the load would go to
    a flash cache line outside the block), or a RAM function of the walk
    loads one from flash;
  - the window part spans more than 512 cache lines (16 KB, one way of the
    32 KB two-way cache: it would share a set with itself) or the block
    more than 1024 (32 KB: more than two of its lines in one set);
  - the block does not start right where `.text` ends: espflash then
    makes it a second flash code (IROM) segment, and the bootloader maps
    only one of them.

Every message goes to stdout; the last line is the summary (sizes, cache
lines, room left) and ends `ok` or `FAILED`. Each function is
disassembled on its own (from its symbol, for its size), so literal pools
between functions are never read as instructions.

It needs the esp toolchain's xtensa-esp32-elf-nm, -objdump, -readelf and
-c++filt on PATH (. ~/export-esp.sh). It never opens the serial port.
"""
import argparse
import fnmatch
import os
import re
import shutil
import subprocess
import sys
from collections import deque

LINE = 32  # bytes in a flash cache line
WINDOW_LINES = 512  # one way of the 32 KB two-way cache
BLOCK_LINES = 1024  # both ways
FLASH = (0x400C2000, 0x40C00000)  # code run from flash (through the cache)
IRAM = (0x40070000, 0x400C0000)  # code run from RAM (no cache)
ROM = (0x40000000, 0x40070000)  # mask ROM (no cache)

# The timed paths' functions that live in RAM (esp-hal's `#[ram]`
# functions and the firmware's own), matched against demangled names with
# the crate hashes taken out. The walk starts here too, so a RAM function
# that calls flash code is caught. `must` ones fail the check if missing.
# A `one` root is one function and must be in RAM; for a `family` only its
# members in RAM are roots (the others may sit in the block), and `must`
# asks for at least one.
RAM_ROOTS = [
    ("all", "must", "one", r"<pocketme_cyd::lcd::Lcd>::scanline_stamped_raw"),
    ("all", "must", "one", r"pocketme_cyd::screen::fill_fast"),
    ("two", "must", "one", r"pocketme_cyd::core2::parked"),
    ("all", "must", "family", r"<esp_hal::spi::master::low_level::Driver>::\w+"),
    ("all", "may", "family", r"<esp_hal::spi::master::dma::SpiDma<esp_hal::Blocking>>::\w+::<.*>"),
    ("all", "may", "family", r"<esp_hal::spi::master::dma::DmaDriver>::\w+::<.*>"),
]

# Every instance of these must be in the window part (whatever the list
# says): the screen's present with its loop over the windows, the
# window's send with the writer's loop, and their closures.
WINDOW_ALWAYS = [
    r"<pocketme_cyd::screen::CydScreen as board_api::screen::Screen>::present::<.*>(::\{closure#\d+\})*",
    r"<pocketme_cyd::screen::CydScreen>::send::<.*>(::\{closure#\d+\})*",
]

# Flash functions the timed paths may reach outside the block: they run
# only when something already went wrong (a panic, its message). Exact
# demangled names (crate hashes out) or exact families.
COLD = [
    r"core::panicking::(panic_const::)?\w+",
    r"core::panicking::assert_failed::<.*>",
    r"core::panicking::assert_failed_inner",
    r"core::option::unwrap_failed",
    r"core::option::expect_failed",
    r"core::result::unwrap_failed",
    r"core::slice::index::slice_(start|end)_index_(len|order)_fail",
    r"core::slice::index::slice_index_order_fail",
    r"core::slice::index::slice_index_fail",
    r"core::cell::panic_already_(mutably_)?borrowed",
    r"core::fmt::write",
    r"<core::fmt::Formatter>::\w+",
    r"<.* as core::fmt::(Debug|Display)>::fmt",
    r"(esp_hal|esp_rtos)::fmt::__unwrap_failed(_with_message)?::<.*>",
    r"esp_sync::panic_lock_not_reentrant",
    r"rust_begin_unwind",
]

# Flash functions the walk reaches that run once at start-up, never in a
# present (exact demangled names): the second core's executor writes the
# worker task into its storage when it spawns it.
ONCE = [
    r"<embassy_executor::raw::util::UninitCell<pocketme_cyd::core2::__worker_task::__worker_task_inner_function::\{closure#0\}>>"
    r"::write_in_place::<pocketme_cyd::core2::worker::\{closure#0\}>",
]


def tool(name):
    path = shutil.which("xtensa-esp32-elf-" + name)
    if not path:
        sys.exit(f"hot-check: xtensa-esp32-elf-{name} not on PATH (. ~/export-esp.sh)")
    return path


def run(*args, stdin=None):
    return subprocess.run(args, input=stdin, capture_output=True, text=True, check=True).stdout


def read_list(path, cores):
    rules = []
    for n, raw in enumerate(open(path), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        cols = line.split()
        if len(cols) != 4:
            sys.exit(f"hot-check: {path}:{n}: want `place cores need glob`")
        place, which, need, glob = cols
        if place not in ("present", "window", "ram") or which not in ("all", "two", "one") or need not in ("must", "may"):
            sys.exit(f"hot-check: {path}:{n}: unknown place, cores or need")
        if which in ("all", cores):
            rules.append((n, place, need, glob))
    return rules


def read_floors(build_rs, cores):
    text = open(build_rs).read()
    m = re.search(rf"\b{cores}:\s*\((\d+),\s*(\d+)\)", text[text.index("const HOT_FLOORS"):])
    if not m:
        sys.exit(f"hot-check: no HOT_FLOORS.{cores} in {build_rs}")
    return int(m[1]), int(m[2])


def full(patterns):
    """One regex that matches a whole name against any of `patterns`."""
    return re.compile("^(?:" + "|".join(f"(?:{p})" for p in patterns) + ")$")


def lines_of(lo, hi):
    """Cache lines that the bytes lo..hi touch."""
    return 0 if hi <= lo else (hi - 1) // LINE - lo // LINE + 1


def inside(a, rng):
    return rng[0] <= a < rng[1]


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser(description="Checks the hot block of a pocketme-cyd ELF.")
    ap.add_argument("elf")
    ap.add_argument("--cores", choices=("two", "one"), required=True)
    ap.add_argument("--list", default=os.path.join(here, "hot-block.list"))
    ap.add_argument("--build-rs", default=os.path.join(here, "build.rs"))
    args = ap.parse_args()
    present_min, window_min = read_floors(args.build_rs, args.cores)

    funcs = {}  # start -> (mangled name, size); the first name at a start wins
    byname = {}  # mangled name -> start
    syms = {}
    for row in run(tool("nm"), "-S", args.elf).splitlines():
        cols = row.split()
        if len(cols) == 4 and cols[2] in "tTwW":
            a, size = int(cols[0], 16), int(cols[1], 16)
            byname[cols[3]] = a
            if size and (a not in funcs or funcs[a][1] < size):
                funcs[a] = (cols[3], size)
        if len(cols) >= 3:
            syms[cols[-1]] = int(cols[0], 16)
    mangled = [n for n, _ in funcs.values()]
    demangled = run(tool("c++filt"), stdin="\n".join(mangled) + "\n").splitlines()
    # Crate hashes out: `esp_hal[63e05236272cfe00]::x` -> `esp_hal::x`.
    clean = {m: re.sub(r"\[[0-9a-f]+\]", "", d) for m, d in zip(mangled, demangled)}

    def name(a):
        return clean[funcs[a][0]] if a in funcs else f"{a:#x}"

    try:
        start, window, end = syms["_cyd_hot_start"], syms["_cyd_hot_window"], syms["_cyd_hot_end"]
        ram_start, ram_end = syms["_cyd_hot_ram_start"], syms["_cyd_hot_ram_end"]
    except KeyError as e:
        sys.exit(f"hot-check: {args.elf} has no {e} (built without hot-block.x?)")
    block = (start, end)
    parts = {"present": (start, window), "window": (window, end), "ram": (ram_start, ram_end)}
    faults = []

    def where(a):
        if inside(a, parts["present"]):
            return "the block's present part"
        if inside(a, parts["window"]):
            return "the block's window part"
        if inside(a, IRAM):
            return "RAM"
        if inside(a, FLASH):
            return "flash outside the block"
        if inside(a, ROM):
            return "ROM"
        return "an unknown place"

    # Sizes.
    present_size, window_size = window - start, end - window
    for part, size, floor in (("present", present_size, present_min), ("window", window_size, window_min)):
        if size <= 0:
            faults.append(f"the {part} part is empty: hot-block.list placed nothing there (or the linker script was not used)")
        elif size < floor:
            faults.append(f"the {part} part is {size} bytes, under its floor of {floor} (build.rs HOT_FLOORS.{args.cores}): a line probably stopped matching")
    window_lines, block_lines = lines_of(window, end), lines_of(start, end)
    if window_lines > WINDOW_LINES:
        faults.append(f"the window part spans {window_lines} cache lines, more than {WINDOW_LINES} (16 KB)")
    if block_lines > BLOCK_LINES:
        faults.append(f"the block spans {block_lines} cache lines, more than {BLOCK_LINES} (32 KB)")

    # Placement, by real address: one line per function.
    misplaced = {}  # start -> message
    for n, place, need, glob in read_list(args.list, args.cores):
        hits = [byname[s] for s in byname if fnmatch.fnmatchcase(s, glob)]
        hits = [a for a in hits if a in funcs]
        if not hits and need == "must":
            faults.append(f"hot-block.list:{n}: `{glob}` matches no function in this build (renamed or inlined?)")
        for a in hits:
            size = funcs[a][1]
            lo, hi = parts[place]
            if not (lo <= a and a + size <= hi):
                misplaced.setdefault(a, f"{name(a)} is at {a:#x}, in {where(a)}, not in the {place} part (hot-block.list:{n})")
    always = full(WINDOW_ALWAYS)
    screen_hits = 0
    for a, (m, size) in funcs.items():
        if always.match(clean[m]):
            screen_hits += 1
            lo, hi = parts["window"]
            if not (lo <= a and a + size <= hi):
                misplaced.setdefault(a, f"{name(a)} is at {a:#x}, in {where(a)}, not in the window part (every instance of the screen's present and send belongs there)")
    if not screen_hits:
        faults.append("no instance of CydScreen's present or send found (renamed?)")
    faults += [misplaced[a] for a in sorted(misplaced)]

    # The walk.
    roots = [a for a, (_, size) in funcs.items() if inside(a, block) or inside(a, parts["ram"])]
    for which, need, kind, pattern in RAM_ROOTS:
        if which not in ("all", args.cores):
            continue
        rx = re.compile(f"^(?:{pattern})$")
        hits = [a for a, (m, _) in funcs.items() if rx.match(clean[m])]
        if kind == "family":
            hits = [a for a in hits if inside(a, IRAM)]
        if not hits and need == "must":
            faults.append(f"RAM root `{pattern}` matches no function in RAM in this build")
        for a in hits:
            if not inside(a, IRAM):
                faults.append(f"{name(a)} is at {a:#x}, in {where(a)}, not in RAM (a timed path's RAM function)")
            else:
                roots.append(a)
    cold, once = full(COLD), full(ONCE)
    seen, queue = set(roots), deque(sorted(set(roots)))
    walked, cold_hit, outside = 0, set(), {}
    while queue:
        a = queue.popleft()
        walked += 1
        mname, size = funcs[a]
        in_block = inside(a, block)
        dis = run(tool("objdump"), "-d", f"--start-address={a:#x}", f"--stop-address={a + size:#x}", args.elf)
        for row in dis.splitlines():
            m = re.match(r"\s*([0-9a-f]+):\s+[0-9a-f]+\s+(\S+)\s+(.*)", row)
            if not m:
                continue
            op, arg = m[2], m[3]
            target = None
            if op == "l32r":
                t = re.search(r", ([0-9a-f]{8}) <[^>]*> \(([0-9a-f]{8})", arg)
                if not t:
                    t = re.search(r", ([0-9a-f]{8}) \(([0-9a-f]{8})", arg)
                if t:
                    lit, value = int(t[1], 16), int(t[2], 16)
                    if in_block and not inside(lit, block):
                        faults.append(f"{name(a)} loads a literal at {lit:#x}, in {where(lit)}, outside the block")
                    elif not in_block and inside(lit, FLASH):
                        faults.append(f"{name(a)} (in RAM) loads a literal at {lit:#x} from flash")
                    if value in funcs:
                        target = value
            elif op.startswith("call") and not op.startswith("callx"):
                t = re.match(r"([0-9a-f]{8})", arg)
                if t:
                    target = int(t[1], 16)
                    if target not in funcs and inside(target, FLASH):
                        faults.append(f"{name(a)} calls {target:#x}, flash code that is no function start")
                        target = None
            if target is None or target not in funcs:
                continue
            if inside(target, block) or inside(target, ROM):
                continue
            if inside(target, IRAM):
                if target not in seen:
                    seen.add(target)
                    queue.append(target)
                continue
            if inside(target, FLASH):
                if cold.match(name(target)) or once.match(name(target)):
                    cold_hit.add(target)
                else:
                    outside.setdefault(target, set()).add(name(a))
    for t in sorted(outside):
        callers = sorted(outside[t])
        more = f" and {len(callers) - 1} more" if len(callers) > 1 else ""
        faults.append(f"{callers[0]}{more} reaches {name(t)} at {t:#x}, flash code outside the block")

    # Right behind .text, so that the image has one flash code segment.
    sec = {}
    for row in run(tool("readelf"), "-S", "-W", args.elf).splitlines():
        m = re.match(r"\s*\[\s*\d+\]\s+(\S+)\s+\S+\s+([0-9a-f]{8})\s+[0-9a-f]+\s+([0-9a-f]+)", row)
        if m:
            sec[m[1]] = (int(m[2], 16), int(m[3], 16))
    if ".text" not in sec or ".text_hot" not in sec:
        faults.append("no .text or .text_hot section")
    elif sec[".text_hot"][0] != sum(sec[".text"]):
        faults.append(f".text_hot starts at {sec['.text_hot'][0]:#x}, not where .text ends ({sum(sec['.text']):#x})")

    seen_f = set()
    for f in faults:
        if f not in seen_f:
            seen_f.add(f)
            print("hot-check: " + f)
    print(
        f"hot-check: {os.path.basename(args.elf)} ({args.cores}-core): block {start:#x}..{end:#x} "
        f"{end - start} bytes in {block_lines} lines ({BLOCK_LINES - block_lines} left); "
        f"present part {present_size} bytes (floor {present_min}); "
        f"window part {window_size} bytes in {window_lines} lines ({WINDOW_LINES - window_lines} left, floor {window_min}); "
        f"RAM part {ram_end - ram_start} bytes; {walked} functions walked, {len(cold_hit)} cold or start-up targets: "
        + ("FAILED" if faults else "ok")
    )
    sys.exit(1 if faults else 0)


main()
