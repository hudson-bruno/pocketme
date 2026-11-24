"""Tests for the pure parts of make_release (made-up bytes, no espflash).

    cd tools/release && python3 -I -B -m unittest
"""

import hashlib
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import make_release as m  # noqa: E402

CSV = """\
# Name, Type, SubType, Offset, Size
nvs,        data, nvs,     0x9000,   0x6000
phy_init,   data, phy,     0xf000,   0x1000
factory,    app,  factory, 0x10000,  0x3e0000
settings,   data, undefined, 0x3fe000, 0x2000
"""


def image(segments, mode=2, config=0x2F, hashed=True, min_rev=0):
    """A made-up ESP image built by the same rules ESP-IDF uses."""
    head = bytearray(24)
    head[0] = 0xE9
    head[1] = len(segments)
    head[2] = mode
    head[3] = config
    head[15:17] = min_rev.to_bytes(2, "little")
    head[17:19] = (399).to_bytes(2, "little")
    head[23] = 1 if hashed else 0
    out = bytearray(head)
    xor = 0xEF
    for load, data in segments:
        out += load.to_bytes(4, "little") + len(data).to_bytes(4, "little") + data
        for b in data:
            xor ^= b
    out += b"\0" * (15 - len(out) % 16)
    out.append(xor)
    if hashed:
        out += hashlib.sha256(bytes(out)).digest()
    return bytes(out)


def merged_from(rows, boot, app, app_addr=0x10000):
    out = bytearray(b"\xff" * (app_addr + len(app)))
    out[0x1000:0x1000 + len(boot)] = boot
    table = m.encode_table(rows)
    out[0x8000:0x8000 + len(table)] = table
    out[app_addr:] = app
    return bytes(out)


class TableTests(unittest.TestCase):
    def test_csv_rows(self):
        rows = m.parse_csv(CSV)
        self.assertEqual(rows, [
            ("nvs", 1, 2, 0x9000, 0x6000, 0),
            ("phy_init", 1, 1, 0xF000, 0x1000, 0),
            ("factory", 0, 0, 0x10000, 0x3E0000, 0),
            ("settings", 1, 6, 0x3FE000, 0x2000, 0),
        ])

    def test_suffixes_and_flags(self):
        rows = m.parse_csv("a, app, ota_1, 64K, 1M, readonly\nb, data, 0x40, 0x9000, 4k, encrypted:readonly\n")
        self.assertEqual(rows[0], ("a", 0, 0x11, 0x10000, 0x100000, 2))
        self.assertEqual(rows[1], ("b", 1, 0x40, 0x9000, 0x1000, 3))

    def test_row_without_offset_is_refused(self):
        with self.assertRaises(m.CheckFailed):
            m.parse_csv("nvs, data, nvs, , 0x6000\n")

    def test_encode_then_parse_round_trip(self):
        rows = m.parse_csv(CSV)
        blob = m.encode_table(rows)
        self.assertEqual(len(blob), m.TABLE_LEN)
        self.assertEqual(blob[:2], b"\xaa\x50")
        self.assertEqual(blob[4 * 32:4 * 32 + 2], b"\xeb\xeb")
        self.assertEqual(m.parse_table(blob), rows)

    def test_first_entry_bytes(self):
        blob = m.encode_table(m.parse_csv(CSV))
        self.assertEqual(blob[:12].hex(), "aa50010200900000" + "00600000")
        self.assertEqual(blob[12:28], b"nvs" + b"\0" * 13)

    def test_bad_md5_is_found(self):
        blob = bytearray(m.encode_table(m.parse_csv(CSV)))
        blob[4 * 32 + 20] ^= 1
        with self.assertRaisesRegex(m.CheckFailed, "MD5"):
            m.parse_table(bytes(blob))

    def test_changed_row_is_found(self):
        blob = bytearray(m.encode_table(m.parse_csv(CSV)))
        blob[5] ^= 0x10  # nvs offset
        with self.assertRaisesRegex(m.CheckFailed, "MD5"):
            m.parse_table(bytes(blob))

    def test_garbage_after_table_is_found(self):
        blob = bytearray(m.encode_table(m.parse_csv(CSV)))
        blob[-1] = 0
        with self.assertRaisesRegex(m.CheckFailed, "0xFF"):
            m.parse_table(bytes(blob))


