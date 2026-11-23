#!/usr/bin/env python3
"""The flash part of a game: which functions and constants go to flash
(`split`), and the check of a game linked that way (`check`).

  flash_split.py split PASS1.elf PASS1.map LIST OUT_DIR
      Reads the game linked the plain way (pass 1: everything in RAM) and
      the game's flash.list, decides what goes to flash, and writes into
      OUT_DIR:
        flash-code.x     input-section lines for game-flash.x's .flash_text
        flash-rodata.x   input-section lines for its .flash_rodata
        split.txt        the decision, function by function, with sizes
      Fails when a `hot` line matches no function (renamed or inlined).

  flash_split.py check GAME.elf LIST PLACE_X
      Checks the game linked with its flash part (pass 2). PLACE_X is the
      flash-place.x made from game_api::layout (the game's place). Prints
      the sizes, the crossings and every flash function that RAM code
      calls (the cost list), and exits 1 when a rule is broken. Nothing is
      packed then.

  flash_split.py selftest GAME.elf LIST PLACE_X
      Runs `check` with two deliberately wrong lists made from LIST and
      fails unless `check` refuses both (games/build-chip.sh runs it for
      every game with a flash part, so the checker is seen to bite on a
      real game, not only on test_flash_split.py's made-up text).

flash.list (one rule per line, `#` starts a comment):
  hot  GLOB   a per-picture root: it and everything it reaches stay in
              RAM, except what it reaches only through a `cold` function.
              Reaching means: a function address loaded by the code (every
              call on this toolchain is l32r + callx8, so a call is one),
              or a function address inside a constant table the code
              reads (vtables, function tables), followed through tables
              that point at other tables.
  cold GLOB   reached from per-picture code, but run only on an event (a
              dock, a jump, a key in a menu): the walk stops there, so it
              and what only it reaches may go to flash.
  ram  GLOB   always RAM.
Globs are fnmatch patterns on the whole mangled (v0) name, as in
boards/cyd/hot-block.list; `*` stands in for crate hashes.

Built in, no line needed: `game_entry` and game_kit::game!'s items
(`*10game_entry*`: update, exit, CALLBACKS), every `.text.unlikely.*`
function (panics) and every function whose address is in the game's
starting values (.data) stay in RAM. Every other function goes to flash.
A constant (one input section of .rodata) goes to flash only when every
reference to it comes from flash code or from another flash constant; one
reference from RAM code, from .data or from a RAM constant keeps it in
RAM, and so does no reference found at all. The game's source needs no
attribute.

The check's rules (any one fails the game):
  1. a function that must stay in RAM (hot walk, `hot`, `ram`, built in)
     lies in flash;
  2. a direct call0/4/8/12, jump, branch or loop crosses between RAM and
     flash (only l32r + callx may: their address is a full 32-bit word);
  3. an l32r in flash code reads a literal outside the flash code, or one
     in RAM code reads a literal in flash (legal on the chip, but the
     split never makes it, so it means the link went wrong);
  4. .flash_text or .flash_rodata is not inside the game's place, or any
     other section lies in it;
  5. game_entry is not in RAM code.
Tools: xtensa-esp32-elf-nm and -objdump on PATH (build-chip.sh has them).
Its own test, with made-up text and no chip tools: test_flash_split.py.
"""
import bisect
import fnmatch
import re
import struct
import subprocess
import sys

# Always RAM, whatever the list says.
PINNED = ["game_entry", "*10game_entry*"]
RULES = ("hot", "cold", "ram")


def run(*a):
    return subprocess.run(a, check=True, capture_output=True, text=True).stdout


# ---------------------------------------------------------------- reading


