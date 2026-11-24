# Release files

`make_release.py` turns a built CYD firmware (the `pocketme-cyd` ELF
file) into files that can be written to a board without building
anything: one merged image, the same thing as three separate parts, a
short build note, third-party licence notices and SHA-256 sums (and,
with `--web-manifest` or `--web-installer`, the files for a 1-click
browser installer). It is Python 3 with the standard library only. The
guide for the people who write these files to their board is
[docs/install.md](../../docs/install.md).

```bash
export PATH="$HOME/.cargo/bin:$PATH"     # espflash
python3 -I tools/release/make_release.py \
    --elf boards/cyd/target/xtensa-esp32-none-elf/release/pocketme-cyd \
    --name pocketme-cyd-<version> --out <folder> \
    --web-installer
```

`--version` sets the version in `BUILD.txt` and `manifest.json`
(default: the name without `pocketme-cyd-`), `--partition-table` another
CSV (default: `boards/cyd/partitions.csv`), `--espflash` the program to
run, `--web-manifest` writes `manifest.json` for
[the web installer](#web-installer--manifest) (off by default), and
`--web-installer` implies `--web-manifest`, copies
`tools/release/web/index.html` into `<folder>/index.html`, and copies
the curated showcase clips listed in `tools/release/web/media.txt` from
`docs/media/` into `<folder>/media/`.

The script runs only `espflash --version` and `espflash save-image`,
which work on files. It never opens a serial port and never touches a
board.

Tests for the parts that need no espflash (made-up bytes):

```bash
cd tools/release && python3 -I -B -m unittest
```

## The files

| File | Write it at | What it is for |
|---|---|---|
| `<name>.bin` | 0x0 | Bootloader, partition table and console in one file, with 0xFF in the gaps. The simplest way: one file, one address. |
| `<name>-bootloader-0x1000.bin` | 0x1000 | The ESP-IDF second-stage bootloader that espflash 4.7.0 carries, with its header set to DIO, 4 MB, 80 MHz. |
| `<name>-partitions-0x8000.bin` | 0x8000 | `boards/cyd/partitions.csv` in binary form (3,072 bytes: rows, MD5 row, 0xFF). |
| `<name>-app-0x10000.bin` | 0x10000 | The console itself, with the fourteen programs (eleven games, two prototypes, BENCH) when the firmware was built with `demo-card`. |
| `BUILD.txt` | - | What the files were made from: the ELF's file name and SHA-256, the espflash version (its bootloader is in the files), the flash settings, and the lowest chip revision the app accepts. |
| `THIRD_PARTY_NOTICES.txt` | - | Licence notices for PocketMe, the `esp-rs`/Embassy crates, the ESP-IDF bootloader and Espressif's radio libraries. |
| `SHA256SUMS.txt` | - | `sha256sum -c SHA256SUMS.txt` checks every flat release file written in the run (`*.bin`, `BUILD.txt`, `THIRD_PARTY_NOTICES.txt`, and `manifest.json` when generated). |
| `manifest.json` | - | With `--web-manifest` or `--web-installer`. Lists the three parts in ESP Web Tools format (`0x1000`, `0x8000`, `0x10000`); read by `index.html` and tested on CYD (`2026-11-24`). See [below](#web-installer--manifest). |
| `index.html`, `media/*` | - | Only with `--web-installer`. Showcase & 1-click Web Serial installer (`esptool-js@0.5.7`, automatic EN / PT-BR i18n, tested on CYD `2026-11-24`) plus the curated clips from `tools/release/web/media.txt`. |

The address is part of each part's file name, so a person using a
flashing tool that asks for "file and address" can read it off the name.

## What it checks

It stops with `CHECK FAILED` and writes nothing when one of these does
not hold:

- the ESP image magic 0xE9 at 0x1000 (bootloader) and at 0x10000 (app);
- each image's own checksum byte and its appended SHA-256 (the
  bootloader's and the app's are both checked, by walking their
  segments the way ESP-IDF lays them out);
- the header bytes of both images say DIO (byte 2 = 2), 4 MB (high
  nibble of byte 3 = 2) and 80 MHz (low nibble of byte 3 = 0xF), chip
  id 0 (ESP32). These values are espflash 4.7.0's encodings
  (`src/flasher/mod.rs`, and the ESP32 frequency table in `src/target`);
- the app's lowest accepted chip revision (header bytes 15-16,
  `min_chip_rev_full`) is 300, that is v3.0. espflash takes it from the
  ELF, and esp-hal 1.2.2 sets 300 for the ESP32 (`esp_config.yml`,
  `min-chip-revision`) and stops at start-up on an older chip
  (`src/lib.rs`). So **an ESP32 older than v3.0 cannot run the
  console**; the tested board has v3.1 (`docs/hardware.md`). A change
  of this number changes which boards can run it, so the check stops
  the script until `WANT_MIN_REV` is changed on purpose;
