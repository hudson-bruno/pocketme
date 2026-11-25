# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

PocketMe (working name) is a hand-held game console written in bare-metal Rust. The prototype board is a "CYD" (Cheap Yellow Display): a classic ESP32 (4 MB flash, 520 KB RAM, no PSRAM), an ST7789 320×240 screen, an XPT2046 touch screen, a microSD slot, and only BOOT and RESET as buttons. Games are native Rust programs loaded into fixed memory slots; they never touch hardware and only use the console's game interface. The same console code also runs on the PC as a simulator.

The root `Cargo.toml` is a **virtual workspace for the 27 PC-checked crates only** (`boards/desktop`, the 12 shared `crates/` listed in its `members`, the 14 `games/`), with one shared `Cargo.lock` at the root (since 2026-11-24). Everything else is excluded on purpose and stands alone with its own `Cargo.lock`: `boards/cyd` (chip toolchain), `crates/debug-link`, `test-card`, `pad-view` (unpinned toolchain), `tools/` and `experiments/` (a workspace there would pull in the old crates). A member can still be built, formatted and tested from its own folder (or from the root with `-p <pkg>`); the standalone ones only from their own folder (three exceptions: `boards/cyd/hci-rules`, `boards/cyd/send-plan` and `boards/cyd/handover`, see below). The root `README.md` and the pages in `docs/` are the introduction for outside readers (written 2026-11-23); they repeat facts that live in the crate READMEs and in the code, so a change to a number, a feature set or a command there must be carried into them too.

The detail is in the READMEs: `boards/cyd/README.md`, `boards/desktop/README.md`, `games/README.md`, `tools/boardctl/README.md` and most `crates/*/README.md`. `pad-proto`, `pad-host`, `boards/cyd/hci-rules`, `boards/cyd/send-plan` and `boards/cyd/handover` have none; read their `//!` crate docs and `boards/cyd/README.md`. Some README numbers lag behind the code (test counts, build counts, sizes): when a README and a script or source file disagree, the script or source is right.

## Layout and layering

Each entry depends only on entries above it; nothing depends on a board or a tool.