class ImageTests(unittest.TestCase):
    def test_length_and_fields(self):
        blob = image([(0x3FFF0000, b"\x01\x02\x03"), (0x40080000, b"\xaa" * 20)])
        info = m.parse_image(blob + b"\xff" * 50, "t")
        self.assertEqual(info["length"], len(blob))
        self.assertEqual((info["mode"], info["size"], info["freq"]), (2, 2, 0xF))
        self.assertEqual(info["segments"], [(0x3FFF0000, 3), (0x40080000, 20)])
        self.assertTrue(info["hashed"])
        m.check_flash_header(info, "t")

    def test_unhashed_length(self):
        blob = image([(0, b"x" * 7)], hashed=False)
        self.assertEqual(len(blob) % 16, 0)
        self.assertEqual(m.parse_image(blob, "t")["length"], len(blob))

    def test_bad_magic(self):
        blob = bytearray(image([(0, b"abc")]))
        blob[0] = 0xE8
        with self.assertRaisesRegex(m.CheckFailed, "magic"):
            m.parse_image(bytes(blob), "t")

    def test_bad_checksum(self):
        blob = bytearray(image([(0, b"abc")], hashed=False))
        blob[-1] ^= 1
        with self.assertRaisesRegex(m.CheckFailed, "checksum"):
            m.parse_image(bytes(blob), "t")

    def test_bad_digest(self):
        blob = bytearray(image([(0, b"abc")]))
        blob[24 + 8] ^= 1  # segment data; fix the checksum so only the digest fails
        blob[-33] ^= 1
        with self.assertRaisesRegex(m.CheckFailed, "SHA-256"):
            m.parse_image(bytes(blob), "t")

    def test_wrong_flash_settings(self):
        for mode, cfg, word in ((0, 0x2F, "DIO"), (2, 0x1F, "4MB"), (2, 0x20, "80MHz")):
            info = m.parse_image(image([(0, b"a")], mode=mode, config=cfg), "t")
            with self.assertRaisesRegex(m.CheckFailed, word):
                m.check_flash_header(info, "t")


class MergedTests(unittest.TestCase):
    def setUp(self):
        self.rows = m.parse_csv(CSV)
        self.boot = image([(0x3FFF0040, b"b" * 100), (0x40078000, b"c" * 37)])
        self.app = image([(0x3F400020, bytes(range(256)) * 4), (0x400D0020, b"d" * 999)],
                         min_rev=300)
        self.merged = merged_from(self.rows, self.boot, self.app)

    def test_good_image_passes_and_cuts(self):
        boot_len, app_len, app_addr, app = m.check_merged(self.merged, self.rows)
        self.assertEqual((boot_len, app_len, app_addr), (len(self.boot), len(self.app), 0x10000))
        self.assertEqual((app["min_rev"], app["max_rev"]), (300, 399))
        parts = m.cut_parts(self.merged, boot_len, app_addr)
        self.assertEqual([a for a, _ in parts], [0x1000, 0x8000, 0x10000])
        self.assertEqual(parts[0][1], self.boot)
        self.assertEqual(parts[1][1], m.encode_table(self.rows))
        self.assertEqual(parts[2][1], self.app)
        self.assertEqual(m.rebuild(parts, len(self.merged)), self.merged)

    def test_wiped_partitions(self):
        self.assertEqual(m.wiped_partitions(self.rows, len(self.merged)),
                         [("nvs", 0x9000, 0x6000), ("phy_init", 0xF000, 0x1000)])

    def test_padded_image_is_refused(self):
        with self.assertRaisesRegex(m.CheckFailed, "padded"):
            m.check_merged(self.merged + b"\xff" * 16, self.rows)

    def test_byte_in_nvs_is_found(self):
        blob = bytearray(self.merged)
        blob[0x9000] = 0
        with self.assertRaisesRegex(m.CheckFailed, "0x10000"):
            m.check_merged(bytes(blob), self.rows)

    def test_byte_after_bootloader_is_found(self):
        blob = bytearray(self.merged)
        blob[0x7FFF] = 0
        with self.assertRaisesRegex(m.CheckFailed, "0x8000"):
            m.check_merged(bytes(blob), self.rows)

    def test_byte_before_bootloader_is_found(self):
        blob = bytearray(self.merged)
        blob[0] = 0
        with self.assertRaisesRegex(m.CheckFailed, "0x1000"):
            m.check_merged(bytes(blob), self.rows)

    def test_table_from_another_csv_is_refused(self):
        other = m.parse_csv(CSV.replace("0x3e0000", "0x3f0000"))
        blob = merged_from(other, self.boot, self.app)
        with self.assertRaisesRegex(m.CheckFailed, "CSV"):
            m.check_merged(blob, self.rows)

    def test_wrong_app_clock_is_refused(self):
        app = image([(0x3F400020, b"e" * 64)], config=0x20, min_rev=300)
        with self.assertRaisesRegex(m.CheckFailed, "app at 0x10000.*80MHz"):
            m.check_merged(merged_from(self.rows, self.boot, app), self.rows)

    def test_other_lowest_chip_revision_is_refused(self):
        for rev, text in ((0, "v0.0"), (301, "v3.1")):
            app = image([(0x3F400020, b"g" * 64)], min_rev=rev)
            with self.assertRaisesRegex(m.CheckFailed, "chip revision %s, expected v3.0" % text):
                m.check_merged(merged_from(self.rows, self.boot, app), self.rows)

    def test_app_too_big_for_its_partition(self):
        rows = m.parse_csv(CSV.replace("0x3e0000", "0x400"))
        blob = merged_from(rows, self.boot, self.app)
        with self.assertRaisesRegex(m.CheckFailed, "partition holds"):
            m.check_merged(blob, rows)

    def test_image_reaching_settings_is_refused(self):
        # A (broken) table whose settings start inside the app's room,
        # under the end of the app.
        rows = m.parse_csv(CSV.replace("0x3fe000", "0x10200").replace("0x3e0000", "0x400"))
        app = image([(0x3F400020, b"f" * 0x300)], min_rev=300)
        with self.assertRaisesRegex(m.CheckFailed, "reaches partition settings"):
            m.check_merged(merged_from(rows, self.boot, app), rows)