class Elf:
    """The sections of a 32-bit little-endian ELF, and its words."""

    def __init__(self, data):
        if data[:4] != b"\x7fELF" or data[4] != 1 or data[5] != 1:
            sys.exit("flash_split: not a 32-bit little-endian ELF file")
        self.data = data
        (shoff,) = struct.unpack_from("<I", data, 0x20)
        es, n, si = struct.unpack_from("<HHH", data, 0x2E)
        raw = [struct.unpack_from("<IIIIII", data, shoff + k * es) for k in range(n)]
        strtab = raw[si][4]
        self.secs = {}
        for name, typ, _flags, addr, off, size in raw:
            nm = data[strtab + name:].split(b"\0", 1)[0].decode()
            self.secs[nm] = (addr, off, size, typ)

    def word(self, a):
        """The 32-bit word at address `a` in a section with contents."""
        if a is None:
            return None
        for addr, off, size, typ in self.secs.values():
            if typ == 1 and addr and addr <= a and a + 4 <= addr + size:
                return struct.unpack_from("<I", self.data, off + a - addr)[0]
        return None

    def span(self, name):
        a, _, s, _ = self.secs.get(name, (0, 0, 0, 0))
        return a, a + s


def parse_nm(text):
    """Functions from `nm -S --defined-only`: sorted [(addr, size, name)]."""
    out = []
    for line in text.splitlines():
        p = line.split()
        if len(p) == 4 and p[2] in ("t", "T") and int(p[1], 16) > 0:
            out.append((int(p[0], 16), int(p[1], 16), p[3]))
    out.sort()
    return out


def owner_of(funcs):
    """address -> name of the function that holds it (or None)."""
    starts = [f[0] for f in funcs]

    def owner(a):
        i = bisect.bisect_right(starts, a) - 1
        if i >= 0 and a < funcs[i][0] + funcs[i][1]:
            return funcs[i][2]
        return None

    return owner


INS = re.compile(r"^\s*([0-9a-f]+):\s+[0-9a-f]+\s+(\S+)\s*(.*)$")


def parse_objdump(text, owner):
    """[(pc, function, op, operands)] of every instruction inside a
    function (literal pools between functions are data, left out)."""
    out = []
    for line in text.splitlines():
        m = INS.match(line)
        if not m:
            continue
        pc = int(m.group(1), 16)
        me = owner(pc)
        if me is not None:
            out.append((pc, me, m.group(2), m.group(3)))
    return out


def l32r_literal(ops):
    t = re.search(r",\s*([0-9a-f]+)", ops)
    return int(t.group(1), 16) if t else None


def branch_target(op, ops):
    """The target of a direct call, jump, branch or loop, or None for any
    other instruction."""
    direct = op in ("call0", "call4", "call8", "call12", "j", "loop", "loopnez", "loopgtz")
    branch = op.startswith("b") and op not in ("break", "break.n")
    if not (direct or branch):
        return None
    t = re.search(r"([0-9a-f]+)\s*<", ops)
    if not t:
        t = re.search(r"\b([0-9a-f]{6,8})\s*$", ops.split(",")[-1].strip())
    return int(t.group(1), 16) if t else None


def parse_map(text):
    """Input sections of an ld map: [(output section, input name, addr,
    size)] for every one with a size."""
    out = []
    cur = None
    lines = text.splitlines()
    head = re.compile(r"^(\.\S+)")
    inp = re.compile(r"^ (\.\S+)(.*)$")
    i = 0
    while i < len(lines):
        line = lines[i]
        h = head.match(line)
        if h:
            cur = h.group(1)
        m = inp.match(line)
        if m and cur:
            rest = m.group(2).split()
            if len(rest) < 2 and i + 1 < len(lines):
                i += 1
                rest = lines[i].split()
            try:
                a, s = int(rest[0], 16), int(rest[1], 16)
                if s > 0:
                    out.append((cur, m.group(1), a, s))
            except (IndexError, ValueError):
                pass
        i += 1
    return out


