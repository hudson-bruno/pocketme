#!/usr/bin/env python3
"""Self-test of check_frames.py on a made-up disassembly (no chip tools).

  python3 games/chip/test_check_frames.py

Run by games/build-chip.sh before it builds anything. Standard library
only.
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_frames as cf  # noqa: E402

# Made up in the shape of xtensa-esp32-elf-objdump -d -C output: a
# literal pool (junk that happens to read as `entry`), small frames in
# decimal, a frame in hex, the big-frame forms (movi and l32r), a
# function without `entry`, and a restart with a big frame.
TEXT = """
x.elf:     file format elf32-xtensa-le


Disassembly of section .text:

40092000 <_game_code_start>:
40092000:\td40c      \tmovi.n\ta4, 13
40092002:\t7f6136    \tentry\ta1, 0x3fb8
40092005:\t3f          \t.byte\t0x3f

40092700 <<game_3d::view::Camera>::to_cam_with::<game_3d::sums::Checked>>:
40092700:\t006136        \tentry\ta1, 48
40092703:\t0020c0        \tmemw

40092730 <demo::game_entry::update>:
40092730:\t06e136        \tentry\ta1, 0x370
40092733:\t02bd      \tmov.n\ta11, a2

40092800 <game_kit::rt::restart::<demo::Demo>>:
40092800:\t552136        \tentry\tsp, 0x2a90
40092803:\te0dcb1        \tl32r\ta11, 40092670 <_game_code_start+0x670> (3fff5858 <GAME_DATA_ORIGIN+0x1858>)

40092900 <<demo::app::App>::draw>:
40092900:\t004136        \tentry\ta1, 32
40092903:\t7fa082        \tmovi\ta8, 0x7f8
40092906:\t8018c0        \tsub\ta8, a1, a8
40092909:\t001810        \tmovsp\ta1, a8

40092a00 <demo::huge>:
40092a00:\t004136        \tentry\ta1, 32
40092a03:\te0dc81        \tl32r\ta8, 40092674 <_game_code_start+0x674> (9000 <x>)
40092a06:\t8018c0        \tsub\ta8, a1, a8
40092a09:\t001810        \tmovsp\ta1, a8

40092a80 <demo::pair>:
40092a80:\t004136        \tentry\ta1, 32
40092a83:\t7fa082        \tmovi\ta8, 0x100
40092a86:\t7fa082        \taddmi\ta8, a8, 0x3000
40092a89:\t8018c0        \tsub\ta8, a1, a8
40092a8c:\t001810        \tmovsp\ta1, a8

40092ac0 <demo::changed>:
40092ac0:\t004136        \tentry\ta1, 32
40092ac3:\t7fa082        \tmovi\ta8, 0x100
40092ac6:\t7fa082        \tslli\ta8, a8, 8
40092ac9:\t8018c0        \tsub\ta8, a1, a8
40092acc:\t001810        \tmovsp\ta1, a8

40092b00 <demo::odd>:
40092b00:\t004136        \tentry\ta1, 32
40092b03:\t8018c0        \tsub\ta8, a1, a9
40092b06:\t001810        \tmovsp\ta1, a8

40092c00 <memcpy>:
40092c00:\t0020c0        \tmemw
40092c03:\t0020c0        \tret
"""


class Frames(unittest.TestCase):
    def setUp(self):
        self.found = {n: f for f, _a, n in cf.frames(TEXT)}

    def test_reads_only_the_first_instruction_of_each_function(self):
        # The literal pool's junk `entry` and `memcpy` (no entry) are
        # not functions with a frame.
        self.assertNotIn("_game_code_start", self.found)
        self.assertNotIn("memcpy", self.found)

    def test_reads_decimal_hex_and_sp(self):
        self.assertEqual(self.found["<game_3d::view::Camera>::to_cam_with::<game_3d::sums::Checked>"], 48)
        self.assertEqual(self.found["demo::game_entry::update"], 0x370)
        self.assertEqual(self.found["game_kit::rt::restart::<demo::Demo>"], 0x2a90)

    def test_reads_the_big_frame_form(self):
        self.assertEqual(self.found["<demo::app::App>::draw"], 32 + 0x7f8)
        self.assertEqual(self.found["demo::huge"], 32 + 0x9000)
        self.assertEqual(self.found["demo::pair"], 32 + 0x3100)
        self.assertIsNone(self.found["demo::odd"])
        # A write the script does not follow: unread, never smaller.
        self.assertIsNone(self.found["demo::changed"])

    def test_limits(self):
        bad = {n for _f, _a, n in cf.over(cf.frames(TEXT))}
        # restart at 10896 is under its own limit; a draw of 2072 under
        # the general one; frames of 12576 and 36896 and unreadable ones
        # are over.
        self.assertEqual(bad, {"demo::huge", "demo::odd", "demo::changed", "demo::pair"})
        self.assertEqual(cf.limit_of("game_kit::rt::restart::<x::Y>"), cf.RESTART_LIMIT)
        self.assertEqual(cf.limit_of("x::game_entry::update"), cf.OTHER_LIMIT)
        at = [(cf.OTHER_LIMIT, 0, "a"), (cf.OTHER_LIMIT + 1, 0, "b"),
              (cf.RESTART_LIMIT, 0, "game_kit::rt::restart::<c>"),
              (cf.RESTART_LIMIT + 1, 0, "game_kit::rt::restart::<d>")]
        self.assertEqual([n for _f, _a, n in cf.over(at)], ["b", "game_kit::rt::restart::<d>"])

    def test_table_is_biggest_first_and_marks_the_bad(self):
        lines = [l for l in cf.table(cf.frames(TEXT)).splitlines() if not l.startswith("#")]
        for i in (0, 1):
            self.assertTrue(lines[i].split()[0] == "?" and lines[i].endswith("TOO BIG"))
        self.assertIn("demo::huge", lines[2])
        self.assertIn("TOO BIG", lines[2])
        self.assertIn("demo::pair", lines[3])
        self.assertIn("game_kit::rt::restart", lines[4])
        self.assertNotIn("TOO BIG", lines[4])


if __name__ == "__main__":
    unittest.main(verbosity=0)
