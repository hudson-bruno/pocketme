#!/usr/bin/env python3
"""Self-test of flash_split.py on made-up input (no chip tools).

  python3 games/chip/test_flash_split.py

Run by games/build-chip.sh before it builds anything. Standard library
only. The made-up game:

  draw   hot root; reads the table `vt`, which points at `paint`
  tick   called by draw
  dock   cold, called by tick
  menu   called only by dock; reads `menu_text`, `shared` and `chain`
  paint  reached only through the table `vt` (so per picture too)
  pinx   pinned to RAM by a `ram` line
  stray  reached by nobody
  game_entry  built in RAM

Constants: vt (read by draw), menu_text (menu only), shared (menu and
tick), chain (menu only; points at leaf), leaf (only through chain),
unref (nobody).
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import flash_split as fs  # noqa: E402

F = {  # name: address (each 0x20 bytes)
    "game_entry": 0x40092000,
    "_RNv4demo4draw": 0x40092020,
    "_RNv4demo4tick": 0x40092040,
    "_RNv4demo4dock": 0x40092060,
    "_RNv4demo4menu": 0x40092080,
    "_RNv4demo5paint": 0x400920A0,
    "_RNv4demo4pinx": 0x400920C0,
    "_RNv4demo5stray": 0x400920E0,
}
NM = "\n".join(f"{a:08x} 00000020 {'T' if n == 'game_entry' else 't'} {n}" for n, a in F.items())
RO = {  # name: (address, size)
    ".rodata.vt": (0x3FFF4000, 8),
    ".rodata.menu_text": (0x3FFF4008, 8),
    ".rodata.shared": (0x3FFF4010, 8),
    ".rodata.chain": (0x3FFF4018, 4),
    ".rodata.leaf": (0x3FFF401C, 4),
    ".rodata.unref": (0x3FFF4020, 4),
}
DATA = (0x3FFF4030, 0x3FFF4034)


def lit(n):
    return 0x40091000 + 4 * n


# Literal words: what each l32r loads.
WORDS = {
    lit(0): F["_RNv4demo4tick"],          # draw calls tick
    lit(1): RO[".rodata.vt"][0],          # draw reads vt
    lit(2): F["_RNv4demo4dock"],          # tick calls dock
    lit(3): RO[".rodata.shared"][0] + 4,  # tick reads shared (inside it)
    lit(4): F["_RNv4demo4menu"],          # dock calls menu
    lit(5): RO[".rodata.menu_text"][0],   # menu reads menu_text
    lit(6): RO[".rodata.shared"][0],      # menu reads shared
    lit(7): RO[".rodata.chain"][0],       # menu reads chain
    RO[".rodata.vt"][0]: F["_RNv4demo5paint"],
    RO[".rodata.vt"][0] + 4: 0,
    RO[".rodata.chain"][0]: RO[".rodata.leaf"][0],
    DATA[0]: 7,
}


def ins(fn, k, n):
    pc = F[fn] + 3 * k
    return f"{pc:08x}:\t000081 \tl32r\ta8, {lit(n):08x} <_game_code_start+0x{4 * n:x}>"


OBJDUMP = "\n".join([
    "Disassembly of section .text:",
    ins("_RNv4demo4draw", 0, 0), ins("_RNv4demo4draw", 1, 1),
    ins("_RNv4demo4tick", 0, 2), ins("_RNv4demo4tick", 1, 3),
    ins("_RNv4demo4dock", 0, 4),
    ins("_RNv4demo4menu", 0, 5), ins("_RNv4demo4menu", 1, 6), ins("_RNv4demo4menu", 2, 7),
])
MAP = "\n".join(
    [".text           0x40092000      0x100"]
    + [f" .text.{n}\n                0x{a:08x}       0x1c x.o\n .literal.{n}\n                0x{a:08x}        0x4 x.o"
       for n, a in F.items() if n != "game_entry"]
    + [" .text.game_entry\n                0x40092000       0x20 x.o"]
    + [".rodata         0x3fff4000       0x24"]
    + [f" {n}\n                0x{a:08x}        0x{s:x} x.o" for n, (a, s) in RO.items()]
    + [".data           0x3fff4030        0x4"]
)
LIST = """
# made up
hot  *4demo4draw
cold *4demo4dock   # an event
ram  *4demo4pinx
"""


def decided(list_text=LIST):
    funcs = fs.parse_nm(NM)
    insns = fs.parse_objdump(OBJDUMP, fs.owner_of(funcs))
    rules = fs.read_list(list_text.splitlines())
    return fs.decide(funcs, insns, WORDS.get, fs.parse_map(MAP), DATA, rules)


class Split(unittest.TestCase):
    def test_the_list_is_read(self):
        r = fs.read_list(LIST.splitlines())
        self.assertEqual(r, {"hot": ["*4demo4draw"], "cold": ["*4demo4dock"], "ram": ["*4demo4pinx"]})
        for bad in ("hot", "warm *x", "hot a b", "hot a.b"):
            with self.assertRaises(SystemExit):
                fs.read_list([bad])
        with self.assertRaises(SystemExit):  # no hot line at all
            fs.read_list(["cold *x"])

    def test_the_map_is_read(self):
        got = {(o, n) for o, n, _a, _s in fs.parse_map(MAP)}
        self.assertIn((".rodata", ".rodata.leaf"), got)
        self.assertIn((".text", ".literal._RNv4demo4menu"), got)

    def test_the_walk_stops_at_cold_and_follows_tables(self):
        d = decided()
        self.assertEqual(d["hot"], {"_RNv4demo4draw", "_RNv4demo4tick", "_RNv4demo5paint"})
        self.assertEqual(sorted(d["flash"]), ["_RNv4demo4dock", "_RNv4demo4menu", "_RNv4demo5stray"])
        self.assertIn("game_entry", d["ram"])
        self.assertIn("_RNv4demo4pinx", d["ram"])

    def test_constants_go_to_flash_only_when_flash_alone_reads_them(self):
        d = decided()
        self.assertEqual(sorted(n for n, _a, _s in d["ro_flash"]),
                         [".rodata.chain", ".rodata.leaf", ".rodata.menu_text"])

    def test_a_constant_named_in_the_starting_values_stays(self):
        words = dict(WORDS)
        words[DATA[0]] = RO[".rodata.menu_text"][0]
        funcs = fs.parse_nm(NM)
        d = fs.decide(funcs, fs.parse_objdump(OBJDUMP, fs.owner_of(funcs)), words.get,
                      fs.parse_map(MAP), DATA, fs.read_list(LIST.splitlines()))
        self.assertNotIn(".rodata.menu_text", [n for n, _a, _s in d["ro_flash"]])

    def test_a_function_named_in_the_starting_values_stays(self):
        words = dict(WORDS)
        words[DATA[0]] = F["_RNv4demo5stray"]
        funcs = fs.parse_nm(NM)
        d = fs.decide(funcs, fs.parse_objdump(OBJDUMP, fs.owner_of(funcs)), words.get,
                      fs.parse_map(MAP), DATA, fs.read_list(LIST.splitlines()))
        self.assertNotIn("_RNv4demo5stray", d["flash"])

    def test_a_ram_line_wins_and_without_cold_everything_reached_stays(self):
        d = decided(LIST.replace("cold *4demo4dock", "ram *4demo4menu"))
        self.assertEqual(d["flash"], ["_RNv4demo5stray"])
        # menu is RAM, so everything it reads stays in RAM too.
        self.assertEqual(d["ro_flash"], [])

    def test_a_hot_line_that_matches_nothing_stops_the_split(self):
        with self.assertRaises(SystemExit):
            decided(LIST + "hot *4demo6renamed\n")


# A pass-2 link: dock and menu in flash, everything else in RAM.
P2 = dict(F, _RNv4demo4dock=0x400D0030, _RNv4demo4menu=0x400D0050)
PLACE = (0x400D0020, 0x100, 0x3F400120, 0x100)
SECTIONS = {".text": (0x40092000, 0x100), ".flash_text": (0x400D0020, 0x60),
            ".flash_rodata": (0x3F400120, 0x10), ".rodata": (0x3FFF4000, 0x24)}


def linked(extra="", words=None, place=PLACE, sections=SECTIONS, list_text=LIST):
    nm = "\n".join(f"{a:08x} 00000020 {'T' if n == 'game_entry' else 't'} {n}" for n, a in P2.items())
    funcs = fs.parse_nm(nm)
    w = {lit(0): P2["_RNv4demo4tick"], lit(2): P2["_RNv4demo4dock"], 0x400D0020: P2["_RNv4demo4menu"]}
    w.update(words or {})
    text = "\n".join([
        f"{P2['_RNv4demo4draw']:08x}:\t000081 \tl32r\ta8, {lit(0):08x} <x>",
        f"{P2['_RNv4demo4tick']:08x}:\t000081 \tl32r\ta8, {lit(2):08x} <x>",
        f"{P2['_RNv4demo4tick'] + 3:08x}:\t0008e0 \tcallx8\ta8",
        f"{P2['_RNv4demo4dock']:08x}:\t000081 \tl32r\ta8, 400d0020 <x>",
        f"{P2['_RNv4demo4dock'] + 3:08x}:\tfff806 \tj\t400d0030 <_RNv4demo4dock>",
    ] + extra.splitlines())
    insns = fs.parse_objdump(text, fs.owner_of(funcs))
    return fs.check_linked(funcs, insns, w.get, sections, fs.read_list(list_text.splitlines()), place)


class Check(unittest.TestCase):
    def test_a_good_link_passes_and_counts_the_crossing(self):
        bad, report = linked()
        self.assertEqual(bad, [])
        self.assertIn("through l32r + callx: 1;", report[1])
        self.assertTrue(any("_RNv4demo4dock  <- _RNv4demo4tick" in r for r in report))

    def test_a_direct_call_across_fails(self):
        bad, _ = linked(f"{P2['_RNv4demo4draw'] + 3:08x}:\t000025 \tcall8\t400d0030 <_RNv4demo4dock>")
        self.assertTrue(any("call8" in b and "crosses" in b for b in bad), bad)

    def test_a_branch_across_fails(self):
        bad, _ = linked(f"{P2['_RNv4demo4menu'] + 6:08x}:\t000016 \tbeqz\ta2, 40092020 <_RNv4demo4draw>")
        self.assertTrue(any("beqz" in b for b in bad), bad)

    def test_a_literal_in_the_other_memory_fails(self):
        bad, _ = linked(f"{P2['_RNv4demo4menu']:08x}:\t000081 \tl32r\ta8, {lit(9):08x} <x>")
        self.assertTrue(any("other memory" in b for b in bad), bad)

    def test_a_per_picture_function_in_flash_fails(self):
        # The list now says dock runs per picture (drift).
        bad, _ = linked(list_text=LIST.replace("cold *4demo4dock", "hot *4demo4dock"))
        self.assertTrue(any("_RNv4demo4dock must stay in RAM" in b for b in bad), bad)
        # Reached per picture: dock is no longer a barrier, so menu too.
        bad, _ = linked(list_text=LIST.replace("cold *4demo4dock", ""))
        self.assertTrue(any("_RNv4demo4menu must stay in RAM (reached" in b for b in bad), bad)

    def test_a_pinned_function_in_flash_fails(self):
        bad, _ = linked(list_text=LIST + "ram *4demo4menu\n")
        self.assertTrue(any("pinned" in b for b in bad), bad)

    def test_a_part_too_big_for_its_place_fails(self):
        bad, _ = linked(place=(0x400D0020, 0x20, 0x3F400120, 0x100))
        self.assertTrue(any(".flash_text" in b and "place" in b for b in bad), bad)
        bad, _ = linked(place=(0x400D0020, 0x100, 0x3F400120, 0x8))
        self.assertTrue(any(".flash_rodata" in b for b in bad), bad)

    def test_another_section_in_the_place_fails(self):
        s = dict(SECTIONS, **{".rodata": (0x3F400130, 4)})
        bad, _ = linked(sections=s)
        self.assertTrue(any(".rodata lies in the flash place" in b for b in bad), bad)

    def test_branch_targets_are_read(self):
        self.assertEqual(fs.branch_target("j", "40092000 <x+0x4>"), 0x40092000)
        self.assertEqual(fs.branch_target("bnez.n", "a2, 400d0030 <y>"), 0x400D0030)
        self.assertEqual(fs.branch_target("loopgtz", "a3, 40092100 <z>"), 0x40092100)
        self.assertIsNone(fs.branch_target("callx8", "a8"))
        self.assertIsNone(fs.branch_target("break", "1, 15"))


if __name__ == "__main__":
    unittest.main(verbosity=0)