def read_list(lines, where="flash.list"):
    """The rules of a flash.list: {"hot": [...], "cold": [...], "ram": [...]}."""
    rules = {k: [] for k in RULES}
    for n, line in enumerate(lines, 1):
        p = line.split("#", 1)[0].split()
        if not p:
            continue
        if len(p) != 2 or p[0] not in rules:
            sys.exit(f"flash_split: {where}:{n}: want `hot|cold|ram GLOB`: {line.strip()}")
        if not re.fullmatch(r"[A-Za-z0-9_*?]+", p[1]):
            sys.exit(f"flash_split: {where}:{n}: a glob is letters, digits, _, * and ?: {p[1]}")
        rules[p[0]].append(p[1])
    if not rules["hot"]:
        sys.exit(f"flash_split: {where}: no `hot` line: nothing would be known to run per picture")
    return rules


def read_place(path):
    """GAME_FLASH_CODE_ORIGIN/LENGTH and GAME_FLASH_RO_ORIGIN/LENGTH of a
    flash-place.x."""
    found = {}
    for line in open(path):
        m = re.match(r"\s*(GAME_FLASH_\w+)\s*=\s*(0x[0-9a-fA-F]+)\s*;", line)
        if m:
            found[m.group(1)] = int(m.group(2), 16)
    names = ("GAME_FLASH_CODE_ORIGIN", "GAME_FLASH_CODE_LENGTH",
             "GAME_FLASH_RO_ORIGIN", "GAME_FLASH_RO_LENGTH")
    missing = [n for n in names if n not in found]
    if missing:
        sys.exit(f"flash_split: {path} has no {', '.join(missing)}")
    return tuple(found[n] for n in names)


def match(n, globs):
    return any(fnmatch.fnmatchcase(n, g) for g in globs)


# ---------------------------------------------------------------- deciding


def decide(funcs, insns, word, inputs, data_span, rules):
    """The split. Pure: everything comes in as arguments (the tests give
    made-up ones).

    funcs    [(addr, size, name)] of pass 1
    insns    [(pc, function, op, operands)] of pass 1
    word     address -> the 32-bit word there (None outside the file)
    inputs   [(output, input, addr, size)] of pass 1's map
    data_span (lo, hi) of .data
    Returns a dict: flash (function names, in address order), ram (set),
    hot (set), ro_flash [(input, addr, size)], own_text {name: bytes},
    data_roots (functions whose address is in .data)."""
    starts = {f[0]: f[2] for f in funcs}
    names = [f[2] for f in funcs]
    for g in rules["hot"]:
        if not any(fnmatch.fnmatchcase(n, g) for n in names):
            sys.exit(f"flash_split: `hot {g}` matches no function (renamed or inlined?): "
                     "fix flash.list")
    ro = [(name, a, s) for out, name, a, s in inputs if out == ".rodata"]
    ro_starts = [r[1] for r in ro]

    def which(v):
        if v is None:
            return None
        i = bisect.bisect_right(ro_starts, v) - 1
        if i >= 0 and v < ro[i][1] + ro[i][2]:
            return i
        return None

    da_lo, da_hi = data_span
    edges = {n: set() for n in names}
    drefs = {n: set() for n in names}  # rodata sections a function reads
    for _pc, me, op, ops in insns:
        if op != "l32r":
            continue
        v = word(l32r_literal(ops))
        if v in starts:
            edges[me].add(starts[v])
        else:
            k = which(v)
            if k is not None:
                drefs[me].add(k)
    # What each constant points at: other constants and functions.
    ro_refs = {k: set() for k in range(len(ro))}
    ro_funcs = {k: set() for k in range(len(ro))}
    for k, (_name, a, s) in enumerate(ro):
        for off in range(0, s - 3, 4):
            v = word(a + off)
            if v in starts:
                ro_funcs[k].add(starts[v])
            j = which(v)
            if j is not None and j != k:
                ro_refs[k].add(j)
    # Functions and constants named by the starting values.
    data_roots = set()
    data_ro = set()
    for a in range(da_lo, da_hi - 3, 4):
        v = word(a)
        if v in starts:
            data_roots.add(starts[v])
        j = which(v)
        if j is not None:
            data_ro.add(j)

    def tables_of(fn):
        """Constants `fn` reads, and those they point at, transitively."""
        seen = set()
        todo = list(drefs[fn])
        while todo:
            k = todo.pop()
            if k in seen:
                continue
            seen.add(k)
            todo.extend(ro_refs[k] - seen)
        return seen

    # The per-picture walk.
    roots = {n for n in names if match(n, rules["hot"])}
    hot = set()
    todo = list(roots)
    while todo:
        n = todo.pop()
        if n in hot or (n not in roots and match(n, rules["cold"])):
            continue
        hot.add(n)
        reach = set(edges[n])
        for k in tables_of(n):
            reach |= ro_funcs[k]
        todo.extend(reach - hot)
    # Each function's own input sections (code and literal pool).
    own_text = {}
    for out, name, _a, s in inputs:
        for pre in (".text.", ".literal."):
            if out == ".text" and name.startswith(pre) and not name.startswith(pre + "unlikely."):
                own_text.setdefault(name[len(pre):], {})[pre] = s
    ram = {n for n in names
           if n in hot or n in data_roots or match(n, rules["ram"]) or match(n, PINNED)
           or n not in own_text or ".text." not in own_text[n]}
    flash = [n for n in names if n not in ram]

    # Constants: flash only when every reference is from flash.
    refs = {k: set() for k in range(len(ro))}
    for fn, ks in drefs.items():
        for k in ks:
            refs[k].add(("f", fn))
    for k, js in ro_refs.items():
        for j in js:
            refs[j].add(("r", k))
    stay = {k for k in refs
            if not refs[k] or k in data_ro or match(ro[k][0], ["*10game_entry*"])}
    changed = True
    while changed:
        changed = False
        for k, who in refs.items():
            if k in stay:
                continue
            if any((t == "f" and x in ram) or (t == "r" and x in stay) for t, x in who):
                stay.add(k)
                changed = True
    ro_flash = [ro[k] for k in range(len(ro)) if k not in stay]
    return {
        "flash": flash,
        "ram": ram,
        "hot": hot,
        "roots": roots,
        "ro_flash": ro_flash,
        "ro_all": ro,
        "own_text": {n: sum(v.values()) for n, v in own_text.items()},
        "data_roots": data_roots,
    }


