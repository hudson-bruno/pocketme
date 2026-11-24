#!/usr/bin/env python3
"""Turn a built CYD firmware (an ELF file) into ready-to-flash files.

    python3 -I tools/release/make_release.py --elf PATH/TO/pocketme-cyd \\
        --name pocketme-cyd-<version> --out DIR

It runs "espflash save-image" (files only; it never opens a serial port)
and writes into DIR:

    <name>.bin                      merged image, written at 0x0
    <name>-bootloader-0x1000.bin    bootloader, written at 0x1000
    <name>-partitions-0x8000.bin    partition table, written at 0x8000
    <name>-app-0x10000.bin          the console, written at 0x10000
    BUILD.txt                       ELF SHA-256, espflash version, chip revision
    THIRD_PARTY_NOTICES.txt         licence notices for bundled libraries
    SHA256SUMS.txt                  what "sha256sum -c" reads
    manifest.json                   with --web-manifest or --web-installer
    index.html, media/*             only with --web-installer (1-click Web Serial installer page)

Before it writes anything it checks the image (ESP image headers and
their checksums and digests, flash mode, size and clock, the partition
table against boards/cyd/partitions.csv, the 0xFF gaps, the app's size
and the lowest chip revision the app accepts)
and stops with "CHECK FAILED" if one check fails. See README.md here.

Standard library only. Needs espflash 4.x on the PATH (or --espflash).
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
DEFAULT_TABLE = os.path.join(ROOT, "boards", "cyd", "partitions.csv")
NOTICES_FILE = os.path.join(HERE, "THIRD_PARTY_NOTICES.txt")
WEB_INDEX = os.path.join(HERE, "web", "index.html")
WEB_MEDIA_LIST = os.path.join(HERE, "web", "media.txt")
DOCS_MEDIA_DIR = os.path.join(ROOT, "docs", "media")

# Where the ESP32's first-stage ROM loader looks: fixed by the chip.
BOOT_ADDR = 0x1000
# espflash's and ESP-IDF's default partition table place.
TABLE_ADDR = 0x8000
# Bytes espflash writes for a table (esp-idf-part 0.6.0, to_bin:
# entries, MD5 row, 0xFF padding up to 0xC00).
TABLE_LEN = 0xC00
# The CYD's flash.
FLASH_SIZE = 4 * 1024 * 1024

# ESP image header (ESP-IDF esp_image_header_t, espflash ImageHeader):
# magic, segment count, SPI mode, size<<4 | frequency, entry, then the
# extended part (wp pin, drive strengths, chip id u16, min rev, min/max
# full rev, 4 reserved, hash_appended) for 24 bytes in all.
IMAGE_MAGIC = 0xE9
HEADER_LEN = 24
CHECKSUM_SEED = 0xEF
# Values espflash 4.7.0 writes (src/flasher/mod.rs, src/target):
SPI_MODES = {0: "QIO", 1: "QOUT", 2: "DIO", 3: "DOUT"}
FLASH_SIZES = {0: "1MB", 1: "2MB", 2: "4MB", 3: "8MB", 4: "16MB"}
ESP32_FREQS = {0x0: "40MHz", 0x1: "26MHz", 0x2: "20MHz", 0xF: "80MHz"}
WANT_MODE, WANT_SIZE, WANT_FREQ = 2, 2, 0xF  # DIO, 4 MB, 80 MHz
ESP32_CHIP_ID = 0
# The lowest ESP32 revision the app accepts (header bytes 15-16,
# min_chip_rev_full, major * 100 + minor). espflash takes it from the ELF;
# esp-hal 1.2.2 sets 300 (v3.0) for the ESP32 (esp_config.yml,
# min-chip-revision) and stops at start-up on an older chip. A change
# here changes which boards can run the console, so it is checked.
WANT_MIN_REV = 300

# Partition table entries (ESP-IDF gen_esp32part.py, esp-idf-part).
ENTRY_MAGIC = b"\xaa\x50"
ENTRY_LEN = 32
MD5_MAGIC = b"\xeb\xeb" + b"\xff" * 14
TYPES = {"app": 0x00, "data": 0x01}
APP_SUBTYPES = {"factory": 0x00, "test": 0x20}
APP_SUBTYPES.update({"ota_%d" % i: 0x10 + i for i in range(16)})
DATA_SUBTYPES = {
    "ota": 0x00, "phy": 0x01, "nvs": 0x02, "coredump": 0x03,
    "nvs_keys": 0x04, "efuse": 0x05, "undefined": 0x06,
    "esphttpd": 0x80, "fat": 0x81, "spiffs": 0x82, "littlefs": 0x83,
}
FLAGS = {"encrypted": 1, "readonly": 2}


class CheckFailed(Exception):
    """One of the checks on the image did not hold."""


def need(cond, msg):
    if not cond:
        raise CheckFailed(msg)


# ---------------------------------------------------------------- tables

def parse_number(text):
    t = text.strip().lower()
    mult = 1
    if t.endswith("k"):
        t, mult = t[:-1], 1024
    elif t.endswith("m"):
        t, mult = t[:-1], 1024 * 1024
    return int(t, 0) * mult


def parse_csv(text):
    """Rows of a partition CSV as (name, type, subtype, offset, size, flags).

    Only what this board uses: every row must give its offset."""
    rows = []
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        cols = [c.strip() for c in line.split(",")]
        need(len(cols) >= 5, "partition CSV row has too few columns: %r" % line)
        name, ty, sub, off, size = cols[:5]
        flags = cols[5] if len(cols) > 5 else ""
        ty_n = TYPES.get(ty.lower())
        if ty_n is None:
            ty_n = parse_number(ty)
        subs = APP_SUBTYPES if ty_n == 0 else DATA_SUBTYPES
        sub_n = subs.get(sub.lower())
        if sub_n is None:
            sub_n = parse_number(sub)
        need(off != "", "partition %s has no offset (not supported here)" % name)
        flag_n = 0
        for f in filter(None, (x.strip() for x in flags.split(":"))):
            need(f in FLAGS, "unknown partition flag %r" % f)
            flag_n |= FLAGS[f]
        rows.append((name, ty_n, sub_n, parse_number(off), parse_number(size), flag_n))
    need(rows, "partition CSV has no rows")
    return rows


def encode_table(rows):
    """The binary table as espflash writes it: TABLE_LEN bytes."""
    out = bytearray()
    for name, ty, sub, off, size, flags in rows:
        raw = name.encode("ascii")
        need(len(raw) <= 16, "partition name too long: %s" % name)
        out += ENTRY_MAGIC + bytes([ty, sub])
        out += off.to_bytes(4, "little") + size.to_bytes(4, "little")
        out += raw + b"\0" * (16 - len(raw)) + flags.to_bytes(4, "little")
    out += MD5_MAGIC + hashlib.md5(bytes(out)).digest()
    need(len(out) <= TABLE_LEN, "partition table too long")
    return bytes(out) + b"\xff" * (TABLE_LEN - len(out))


def parse_table(blob):
    """Decode a binary table; checks its MD5 row. Returns the rows."""
    rows = []
    pos = 0
    while True:
        need(pos + ENTRY_LEN <= len(blob), "partition table has no end (no MD5 row)")
        entry = blob[pos:pos + ENTRY_LEN]
        if entry[:2] == ENTRY_MAGIC:
            ty, sub = entry[2], entry[3]
            off = int.from_bytes(entry[4:8], "little")
            size = int.from_bytes(entry[8:12], "little")
            name = entry[12:28].split(b"\0")[0].decode("ascii")
            flags = int.from_bytes(entry[28:32], "little")
            rows.append((name, ty, sub, off, size, flags))
            pos += ENTRY_LEN
            continue
        need(entry[:16] == MD5_MAGIC,
             "partition table: entry at +0x%x is neither a row (0xAA50) "
             "nor the MD5 row (0xEBEB)" % pos)
        need(entry[16:] == hashlib.md5(blob[:pos]).digest(),
             "partition table: MD5 row does not match the rows")
        end = pos + ENTRY_LEN
        need(all(b == 0xFF for b in blob[end:]),
             "partition table: bytes after the MD5 row are not 0xFF")
        return rows


# ---------------------------------------------------------------- images

def parse_image(blob, where):
    """Walk one ESP image at the start of blob.

    Returns a dict with its header fields and its length (header, segments,
    padding, checksum byte, and the SHA-256 when hash_appended is 1).
    Checks the magic, the checksum and the appended SHA-256."""
    need(len(blob) >= HEADER_LEN, "%s: shorter than an image header" % where)
    need(blob[0] == IMAGE_MAGIC,
         "%s: image magic is 0x%02X, not 0xE9" % (where, blob[0]))
    count = blob[1]
    pos = HEADER_LEN
    xor = CHECKSUM_SEED
    segments = []
    for i in range(count):
        need(pos + 8 <= len(blob), "%s: segment %d header past the end" % (where, i))
        load = int.from_bytes(blob[pos:pos + 4], "little")
        length = int.from_bytes(blob[pos + 4:pos + 8], "little")
        pos += 8
        need(pos + length <= len(blob), "%s: segment %d runs past the end" % (where, i))
        for b in blob[pos:pos + length]:
            xor ^= b
        segments.append((load, length))
        pos += length
    # Padding so that the checksum byte ends a 16-byte block.
    pos += (15 - pos % 16)
    need(pos < len(blob), "%s: checksum byte past the end" % where)
    need(blob[pos] == xor,
         "%s: checksum byte 0x%02X, segments give 0x%02X" % (where, blob[pos], xor))
    pos += 1
    hashed = blob[23] == 1
    if hashed:
        need(pos + 32 <= len(blob), "%s: SHA-256 past the end" % where)
        need(hashlib.sha256(blob[:pos]).digest() == blob[pos:pos + 32],
             "%s: appended SHA-256 does not match" % where)
        pos += 32
    return {
        "mode": blob[2],
        "size": blob[3] >> 4,
        "freq": blob[3] & 0xF,
        "chip_id": int.from_bytes(blob[12:14], "little"),
        "min_rev": int.from_bytes(blob[15:17], "little"),
        "max_rev": int.from_bytes(blob[17:19], "little"),
        "segments": segments,
        "hashed": hashed,
        "length": pos,
    }


def check_flash_header(info, where):
    need(info["mode"] == WANT_MODE, "%s: flash mode %s, want DIO"
         % (where, SPI_MODES.get(info["mode"], hex(info["mode"]))))
    need(info["size"] == WANT_SIZE, "%s: flash size %s, want 4MB"
         % (where, FLASH_SIZES.get(info["size"], hex(info["size"]))))
    need(info["freq"] == WANT_FREQ, "%s: flash clock %s, want 80MHz"
         % (where, ESP32_FREQS.get(info["freq"], hex(info["freq"]))))
    need(info["chip_id"] == ESP32_CHIP_ID,
         "%s: chip id %d, want 0 (ESP32)" % (where, info["chip_id"]))


def check_min_rev(info, where):
    need(info["min_rev"] == WANT_MIN_REV,
         "%s: lowest chip revision v%d.%d, expected v%d.%d (see WANT_MIN_REV)"
         % (where, info["min_rev"] // 100, info["min_rev"] % 100,
            WANT_MIN_REV // 100, WANT_MIN_REV % 100))


def rev_text(rev):
    return "v%d.%d" % (rev // 100, rev % 100)


def all_ff(blob):
    return blob.count(0xFF) == len(blob)


def find_row(rows, ty, sub):
    for r in rows:
        if r[1] == ty and r[2] == sub:
            return r
    return None


def check_merged(merged, rows):
    """Check a merged image (written at 0x0) against the table rows.

    Returns (bootloader length, app length, app address, app header)."""
    need(len(merged) > BOOT_ADDR, "merged image ends before the bootloader")
    need(all_ff(merged[:BOOT_ADDR]), "bytes 0x0..0x1000 are not all 0xFF")
    boot = parse_image(merged[BOOT_ADDR:TABLE_ADDR], "bootloader at 0x1000")
    check_flash_header(boot, "bootloader at 0x1000")
    boot_end = BOOT_ADDR + boot["length"]
    need(all_ff(merged[boot_end:TABLE_ADDR]),
         "bytes between the bootloader's end (0x%X) and 0x8000 are not all 0xFF" % boot_end)

    table = merged[TABLE_ADDR:TABLE_ADDR + TABLE_LEN]
    need(len(table) == TABLE_LEN, "merged image ends inside the partition table")
    need(parse_table(table) == rows,
         "partition table at 0x8000 does not decode to the CSV's rows")
    need(table == encode_table(rows),
         "partition table at 0x8000 differs from the CSV encoded here")

    factory = find_row(rows, TYPES["app"], APP_SUBTYPES["factory"])
    need(factory is not None, "the CSV has no factory app partition")
    app_addr, app_room = factory[3], factory[4]
    need(app_addr > TABLE_ADDR + TABLE_LEN, "factory partition overlaps the table")
    need(all_ff(merged[TABLE_ADDR + TABLE_LEN:app_addr]),
         "bytes between the table's end (0x%X) and the app (0x%X) are not all 0xFF"
         % (TABLE_ADDR + TABLE_LEN, app_addr))

    app = parse_image(merged[app_addr:], "app at 0x%X" % app_addr)
    check_flash_header(app, "app at 0x%X" % app_addr)
    check_min_rev(app, "app at 0x%X" % app_addr)
    app_end = app_addr + app["length"]
    need(app["length"] <= app_room,
         "app is %d bytes, its partition holds %d" % (app["length"], app_room))
    need(len(merged) == app_end,
         "merged image is %d bytes but ends its app at %d (padded or cut?)"
         % (len(merged), app_end))
    need(len(merged) <= FLASH_SIZE, "merged image is bigger than the 4 MB flash")
    # Nothing may reach a partition that starts after the app.
    for name, ty, sub, off, size, _ in rows:
        if off >= app_addr + app_room or (off > app_addr and ty != TYPES["app"]):
            need(len(merged) <= off,
                 "merged image reaches partition %s at 0x%X" % (name, off))
    return boot["length"], app["length"], app_addr, app


def cut_parts(merged, boot_len, app_addr):
    """The three parts as (address, bytes), cut out of the merged image."""
    return [
        (BOOT_ADDR, merged[BOOT_ADDR:BOOT_ADDR + boot_len]),
        (TABLE_ADDR, merged[TABLE_ADDR:TABLE_ADDR + TABLE_LEN]),
        (app_addr, merged[app_addr:]),
    ]


def rebuild(parts, length):
    """Lay the parts out on 0xFF, as a flasher writing them would."""
    out = bytearray(b"\xff" * length)
    for addr, blob in parts:
        out[addr:addr + len(blob)] = blob
    return bytes(out)


def wiped_partitions(rows, length):
    """Partitions (name, offset, size) that a write of `length` bytes at 0x0
    covers, other than the app: they are written with 0xFF."""
    out = []
    for name, ty, sub, off, size, _ in rows:
        if ty != TYPES["app"] and off < length:
            out.append((name, off, min(size, length - off)))
    return out


def manifest(name, version, part_files):
    """Web installer manifest (compatible with ESP Web Tools and index.html).

    Written with --web-manifest or --web-installer. It lists the three
    separate parts (bootloader at 4096, partition table at 32768, app at
    65536) rather than a single merged image so that both fresh installs
    and updates work while leaving 0x9000..0x10000 (saved Bluetooth
    gamepad pairing) and 0x3FE000..0x400000 (console settings) untouched
    whenever the user chooses KEEP DATA. ESP Web Tools compatible fields
    (new_install_prompt_erase: true, new_install_improv_wait_time: 0)
    are kept so external ESP Web Tools hosts also prompt instead of
    wiping NVS/settings without asking."""
    return {
        "name": name,
        "version": version,
        "new_install_prompt_erase": True,
        "new_install_improv_wait_time": 0,
        "builds": [{
            "chipFamily": "ESP32",
            "parts": [{"path": path, "offset": offset} for offset, path in part_files],
        }],
    }


def load_web_media(list_path=WEB_MEDIA_LIST):
    """Curated list of media filenames bundled alongside the 1-click installer."""
    with open(list_path, encoding="utf-8") as f:
        return [line.strip() for line in f if line.strip()]


def build_text(name, version, elf_name, elf_sha, espflash_version, app_info, table_name):
    """BUILD.txt: what the files were made from, so a release can be traced."""
    return "".join("%s\n" % line for line in (
        "name: %s" % name,
        "version: %s" % version,
        "elf: %s" % elf_name,
        "elf sha256: %s" % elf_sha,
        "made with: %s (save-image)" % espflash_version,
        "partition table: %s" % table_name,
        "flash: DIO, 4MB, 80MHz",
        "chip: ESP32, revision %s or later (app header; up to %s)"
        % (rev_text(app_info["min_rev"]), rev_text(app_info["max_rev"])),
    ))


# ---------------------------------------------------------------- main

def run_espflash(espflash, args):
    env = dict(os.environ, ESPFLASH_SKIP_UPDATE_CHECK="true")
    proc = subprocess.run([espflash] + args, env=env,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stdout)
        raise CheckFailed("espflash %s failed (exit %d)" % (args[0], proc.returncode))
    return proc.stdout


def sha256(blob):
    return hashlib.sha256(blob).hexdigest()


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--elf", required=True, help="built pocketme-cyd ELF file")
    ap.add_argument("--name", required=True, help="file name stem, e.g. pocketme-cyd-1.0")
    ap.add_argument("--out", required=True, help="output folder (made if missing)")
    ap.add_argument("--version", help="version for BUILD.txt and manifest.json "
                    "(default: --name without 'pocketme-cyd-')")
    ap.add_argument("--web-manifest", action="store_true",
                    help="also write manifest.json for the 1-click browser installer / ESP Web Tools "
                    "(three parts at 0x1000/0x8000/0x10000)")
    ap.add_argument("--web-installer", action="store_true",
                    help="also write manifest.json, index.html and curated media/* for the "
                    "1-click PocketMe browser installer (implies --web-manifest)")
    ap.add_argument("--partition-table", default=DEFAULT_TABLE,
                    help="partition CSV (default: boards/cyd/partitions.csv)")
    ap.add_argument("--espflash", default=None, help="espflash program (default: from PATH)")
    a = ap.parse_args(argv)

    espflash = a.espflash or shutil.which("espflash")
    if not espflash:
        sys.exit("make_release: espflash not found on the PATH (try --espflash)")
    if not os.path.isfile(a.elf):
        sys.exit("make_release: no such ELF file: %s" % a.elf)
    if os.sep in a.name or a.name in ("", ".", ".."):
        sys.exit("make_release: --name must be a plain file name stem")
    version = a.version or (a.name[len("pocketme-cyd-"):]
                            if a.name.startswith("pocketme-cyd-") else a.name)

    if not os.path.isfile(NOTICES_FILE):
        sys.exit("make_release: missing notices file: %s" % NOTICES_FILE)
    web_media = []
    if a.web_installer:
        if not os.path.isfile(WEB_INDEX):
            sys.exit("make_release: missing web installer page: %s" % WEB_INDEX)
        if not os.path.isfile(WEB_MEDIA_LIST):
            sys.exit("make_release: missing web media list: %s" % WEB_MEDIA_LIST)
        web_media = load_web_media()
        if not web_media:
            sys.exit("make_release: web media list is empty: %s" % WEB_MEDIA_LIST)
        for fname in web_media:
            if not os.path.isfile(os.path.join(DOCS_MEDIA_DIR, fname)):
                sys.exit("make_release: missing media asset: %s"
                         % os.path.join(DOCS_MEDIA_DIR, fname))
    write_manifest = a.web_manifest or a.web_installer

    with open(a.partition_table, encoding="utf-8") as f:
        csv_text = f.read()
    with open(NOTICES_FILE, encoding="utf-8") as f:
        notices_text = f.read()
    web_index_text = None
    if a.web_installer:
        with open(WEB_INDEX, encoding="utf-8") as f:
            web_index_text = f.read()
    os.makedirs(a.out, exist_ok=True)
    common = ["save-image", "--chip", "esp32", "--partition-table", a.partition_table,
              "--flash-mode", "dio", "--flash-size", "4mb", "--flash-freq", "80mhz",
              "--skip-update-check"]
    try:
        rows = parse_csv(csv_text)
        espflash_version = run_espflash(espflash, ["--version"]).strip()
        with tempfile.TemporaryDirectory(prefix=".make_release-", dir=a.out) as tmp:
            merged_path = os.path.join(tmp, "merged.bin")
            app_path = os.path.join(tmp, "app.bin")
            # --skip-padding: end after the app instead of filling all
            # 4 MB with 0xFF (which would also wipe the settings).
            run_espflash(espflash, common + ["--merge", "--skip-padding", a.elf, merged_path])
            run_espflash(espflash, common + [a.elf, app_path])
            with open(merged_path, "rb") as f:
                merged = f.read()
            with open(app_path, "rb") as f:
                app_alone = f.read()

        boot_len, app_len, app_addr, app_info = check_merged(merged, rows)
        parts = cut_parts(merged, boot_len, app_addr)
        need(parts[2][1] == app_alone,
             "app cut from the merged image differs from save-image without --merge")
        need(rebuild(parts, len(merged)) == merged,
             "the three parts laid out on 0xFF do not give the merged image")
    except CheckFailed as e:
        sys.exit("make_release: CHECK FAILED: %s" % e)

    files = [("%s.bin" % a.name, 0x0, merged)]
    labels = {BOOT_ADDR: "bootloader", TABLE_ADDR: "partitions", app_addr: "app"}
    for addr, blob in parts:
        files.append(("%s-%s-0x%X.bin" % (a.name, labels[addr], addr), addr, blob))
    with open(a.elf, "rb") as f:
        elf_sha = sha256(f.read())
    texts = [
        ("BUILD.txt", build_text(
            a.name, version, os.path.basename(a.elf), elf_sha, espflash_version,
            app_info, os.path.basename(a.partition_table))),
        ("THIRD_PARTY_NOTICES.txt", notices_text),
    ]
    if write_manifest:
        man = manifest("PocketMe", version, [(addr, fname) for fname, addr, _ in files[1:]])
        texts.append(("manifest.json", json.dumps(man, indent=2) + "\n"))
    stale_files = []
    if not write_manifest:
        stale_manifest = os.path.join(a.out, "manifest.json")
        if os.path.exists(stale_manifest):
            stale_files.append(stale_manifest)
    if not a.web_installer:
        stale_index = os.path.join(a.out, "index.html")
        if os.path.exists(stale_index):
            stale_files.append(stale_index)
    for fname, _, blob in files:
        with open(os.path.join(a.out, fname), "wb") as f:
            f.write(blob)
    for fname, text in texts:
        with open(os.path.join(a.out, fname), "w", encoding="utf-8", newline="") as f:
            f.write(text)
    sums = [(sha256(blob), fname) for fname, _, blob in files]
    sums += [(sha256(text.encode("utf-8")), fname) for fname, text in texts]
    with open(os.path.join(a.out, "SHA256SUMS.txt"), "w", encoding="utf-8", newline="") as f:
        for digest, fname in sums:
            f.write("%s  %s\n" % (digest, fname))
    extra_written = []
    if a.web_installer:
        with open(os.path.join(a.out, "index.html"), "w", encoding="utf-8", newline="") as f:
            f.write(web_index_text)
        media_out = os.path.join(a.out, "media")
        os.makedirs(media_out, exist_ok=True)
        for fname in web_media:
            shutil.copy2(os.path.join(DOCS_MEDIA_DIR, fname), os.path.join(media_out, fname))
        extra_written.append("index.html")
        extra_written.append("media/ (%d files)" % len(web_media))

    factory = find_row(rows, TYPES["app"], APP_SUBTYPES["factory"])
    print("make_release: all checks passed (DIO, 4MB, 80MHz; table = %s)"
          % os.path.relpath(a.partition_table, ROOT))
    print("%-10s %10s  %-44s %s" % ("address", "bytes", "file", "sha256"))
    for fname, addr, blob in files:
        print("0x%08X %10d  %-44s %s" % (addr, len(blob), fname, sha256(blob)[:16]))
    print("app: %d of %d bytes in its partition (%.1f %%); merged image ends at 0x%X"
          % (app_len, factory[4], 100.0 * app_len / factory[4], len(merged)))
    wiped = ", ".join("%s 0x%X+0x%X" % w for w in wiped_partitions(rows, len(merged)))
    print("the merged image writes 0xFF over: %s" % (wiped or "nothing"))
    print("chip: ESP32 revision %s or later (app header); %s"
          % (rev_text(app_info["min_rev"]), espflash_version))
    wrote_list = [f for f, _, _ in files] + [f for f, _ in texts] + ["SHA256SUMS.txt"] + extra_written
    print("wrote %s in %s" % (", ".join(wrote_list), a.out))
    for stale in stale_files:
        # Not written by this run and not in SHA256SUMS.txt: it must not be
        # shipped by mistake.
        sys.stderr.write("make_release: WARNING: %s is from an earlier run and is "
                         "not part of these files; remove it before release\n" % stale)


if __name__ == "__main__":
    main()