- the table at 0x8000 decodes (magic 0xAA50, type, subtype, offset,
  size, name, flags, then the 0xEBEB MD5 row, whose MD5 is checked) to
  exactly the rows of the CSV, and is byte for byte the CSV encoded by
  this script;
- 0x0..0x1000, the gap after the bootloader up to 0x8000, and the gap
  after the table up to 0x10000 are all 0xFF;
- the app fits its partition (0x3E0000 bytes from the CSV);
- the merged image ends exactly where the app ends, so it is not padded
  and does not reach the settings partition at 0x3FE000 or any other
  partition after the app;
- the app cut from the merged image is byte for byte what
  `save-image` makes without `--merge`, and the three parts laid on
  0xFF give back the merged image.

## Padding

`espflash save-image --merge` pads the merged file with 0xFF to the full
flash size (4,194,304 bytes), unless `--skip-padding` is given. A
padded file written at 0x0 would also write 0xFF over the console's
settings at 0x3FE000 (brightness and boot animation). The script passes
`--skip-padding`, so the file ends after the app (2,066,208 bytes for the
trial build below) and the settings partition is not written.

## What a write erases

- **The merged image** writes 0xFF over 0x9000..0x10000: the `nvs`
  partition, whose first 8 KB hold a saved gamepad pairing, and
  `phy_init`. So it **forgets a saved gamepad pairing**. A new board has
  none, so nothing is lost there. The console's settings are kept.
- **The three parts** do not write 0x9000..0x10000, so a saved pairing
  and the settings stay, as long as the flashing tool writes only the
  parts and does not erase the whole chip first.
- **1-click web installer** (`index.html`): writes only the three parts
  when **KEEP DATA** is selected (or wipes the whole chip with
  **ERASE ALL**); see the next section.

The project's own development board is never written this way: its saved
gamepad pairing must not be put at risk (see "Rules for the real board"
in `CLAUDE.md`).

## Web installer & manifest

`manifest.json` is written with `--web-manifest` or `--web-installer`,
and `--web-installer` also copies `tools/release/web/index.html` and
the clips listed in `tools/release/web/media.txt` (`docs/media/*`) into
the output folder.

`index.html` renders the PocketMe-styled showcase and a custom
`esptool-js@0.5.7` Web Serial modal (with automatic `en` / `pt-BR`
detection from `navigator.languages`, an `EN | PT-BR` header switch, an
`ESP32-2432S028 (2 USB)` preset, and `CUSTOM` baud/reset/debug options)
that reads `./manifest.json` directly on the same origin.

What `manifest.json` holds, and why (matching the ESP Web Tools
specification and `src/install-dialog.ts`):

- **The three separate parts (`0x1000`, `0x8000`, `0x10000`).** Writing
  the three parts leaves `0x9000..0x10000` (saved Bluetooth gamepad
  pairing) and `0x3FE000..0x400000` (console settings) untouched
  whenever **KEEP DATA** is selected (or **"Erase device"** is unchecked
  in an external ESP Web Tools host), while still working cleanly on a
  fresh board when **ERASE ALL** is chosen.
- **`new_install_prompt_erase` is true.** Kept in `manifest.json` so
  external ESP Web Tools hosts ask before erasing rather than wiping NVS
  and settings silently (the firmware has no Improv Serial).
- **`new_install_improv_wait_time` is 0**, so external Improv-aware
  hosts do not wait 10 seconds for an Improv answer that never comes.

To test the 1-click installer locally (Web Serial requires `localhost`
or `https://`, not `file://`):

```bash
python3 -m http.server 8765 --bind 127.0.0.1 --directory <folder>
```