def split(elf_path, map_path, list_path, out_dir):
    rules = read_list(open(list_path).read().splitlines(), list_path)
    elf = Elf(open(elf_path, "rb").read())
    funcs = parse_nm(run("xtensa-esp32-elf-nm", "-S", "--defined-only", elf_path))
    insns = parse_objdump(run("xtensa-esp32-elf-objdump", "-d", elf_path), owner_of(funcs))
    inputs = parse_map(open(map_path).read())
    d = decide(funcs, insns, elf.word, inputs, elf.span(".data"), rules)
    own = d["own_text"]
    with open(f"{out_dir}/flash-code.x", "w") as f:
        f.write("/* Made by games/chip/flash_split.py from flash.list and pass 1. Do not edit. */\n")
        for n in d["flash"]:
            f.write(f"    *(.literal.{n} .text.{n})\n")
    with open(f"{out_dir}/flash-rodata.x", "w") as f:
        f.write("/* Made by games/chip/flash_split.py from flash.list and pass 1. Do not edit. */\n")
        for name, _a, _s in d["ro_flash"]:
            f.write(f"    *({name})\n")
    code = sum(own[n] for n in d["flash"])
    rod = sum(s for _, _, s in d["ro_flash"])
    ram_code = sum(own.get(n, 0) for n in d["ram"])
    with open(f"{out_dir}/split.txt", "w") as f:
        f.write(f"list {list_path}\n")
        f.write(f"flash functions {len(d['flash'])}, {code} bytes (code + own literals, by the pass-1 map)\n")
        f.write(f"flash constants {len(d['ro_flash'])} sections, {rod} bytes\n")
        f.write(f"RAM functions {len(d['ram'])}, {ram_code} bytes; hot (per picture) {len(d['hot'])}; "
                f"kept in RAM by .data {len(d['data_roots'])}\n")
        for n in sorted(d["flash"], key=lambda n: (-own[n], n)):
            f.write(f"F {own[n]:6d} {n}\n")
        for name, _a, s in sorted(d["ro_flash"], key=lambda r: (-r[2], r[0])):
            f.write(f"R {s:6d} {name}\n")
        for n in sorted(d["ram"], key=lambda n: (-own.get(n, 0), n)):
            why = "hot" if n in d["hot"] else "data" if n in d["data_roots"] else "pinned"
            f.write(f"M {own.get(n, 0):6d} {why:6s} {n}\n")
    print(f"flash_split: {len(d['flash'])} functions ({code} bytes) and {len(d['ro_flash'])} "
          f"constants ({rod} bytes) to flash; {len(d['hot'])} functions per picture")