class ManifestTests(unittest.TestCase):
    def test_shape(self):
        parts = [
            (0x1000, "pocketme-cyd-1.0-bootloader-0x1000.bin"),
            (0x8000, "pocketme-cyd-1.0-partitions-0x8000.bin"),
            (0x10000, "pocketme-cyd-1.0-app-0x10000.bin"),
        ]
        man = m.manifest("PocketMe", "1.0", parts)
        self.assertEqual(man["name"], "PocketMe")
        # True: the person is asked; false would erase the whole chip.
        self.assertIs(man["new_install_prompt_erase"], True)
        self.assertEqual(man["new_install_improv_wait_time"], 0)
        self.assertEqual(len(man["builds"]), 1)
        build = man["builds"][0]
        self.assertEqual(build["chipFamily"], "ESP32")
        self.assertEqual(build["parts"], [
            {"path": "pocketme-cyd-1.0-bootloader-0x1000.bin", "offset": 4096},
            {"path": "pocketme-cyd-1.0-partitions-0x8000.bin", "offset": 32768},
            {"path": "pocketme-cyd-1.0-app-0x10000.bin", "offset": 65536},
        ])


class WebInstallerAssetsTests(unittest.TestCase):
    def test_web_index_has_flash_installer(self):
        self.assertTrue(os.path.isfile(m.WEB_INDEX), m.WEB_INDEX)
        with open(m.WEB_INDEX, encoding="utf-8") as f:
            html = f.read()
        self.assertIn("esptool-js@0.5.7", html)
        self.assertIn("manifest.json", html)
        self.assertIn('id="flash-modal"', html)
        self.assertIn("navigator.serial", html)
        self.assertIn('rel="icon"', html)
        self.assertIn('property="og:image"', html)
        self.assertIn("https://hudson-bruno.github.io/pocketme/media/camber-carve-lap.gif", html)
        self.assertIn("summary_large_image", html)
        self.assertIn('name="theme-color" content="#f7d154"', html)
        self.assertIn("ESP32-2432S028 (2 USB)", html)
        self.assertIn('id="flash-custom-opts"', html)
        self.assertIn('id="lang-switch"', html)
        self.assertIn("flag-ico", html)
        self.assertIn('"pt-BR"', html)
        self.assertIn("navigator.languages", html)
        self.assertIn("pocketme_lang", html)
        self.assertNotIn("&blacktriangleright;", html)

    def test_notices_file_exists_and_non_empty(self):
        self.assertTrue(os.path.isfile(m.NOTICES_FILE), m.NOTICES_FILE)
        with open(m.NOTICES_FILE, encoding="utf-8") as f:
            text = f.read().strip()
        self.assertTrue(text)
        self.assertIn("MIT", text)
        self.assertIn("Apache-2.0", text)

    def test_web_media_list_exists_and_matches_html(self):
        self.assertTrue(os.path.isfile(m.WEB_MEDIA_LIST), m.WEB_MEDIA_LIST)
        media = m.load_web_media()
        self.assertGreater(len(media), 0)
        with open(m.WEB_INDEX, encoding="utf-8") as f:
            html = f.read()
        for fname in media:
            self.assertTrue(
                os.path.isfile(os.path.join(m.DOCS_MEDIA_DIR, fname)),
                "missing docs/media asset: %s" % fname,
            )
            self.assertIn("media/%s" % fname, html)


class BuildTextTests(unittest.TestCase):
    def test_lines(self):
        text = m.build_text("pocketme-cyd-1.0", "1.0", "pocketme-cyd", "ab" * 32,
                            "espflash 4.7.0", {"min_rev": 300, "max_rev": 399},
                            "partitions.csv")
        lines = text.splitlines()
        self.assertIn("elf sha256: " + "ab" * 32, lines)
        self.assertIn("made with: espflash 4.7.0 (save-image)", lines)
        self.assertIn("chip: ESP32, revision v3.0 or later (app header; up to v3.99)", lines)
        self.assertTrue(text.endswith("\n"))


if __name__ == "__main__":
    unittest.main()