Then open `http://localhost:8765` in Chrome or Edge. When a GitHub
Release is published with `--web-installer` / `--web-manifest` assets,
`../../.github/workflows/pages.yml` builds `_site/` from the release
assets plus `tools/release/web/index.html` and the clips listed in
`tools/release/web/media.txt` for GitHub Pages.

## Which firmware may be handed out

Build from a committed tree (`git status` shows no changes), so that
the release can be traced to its source, and give the commit in the
release text next to `BUILD.txt`. Build the owner's full set (including
`pair`, so the installer supports on-console Bluetooth gamepad pairing)
and nothing more:

```bash
cd boards/cyd
GAMES_TARGET=$PWD/target ../../games/build-chip.sh
cargo build --release --offline --no-default-features \
  --features demo-card,touch,link,store,pad,core2,boot-gather,game-flash,pair
```

- **Must have** `demo-card` (the games are built into the firmware, so
  no card is needed), `touch` (without it the console has only the
  BOOT button) and `game-flash` (MERCATOR only fits with its flash
  part). The rest of the set (`link`, `store`, `pad`, `core2`,
  `boot-gather`, `pair`) matches the owner's full board firmware.
  `tour` is a demo and stays out, as in the owner's builds.
- **Must not have** anything that is measuring-only (from `CLAUDE.md`):
  the features `wide-strips`, `send-ab`, `hang-test` and `xip-probe`;
  the build variables `CYD_DEMO_EXTRA` and `CYD_LAYOUT_PAD`; game files
  built with `GAMES_SLOTS_X`; CAMBER built with its `rec` feature;
  MERCATOR, or its flash part, built with the cfgs `merc_sizes_b` or
  `merc_site_test`. Build the games and the firmware with `RUSTFLAGS`
  and those variables unset.
- `make_release.py` cannot see features. Before handing out, run the
  firmware's own checks on the ELF (from `boards/cyd`):
  `python3 hot-check.py <elf> --cores two`,
  `python3 xip-check.py <elf> --absent` and
  `python3 game-flash-check.py <elf> <the game files it was built with>`.
  `game-flash-check.py` needs the game files the ELF was really built
  with: without that argument it reads `boards/cyd/target/chip-games`,
  which may be another games build, and fails.

A person who installs this build gets touch, the BOOT button, and
on-console classic Bluetooth gamepad pairing (`GAMEPADS` tile on the
Home screen). Once a gamepad is paired, all fourteen programs including
ROOFTOP, DRIVE3D and PROTO3D can be played.

The firmware was made for and tested on one board (ESP32-2432S028 with
an ST7789 panel and an ESP32 of revision v3.1). Other CYD variants (an
ILI9341 panel is a commonly named one; a general fact, not checked
here) are untested, and an ESP32 older than v3.0 cannot run it (see
"What it checks").

## Open before a public release

- **Release link & GitHub Pages.** Create a GitHub Release with the files
  built from a clean committed tree (including `THIRD_PARTY_NOTICES.txt`,
  `manifest.json`, and the three firmware parts), enable GitHub Pages
  for `.github/workflows/pages.yml` once the repository is public, and
  add the release link in `docs/install.md`. (The 1-click browser
  installer itself was hardware-verified on a real CYD board on
  `2026-11-24`.)

## Trial run

On 2026-11-24 the script ran on a two-core build with the features
`demo-card,touch,link,store,pad,core2,boot-gather,game-flash,pair`
(espflash 4.7.0). All checks passed, and `sha256sum -c` verified every
flat file. The app header gives v3.0 as the lowest chip revision (300,
with 399 as the highest):

| File | Address | Bytes |
|---|---|---|
| merged image | 0x0 | 2,066,208 (ends at 0x1F8720) |
| bootloader | 0x1000 | 25,856 |
| partition table | 0x8000 | 3,072 |
| app | 0x10000 | 2,000,672 (49.2 % of its 4,063,232-byte partition) |

The same ELF passed `hot-check.py --cores two`, `xip-check.py --absent`
and `game-flash-check.py` with its game files (`loaded bytes 2000564: ok`).
Flashing the three parts (`0x1000`, `0x8000`, `0x10000`) through the
1-click browser modal succeeded on the project's `ESP32-2432S028` (2 USB)
board when holding **BOOT** while connecting. The standalone merged `.bin`
and manual CLI steps were not run on a board.