# ---------------------------------------------------------------- checking


def check_linked(funcs, insns, word, sections, rules, place):
    """The rules of a pass-2 link. Pure (the tests give made-up input).
    sections {name: (addr, size)}; place (code_org, code_len, ro_org,
    ro_len). Returns (problems, report lines)."""
    code_org, code_len, ro_org, ro_len = place
    in_fc = lambda a: a is not None and code_org <= a < code_org + code_len
    in_fr = lambda a: a is not None and ro_org <= a < ro_org + ro_len
    starts = {f[0]: f[2] for f in funcs}
    bad = []
    t_lo, t_sz = sections.get(".flash_text", (0, 0))
    r_lo, r_sz = sections.get(".flash_rodata", (0, 0))
    if t_sz == 0:
        bad.append(".flash_text is empty or missing (no function went to flash)")
    if t_sz and not (code_org <= t_lo and t_lo + t_sz <= code_org + code_len):
        bad.append(f".flash_text {t_lo:#x}+{t_sz} is not inside its place "
                   f"{code_org:#x}+{code_len} (too big for game_api::layout's place?)")
    if r_sz and not (ro_org <= r_lo and r_lo + r_sz <= ro_org + ro_len):
        bad.append(f".flash_rodata {r_lo:#x}+{r_sz} is not inside its place "
                   f"{ro_org:#x}+{ro_len} (too big for game_api::layout's place?)")
    for name, (a, s) in sections.items():
        if name not in (".flash_text", ".flash_rodata") and s and (in_fc(a) or in_fr(a)):
            bad.append(f"{name} lies in the flash place")
    entry = [a for a, _s, n in funcs if n == "game_entry"]
    if not entry or in_fc(entry[0]):
        bad.append("game_entry is not in RAM code")
    # The per-picture walk again, on this link: from every `hot` function
    # along every function address the code loads, stopping at `cold`.
    # (The split also followed constant tables; their functions are not
    # re-walked here, so this is the floor of what must be in RAM.)
    names = [f[2] for f in funcs]
    edges = {n: set() for n in names}
    for _pc, me, op, ops in insns:
        if op == "l32r":
            v = word(l32r_literal(ops))
            if v in starts:
                edges[me].add(starts[v])
    roots = {n for n in names if match(n, rules["hot"])}
    for g in rules["hot"]:
        if not any(fnmatch.fnmatchcase(n, g) for n in names):
            bad.append(f"`hot {g}` matches no function of this link")
    hot = set()
    todo = list(roots)
    while todo:
        n = todo.pop()
        if n in hot or (n not in roots and match(n, rules["cold"])):
            continue
        hot.add(n)
        todo.extend(edges[n] - hot)
    for a, _s, n in funcs:
        must = n in hot or match(n, rules["ram"]) or match(n, PINNED)
        if in_fc(a) and must:
            why = "reached per picture" if n in hot else "pinned to RAM"
            bad.append(f"{n} must stay in RAM ({why}) but is in flash at {a:#x}")
    entries = {}
    crossings = 0
    for pc, me, op, ops in insns:
        here = in_fc(pc)
        if op == "l32r":
            lit = l32r_literal(ops)
            if here != in_fc(lit):
                bad.append(f"{me}: l32r at {pc:#x} reads its literal at {lit:#x} in the other memory")
            v = word(lit)
            if v in starts and in_fc(v) != here:
                crossings += 1
                if not here:
                    entries.setdefault(starts[v], set()).add(me)
            continue
        t = branch_target(op, ops)
        if t is not None and in_fc(t) != here:
            bad.append(f"{me}: {op} at {pc:#x} to {t:#x} crosses RAM/flash (only l32r + callx may)")
    fsize = {n: s for a, s, n in funcs if in_fc(a)}
    report = [
        f"flash code {t_sz} of {code_len} bytes at {t_lo:#x}, flash constants {r_sz} of {ro_len} at {r_lo:#x}",
        f"calls between RAM and flash through l32r + callx: {crossings}; "
        f"flash functions {len(fsize)}, of them called from RAM code: {len(entries)}",
    ]
    for n in sorted(entries, key=lambda n: (-fsize.get(n, 0), n)):
        report.append(f"   {fsize.get(n, 0):5d} {n}  <- {sorted(entries[n])[0]}")
    return bad, report