- `crates/game-api`: the byte-level contract between console and games (no dependencies, `repr(C)` only, `API_VERSION`). `game_api::layout` in `src/file.rs` is the **only** place where the game slot addresses and sizes are written (code slot 56 KB in instruction RAM, data slot 48 KB). Scripts and linker files are generated from it; never write these numbers anywhere else. Moving a slot means rebuilding every game. Stage 1 (2026-11-21): a game's code may run on past the code slot into the **spill** (`CODE_SPILL_ADDR` 0x400A0000, at most `CODE_SPILL_MAX` 48 KB), which is the data slot's own RAM seen from the instruction bus, from its top down in reverse word order (`sram1_iram_to_dram`, from ESP-IDF). The spill takes the data slot from the top, data and zeroed variables fill it from the bottom: spill + data + zeroed <= 48 KB (`fits`; the loader's notice is GAME TOO BIG FOR SLOTS). No header or version change; games that fit the code slot are byte-identical. The mirror order was checked on the CYD on 2026-11-22 with `experiments/spill-probe`; the firmware still checks it at every start-up and turns the spill off if it is wrong (`boards/cyd/README.md`, "Code past the code slot"). MERCATOR, PROTO3D and DRIVE3D use the spill and run on the board that way (sizes in `games/README.md`). Game flash parts (2026-11-22; on the board since 2026-11-23, in the owner's builds): `layout` also holds the two flash windows (`FLASH_CODE_ADDR` 0x400D0020 + 128 KB, `FLASH_RO_ADDR` 0x3F400120 + 64 KB) and each flash-part game's place in them (`FLASH_PLACES`, MERCATOR only); a game with `games/<name>/flash.list` is built as a "GAMF" file (header + 32-byte extension) whose listed parts live in the firmware (`games/README.md`, "Flash parts"). A new build of such a game needs a new firmware.
- `crates/board-api`: small traits a board must provide (`Screen`, `Touch`, `Keys`, `Pad`, `Clock`, `Files`, `Link`, `LogOut`, `Store`; optional `Hci`, `Beacon`, `Gps`, `Audio` behind the features `bt-classic`, `passby`, `gps`, `sound`). Traits and small types only, no logic. A new method on an existing trait needs a default body. The `testing` feature (`FakeBoard`) is for dev-dependencies only.
- `crates/console-video`: the 160×120 one-byte-per-pixel picture, palette, and the diff that turns a changed picture into at most 16 screen areas.
- `crates/console-core`: the whole console (boot screen, home, menus, notices, game running). `Console` is one plain struct; the board calls `Console::step(&mut board)` (generic over `B: Board`) and gets back when to call again (`board_api::Wake`). No std, no heap, no unsafe, no floating point; it never waits and takes the time only from the board's `Clock`, once per step. `Console::new()` must stay all zero bytes (the ~45 KB value lives in `.bss`; `tests/zeros.rs` checks it).
- `crates/game-kit`: what a game is written against (`Game` trait, `game!` macro which makes `game_entry` and, on the chip, the panic handler). `crates/game-3d`: the first game library crate (flat-shaded 3D polygons in whole numbers; compile-time choices through its `Setup` trait); it depends only on `game-kit`, is compiled into each game that uses it (KESTREL, CAMBER, PROTO3D and DRIVE3D today), never into the console, and follows the game rules. `crates/game-loader`: checks a `GAME.BIN` (header, CRC-32) and copies it into the slots through the `GameSlots` trait.
- Smaller parts: `crates/settings-store` (power-cut-safe records in two 4 KB flash sectors), `crates/touch-filter`, `crates/debug-link` (framed PC↔console protocol on the serial port), `crates/test-card`.
- Gamepad: `crates/pad-proto` (classic Bluetooth state machine), `crates/pad-view` (report decoding per pad mode), `crates/pad-store` (saved pairings in flash), and `crates/pad-host`, which ties them together for the console and is reconnect-only.
- `games/`: the games (`camber`, `hairpin`, `kestrel`, `lander`, `lantern`, `mercator`, `padtest`, `rooftop`, `skimmer`, `stardust`, `tundra`), two prototypes built and registered like games but not finished games, with working names (`drive3d`: DRIVE3D, a rally, gamepad only; `proto3d`: PROTO3D, a 3D platformer with four test areas), `bench` (BENCH, a speed test built and registered like a game: a measuring tool, not a game) and the chip build (`build-chip.sh`, `chip/`). A game is a `no_std` library that depends only on `game-kit` and the game library crates (`crates/game-3d`): no heap, no floating point, no unsafe, no own panic handler. Starting settings (the owner's decision, 2026-11-23): a game with a detail setting starts at its level called HIGH (HEAVY in HAIRPIN and LANTERN; HIGH, not MAX, in DRIVE3D and PROTO3D), a game with a 30 or 60 setting starts at 60, and MERCATOR starts in its CHASE view; every option stays in the menus (`games/README.md`, "Starting settings").
- `boards/cyd`: the firmware (`pocketme-cyd`). `boards/desktop`: the simulator (`desktop-board` crate, binary `pocketme-sim`). Both implement `board-api` and hand the same `console-core` its parts.
- `tools/boardctl`: PC tool that talks to the board over the debug link.
- `tools/media`: Python scripts (standard library only) that make the pictures and clips in `docs/media/` and `docs/gallery.md` (clips only, several per topic) from simulator runs (`tools/media/scenes`). Never edit those outputs by hand: change a scene and run `python3 -I tools/media/make.py --sim <built pocketme-sim>`. The pictures come from the simulator, so they never stand for speed or for a photograph of the board, and a clip played by a script never stands for how hard a game is.
- `tools/release`: `make_release.py` (Python, standard library only) turns a built firmware ELF into ready-to-flash files (one merged image for address 0x0, the same as three parts, `BUILD.txt`, SHA-256 sums) with `espflash save-image`, and checks them byte by byte; it never opens a serial port. With `--web-installer` it also writes `manifest.json` and the 1-click browser installer (`tools/release/web/index.html`, EN / PT-BR, plus the clips in `web/media.txt`); `.github/workflows/pages.yml` publishes it on GitHub Pages from each published Release. `docs/install.md` is the guide for people who write those files to their own board from a browser. Tested on the project's board on 2026-11-24: the 1-click installer with the three parts (0x1000, 0x8000, 0x10000), which leaves the pairing area alone while KEEP DATA stays selected; the merged image at 0x0, the manual esptool-js page and the command-line steps are untried (`docs/install.md` lists what was and was not tried). The merged image written at 0x0, and the installer's ERASE ALL, forget a saved gamepad pairing, so never use either on the shared board.
- `experiments/`: finished trials, kept for their READMEs and measurements. Two are still needed. `experiments/bt-classic-probe/vendor/esp-radio` is a patched copy of esp-radio 1.0.0-beta.1 with classic Bluetooth; `boards/cyd` uses it by path through `[patch.crates-io]`, and every change is listed in `vendor/PATCH-NOTES.md` beside it. `experiments/bt-gamepad-test` is no dependency of anything, but it is the only firmware that pairs a pad and writes the pairing.

### One game source, two worlds

A game crate is linked straight into the simulator and, for the chip, built by `games/build-chip.sh` as a static library, linked for the two slots with `games/chip/game.x` plus a generated `slots.x`, and packed into `<NAME>.BIN`. Chip-only build settings (panic=abort, opt-level s, LTO, overflow checks on, trimmed paths) are passed on that script's command line, not in the game's `Cargo.toml`. Overflow checks are on for every game unless its folder name is in `OVERFLOW_CHECKS_OFF` at the top of `build-chip.sh`; today only `camber` is (owner's decision, 2026-11-19). There a sum that runs over wraps round on the chip, so a range mistake shows as a wrong picture instead of a stop; the PC tests (debug builds) keep the checks on. A name in that list that is not a game folder in `games/` stops the script with an error. A new game gets the checks without doing anything; turn them off only on the owner's word, after the game's scripted plays ran with them on.

The game's folder name in capitals (`-` becomes `_`, at most 8 characters) is its folder on the card: `CONSOLE/GAMES/<NAME>/{GAME.BIN,INFO.TXT}`. `games/<name>/INFO.TXT` must be byte-identical to `boards/desktop/card/CONSOLE/GAMES/<NAME>/INFO.TXT`. Every `GAME.BIN` on the simulator's sample card, the real games' too, is deliberately a line of text and not a game file (made by `boards/desktop/tools/make_sample_card.py`).

A new game touches seven places besides its own crate: `Cargo.toml` (`workspace.members`), `boards/desktop/Cargo.toml`, `boards/desktop/src/games.rs`, `LINKED` in `make_sample_card.py` (then run it), `CRATES` (and `BATCH_GAMES`) in `boards/desktop/check.sh`, `TILES` in `boards/desktop/tests/games.rs`, and the list in `the_card_folder_lists_the_sample_games` in `boards/desktop/tests/board.rs`.

### The firmware (`boards/cyd`)

- One embassy task (`src/app.rs`) on the first core runs the console in a loop: poll the gamepad host, read inputs and step the console, keep settings, drain the log, wait. With `pad`, the radio driver's own task only queues the controller's packets. With `core2` (the default) the second core sends each picture while the first goes on (`src/core2.rs`; every wait for it ends after 500 ms with a panic that says so); without it, sending a picture does not yield (up to about 35 ms). No other task or interrupt may do work that cuts into the timed screen windows; radio timers may only bring the loop round, never force a console step.
- The screen path (`screen.rs`, `beam.rs`, `lcd.rs`) sends changed areas in step with the panel's own refresh (read with the Get Scanline command) so nothing tears; each present sends the changed areas as their own windows or as one box around them, whichever `boards/cyd/send-plan` says ends sooner. esp-hal's low-level SPI driver, the scanline read (`Lcd::scanline_stamped_raw`) and `fill_fast` run from RAM. The rest of the timed paths (the screen's `present` with its loop over the windows, `send` with the writer's loop, the window set-up, esp-hal's `SpiDma` read/write/wait, the DMA set-up and SPI clock switches, the send plan, and with `core2` the second core's executor and `worker` task) runs from flash as one **hot block** right after all other flash code, so that no three of its lines share a flash cache set whatever the linker does elsewhere. `boards/cyd/hot-block.list` names those functions (mangled-name globs); `build.rs` turns it into the linker script `hot-block.x`; `boards/cyd/hot-check.py <elf> --cores two|one` checks a built firmware (every listed function found and in its part, every instance of the screen's `present`/`send` in the window part, no flash calls out of the block from the timed paths except panics, literals inside it, sizes in cache lines) and `check.sh` runs it on the owner's builds with the gamepad (two cores and one, each with and without `boot-gather`, the same two with `boot-gather` and `game-flash`, and the two-core one with `xip-probe`); `hot-block.x` itself stops the link when a `must` line places nothing, a part is under its floor (`HOT_FLOORS` in build.rs) or too big. When a hot function is renamed, inlined or added (an esp-hal update, a refactor), fix the list until hot-check passes. After any change to the memory layout check the boot log's `panel:` line (`fastest_read_us=17`, `calibrated=true`).
- `build.rs` writes the slot symbols from `game_api::layout`; `game-slots.x` stops the link if the console's RAM code or heap grows into a slot, and the firmware refuses every game at start-up if linked and expected slots differ. The code slot can only be accessed as whole aligned 32-bit words, and so can the spill (`.game_spill`, NOLOAD, right after `.game_code`; `src/games.rs` checks its mirror order at start-up, reads it back after every load and counts what a wipe left in it; `check.sh` fails a build whose ELF lacks that start-up check's log lines, since the compiler once dropped it). A game panic stops the chip until RST (no watchdog yet).
- Big values never go on the stack: the `Console` is a static, the `PadHost` (~19.7 KB) is copied onto the heap once and never moved.
- Features: default is `demo-card, touch, link, tour, store, core2`; the owner's builds leave out `tour` and add `boot-gather` and, since 2026-11-23, `game-flash` (`demo-card,touch,link,store,core2,boot-gather,game-flash[,pad]`). One core (the same sets without `core2`) still builds and is checked by the scripts, and its behaviour must not change when the two-core path does; but since 2026-11-19 the owner wants no more work on it (no board rounds, no report columns, no tuning): if it breaks, ask before spending time on it, and do not delete its code without asking.
  - `core2`: the second core sends the pictures. The first core's side of the hand-over (job box, bounded waits, parking for the flash) is `boards/cyd/handover`, tested on the PC against a model of the second core in every order; keep `src/core2.rs`'s `worker` and `parked` in step with that model.
  - `demo-card` embeds the chip games, so they must be built first. `sd-card` reads the real card (read only; it wins over `demo-card` when both are on).
  - `link` is the debug link (without it `boardctl` gets no answers), `store` the saved settings.
  - `tour` is a demo; it also makes BOOT count on release and toggle the tour on a 1 s hold.
  - `xip-probe` (measuring only, turns on `core2`) puts 128 KB of probe code into the flash code before the hot block (`src/xip_probe.rs`) and adds the letters `a` (one flash cache miss's cost), `0`..`5` (a load of probe code each console step), `j` (start offset) and `d` (delay after a handed picture) to measure flash code run by the first core against the second core's timed windows. `xip-check.py <elf>` checks the block on the PC (`--absent`: a build without it has no trace of it). Without the feature the owner's build is byte-identical. Build it with `--target-dir target/xip-probe`. Its phase A-i was measured on the board on 2026-11-22; the results are kept outside the repository (`boards/cyd/README.md`, "Flash code probe", says what ran).
  - `boot-gather` turns on console-core's `boot-gather`: the GATHER boot animation (`src/gather/`, `src/boot_gather.rs`) and a BOOT ANIM setting (FULL, SHORT, OFF) kept beside the brightness. The owner saw it on the board and approved it on 2026-11-19, so it is in the owner's builds; it is not in the default set. Without it console-core's boot is today's, unchanged; the simulator has it on by default (`--boot full|short|off`, `--no-default-features` without it).
  - `game-flash` (not in `default`; in the owner's builds since 2026-11-23 by the owner's decision, because MERCATOR with its planet only fits with a flash part) reserves the two flash windows at the start of the firmware's flash code and data and embeds the games' flash parts from `<game files>/flash/<NAME>/` (build.rs `game_flash`, `game-flash.x`, checked by `game-flash-check.py`); a GAMF game starts only when the firmware holds its part byte for byte (CRC-32s read from flash at each start), otherwise FLASH PART MISSING or DIFFERS. It moves `.text`, `.rodata` and the hot block (run hot-check). The console never writes, erases or remaps that flash. Every build with it prints one harmless linker warning (four lines about `irom_seg`/`drom_seg`, `boards/cyd/README.md`, "Game flash parts").
  - `pad` is the gamepad radio. With it the firmware is reconnect-only: it never searches, is never discoverable, never starts a connection, never pairs. Every packet to the Bluetooth controller passes the allow-list in `boards/cyd/hci-rules` (`Gate`); pad-proto's `Config::reconnect_only` blocks the same things one layer up. Do not weaken either layer.

### The simulator (`boards/desktop`)

It shows 14 tiles, all linked in and on its sample card: the eleven games, the two prototypes and BENCH, in the home order BENCH, CAMBER, DRIVE3D, HAIRPIN, KESTREL, LANDER, LANTERN, MERCATOR, PADTEST, PROTO3D, ROOFTOP, SKIMMER, STARDUST, TUNDRA. Deterministic: time is simulated in fixed ticks, runs without a window give the same pictures every time, and a run fails loudly on things the real board could not do (more than 16 areas in one send, two sends in one step, and so on). The no-window build (`--no-default-features`) must not depend on SDL3. Window-only effects (backlight dimming, the F1 overlay) never go into PNGs or fingerprints. `keymap.rs` is the single key table and a test checks the README lists every key. The simulator cannot judge memory, speed, tearing, radio or real touch: the board decides those.

## Commands

Toolchains: the crates that `boards/desktop/check.sh` checks, the games, `boards/cyd/hci-rules`, `boards/cyd/send-plan` and `boards/cyd/handover` pin Rust 1.99.0 in their own `rust-toolchain.toml`. The unpinned ones (`crates/debug-link`, `test-card`, `pad-proto`, `pad-view`, `pad-store`, `tools/boardctl`) use the rustup default, which on the owner's PC is `nightly`. `boards/cyd` uses the `esp` toolchain (target `xtensa-esp32-none-elf`, build-std). Chip builds need this first:

```bash
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh
```

Scripted cargo calls are `--offline` (`boards/desktop/check.sh` and `games/build-chip.sh` also `--locked`); use `--offline` by hand too. Do not install or download tools (pacman, sudo, rustup, cargo install) without the owner's go-ahead.

### PC side

The 27 PC-checked crates (`boards/desktop`, the 12 shared `crates/`, and the 14 `games/`) share an explicit root virtual Cargo workspace (`Cargo.toml`, with a single `Cargo.lock`). `boards/cyd`, `experiments/`, `tools/`, and `crates/debug-link`, `test-card`, `pad-view` are explicitly excluded so they stay standalone. The root `Cargo.toml` sets `[profile.dev] opt-level = 2` (keeping `overflow-checks = true` and `debug-assertions = true`) and `[profile.release-checked]` for fast optimized test/fuzz runs without affecting chip builds.

```bash
boards/desktop/check.sh                             # everything on the PC side; ends with "check: ok"
boards/desktop/check.sh games/mercator              # scope to one or more crates (skips chip/no-std if unused)
boards/desktop/check.sh --no-chip --no-nostd        # skip chip game builds or no_std cross-builds
boards/desktop/check.sh --cargo-test                # force plain `cargo test` instead of `cargo nextest`
```

It overlaps the chip game build in the background (when `boards/desktop` or `crates/game-loader` is selected), checks formatting on selected crates, runs `clippy` (`-D warnings`) and tests via `cargo nextest` (automatically falling back to `cargo test`) plus `cargo test --doc`, compiles selected `SHARED` crates `no_std` for the ESP32 and a bare PC in parallel, and runs the simulator once. It skips redundant `--all-features` passes on crates without `[features]`, batches the 13 games that share a feature graph together for multi-core parallelism, and stores builds in `boards/desktop/target/check`. It does **not** cover `crates/debug-link`, `test-card` or `pad-view`; run `cargo test --offline` in their folders when you change them.

One crate, by hand, from the crate's folder (or from the workspace root with `-p <pkg>`):

```bash
cargo fmt --check -p <pkg>
cargo clippy --offline --locked --all-targets --all-features -- -D warnings
cargo nextest run --offline --locked --all-features --no-tests=pass   # plus `cargo test --doc` (or `cargo test`)
```

Never `cargo fmt --all` (it reformats path dependencies in other folders).

A single test: integration test file name after `--test`, then the test name. Examples:

```bash
cd crates/console-core && cargo test --offline --test game a_game_starts_and_its_picture_is_shown
cd crates/board-api && cargo test --offline --features testing --test fake_board noted_presses_are_handed_out_once
cd crates/pad-host && cargo test --offline --test scripted the_link_key_never_reaches_any_output
cd boards/desktop && cargo test --offline --test games padtest_shows_input_and_motion_and_sleeps_when_still
```

Things that are easy to miss:

- Tests that need the built game files. `check.sh` sets `GAME_FILES_DIR=boards/desktop/target/check/chip-games/files`; by hand, run `games/build-chip.sh` first and export that absolute path. Without it the simulator's chip-file tests print a note and pass without checking anything, and `crates/game-loader`'s `real_files` test looks in `crates/game-loader/target/chip/chip-games/files` and fails if that is empty.
- After a deliberate change to the console's look: look at the new pictures in the simulator first, then `UPDATE_GOLDEN=1 cargo test --test golden` in `crates/console-core`. After changing console-video's diff rule: `node tests/data/sim_areas.mjs > tests/data/sim_areas.txt` in `crates/console-video`.
- Longer runs: `PAD_FUZZ=200 cargo test --profile release-checked --test fuzz` (pad-host), `CUT_SAVES=3000 cargo test --profile release-checked --test cuts` (settings-store).

Simulator, from `boards/desktop` (`cargo run -- --help` lists every option and key):

```bash
cargo run -- --frames 60 --png ../../.local/shot.png                        # no window, one picture
cargo run --no-default-features -- --frames 60 --png ../../.local/shot.png  # same, without SDL3
cargo run -- --window --close-after 3 --out ../../.local/shots              # a window that closes by itself
```

Pictures written inside `boards/desktop` are not git-ignored, so send them to `.local/`. `--script`, `--shot`, `--png`, `--prints` and `--trace` cannot be combined with `--window`. A real window opened from a task must close by itself within a few seconds (`--close-after`).

Games for the chip, from anywhere:

```bash
games/build-chip.sh            # all games; or name game folders
```

Output goes to `$GAMES_TARGET/chip-games/files/` (`GAMES_TARGET` defaults to `boards/desktop/target/check`), with a ready card tree under `files/card/`. `GAMES_SLOTS_X` is for tests only; never put a game built with it on a card.

### Firmware (`boards/cyd`)

```bash
cd boards/cyd
GAMES_TARGET=$PWD/target ../../games/build-chip.sh      # needed before any demo-card build
cargo build --release --offline                          # default features (two cores)
cargo build --release --offline --no-default-features --features demo-card,touch,link,store,core2,pad,boot-gather,game-flash   # the owner's build with the gamepad
cargo build --release --offline --no-default-features --features demo-card,touch,link,store,pad,boot-gather,game-flash         # the same on one core
python3 hot-check.py target/xtensa-esp32-none-elf/release/pocketme-cyd --cores two   # the hot block of a built firmware (--cores one without core2)
```

```bash
boards/cyd/check.sh            # long (about 22 minutes)
```

`check.sh` runs the games build, hci-rules, send-plan, handover, format check, clippy on eleven feature sets (both the two-core default and one-core sets), and then builds every combination of the seven features that are not `pad`, `core2`, `send-ab`, `hang-test`, `xip-probe`, `boot-gather` or `game-flash`, plus fixed sets with `pad`, `send-ab`, `core2`, `hang-test`, `xip-probe` and `boot-gather`, and last the owner's builds with the gamepad again (two cores and one, each with and without `boot-gather`), each checked by `hot-check.py` and `xip-check.py --absent`, plus the owner's builds with the gamepad and `game-flash` (two cores and one), also checked by `game-flash-check.py` (the others by `game-flash-check.py --absent`), plus the two-core one with `boot-gather` and `xip-probe`, checked by `hot-check.py` and `xip-check.py` (154 builds, ending `all 154 builds ok`; it prints `FAILED (build)`, `FAILED (game slots)`, `FAILED (hot-check)`, `FAILED (xip-check)` or `FAILED (game flash)` and the guards' summary lines, and refuses to run while `CYD_LAYOUT_PAD` or `CYD_DEMO_EXTRA` is set). It builds into `target/check`, so the flashable file in `target/` is untouched, and it never opens the serial port. It does rebuild `boards/cyd/target/chip-games/files`, the games a later `demo-card` build embeds (`CYD_GAME_FILES` points `build.rs` at another folder).

`boards/cyd/hci-rules`, `boards/cyd/send-plan` (the screen's send timing rules, and in its module `beam` the decisions of `src/beam.rs`: survey, calibration checks, anchor; its PC-only feature `model` holds the replay model) and `boards/cyd/handover` (the two cores' hand-over) are PC-tested crates inside the firmware folder. Run them **from the repository root** with their own toolchain, otherwise cargo applies the chip settings of `boards/cyd/.cargo/config.toml`:

```bash
cargo +1.99.0 test --offline --manifest-path boards/cyd/hci-rules/Cargo.toml
cargo +1.99.0 test --offline --manifest-path boards/cyd/hci-rules/Cargo.toml --test rules the_event_mask_leaves_out_search_results_only
cargo +1.99.0 test --offline --manifest-path boards/cyd/send-plan/Cargo.toml --all-features
cargo +1.99.0 test --offline --manifest-path boards/cyd/handover/Cargo.toml
```

hci-rules' tests include `crates/pad-host/tests/common` by path, so changes there can break them.

Flash (from `boards/cyd`, under the board lock described below):

```bash
espflash flash --chip esp32 --port /dev/ttyUSB0 --non-interactive --partition-table partitions.csv --flash-freq 80mhz target/xtensa-esp32-none-elf/release/pocketme-cyd
```

### Board tool

```bash
cd tools/boardctl && cargo build --release --offline && cargo test --release --offline
```

`boardctl --help` lists the commands (default port `/dev/ttyUSB0`, `--port` to change). It needs a firmware built with `link`. `boardctl pad …` plays an injected gamepad through the real decoder (needs `pad` too). `testcard` and `compare` need no board; `boardctl compare a.png b.png` compares a simulator picture with a board screenshot.

## Rules for the real board

There is one board and it is shared between everyone working here.

- **Board lock.** Wrap every command that opens the serial port (espflash, boardctl) like this: `flock <checkout>/.local/board.lock timeout 150 <command>`. `boardctl` does not take the lock itself. Every session must use the same lock file; `.local/` is per checkout, so the `.local/board.lock` of another checkout or worktree is a different lock.
- **Restarts.** Opening the port with espflash restarts the board; of the boardctl commands only `reset` and `log --reset` do. When a person has to restart it, ask for the RST button, not for unplugging.
- **Always flash with `--partition-table partitions.csv`.** So never use `cargo run` in `boards/cyd`: its runner flashes without the table.
- **Flash with `--flash-freq 80mhz`** (the owner's choice, on the board since 2026-11-22). The option only sets the flash clock in the image's header (the mode stays DIO); without it espflash writes 40 MHz, which works but makes every read from flash slower. The boot log must say `SPI Speed      : 80MHz`. Compare timings only between firmware flashed at the same speed.
- **Never run `espflash erase-flash`**, `erase-region`, `erase-parts` or `write-bin`. The first 8 KB of the `nvs` partition (0x9000..0xB000) hold the owner's saved gamepad pairing, and re-pairing needs the owner and another firmware.
- **The pairing area is secret and read-only.** Never read it to the PC (`read-flash`), and never log or store a link key. Builds with `store` or `pad` log the area's CRC-32 at each start (`pairing area crc32=…`); use that line to show it is unchanged. The only flash the console writes is the `settings` partition (0x3FE000..0x400000), through `src/flash_store.rs`, which checks every write and erase against that range; use `write_nor`, never `FlashStorage::write`.
- **Serial letters go to whatever firmware is flashed.** `boardctl text`, `--then` and `--send` send single letters. Never send `f` while `experiments/bt-gamepad-test` is on the board: there it forgets every saved pairing. In the console firmware, do not send `w` (a store wear test) except when measuring the store.
- **Addresses.** Logs print at most the last two bytes of a Bluetooth address, and tests fail if a key or full address reaches any output. Keep it that way; do not copy device addresses, other people's device names, or GPS positions into reports, tests or commits. Test fixtures use made-up addresses and keys.
- Flashing test firmware is allowed. Writing to or erasing the microSD card is not, unless the owner agreed to that specific write.
- Measuring-only switches must not be in a build handed over for use: features `wide-strips`, `send-ab`, `hang-test` and `xip-probe`, `CYD_DEMO_EXTRA` and `CYD_LAYOUT_PAD` (`build.rs`; the second moves the hot block to test the flash cache), game files built with `GAMES_SLOTS_X`, and CAMBER's game feature `rec` (it records races over the log; built with `RUSTFLAGS='--cfg feature="rec"' games/build-chip.sh games/camber`) with every game file built with it, and MERCATOR's cfgs `merc_sizes_b` (ship size variant B) and `merc_site_test` (the flight starts above the landing site; `RUSTFLAGS='--cfg merc_site_test' games/build-chip.sh games/mercator`) with every MERCATOR.BIN and flash part built with them and every `game-flash` firmware that embeds such a part.

## Other rules

- `.local/` (git-ignored) is the place for logs, reports, evidence, scratch builds and anything holding device addresses. `/tmp` is wiped when WSL restarts.
- Licences: the Bluetooth host is written from the Bluetooth specification; ESP-IDF (Apache-2.0) may be read. No code may be copied from GPL sources or from BTstack (non-commercial licence).
- Numbers kept in step by hand (no compile-time link): the saved-pad count (`hci_rules::SAVED`, pad-proto `KEYS`, pad-store `MAX_PADS`), the lowest allowed link supervision timeout (pad-proto and hci-rules), `settings_store::BRIGHTNESS_MAX` and console-core's `theme::BRIGHT_STEPS - 1`, the button bits in `debug_link::msg::button` and `game_api::button`, the player count (`game_api::PLAYERS` and debug-link's `PlayerButtons::PLAYERS`), console-core's `input::BUTTON_MAP` with `gamein::GAME_BUTTONS`, the boot animation values (`settings_store::BOOT_ANIM_FULL`, `_SHORT`, `_OFF` and console-core's `boot::Anim` with `boot-gather`), and the ESP32's SRAM1 ends used by the spill (`game_api::layout::SRAM1_IRAM_ADDR` 0x400A0000 and `DRAM2_END` 0x40000000, written again in the ASSERTs of `boards/cyd/game-slots.x`; these are chip facts, not slot choices), and the spill's reverse word order (`game_api::layout::sram1_iram_to_dram`, copied in `place` of `games/chip/check_load.py`, which checks its copy against the two data words slots.x names, and in `Mirror::Reversed` of `crates/game-loader/tests/common/mod.rs`, which `the_fake_mirror_is_the_layouts_rule` checks), and the flash windows' origins (`game_api::layout::FLASH_CODE_ADDR` 0x400D0020 and `FLASH_RO_ADDR` 0x3F400120: esp-hal 1.2.2's `irom_seg` origin and its `drom_seg` origin plus the 0x100-byte app descriptor; the firmware's `game-flash.x` ASSERTs stop the link when they no longer match). Change both sides together.
- Design choices that are the owner's (video modes, input set, slot sizes, one core or two, menu looks) are decided by the owner: bring measured options, do not settle them in code.