def load_linked(elf_path):
    elf = Elf(open(elf_path, "rb").read())
    funcs = parse_nm(run("xtensa-esp32-elf-nm", "-S", "--defined-only", elf_path))
    insns = parse_objdump(run("xtensa-esp32-elf-objdump", "-d", elf_path), owner_of(funcs))
    sections = {n: (a, s) for n, (a, _o, s, typ) in elf.secs.items() if a and s}
    return elf, funcs, insns, sections


def check(elf_path, list_path, place_path, quiet=False, linked=None, rules=None):
    if rules is None:
        rules = read_list(open(list_path).read().splitlines(), list_path)
    place = read_place(place_path)
    elf, funcs, insns, sections = linked or load_linked(elf_path)
    bad, report = check_linked(funcs, insns, elf.word, sections, rules, place)
    if not quiet:
        for line in report:
            print("flash check:", line)
        for b in bad:
            print("flash check: FAIL:", b)
    return 1 if bad else 0


def selftest(elf_path, list_path, place_path):
    """The checker must refuse two wrong lists on this real game."""
    rules = read_list(open(list_path).read().splitlines(), list_path)
    linked = load_linked(elf_path)
    _elf, funcs, _insns, _sections = linked
    code_org, code_len, _ro_org, _ro_len = read_place(place_path)
    in_flash = sorted(n for a, _s, n in funcs if code_org <= a < code_org + code_len)
    if not in_flash:
        print("flash selftest: FAIL: no function in flash to test with")
        return 1
    ok = True
    # 1. Drift: a function the split put in flash is now named per picture
    #    (as if the game changed and the list was updated, but not the build).
    victim = in_flash[0]
    wrong = {k: list(v) for k, v in rules.items()}
    wrong["hot"].append(victim)
    if check(elf_path, list_path, place_path, quiet=True, linked=linked, rules=wrong) == 0:
        print(f"flash selftest: FAIL: the check passed a list with `hot {victim}`")
        ok = False
    # 2. A `ram` glob that matches every flash function (`ram *`).
    wrong = {k: list(v) for k, v in rules.items()}
    wrong["ram"].append("*")
    if check(elf_path, list_path, place_path, quiet=True, linked=linked, rules=wrong) == 0:
        print("flash selftest: FAIL: the check passed a list with `ram *`")
        ok = False
    if ok:
        print(f"flash selftest: the check refused both wrong lists (`hot {victim[:40]}...`, `ram *`)")
    return 0 if ok else 1


if __name__ == "__main__":
    a = sys.argv
    if len(a) == 6 and a[1] == "split":
        split(*a[2:])
    elif len(a) == 5 and a[1] == "check":
        sys.exit(check(a[2], a[3], a[4]))
    elif len(a) == 5 and a[1] == "selftest":
        sys.exit(selftest(a[2], a[3], a[4]))
    else:
        sys.exit(__doc__)
