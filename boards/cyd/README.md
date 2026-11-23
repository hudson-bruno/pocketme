# pocketme-cyd

The console on the CYD board: an ESP32 (rev 3.1, 4 MB flash, 520 KB RAM),
a 320x240 ST7789 panel, a resistive touch screen (XPT2046), a microSD slot
and one button (BOOT, GPIO0). It runs `console_core::Console` on the
board's parts, and starts games: a game file from the card is loaded
into the two game slots in RAM (crates/game-loader) and run (games.rs).

## Build and flash

```sh
cd boards/cyd
export PATH="$HOME/.cargo/bin:$PATH"
. ~/export-esp.sh
GAMES_TARGET=$PWD/target ../../games/build-chip.sh    # first: the real games (demo-card)
cargo build --release --offline                       # default features (two cores)
cargo build --release --offline --features sd-card    # the real card
# the owner's build: two cores, no tour, the boot animation, the game flash parts (add ,pad for the gamepad)
cargo build --release --offline --no-default-features --features demo-card,touch,link,store,core2,boot-gather,game-flash
# one core (still supported): leave core2 out
cargo build --release --offline --no-default-features --features demo-card,touch,link,store,boot-gather,game-flash
# the hot block of the firmware just built (--cores one without core2)
python3 hot-check.py target/xtensa-esp32-none-elf/release/pocketme-cyd --cores two
espflash flash --chip esp32 --port /dev/ttyUSB0 --non-interactive \
    --partition-table partitions.csv --flash-freq 80mhz \
    target/xtensa-esp32-none-elf/release/pocketme-cyd
```

Always flash with `--partition-table partitions.csv` (since round 5).
It adds the 8 KB "settings" partition at the top of the chip
(0x3FE000); nvs and phy_init keep their places, only the program's
partition ends earlier (0x3F0000 instead of 0x400000). `espflash flash`
writes only the bootloader (0x1000), this table (0x8000) and the
program (0x10000): nvs and the settings are never written by it.
Flashed without the table, the board keeps the old one and the console
simply has no store (the brightness lasts until power off).

Flash with `--flash-freq 80mhz` (the owner's choice; on the board since
2026-11-22). The option only sets the flash clock in the image's header:
the mode stays DIO and the firmware file is the same. Without it
espflash writes 40 MHz, which works but makes every read from flash
slower. The boot log must say `SPI Speed      : 80MHz`. All measured on
the one CYD we have, so no word about other boards:

- 2026-11-19, the same firmware file at both speeds: steps about 0.5 ms
  shorter at 80 MHz (CAMBER 17.8 ms against 18.3 ms), 4 starts fine.
- 2026-11-22, the stage 1 build: 10 starts in a row at 80 MHz, each
  with the pairing area's CRC unchanged, `fastest_read_us=17`,
  `calibrated=true`, `games=11`, the game slots and spill mirror ok and
  no WARN or ERROR line. The settings load takes 3,750 us (5,012 us at
  40 MHz). A real save at 80 MHz (brightness 4 to 3, restart, back to 4,
  restart) held both times: one save 2,588 us, one flash write 108 us.
  CAMBER and MERCATOR ran with `late=0 crossed=0`.

Timings in this file from before that date were taken at 40 MHz;
compare only firmware flashed at the same speed.

Never run `espflash erase-flash`: the nvs partition holds a saved
gamepad pairing. The firmware writes flash only inside the settings
partition (feature `store`, below), and never writes the card.

The owner's test build of round 5 (2026-11-17) has the features
`demo-card`, `touch`, `link` and `store`, and not `tour`: without the
tour, the BOOT button counts as soon as it goes down, and holding it
does nothing else. It was built into its own folder and flashed with
the table:

```sh
H=<a build folder>
GAMES_TARGET=$H/target ../../games/build-chip.sh
CYD_GAME_FILES=$H/target/chip-games/files cargo build --release --offline \
    --no-default-features --features demo-card,touch,link,store --target-dir $H/target
cp $H/target/xtensa-esp32-none-elf/release/pocketme-cyd $H/pocketme-cyd-owner-test.elf
espflash flash --chip esp32 --port /dev/ttyUSB0 --non-interactive \
    --partition-table partitions.csv $H/pocketme-cyd-owner-test.elf
```

(Round 4's owner build had no `store`.)

Since the two-cores round (2026-11-18) the owner's builds are
`demo-card,touch,link,store,core2` and, with the gamepad,
`demo-card,touch,link,store,core2,pad`; since 2026-11-19 (the owner saw
the GATHER boot animation on the board and approved it) they add
`boot-gather`, and since 2026-11-23 (MERCATOR with its planet only fits
with a flash part; the owner's decision) `game-flash`. The one-core builds (the same sets without `core2`) stay
supported and checked; see "One core or two".

The build order: `games/build-chip.sh` makes every game in `games/`
(GAME.BIN + INFO.TXT) in `target/chip-games/files/card/`; build.rs puts
them into the demo card and stops with these instructions when they are
missing. `CYD_GAME_FILES` names another folder. build.rs also checks
each file's header against `game_api::layout`, so a file for other
slots never reaches the demo card.

`./check.sh` builds the games first, then checks `hci-rules`,
`send-plan` and `handover` on the PC (format, clippy, tests), runs
`cargo fmt --check` (this crate only), clippy on eleven feature sets
(the default, now with `core2`; none; all features, with `pad`, `core2`
and `send-ab`; the one-core default `demo-card,touch,link,tour,store`;
the one-core owner's build with `pad` and `send-ab`; then `core2`
alone, the owner's build with `core2`, the owner's build with
`pad`, `core2` and `send-ab`, `core2,xip-probe`, the owner's build
with `pad`, `core2`, `boot-gather` and `game-flash`, and `game-flash`
alone) and a build of all
128 combinations of the 7 features without `core2` (one core) plus nine
sets with `pad`, `send-ab` or `boot-gather` and ten with `core2` (two of
them with `send-ab`, one with `hang-test`, one with `xip-probe`, one
with `boot-gather`; one of them is the default set), then the owner's
builds with the gamepad (`demo-card,touch,link,store,pad`, with and
without `core2`, each with and without `boot-gather`) once more, each
checked by `hot-check.py` (see "The timed paths and the flash cache")
and by `xip-check.py --absent` (no trace of the flash code probe), and
the two-core one with `boot-gather` and the measuring switch
`xip-probe`, checked by `hot-check.py` and `xip-check.py` (see "Flash
code probe"), and the owner's builds with the gamepad and `game-flash`
(two cores and one), checked the same way and by `game-flash-check.py`
(the other sets by `game-flash-check.py --absent`; see "Game flash
parts"): 154 builds (the script's count; MEASURED on 2026-11-23: 22 minutes
17 seconds, with the PC side's check running beside it). Clippy also runs on the owner's two-core set
with `game-flash` and on `game-flash` alone. `pad`, `core2`, `send-ab`, `xip-probe` and
`boot-gather` are built on a few sets only, not on every combination,
which would double the time. A failed build shows the linker's `HOT
BLOCK` or `GAME SLOT` message when that is the cause; the hot block step
says `FAILED (build)`, `FAILED (game slots)`, `FAILED (hot-check)`,
`FAILED (xip-check)` or `FAILED (game flash)` and
shows the guards' summary lines (sizes, cache lines left). It refuses to run while `CYD_LAYOUT_PAD` or
`CYD_DEMO_EXTRA` (measuring only) is set. It never touches the board.

`send-plan` (the screen's timing rules, plain logic; its module
`beam` holds the decisions of `src/beam.rs`: which survey reads are
trusted, the checks before a survey is used, among them odd steps and
uneven wraps, the anchor's time stamp)
and `handover` (the two cores' hand-over, plain logic, with a model of
the second core checked in every order) are PC-tested crates inside
this folder, like `hci-rules`. Run them from the repository root with
their own toolchain:

```sh
cargo +1.99.0 test --offline --manifest-path boards/cyd/send-plan/Cargo.toml --all-features
cargo +1.99.0 test --offline --manifest-path boards/cyd/handover/Cargo.toml
cargo +1.99.0 run --offline --release --features model \
    --manifest-path boards/cyd/send-plan/Cargo.toml --example score -- <folder of area lists>
```

## Features

| feature | default | what it switches |
|---|---|---|
| `demo-card` | on | A card built into the firmware, read only: every game in `games/` as `games/build-chip.sh` built it (GAME.BIN, INFO.TXT and the tile picture ART.BIN). Fourteen tiles, in this order: BENCH (a speed test, a measuring tool and not a game), CAMBER, DRIVE3D (a rally prototype, third), HAIRPIN, KESTREL, LANDER, LANTERN, MERCATOR, PADTEST, PROTO3D (a 3D platformer prototype, tenth), ROOFTOP, SKIMMER, STARDUST, TUNDRA. (The four made-up games it had before are gone.) |
| `sd-card` | off | The real microSD card, **read only** (every write is refused before it reaches the card). With `demo-card` also on, the real card wins. With neither, the console sees "no card". |
| `touch` | on | The touch chip (CLK 25, MOSI 32, MISO 39, CS 33, IRQ 36), through the touch filter (below). One log line at each touch's start (raw values and panel pixels) and one at its end (what it did). The calibration is a first guess. |
| `link` | on | The debug link on the USB serial port: `boardctl shot`, `key`, `tap`, `stats`, `text`. Without it the port carries only the log. |
| `tour` | on | The tour demo (below). Off at start; started by a letter or a long press. Also makes BOOT a "click" (see below). |
| `wide-strips` | off | Measuring only: 2 x 15,360-byte strip buffers instead of 2 x 5,120, and the letter `b` runs the strip bench. |
| `store` | on | The brightness (and, with `boot-gather`, the boot animation choice) kept in the chip's flash, in the "settings" partition (below). Without it, or without the partition, every start is at full light. |
| `core2` | on | The chip's second core sends the pictures to the panel; the console and the radio stay on the first core. In the owner's build since 2026-11-18 (the owner chose two cores); leave it out for one core, which stays supported. See "One core or two" and "The second core sends the pictures (core2)". With `pad` it cannot be combined with `wide-strips` (the main stack would be under 8 KB). |
| `send-ab` | off | Measuring only, not for a build handed over: the letters `o` (today's send plan), `p` (the new one, the default) and `u` (the tear-free timing off / on: fast, may tear), to compare on the board. See "Sending a picture" below. Without it the new plan is always used, and the `stats` frame has no `send` line (that frame is already full with `pad` and `store`). Works with `core2` too. |
| `hang-test` | off | Measuring only, not for a build handed over (turns on `core2`): the letter `H` parks the second core for good, as a hang would, to see the watchdog on the board. About 500 ms later the console stops with `core2: the second core still holds job 2 after 500 ms (join); console stopped, press RST` (or `(park)` if a settings save came first); press RST. See "One core or two". |
| `xip-probe` | off | Measuring only, not for a build handed over (turns on `core2`; phase A-i of the study of game code run straight from flash): 128 KB of probe code in the firmware's flash code, before the hot block, run on the first core. The letters `a` (the cost of one flash cache miss), `0`..`5` (a load of 0/8/16/32/48/64 KB of probe code each console step), `j` (the next start offset) and `d` (the next delay after a handed picture: 0/2/4/8 ms); `r` and `m` also clear and log its counters. The build prints `MEASURING BUILD: xip-probe`. Phase A-i was run on the board on 2026-11-22; its results are kept outside the repository. See "Flash code probe". |
| `boot-gather` | off | In the owner's builds since 2026-11-19 (the owner saw it on the board and approved it); not in the default set. console-core's GATHER boot animation (2,048 points gather into POCKETME, which shrinks into home's title; about 2.7 s, the console asks for 60 frames a second while it moves) instead of today's boot screen, and a BOOT ANIM line (FULL, SHORT, OFF) in the home screen's menu. Any button, BOOT or a tap skips it. With `store` the choice is kept in the settings record beside the brightness (byte 6; the log says `settings: boot anim FULL` at start). Its timing on the board (frame times, sends) is not measured. See `crates/console-core/README.md`, "Boot animation GATHER". |
| `game-flash` | off | In the owner's builds since 2026-11-23 (the owner's decision); not in the default set. Game flash parts (2026-11-22; on the board since 2026-11-23, MERCATOR's part: the start-up lines, and the planet's whole visit timed at the three detail levels): two windows at the start of the firmware's flash (code 0x400D0020 + 128 KB, read-only data 0x3F400120 + 64 KB, from `game_api::layout`) hold the flash parts of the games that have one (MERCATOR today), taken from `<game files>/flash/<NAME>/` at build time. A game file with a flash part ("GAMF") starts only when this firmware holds that part byte for byte; without the feature every such game is refused (FLASH PART MISSING). Not a measuring switch. See "Game flash parts". |
| `pad` | off | The gamepad (round 6, replaces `radio-fit`): the saved pairings read (read only), the Bluetooth radio in dual mode, and crates/pad-host, reconnect only: a saved pad may call, nothing else (no search, not discoverable, no pairing). The pad is the console's pad, with motion and battery; the `stats` frame gets a `pad` line; with `link`, `boardctl pad` plays a pad. See "The gamepad radio (pad)". Same game slots as every other build. |

Measuring-only build switches (environment variables read by
`build.rs`; never in a build handed over for use):

- `CYD_DEMO_EXTRA=FOLDER=path;...`: extra, unchecked files on the demo
  card (test files the console must refuse).
- `CYD_LAYOUT_PAD=<before>[,<between>]` (bytes, multiples of 4): empty
  space in front of the hot block, which moves it against all other
  code and the read-only data, and between its two parts, which moves
  its functions against each other. To check that the timed paths do
  not depend on where the linker puts things; see "The timed paths and
  the flash cache". Unset (the default) the build is unchanged; set, the
  build prints a `MEASURING BUILD` warning.

## Using it

- `boardctl key right` (and the other buttons) moves the home screen
  selection. The link's buttons are a "cable pad": pad 0 shows as
  connected from the first press. Link A is the bottom face button.
  `boardctl key a --player 2` (2 to 4) presses on cable pads 1 to 3,
  the game's players 2 to 4; each shows as connected from its first
  press. Only player 1 moves the menus. The Bluetooth gamepad is
  always pad 0 (player 1); pads 1 to 3 have no radio pad yet.
- `boardctl shot -o FILE` takes the panel's own memory (`--source fb`
  takes the console's 160x120 picture).
- `boardctl stats`: features, page, selection, screen counters, heap,
  stack, the touch and BOOT counters, the backlight, and the time per
  scene (short form; the log's `perf` lines have it all).
- Letters (`boardctl text X`): `w` wear test of the settings store
  (`store`, measuring only: saves in a row until a sector is erased,
  timing each write and the erase, then the console's own brightness is
  saved back), `t` tour on/off, `m` print the
  measurements now, `r` reset them (touch and BOOT counters too), `b`
  strip bench (`wide-strips`), `n`/`c` act as if the card were out /
  back in (`sd-card`, measuring only: it keeps the card's chip-select
  high), `h` one press of Home (the link has no Home button: in a game
  it opens the pause menu), `z` log how many words of the two game slots
  are not zero, `i` log the touch and BOOT counters. With `send-ab`
  (measuring only): `o` today's send plan, `p` the new one, `u` the
  tear-free timing off / on (send `r` after a switch). With `core2`:
  `V` the picture check on the second core on / off. With `hang-test`
  (measuring only): `H` parks the second core for good, so the
  console stops 500 ms later with the watchdog's message (press RST).
  With `xip-probe` (measuring only): `a` the cost of one flash cache
  miss, `0`..`5` a load of probe code each console step, `j` the next
  start offset, `d` the next delay after a handed picture; `r` and `m`
  also clear and log the probe's counters (see
  "Flash code probe"). Without the feature these letters are not
  compiled.
- The `send` line (the log's report; with `send-ab`, `stats` has a
  short form, `send <plan> <timing> as=<windows>/<box>/<no plan> wait= send= lead=`): how
  pictures were sent. `plan` new or old, `timing` on or off, `presents`, `win_x100`
  windows per present (x100), `as_windows` / `as_box` presents the new
  plan sent as their own windows / as one box, in one refresh pass,
  `no_plan` presents with no one-pass plan (each window then waits for
  its own pass), `late`, `crossed`, `wait_us` and `send_us` the waiting
  and the sending part of a present (average/largest), `lead_us` from a
  window's send call to its first pixel byte (average/largest), `px`
  pixels sent per present, `refreshes_x100` refresh passes begun per
  present (x100).
- The `pad` line of `stats` (`pad`): where the gamepad is and how its
  reports come; see "The `pad` line" below. `boardctl pad ...` plays a
  gamepad (`pad` with `link`, below).
- Measuring letters (use `boardctl log --send X` to see their result):
  `k` pulls BOOT low for 5 ms (a press shorter than a frame, no finger
  needed), `x` times 20 full touch readings, `l` checks the backlight
  from the chip's side (below).
- `boardctl touch SCRIPT`: a scripted finger (down, move, up, noise),
  through the same filter as the chip's readings (tools/boardctl).
- `boardctl tap X Y`: one touch that started and ended between two
  console steps (the console gets it as a noted tap).
- Games: pick PADTEST or STARDUST, `boardctl key a`. To leave: `boardctl
  text h`, `key down`, `key down`, `key a` (BACK TO HOME).
- BOOT: with `tour`, a short press reaches the console as one press when
  you let go; held 1 s, it starts or stops the tour. Without `tour`, the
  console sees the button as it is (debounced), and a press is noted on
  its edge. Either way a press shorter than a frame is seen once.
- While the panel is touched, the log gets the touch counters every 5 s.
- The `touch` line of `stats`: `n` touches (all / from the link),
  `drop` drop-outs held over (all / that ended a touch), `jump` largest
  jump between two readings (px), `jit` resting jitter (average in
  tenths of a px / largest), `brk` dead band break-outs (all / ones that
  moved the point by jitter alone), `tap`, `drag`, `flick` as the
  console's gesture code sees them, `read_us` one chip reading (average
  / largest), `lost` touch edges with no reading, `spk` lone wild
  readings left out (crates/touch-filter). The `boot` line:
  `edges` seen by the pin, `notes` kept, `got` presses the console took.
- The `settings` line (`store`): `on` (the partition was found), `found`
  (saved settings were read), `b` the brightness step held, `n` saves
  written / unchanged / failed, `rec` records used in the sector, `pend`
  a save waits, `save` one whole save, `wr` one flash write, `er` erases
  (count / average / largest), `load` the start's flash work, all in us.
- The `perf` lines: `fps` counts every frame of the scene over its
  whole time; `run`, shown only when it differs, counts frames that came
  back to back (gaps up to 100 ms). A drag with rests in it has a low
  `fps` and a `run` near 30 (the "22.33" of the owner's test, see
  src/stats.rs).
- `boardctl stats` is one link frame of 1 KB. If it is ever full, its
  last line says `(cut: frame full)`. (The write clock and strip size
  left it in round 5; the boot log's `panel:` line has them.) With `pad`
  and `store` it is full after a few scenes (ESTIMATE from the format
  strings, 2026-11-18: about 1,250 bytes of text after a 24 s run, about
  1,650 after a long session), so the last `perf` lines and the `loop`
  line are cut; the log's `m` report has them all.
- The tour: one step right per game on the card (fourteen on the demo
  card, one more than the row needs, so the last tile is always
  reached), a notification, as many steps left, another notification;
  one step every 0.7 s, round and round.
- Every 30 s the log gets the measurements (`perf`, `card`, `screen`,
  `mem` lines).

## How it works

- **Screen** (screen.rs, lcd.rs, beam.rs): the panel is written in
  columns (MADCTL 40h). At start the panel's refresh is measured by
  reading its line counter (command 45h) for 200 ms; each present then
  waits for the right moment so that no window is drawn over while the
  panel shows it (tear-free). The picture goes out in strips of 5,120
  bytes (2 buffers, DMA), filled while the previous one is on the wire.
  Which windows are sent, and when: see "Sending a picture" below.
- esp-hal's low-level SPI driver runs from RAM (`.cargo/esp-config.toml`),
  and so do the scanline read (`Lcd::scanline_stamped_raw`) and
  `fill_fast`. From flash, a cache miss stretched every counter read
  (41 us and more in one build), and the timing could not be measured
  at all. The rest of the timed paths runs from flash, as one hot block
  (`hot-block.list`): the screen's `present` and `send` (with the
  writer's loop), the window set-up (`Lcd::begin_in`), esp-hal's
  `SpiDma` read, write and wait, the DMA set-up, the SPI clock switches,
  the send plan, and with `core2` the second core's executor and task.
  See "The timed paths and the flash cache".
- **Console**: one `static` (45,176 bytes in the owner's build of
  2026-11-23, read from the firmware file's symbol table; all zero at
  start, in `.bss`), never on the stack.
- **Home lives in the game data slot** (games.rs, `GameSlots::lend`;
  board.rs, `parts_and_lent`): while no game is loaded, the 48 KB game
  data slot is empty, so the board lends it to the console, and the
  home screen keeps its game list and tile pictures there (5,396 bytes;
  rules in `crates/board-api/src/lent.rs`). The `unsafe` that makes a
  byte slice of the slot is in `GameSlots::lend`, next to the other slot
  code: it gives nothing while a game is loaded, and the slice borrows
  the `GameSlots`, so no game can be loaded or wiped while the console
  holds it. The code slot is never lent (whole aligned words only).
  Starting a game wipes the slot and loads the game over the list; on
  the way back home the console reads the list and the pictures from the
  card again (3 steps; the log line `home reload: <steps> steps, <calls>
  card calls in <us> us, home drawn <us> us after the game ended`, and
  `reload` in the `stats` report, give the real time).
  Owner build with the pad (`demo-card,touch,link,store,pad`), linker
  numbers, not measured on the board: `CONSOLE` 50,456 to 45,032 bytes,
  `.bss` 80,484 to 75,148, `.data` 15,684 to 15,700, stack room
  (0x3FFE0000 minus the end of `.bss`) 33,892 to 39,212 (+5,320).
- **The loop** (app.rs): one task. It looks at BOOT and the panel, calls
  `Console::step`, then sleeps until the returned time, or until a byte
  on the serial port, or a press of BOOT or the touch screen. While BOOT
  or the panel is held it wakes every 16 ms to look again, but calls
  `step` only when the console would see something new (or its own time
  came): a finger resting on the panel costs a chip read, not a console
  step (`skipped` in `stats`). When the console is already behind (its
  next step is due at once) the loop does not sleep; it then reads the
  bytes already waiting on the serial port, without waiting for more, so
  `boardctl` still gets answers. (Before 2026-11-18 it did not: a game
  behind on every frame, LANTERN in HEAVY at 60, left the link without
  answers until a restart.) A present takes up to about 35 ms and
  does not give way to other tasks while it runs. With `pad`, the loop
  also reads the radio once per turn, before it looks at the input, and
  wakes when the radio has something (see "The gamepad radio (pad)":
  the radio's controller has its own esp-rtos task, and the driver
  queues every packet, so nothing is lost while a frame is drawn).
- **Input notes** (board.rs, the board-api rule "a board that notes
  presses"): the level and the press note are read in the same look,
  before the step, and the console reads both in its step with nothing
  in between, so they always agree and a press is never counted twice.
  - BOOT: the GPIO edge latch (the interrupt status bit, set by the
    hardware even while the loop is busy drawing) plus the level. A new
    press needs the button up for 30 ms first (bounce). `held()` gives
    the level as the look saw it; `take_pressed()` the press.
  - Cable pad (link buttons, tour): `read` gives the held buttons only;
    `take_pressed` hands over every press since the console's last look.
    Pads 1 to 3 (players 2 to 4) are three more cable pads, set only by
    the link's `PlayerButtons`; the console asks for all four pads each
    step.
  - Touch: `read` gives the filtered point; `take_tap` where a touch
    started since the last look (also the link's `tap`, which is never
    shown as a held point).
- **Touch filter** (crates/touch-filter, touch.rs; tuned on the owner's
  finger test in round 5, see crates/touch-filter/README.md): one reading is the pressure,
  then x and y each as the middle of three conversions (after one thrown
  away), then the pressure again; about 1.6 ms. A touch starts at
  pressure 300 and goes on down to 180 (two limits, so a finger near the
  limit does not flicker). Up to 2 light readings in a row inside a
  touch keep the last point (drop-outs); the line going high ends the
  touch at once. A resting finger's point does not move until a reading
  lands more than 3 panel pixels from the average of its resting
  readings (dead band) on two firm readings in a row (one wild reading
  is left out and counted as `spk`), then follows every reading until 3
  in a row stay within 3 pixels. Counters: touches, drop-outs, largest jump, resting
  jitter, dead band break-outs, and taps, drags and flicks as
  console-core's own gesture code sees the filtered points.
- **Touch values and why** (round 5; the filter's own notes are in
  crates/touch-filter/README.md, "Tuning"):

  | value | what | why |
  |---|---|---|
  | `Z_ON` 300 / `Z_OFF` 180 | pressure to start / to go on | the owner's lightest real tap started at 323 |
  | `HOLD_READS` 2 | light readings held inside a touch | never more than 2 in a row in the owner's test |
  | `DEAD_PX` 3, `REST_READS` 3 | dead band at rest, readings to rest again | resting jitter 1 px on average, 3 at most |
  | `CONFIRM_READS` 2 (was 1) | firm readings outside the band before a resting finger moves | one bad reading made 13 of the owner's taps into short "drags" |
  | `TOUCH_GRACE` 1 frame (console-core, decision D1) | a finger missing for one frame is still the same touch, also after the console took it ("spent") | a contact lost for one reading is not a second tap |
  | `TAP_MAX_US` 500 ms (D5) | a press held longer gives no tap | a finger left resting on a tile does nothing when it lifts; only 2 of the owner's 150 taps were over 300 ms |
  | a touch on a gliding row stops it (D4) | that touch is no tap; after the lift the row settles at the nearest place | as on a phone |

  The menu's bars and CLOSE act when the finger goes down, so a long
  press on them still works (D5 does not change them).

  What the second reader measured (the logs are kept outside the
  repository): on the PC, one wild reading in a tap of 2 to 6 readings
  now always stays a tap (before: 22 to 100% became drags or flicks); a
  finger resting with ±2 px of noise broke out of the band once in
  60,000 readings (7 before), with ±3 px 490 times (3,301 before), never
  ending the touch or making a drag; a drag from rest starts one reading
  later (16 to 24 ms), then follows with no lag. The price: a swipe the
  panel reads only twice is a tap, and a short quick flick leaves the
  finger a little faster, because the held first move arrives as one
  jump (round 3's scripted flick 391 to 447 px/s at the median). On
  the board that flick now ends one tile further than in the
  simulator, which has no filter. Through the link on the board: a
  press with a 33 ms gap is one
  tap; a 700 ms hold on a tile does nothing on lift, also on the
  selected one; a 300 ms tap starts the game; 700 ms holds on a bar and
  on CLOSE work; a finger on the gliding row stops it short (scroll 88
  against 124) and starts nothing; BOOT in a game opens the pause menu
  once and closes it once.
- **Backlight** (backlight.rs): GPIO21 from the LEDC unit, low-speed
  timer 0 from the 80 MHz APB clock, 12-bit duty, divider 1.0: 19,531 Hz,
  4,096 steps (the dimmest value, 16, is 257 steps). 0 is off (pin low),
  255 is on (pin high). Off until the console's first `backlight` call
  (after its first picture). With `store`, the saved step is set before
  the console's first step, so a start after a dim choice is dim from
  the first picture. Nothing else uses LEDC (the timers of esp-rtos and
  embassy are the timer groups).
- **Card** (sd.rs): SPI3 (SCK 18, MOSI 23, MISO 19, CS 5), woken at
  400 kHz, read at 20 MHz, FAT 8.3 names, every handle closed before a
  call returns. Before the card library wakes a card, one "go idle"
  command checks that a card answers at all (16 bytes); the library alone
  would retry for seconds with no card in, every 2 s.
- **Log** (logbuf.rs): a 4 KB ring, emptied by the loop when the port is
  free; a line that does not fit is dropped and counted, never waited for.
- **Games** (games.rs): see "Games" below.
- **Settings** (flash_store.rs, `store`): see "Settings in flash" below.

## Sending a picture: windows or one box (2026-11-18)

### In plain words

The panel shows its memory by "refreshing" it: it reads one column of
pixels after the other, left to right, about 343 lines in 17 ms, and
then starts again. If the console writes into a column while the
refresh reads it, the panel shows half the old and half the new picture
for one frame (a "tear"). So every picture is sent in step with the
refresh.

Each frame the console finds the parts of its picture that changed (up
to 16 rectangles, "areas"). Each area is sent as a "window": a rectangle
of the panel, written column by column. A window may only start when
the refresh has just passed its first column and will not catch up with
the writer before the window is done (send-plan: `Job`, its start
window, `plan`). The chip knows where the refresh is by asking the panel
for its line (the Get Scanline command) once per picture, and counts
from there with its own clock.

Before 2026-11-18 every area was sent as its own window, all in one
refresh pass if they fitted, else each in its own pass. A turning 3D
view (LANTERN) changes about ten wide, low bands a frame. Such a band
is written about 20 times faster than the refresh moves, so it may only
start in a narrow span late in the refresh; ten of them seldom fit one
pass, and the present waited refresh after refresh. On the board
(MEASURED, L4 logs of 2026-11-17) NORMAL
detail at the 60 setting ran 33.35 frames a second with a 27 ms
average present and 582 of 911 presents without a one-pass plan.

Now (send-plan's `choose`, called from `CydScreen::present`) each
present also looks at ONE box around all the areas and takes whichever
of the two the timing model says is done sooner:

1. Read the refresh's line from the panel.
2. Work out both ways (the windows, or one box), take the sooner one,
   and pick the refresh pass it fits in. All of this happens before the
   first window is sent; between the first and the last window only the
   wait, the send and a few stores run (no planning, no float).
3. For each window: wait until its start moment, send it.
4. If no one-pass plan is found, today's way is used (each window in its
   own pass) and counted as `no_plan`.

A box also sends pixels that did not change, with the values the panel
already shows, so the picture is the same. A tall window is written
about as fast as the refresh moves, so it may start almost anywhere in
the refresh and nearly always fits one pass. Small areas spread across
the screen stay windows (the box would be no sooner). Groups of windows
that share columns ("a few boxes") were tried in the model too: over
LANTERN's area lists they were always the same as the one box, so the
firmware leaves them out.

**With `core2`** the second core makes the same choice for the picture
it was handed (`present_held`). A box there reads pixels outside the
changed areas, so the whole handed picture must stay unchanged during
the send. It does (read in the code, src/core2.rs "The send plan"): the
second core reads only the handed picture (the whole 19,200 bytes, a
box never reaches past it); the first core does not write that picture
until its next present has waited for the second core to finish
(`JOB` = 0); `frame_mut` and `game_buffers` only ever give the other
picture; and outside the areas the handed picture already equals the
one the panel shows (the catch-up copy of the step before). Not
measured on the board yet.

### Measured on the board

BOARD numbers (MEASURED ON THE BOARD 2026-11-18; the report is kept
outside the repository): one core, five games (a
trial build with the same send code as this one), features
`demo-card,touch,link,store,pad,send-ab`, LANTERN at level 4, a
scripted 24 s run, the board restarted
before every run, a gamepad connected. "old" is the letter `o` (before
2026-11-18), "new" the letter `p` (the default). "present" is the time
one present takes; "no one-pass plan" is presents with no plan of all
presents; "late" and "crossed" are windows (crossed: the refresh may
have caught the writer, a possible tear).

| detail / rate | fps old → new | present avg ms old → new | longest ms old → new | no one-pass plan old → new | late old → new | crossed old → new |
|---|---|---|---|---|---|---|
| LIGHT / 30 | 30.00 → 30.00 | 13.0 → 8.7 | 33.4 → 24.7 | 30/530 → 0/528 | 146 → 16 | 92 → 3 |
| NORMAL / 30 | 26.31 → 30.00 | 29.8 → 14.0 | 82.0 → 28.1 | 281/452 → 7/528 | 137 → 17 | 83 → 2 |
| HEAVY / 30 | 26.06 → 30.00 | 32.5 → 14.8 | 82.2 → 18.4 | 236/370 → 0/535 | 104 → 13 | 55 → 1 |
| LIGHT / 60 | 57.78 → 59.95 | 12.2 → 8.8 | 33.4 → 22.2 | 46/995 → 0/1057 | 195 → 34 | 133 → 3 |
| NORMAL / 60 | 39.75 → 56.16 | 28.9 → 13.8 | 80.7 → 18.2 | 296/458 → 13/964 | 146 → 10 | 78 → 0 |
| HEAVY / 60 | 36.56 → 52.46 | 27.2 → 14.3 | 98.2 → 36.8 | 265/546 → 3/883 | 136 → 12 | 78 → 0 |

With the timing off (letter `u`, "fast, may tear"), new plan: NORMAL /
60 ran 59.72 fps (present 10.1 ms), HEAVY / 60 55.73 fps (12.6 ms).
In the same runs: code RAM left 8,260 bytes (unchanged), deepest main
stack 19,056 of 32,988, heap free 10,300, `panel: calibrated=true`, the
pairing area's CRC the same at every start, and the debug link kept
answering.

What the model had said before (ESTIMATE/MODEL: send-plan's replay
model over the simulator's recorded LANTERN area lists, 40 refresh
phases, the board's measured draw times):

| detail / setting | before: fps, present avg / max | new: fps, present avg / max | no one-pass plan, before → new | sent as one box |
|---|---|---|---|---|
| LIGHT / 30 | 30.0, 10.9 / 32.1 ms | 30.0, 7.4 / 16.1 ms | 6% → 0% | 69% |
| LIGHT / 60 | 57.6, 11.9 / 29.9 ms | 60.0, 7.0 / 12.8 ms | 5% → 0% | 67% |
| NORMAL / 30 | 26.8, 28.1 / 81.4 ms | 30.0, 12.7 / 17.6 ms | 70% → 1% | 89% |
| NORMAL / 60 | 30.9, 28.6 / 82.6 ms | 58.6, 13.1 / 17.2 ms | 66% → 1% | 92% |
| HEAVY / 30 | 25.6, 30.0 / 81.2 ms | 30.0, 13.8 / 17.6 ms | 65% → 0% | 81% |
| HEAVY / 60 | 27.0, 32.0 / 96.7 ms | 53.9, 13.6 / 17.2 ms | 65% → 0% | 84% |

The model's "before" was close to the board's (NORMAL / 30: 26.8
against 26.64 measured on 2026-11-17, 26.31 in the table above). A box
sends more pixels (NORMAL / 30: about 56,500 a present against 43,900,
model), so the wire time goes up a little, but the waiting
nearly goes away.

To compare on the board, build with `send-ab` and switch with the
letters `o` / `p` (and `u` for no timing at all); send `r` after a
switch. The log's `send` line and `screen presents=` line show the
difference. Look at: the boot log's `panel:` line (`calibrated=true`),
tearing by eye while turning, `late` and `crossed`, `lead_us`, and the
`pairing area crc32=` line (unchanged).

### Open points

- **The lead.** The planner assumes 35 us from a window's send call to
  its first pixel byte (`LEAD_US`). The board measured 85 to 158 us on
  average and 224 to 717 us at the longest (MEASURED ON THE BOARD). The
  model said the new plan stays ahead even with 40 or 120 us more,
  and the board table above
  agrees, but `LEAD_US` was deliberately not changed yet.
- **Late windows are still sent.** A window whose start moment was
  missed is counted `late` and sent anyway (10 to 34 per 24 s run with
  the new plan). A rule "wait for the next safe span instead" is a
  possible next step; not built.
- **Crossed counts of the old plan.** The old plan's `crossed` (55 to
  133 per run) is far higher here than in the runs of 2026-11-17 (1 to
  6). A gamepad was connected this time; whether that is the cause was
  not checked.
- **`core2` with the new plan** was run on the board later the same
  day: 0 crossed windows in every
  run; see "One core or two".

### Memory

MEASURED ON THE PC from the ELF files of 2026-11-18, seven games,
owner's set with `pad` (`demo-card,touch,link,store,pad`), against the
builds just before the new plan (one core and two):

| | one core: before → new | two cores (`core2`): before → new |
|---|---|---|
| code RAM left below the game slot | 8,260 → 8,260 | 8,204 → 8,204 |
| `.bss` | 75,148 → 75,996 | 83,688 → 84,544 |
| main stack | 39,212 → 38,364 | 30,536 → 29,648 |
| largest frame on the send path | console step 1,376 (send built in) → 1,296 plus `CydScreen::send` 320 | second core's task 944 → 816 plus `send` 320 |

`.bss` grows by about 850 bytes (the screen's per-window marks and new
counters). With `send-ab` the numbers are the same (two-core main stack
16 bytes less). The plan code itself (`send_plan::plan`, `plan_end`)
has 48-byte frames and runs from flash.

Nine games (SKIMMER added), MEASURED ON THE PC from the ELF files of
2026-11-18, owner's set with `pad`, one core and two cores
(against the eight-game builds): code RAM left 8,260 and 8,204,
`.bss` 76,180 and 84,728, main stack 38,180 and 29,448, the same as with
eight games. The demo card's files are constants in flash: `.rodata`
grows by 45,368 bytes (one core) and 45,376 (two cores), `.text` by 52.

## The timed paths and the flash cache (2026-11-18)

### In plain words

Code in flash runs through a cache: 32 KB for each core, two "ways",
lines of 32 bytes, 512 sets. Two addresses 16 KB apart (or any multiple
of it) fall into the same set, and a set holds two lines. When three
lines that a loop needs fall into one set, every pass through the loop
throws one out and loads it again from flash (a "miss", a few
microseconds each, ESTIMATE). The constants a function loads (its
"literal pool") and read-only data go through the same cache.

Before this change the linker put the send path wherever it liked, and
that decided how fast it was: the lead (from a window's send call to
its first pixel byte) was 30 us on average in a game in one build
and 100 to 122 us
in six builds that differed only in where the linker put the code,
and the scanline read took 17 us in
some of those layouts and 25 us in others, with no change to the code
(MEASURED ON THE BOARD). A "cache lottery".

Now the functions of the timed paths sit together in one block of
flash, the **hot block**, placed right after all other flash code:

- `hot-block.list` names them (one line each, a wildcard on the
  function's mangled name), in two parts: `present` (run once a
  present: the plan, the second core's executor and task, the
  scanline read's `SpiDma::read` and `apply_config`) and `window` (run
  for every window: the screen's `present` loop, `send` with the
  writer's loop, the window set-up, esp-hal's `SpiDma` write and wait,
  the DMA set-up and the SPI clock switches). Lines can say `two`
  (with `core2`), `one` (without) or `all`.
- `build.rs` turns the list into the linker script `hot-block.x`: one
  output section `.text_hot` that starts exactly where `.text` ends
  (the bootloader maps only one flash code segment, so there may be no
  gap), each function with its literal pool. Being its own output
  section, its literals are never shared with other code. A third
  place, `ram`, puts a line in executable RAM instead (`.rwtext_hot`,
  after `.rwtext.wifi`); no line uses it now (options B and C under
  "Measured on the board").
- The window part is under 16 KB and the whole block under 32 KB, so
  the block alone never puts three lines into one set, wherever the
  linker puts everything else. Today (MEASURED ON THE PC, owner's
  builds with `pad`): two cores 20,924 bytes, window part 13,652 bytes
  in 427 lines (85 left); one core 19,536 bytes, window part 14,732
  bytes in 461 lines (51 left).
- The link itself stops (an `ASSERT` in `hot-block.x`, message
  starting `HOT BLOCK:`) when a `must` line places nothing (each line is
  framed by two symbols), when the window part covers more than 512
  cache lines (16 KB) or the block more than 32 KB, when the block is
  not inside the flash code region, or when a part is under its floor
  (`HOT_FLOORS` in build.rs: present part 5,120 bytes with two cores and
  4,096 with one, window part 12,288 for both; over all the sets
  check.sh builds the parts measured 6,667 to 7,280 / 13,513 to 13,720
  bytes with two cores and 4,795 to 4,804 / 14,569 to 14,904 with one,
  MEASURED ON THE PC, so each floor leaves 700 to 2,300 bytes of room).
  So a plain `cargo build` already fails on an empty or too big block.
- `hot-check.py <elf> --cores two|one` checks a built firmware in more
  detail. By real address, one line per misplaced function: every
  `must` line matches a function and each match is in its part, and
  every instance of the screen's `present` and `send` (with their
  closures) is in the window part whether a line names it or not. It
  walks the calls (direct calls and function addresses loaded with
  L32R) from every function of the block and from the timed paths' RAM
  functions (through RAM) and fails on any flash function outside the
  block it reaches, except the cold paths (panics and their messages,
  an exact list of demangled names) and one start-up function. Every
  literal a block function loads must be inside the block, the parts
  must be above the same floors, the window part at most 512 cache
  lines and the block at most 1,024, and the block must follow `.text`
  without a gap. Its last line gives the sizes and the cache lines
  left. `check.sh` runs it on the owner's builds with the gamepad (two
  cores and one, each with and without `boot-gather`). When an esp-hal
  update or a refactor renames, inlines or adds a function on the path,
  the link or hot-check names the line to fix.

The scanline read itself (`Lcd::scanline_stamped_raw`), `fill_fast`,
`core2::parked` and esp-hal's low-level SPI driver stay in RAM as
before. The block costs no RAM.

### Measured on the board

**Final round on this tree (2026-11-19)**, MEASURED ON THE BOARD (the
logs and the report are kept outside the repository; a first try of
"five plain boots" never flashed and is not used). The ELFs were built from
this tree with the beam logic in `send_plan::beam`, `MAX_FLOOR_US` 40
and the `missed_wrap` check. The owner's sets with `pad`, KESTREL with
the same 24 s script as below (home, then the game), **two runs per
layout**. Lead = average / longest in microseconds; `late` and
`crossed` count the whole run.

| layout | two cores: home lead | game lead | late | anchor misses | one core: home lead | game lead | late | anchor misses |
|---|---|---|---|---|---|---|---|---|
| p0 | 38, 38 | 27 / 240, 27 / 240 | 0, 0 | 0/0/131/0, 0/0/110/0 | 119, 94 | 47 / 681, 47 / 560 | 11, 15 | 0/0/113/0, 0/0/104/0 |
| p1024 | 34, 34 | 26 / 240, 26 / 240 | 1, 4 | 0/0/104/0, 0/0/107/0 | 130, 124 | 45 / 257, 44 / 257 | 15, 9 | 0/0/108/0, 0/0/119/0 |
| p4096 | 40, 40 | 28 / 240, 28 / 240 | 0, 0 | 0/0/125/0, 0/0/130/0 | 124, 124 | 47 / 257, 47 / 257 | 6, 2 | 0/0/101/0, 0/0/130/0 |
| p8192 | 34, 34 | 26 / 240, 26 / 240 | 0, 0 | 0/0/115/0, 0/0/124/0 | 128, 112 | 42 / 653, 42 / 249 | 6, 6 | 0/0/113/0, 1/0/140/0 |
| p0b2048 | 34, 34 | 26 / 240, 26 / 240 | 0, 0 | 0/0/123/0, 0/0/115/0 | 142, 152 | 54 / 257, 55 / 251 | 7, 5 | 0/0/120/0, 0/0/110/0 |

All 20 runs: `fastest_read_us=17`, `rejected=0` (`slow=0 bits=0`),
the whole survey within 1 us (8,402 or 8,403 reads), `calibrated=true`,
no `survey not used` line, `iram_left` 8,204 (two cores) and 8,260
(one core), the pairing area's CRC unchanged, 0 untimed, 0 no anchor,
**0 crossed**. One one-core run had 1 slow anchor read.

Other games, normal owner builds (no measuring switch), **one run
each** (TUNDRA on one core: two runs), the game started from the home
carousel and played with scripted keys, no setting changed by the script,
no gamepad (two cores: two screenshots a second apart, taken with a key
held after the counters were read, differ;
one core: no screenshots, but the counters show 199 to 562 presents of
5,300 to 74,000 pixels each after the home part). Game lead, late,
crossed:

| game | two cores | one core |
|---|---|---|
| HAIRPIN (race) | 33 / 240, 0 late, 0 crossed | 50 / 247, 13 late, 0 crossed |
| TUNDRA (match) | 31 / 240, 0 late, 0 crossed | 74 / 247, 10 late, **1 crossed**; again: 73 / 247, 8 late, **1 crossed** |
| LANTERN (level 1) | 35 / 240, 0 late, 0 crossed | 98 / 247, 1 late, 0 crossed |
| STARDUST | 35 / 240, 1 late, 0 crossed | 151 / 247, 0 late, 0 crossed |

Five plain starts of the normal two-core build: `calibrated=true`,
`fastest_read_us=17` and the pairing area's CRC unchanged each time.

So with two cores the timing does not depend on the layout: game lead
26 to 28 us in all ten KESTREL runs and 31 to 35 in the other games,
0 crossed in all 14 runs. One KESTREL run had 4 late (the others 0 or
1). One core is about as before the move (round 3 below: game lead 44
to 50, late 2 to 12; now 42 to 55 and 2 to 15). It still has crossed
windows in TUNDRA (1 in each of two runs), and its game lead in the
other games is longer (50 to 151 us). One core had crossed windows
before the hot block too ("One core or two" below: TUNDRA 17 late /
2 crossed), but those runs used the 60 setting, two players and a
connected gamepad, so they are not a like-for-like "before". The
closest "before" for TUNDRA is one run at the 30 setting, two players,
a gamepad connected: one core 10 late / 0 crossed of 2,250 windows.
There is no one-core run of
HAIRPIN, LANTERN or STARDUST at the 30 setting from before the hot
block. So whether TUNDRA's crossed window on one core is old or new is
not known: one run before with 0, two runs now with 1 each, under
different conditions. For KESTREL on one core the
before-block runs gave game lead 48 and 49 us, 7 and 10 late, 0 crossed
(one run each); now at p0 47 and 47 us,
11 and 15 late, 0 crossed: about the same, with slightly more late.

Not measured yet: LANDER, PADTEST and ROOFTOP; the games at their
60 setting or HEAVY detail; a connected gamepad (none was connected);
`sd-card` builds; long play (each run is under a minute); a start whose
survey is refused (`reads_too_slow`, `missed_wrap`: never happened,
all reads took 17 us). The `odd_step` and `uneven_wraps` refusals and
the second survey (below) came after this round (on the board since:
"Survey guard" below).

**Earlier runs (older ELFs from before the move to `send_plan::beam`,
same hot block code byte for byte, MEASURED ON THE PC).**
MEASURED ON THE BOARD (the "A runs"): the owner's
build with `pad`, the same 24 s run each time (home, then a game).
**One run per layout.** `CYD_LAYOUT_PAD` moved the block by 0, 1,024,
4,096 or 8,192 bytes against everything else, or (`p0b2048`) put 2,048
bytes between its two parts. Lead = from a window's send call to its
first pixel byte, average / longest, in microseconds. `late` and
`crossed` count the whole run.

| two cores | fastest read | survey spread | home lead | game lead | late | crossed | untimed, no anchor | anchor misses |
|---|---|---|---|---|---|---|---|---|
| p0 | 17 us | 8,436 within 1 us | 40 / 231 | 28 / 231 | 0 | 0 | 0, 0 | 0/0/119/0 |
| p1024 | 17 us | 8,436 within 1 us | 34 / 231 | 26 / 231 | 1 | 0 | 0, 0 | 0/0/119/0 |
| p4096 | 17 us | 8,436 within 1 us | 34 / 231 | 26 / 231 | 0 | 0 | 0, 0 | 0/0/111/0 |
| p8192 | 17 us | 8,435 within 1 us, 1 at 5 to 8 us | 34 / 231 | 26 / 231 | 0 | 0 | 0, 0 | 0/0/111/0 |
| p0b2048 | 17 us | 8,436 within 1 us | 49 / 231 | 30 / 231 | 0 | 0 | 0, 0 | 0/0/105/0 |

Every run: `calibrated=true`, read limit 25 us, `iram_left=8204`, the
pairing area's CRC unchanged. The anchor misses are slow / bits /
range / failed: no anchor read in these runs was too slow (one was in
a one-core run, below); the third column counts reads too near the
ends of the counter's range or its held value, which happens by
design (the next try is used).

Against the runs before the block (MEASURED ON THE BOARD): the best
build of the old way gave a game lead of 30 / 256 and 2 late, and six
builds that differed only in their layout gave 100 to 122 us, 2 to 16
late, 0 to 7 crossed and reads of 17 or 25 us. With the block, every
layout gave the same timing.

**One core** (MEASURED ON THE BOARD, round 3,
the same run; the A runs' one-core run stopped after the home part):
two runs each of p0, p1024 and p4096; p8192 and p0b2048 were built
(all pass hot-check) but not run in
round 3 (they ran in the final round above). Every
run: fastest read 17 us, spread 8,440 within 1 us, `calibrated=true`,
`iram_left=8260`, 0 crossed, 0 untimed, 0 no anchor. Game lead 44 to
50 us on average, longest 242 to 794; home lead 98 to 172 on average
(p0 98 and 111, p1024 146 and 131, p4096 172 and 166); late 2 to 12 a
run; anchor misses 0/0/100 to 115/0, except one run (p1024, first)
with 1 slow anchor read. So one core is not as steady as two: its
home lead depends on the layout, and its late count is about the same
as before the block (7 and 10 late, game lead 48 and 49). A first try
of the whole batch stopped
with "CRC LINE MISSING OR DIFFERENT" from the run script; why was not
found, and that batch was not used.

**Options B and C: one run each, p0 (and C one core also p1024)**
(MEASURED ON THE BOARD, round 3). B kept the
block empty and put esp-hal's `apply_config`, `SpiDma` read, write and
wait, the DMA set-up and the clock switches into RAM (`ram` lines);
C put the same functions
in RAM and the rest into the flash block. Game
lead average / longest and late: two cores B 40 / 264, 2 late; C
26 / 235, 0 late (A p0 in the same batch: 28 / 231, 0 late); one core
B 39 / 265, 4 late; C 40 / 245 and 39 / 415, 9 late each. Too few
runs to rank them. Both cost 6,096 bytes of RAM below the game slot
(MEASURED ON THE PC from the B and C builds, two cores and
one; the board printed `iram_left=2108` and `2164` against 8,204 and
8,260 for A), against about 8.2 KB left. Option A (this one) costs no
RAM.

What changed on the PC since those runs (measured on the board only
in the final round above, 2026-11-19):
the beam logic moved to `send_plan::beam` with the same arithmetic
(the hot block's sizes did not change by a byte: two cores 20,924 /
13,652, one core 19,536 / 14,732, MEASURED ON THE PC), a survey that
missed a refresh is now refused (`missed_wrap`), the floor-read limit
went from 60 to 40 us, the linker guards and the new hot-check came
in. The boot log prints `fastest_read_us=none` when no read worked, and
a refused survey adds a line `panel: survey not used: <why>`.

**Survey guard (2026-11-19).** One
start in 552 printed `lines=325.19 line_us=47.978 calibrated=true`,
figures no panel has (BOARD; the board
printed only these figures). The explanation was worked out on the PC:
one wrong answer of the 45h read (about half or double the true line),
or a run of them, was taken for a wrap. Two refusals now catch that: `odd_step` (two
reads close in time show a step the counter cannot make) and
`uneven_wraps` (two wraps seen by close reads much too soon after each
other). A drop right after the survey's first kept answer is no wrap
(nothing can check that answer). They do not catch (as before): a run of
wrong answers that starts while the counter shows 20 or less and lasts
to the end of the survey, a wrong answer at the first read after a
pause longer than the "close" limit or at the survey's very last read,
or wrong figures from pauses. The rules and tests are in `send_plan::beam`.

On the board (guard v2, 40 MHz flash, 220 starts, 225 surveys, BOARD,
two batches): 6 surveys
refused. Five were `odd_step`, each followed by a clean second survey;
their first odd steps look like one answer with one or two bits wrong
(for example 64 right after 67). One start (boot 169 of the second batch)
had an odd step but was refused as `lines_out_of_range`, was not
surveyed again and kept the defaults (its own figures were not
printed). On the PC one wrong answer does that when it is taken for
one more wrap with only one odd step: 20 or less in the middle of a
refresh (256 to 265 with bit 8 lost), or above 140 while the counter
shows 1 to 20 (1 with bit 8 or 9 set). The average refresh then gets
about one in eleven short (about 310 to 322 lines on the PC), and the
line
count was checked before the odd steps.

**Guard v3 (2026-11-19, PC only, not on the board yet).** `calibration`
now checks `odd_step` and `uneven_wraps` first, so a survey with a
wrong answer the guard sees is always named for that, whatever else it
fails (it does not see an answer one or two lines off);
then no fit, lines, slow reads, missed wrap, as before (they refuse the
same surveys as before; only the reason can change). `main` surveys
again only for `odd_step` and `uneven_wraps` (`Refused::retry`), up to
three surveys in all (`MAX_SURVEYS`, about 200 ms each), and logs
`panel: survey not used: <why> (lines=… line_us=… odd_steps=…
first_odd(from,to,us)=… wraps=… samples=…), surveying again` for each.
The last refusal logs the same line with the refused survey's own
figures (not the defaults) and `keeping the defaults (last survey)` or
`keeping the defaults (a new survey would not cure this)`. The `panel:`
line shows `odd_steps` and, at its end, `surveys=N`.

### What is not covered

- Read-only data (tables, strings, the `match` jump tables) cannot be
  placed by function name and stays where the linker puts it. A data
  line the send path reads can still meet two block lines in one set.
  Counting the lines the timed paths use (code, literals, read-only
  data), the five two-core builds of the table have 0 to 11 cache sets
  with three or more of them, all because of the data
  (MEASURED ON THE PC; code and
  literals alone: 0 everywhere; p4096 has the 11, the one-core builds 1
  to 4), and p4096's timing in the table is no worse than the others.
  These counts are from the older ELFs; the final round's ELFs were not
  counted. Two runs per layout in the final round, one before, so a
  small effect would not show.
- With one core, the console's step runs between two presents on the
  same core and throws block lines out of the cache, so the first
  window of a present refills them. That is the likely reason (ESTIMATE)
  why the one-core runs' home lead (98 to 172 us in round 3, 94 to 152
  in the final round) is longer than the two-core ones (34 to 49 us
  before, 34 to 40 in the final round). In round 3 it also changed with the
  layout (p4096 the longest), although p4096 has the fewest crowded sets
  of the one-core builds, so the counted sets do not explain it. The
  final round did not repeat that: p4096 gave 124 and 124, p0b2048 the
  longest (142 and 152), the others 94 to 130. So the one-core home
  lead varies by about 60 us from run to run and layout to layout, and
  why is not known.
- `CYD_LAYOUT_PAD=<before>[,<between>]` (measuring only, see
  "Features") moves the block against everything else, or its two
  parts against each other, to repeat these checks after a bigger
  change.

## Numbers (all from the board's own log)

The log files named below are kept outside the repository.

Strip size (step2-bench-strips.log, 20 sends each, time on the wire):

| area | 2 x 5,120 B | 2 x 15,360 B |
|---|---|---|
| full screen 320x240 | 15,629 us | 15,480 us |
| small 72x72 | 1,101 us | 1,095 us |
| row 320x88 | 5,750 us | 5,709 us |

The small strips cost at most 1% and save 20,480 bytes of RAM: chosen.

Step and present times, default build (step7-boot.log, step7-tour.log):

| scene | frames | step avg / max | present avg / max | frames a second |
|---|---|---|---|---|
| boot animation | 39 | 2,101 / 17,669 us | 1,447 / 16,896 us | 29.99 |
| row scroll | 55 | 17,695 / 22,977 us | 15,690 / 20,935 us | 29.91 |
| notification sliding | 132 | 6,199 / 12,378 us | 4,539 / 10,341 us | 29.93 |
| selection moves | 21 | 10,508 / 14,377 us | 8,427 / 12,370 us | (single frames) |
| idle home, nothing moving | 11 in 20 s | 1,676 / 1,708 us | none | one step per 2 s |

Tour, 45 s: 231 presents, 700 windows, 0 sent without the timing, 0
late, 0 drawn over the panel's refresh.

Second check, 2026-11-17 (soak-*.log):

- 5 minutes on the idle home screen, then 5 minutes of tour, no restart
  (uptime 617 s at the end), no warning or error line, heap and deepest
  stack use (8,944) the same from start to end.
- Idle: 226 loop wakes in about 300 s (0.75 a second; 150 of them the
  2-second card check), `early` 0. `loop wakes` and `early` are in
  `boardctl stats` and the log.
- Tour, 300 s: scroll 18,443 / 25,230 us step and 16,407 / 23,159 us
  present, 29.91 frames a second; 4,277 windows, 0 untimed, 0 crossed,
  9 late (started after their start window, the model still says not
  crossed), 165 presents that needed more than one refresh pass.
- The 2-second check on the real card (no games): 4,426 / 4,705 us.

Input, touch and backlight, 2026-11-17 (no finger was on the panel:
the touch numbers come from the link's scripted finger, which goes
through the same filter):

- A press shorter than a frame is seen once. BOOT pulled low for 5 ms
  by the board itself (letter `k`, v1-boot-pulse.txt, v3-boot-menu.txt):
  each pulse 1 edge, 1 note, 1 press the console got; on the home screen
  it started the selected game once, in STARDUST one pulse opened the
  pause menu and it stayed open (b-menu.png). Link presses with no hold
  (`key right --hold-ms 0`, v1-pad-short.txt): 5 right then 2 left moved
  the selection 0 to 5 to 3, one tile each.
- Scripted drag (drag.txt: 10 moves of 20 px, still, up) and flick
  (flick.txt: 4 moves of 30 px, up while moving): the picture once the
  row came to rest is the simulator's for the same script, 0 of 76,800
  pixels differ (sim/, b-drag-end.png, b-flick-end.png). The board's
  gesture count: 1 drag; 1 drag and 1 flick.
- Frame timing while dragging (drag-long.txt, 3.4 s back and forth,
  v4-drag-long.txt): 95 frames, 30.31 frames a second, step 13,674 /
  35,209 us (avg / max), present 14,820 / 32,789 us; 1 late, 0 crossed.
- A finger resting 4 s with 1 or 2 px of noise on each reading
  (v2-rest-n1.txt, v2-rest-n2.txt): 250 readings, 0 dead band
  break-outs, jitter at most 2 / 3 px; 9 / 8 console steps from the
  reset to the end (about 5.5 s), 0 presents; about 255 / 261 looks needed no
  step.
- One full touch reading (letter `x`, v3-readtime.txt): 1,621 us on
  average, 2,217 at most (20 tries).
- Backlight from the chip's side (letter `l`, v1-light.txt): timer
  divider 1.000, 12 bits, 19,531 Hz. For 0, 16, 42, 87, 156, 255: duty
  read back 0, 257, 674, 1,397, 2,505, 4,096 of 4,096; the pin sampled
  200,000 times was high 0.0, 6.3, 16.5, 34.1, 61.1, 100.0 % of the
  time, switching at 19,511 to 19,555 Hz (0 and 255: no switching).
  How the five steps look is for a person to judge.
- 5-minute tour, before / after this change (before-tour5.txt,
  after-tour5.txt): 2,103 / 2,110 presents, 40 / 29 late, 5 / 6
  crossed, 0 / 0 untimed; scroll 30.01 / 30.14 frames a second.

Card (step7-boot.log, step4-sd.log):

| | time |
|---|---|
| home scan, demo card (4 games) | 555 us |
| home scan, demo card (6 games, 2026-11-17) | 668 us |
| home scan, real card (no games on it) | 12,030 us, of which 7,887 us waking the card |
| 2-second check, demo card | 66 us |
| 2-second check, real card, no games | 4,444 us |
| 2-second check, no card | 1,272 us |

Memory, default build, 2026-11-17, after the input, touch and
backlight work (`xtensa-esp32-elf-size -A`, the `mem` log line, the
flash tool's image size):

| | bytes |
|---|---|
| flash image | 334,640 of 4,128,768 (8.1%) |
| `.text` (flash) | 244,873 |
| `.rodata` (flash, includes PADTEST 11,484 and STARDUST 10,516) | 61,492 |
| `.vectors` + `.rwtext` (IRAM) | 1,024 + 16,900 |
| `.data` (DRAM) | 9,984 |
| `.bss` (DRAM, includes the console, 46,336, all zero at start) | 68,416 |
| stack (DRAM, the rest up to 0x3FFE0000) | 118,208; deepest use seen at start 9,968 |
| heap (DRAM 0x3FFE7E30 up to the data slot) | 49,616, none used |
| game code slot (IRAM 0x4009_2000 to 0x400A_0000) | 57,344 |
| game data slot (DRAM 0x3FFF_4000) | 49,152 |

Before the input, touch and backlight work (same day): `.text`
228,685, `.rodata` 59,996, `.data` 9,832, `.bss` 67,944, stack
118,832 (deepest seen 9,264, also after 40 games); the image was
given as 316,800. So this work
costs about 17.8 KB of flash (image +17,840) and 624 bytes of RAM (`.data` + `.bss`).

(Before that, 2026-11-17 step 2: image 316,080, `.text` 207,057,
`.rodata` 35,148, `.data` 55,592 with the console in it, `.bss` 21,352,
stack 119,664.)

Code RAM (IRAM) left between the console's RAM code and the code slot:
55,804 bytes (rwtext ends at 0x40084604); with radio-fit 3,972. With
`store` (round 5): 53,044; with radio-fit and `store` 1,212. Round 6
(the scanline read now in RAM, part of the radio's set-up code in RTC
fast memory): 52,608 for the owner's build, 8,260 with `pad` and
`store` (see "The gamepad radio (pad)").

### Handover check, 2026-11-17 (the owner's test build)

The build without `tour` (above), sha256 `6c1e53c5…5f14`. Last
round's normal build
(step 2) beside it, where there is a number.

| | owner test build | step 2 normal build |
|---|---|---|
| flash image | 333,424 | 316,080 |
| `.text` / `.rodata` | 243,717 / 61,436 | 207,057 / 35,148 |
| `.vectors` + `.rwtext` | 1,024 + 16,900 | 1,024 + 16,900 |
| `.data` / `.bss` | 9,984 / 68,400 | 55,592 / 21,352 |
| stack (deepest use seen) | 118,224 (9,968) | 119,664 (8,944) |
| heap free | 49,616 | 49,616 |
| code RAM left before the code slot | 55,804 | not measured (64 KB slot then) |
| row scroll by keys: frames a second, step avg / max, present avg / max | 29.93; 17,526 / 34,979 us; 15,219 / 32,739 us (30 frames) | 29.91; 18,443 / 25,230 us; 16,407 / 23,159 us |
| scripted drag, 3.4 s | 29.99; 13,755 / 35,171 us; 14,820 / 32,874 us (95 frames, 2 late, 0 crossed) | (no drag then) |
| PADTEST, 16 link buttons | on demand: 297 steps, 39 presents; step 1,152 / 15,601 us; present 4,417 / 14,631 us | (no games then) |
| STARDUST, 10 s | 30.00; step 17,478 / 19,234 us; present 16,869 / 18,615 us (307 frames, 0 late, 0 crossed) | |
| load: PADTEST / STARDUST | 4,760 / 4,419 us | |
| press to first game frame: PADTEST / STARDUST | 24,785 / 24,096 us | |
| leave: both slots wiped | 1,093 / 1,092 us | |
| home scan of the demo card (6 games) | 689 us | |

### Handover check, round 5 (the owner's test build with `store`)

sha256 `88ec89d7…f214`, flashed with partitions.csv.

| | round 5 owner build | round 4 owner build |
|---|---|---|
| flash image (espflash's app size) | 354,432 | 333,424 |
| `.text` / `.rodata` | 260,561 / 62,716 | 243,717 / 61,436 |
| `.vectors` + `.rwtext` | 1,024 + 19,660 | 1,024 + 16,900 |
| `.data` / `.bss` | 10,100 / 68,848 | 9,984 / 68,400 |
| stack (deepest use seen) | 117,656 (12,896) | 118,224 (9,968) |
| heap free | 49,616 | 49,616 |
| code RAM left before the code slot | 53,044 (1,212 with radio-fit) | 55,804 (3,972) |
| start: pairing CRC, table, load, prepare | 5,026 us | none |
| row scroll by keys: frames a second, step avg / max, present avg / max | 31.75; 19,830 / 34,517 us; 17,257 / 31,966 us (30 frames, 0 late) | 29.93; 17,526 / 34,979; 15,219 / 32,739 |
| STARDUST, 10 s | 29.99; step 17,034 / 23,555 us; present 16,425 / 18,118 us (305 frames, 1 late, 0 crossed) | 30.00; 17,478 / 19,234; 16,869 / 18,615 |
| press to first game frame (STARDUST) | 23,748 us | 24,096 us |

The same sources in the default build (with `tour`), 2026-11-17: 50
starts and leaves without a restart (25 PADTEST, 25 STARDUST), 50 of 50
came home, heap and deepest stack the same before and after, 0 words
left in the slots, 6 late, 0 crossed, first frame 23.7 to 26.0 ms.
A 5-minute tour (about 310 s):
2,224 presents, 35 late, 9 crossed, 0 untimed, row scroll 29.96 frames a
second (the builder's run 2,130 / 33 / 6, round 4's 2,110 /
29 / 6). Board pictures after scripted input equal the simulator's
(0 of 76,800 pixels) for a drag, the home screen, the menu at step 0,
PADTEST and its pause menu; not for a quick flick (see "Touch values
and why").

### Handover check, round 6 (the owner's test build, no `pad`)

sha256 `db277bf9…2397`, flashed with partitions.csv and left on the
board. Changed since round 5: the scanline read is in RAM (code RAM
left 52,608, was 53,044), the pairing CRC moved to its own file, and
the `screen` log line has `anchor_miss`. Pairing area CRC unchanged,
brightness 1 (saved, 8 records), survey 8,473 reads kept, 0 rejected,
17 us. Row scroll by keys: 31.66 frames a second, step 19,388 /
33,573 us, present 16,952 / 31,162 us (30 frames). STARDUST 10 s: 30.00,
step 16,969 / 18,621 us, present 16,412 / 18,058 us (307 frames, 1
late, 0 crossed); first frame 23,679 us after the press (round 5:
23,748 us).

### Handover check, round 6 (the owner's test build with `pad`, the switch-off fix)

`demo-card,touch,link,store,pad` (no tour), built in its own target
folder, sha256 `c4b02f6b…49149f7`, flashed with
partitions.csv and left on the board. The same build without `pad`
(fallback): sha256 `6f086631…ee5be30`. Sizes (with / without `pad`): app
807,472 bytes of 4,063,232 (19.9 %) with; `.text` 592,513 / 260,637,
`.rodata` 124,864 / 62,844, `.rwtext` 24,076 / 20,096, `.data` 15,612 /
10,100, `.bss` 76,380 / 68,880, stack 38,068 / 117,624. On the board:
pairing area CRC unchanged at every start, the host's 13 set-up
commands (the last: page scan only), gate refusals 0; code RAM left
8,260 bytes, heap free 10,300 (lowest 10,292), stack deepest 18,608 of
38,068.
Brightness was 0 (changed by hand before the run); set back to step 1
with the injected pad (Home, right, Home), saved, and still 1 after a
restart. By the injected pad: Home moves, key repeat (right held 1.5 s
went 0 to 5), the menu, PADTEST with every input and the pad gone
mid-game (buttons let go, NO MOTION); a tap and BOOT (letter k)
worked. 50 STARDUST start-and-leave cycles with the injected pad at
100 reports a second: 52 starts, 0 errors, 1 crossed window, heap
free 10,300 the whole time. STARDUST 10 s: radio waiting 30.00 frames
a second, step 17,181 / 18,743 us, 3 late, 0 crossed; injected pad at
100/s 30.00, step 17,633 / 19,179 us, 3 late, 0 crossed.

Tour, pad against no pad (the default build with the tour, built from
the same sources, 180 s each): with `pad` (app 808,736 bytes) and the
radio waiting, row scroll 30.13 frames a second, step 17,706 /
36,090 us, 28 late and 6 crossed windows; with the injected pad at
100/s, scroll 30.00, step 16,633 / 36,252 us, notice step max
31,780 us, 31 late, 7 crossed, heap free 10,300 the whole time.
Without `pad` (app 356,224 bytes): scroll 29.92, step 17,195 /
35,375 us, 17 late, 0 crossed. STARDUST 10 s: 30.00 with
and without (step 17,167 / 18,843 against 17,006 / 18,669 us; 3 and 1
late, 0 crossed). So the radio costs no frames, but a few tour windows
cross the beam with it (6 or 7 of about 5,200) and none without; a
crossed window shows as a short tear, not a lost frame. Not chased
this round. The owner build went back on the board afterwards: Home,
brightness 1 (saved), pad waiting, pairing CRC unchanged.

## Settings in flash (store, round 5)

What is kept: the brightness step (crates/settings-store: 16-byte
records with a CRC, two 4 KB sectors used in turn, so a reset in the
middle of a save leaves the old or the new value).

Where: the "settings" partition of partitions.csv, data / "undefined",
0x3FE000, 8 KB: the last 8 KB of the 4 MB chip, far from nvs (the
saved gamepad pairing, 0x9000..0xB000) and from the program.

The checks, before any write:

- At start the partition table is read one 32-byte line at a time
  (`settings_store::area::Entry::parse`) and handed to `find_area`: the
  partition must be named "settings", data / undefined, exactly 8 KB, at
  0x3FE000, on a sector, not read-only, not encrypted, above the table,
  inside the chip and clear of every other partition. Otherwise there is
  no store at all: one warning line at start, and each later change of
  the brightness logs `settings: not saved (Area)` three times (the
  first try and two tries again); nothing is written.
- `AreaStore` refuses every read, write or erase that is not inside the
  area, and `EspFlash` checks the chip address once more before the
  flash is called. A write never erases; an erase is one 4 KB sector,
  never a 64 KB block.
- nvs is never written. At start the firmware logs a CRC-32 of the
  pairing's 8 KB (never the bytes): the `pairing area crc32=` line was
  the same on every boot of this round.

When it saves (`settings_store::policy`): 2 s after the last change, or
at once when the menu closes; never while a game runs unpaused (a save
that comes due then waits until the game is paused or left). A failed
save is logged and tried again after another quiet time, at most twice
in a row, then at the next change; the console never waits for it.

At start the saved step is read (about 5 ms with the pairing CRC), the
spare sector is erased if it will be needed soon, and the backlight
gets the step before the console's first step.

Measured on the board:

| | |
|---|---|
| one record write | 36 us average, 118 us largest (438 saves, letter `w`) |
| one sector erase | 29.0 ms, once every 256 saves |
| a whole save | 122 us average; 29.1 ms with an erase |
| start (pairing CRC, table, load, prepare) | 4,976 us |
| change, close the menu, reset | 50 of 50 came back (light level and PWM duty), then 10 of 10 on the last build |
| reset at a random moment after a change | 40 of 40, then 30 of 30 on the last build: old or new value, home every time |
| pairing CRC | the same on every boot |
| 5-minute tour | 2,130 presents, 33 late, 6 crossed, scroll 30.16 fps (before: 2,110, 29, 6, 30.14) |

Second reader's check (same day; the log files named below are kept
outside the repository):

| | |
|---|---|
| reset (EN) at a random moment 0 to 75 ms into a burst of saves (letter `w`) | 100 of 100: old or new value, home, pairing CRC the same; 14 starts found a half-erased spare sector and erased it again (`cuts-100.log`) |
| about 2,000 saves (8 rounds: a menu change, a `w` burst of about 255 saves, a reset) | 2,041 saves, 8 erases, 0 failures, the value set in the menu came back after every reset (`wear2000.log`; checked every ~255 saves) |
| longest stop of the loop by a save | 0.43 ms for a normal save; 27.1 to 27.4 ms when the save must erase a sector first (once every 256 saves, only when more than 128 saves happen without a restart, as `prepare` erases at start) |
| a save while STARDUST runs (pause, one step down, resume) | saved at once on resume in 431 us; 0 late, 0 crossed (`stardust-save.log`) |
| old build (default table) over the new one, then the new one again | the old build starts; the new one without the table has no store and writes nothing; with the table again the saved value is back; the pairing CRC is the same throughout (`old-*.log`) |
| PC: every cut point through 800 saves and 2 erases, rubbish in the area, bit flips, a sector that will not erase, a worn slot | `crates/settings-store/tests/second_reader.rs`, 8 tests; 9 of 10 code changes ("mutants") caught, the 10th makes no difference |

Known limit (pinned by a test, not fixed): a record number near
4,294,967,295 stops saves. It cannot come from saves (that is 4 billion
saves), only from rubbish in the area that has a valid CRC by chance
(about 1 in 4 billion per slot). The fix needs a format change.

What it costs (same features with and without `store`):

| | cost |
|---|---|
| flash image | +19,008 bytes (336,560 to 355,568) |
| code RAM (`.rwtext`; esp-storage runs from RAM) | +2,760 |
| `.data` + `.bss` | +116 + 224 |
| deepest stack at start | 12,896 (9,968 without; esp-storage keeps a 4 KB buffer in its frames) |
| start-up | about 5 ms |

## Games (games.rs, 2026-11-17)

The slot numbers come from one place, `game_api::layout`: build.rs
writes the linker's numbers from it (`write_linker_symbols`), and
`games/build-chip.sh` links the games with the same text. At start-up
the firmware reads the slots back from the linker's symbols, compares
them with the loader's `Slots::CONSOLE`, and logs one line:

```text
INFO - game slots ok: code 0x40092000 + 57344 bytes, spill 0x400a0000 + 49152 bytes, data 0x3fff4000 + 49152 bytes (linker = game_api::layout)
```

If they differ, it logs `game slots DIFFER` and every start is refused
(GAME SLOTS NOT READY). The spill (code past the code slot) is in
"Code past the code slot" below.

- **Start** (`Board::start_game`): `game_loader::load` on
  `/CONSOLE/GAMES/<folder>/GAME.BIN` (demo card or SD card, the same
  code), then ISYNC, the entry address to a `GameEntry` and
  `game_api::open_game` with the console's `ConsoleApi`. The `Loaded`
  and the entry address live only inside that call. A refusal shows the
  loader's own notice (`StartError::Refused`), for example NOT A GAME
  FILE or GAME FOR OTHER CODE SLOT; a missing file shows GAME NOT FOUND.
- **End** (`Board::end_game`): both slots are set to zero (code word by
  word). Chosen over "forget only" because it costs 1.09 ms once per
  game, nothing of one game is left for the next, and a stray jump into
  the code slot meets the all-zero word, which is ILL (illegal
  instruction) on this core, instead of old code. A load that failed
  after writing also wipes, and so does start-up. `boardctl text z`
  counts what is not zero.
- **Abort and panic**: unchanged. A game's panic calls `abort`, which
  panics in the firmware: esp-backtrace prints "game aborted: panic,
  source line N" and a backtrace, then the chip stops with interrupts
  off. It does NOT restart (esp-hal turns the watchdogs off); the RST
  button or `boardctl reset` brings it back. Measured with a test game
  (not on the demo card).

Measured on the board (default build, demo card):

| | PADTEST | STARDUST |
|---|---|---|
| file | 11,484 bytes | 10,516 bytes |
| load (card reads, checks, slot writes) | 4.69 to 4.76 ms | 4.33 to 4.35 ms |
| entry + table check | 25 us | 21 us |
| press to first game frame on the panel | 23.2 to 24.9 ms over 40 starts of both (load + the first step, which draws and sends the whole screen) | |
| frames | on demand (it sleeps when nothing changes); 28.8 a second while buttons change | 30.00 a second |
| step avg / max | 1,496 / 17,115 us (buttons changing) | 16,743 / 18,458 us |
| present avg / max | 4,886 / 16,252 us | 16,251 / 17,972 us (whole screen every frame) |

Start and leave (pause menu, BACK TO HOME) 20 times each without a
restart: 40 of 40 started and came back home; heap free 49,616 and
deepest stack 9,264 the same before, during and after; 0 words left in
the slots. PADTEST's panel picture is the
same as the simulator's for the same inputs, 0 of 76,800 pixels
different (`boardctl compare`).

## Code past the code slot (stage 1, 2026-11-21)

**Checked on the board on 2026-11-22** (the CYD, the owner's two-core
build with the gamepad): every step passed. The numbers are under "The
probe on the board" at the end of this section.

### In plain words

A game's code may run on past the 56 KB code slot, from 0x400A0000 (the
**spill**, `game_api::layout::CODE_SPILL_ADDR`). That address range is
SRAM1 seen from the instruction bus: the same RAM as the data slot
(0x3FFF4000..0x40000000), but from its top down and in reverse word
order. ESP-IDF says instruction word I is data word
`0x40000000 - 4 - (I - 0x400A0000)` (`layout::sram1_iram_to_dram`); that
order was seen to hold on the board (mirror check ok, 2026-11-22).

- The game's code past the slot takes the data slot from the top; its
  data and zeroed variables fill it from the bottom. The rule is
  spill + data + zeroed <= 48 KB. The game build (games/chip/game.x)
  stops a game that breaks it; the loader refuses such a file with
  GAME TOO BIG FOR SLOTS before it writes anything.
- No header or version change: a file says only how much code it has.
  Games that fit the code slot are byte-identical to before.
- The spill, like the code slot, is only read and written as whole
  aligned 32-bit words (src/games.rs).

### What the firmware does

- `game-slots.x` reserves 0x400A0000..0x400AC000 as `.game_spill`
  (NOLOAD, no bytes in the image) right after `.game_code`, and stops the
  link if the spill does not follow the code slot, is not at 0x400A0000,
  is bigger than the data slot, or the data slot does not end at
  0x40000000. `build.rs` writes `GAME_SPILL_ORIGIN` and
  `GAME_SPILL_LENGTH` from `game_api::layout`.
- **Mirror check at every start-up** (`spill_mirror_check` in
  src/games.rs), before the first wipe and before home lends the data
  slot: every data slot word gets a pattern made from its own address;
  every spill word is read from the instruction side and must show the
  pattern of the data word the formula names. Only then are three words
  (first, middle, last) written from the instruction side and read back
  from the data side; each step's stores end with MEMW before the other
  side reads. If any of it is wrong, the spill is turned off
  (`spill_size = 0`): games past the code slot are refused with GAME CODE
  TOO BIG, every other game runs as before. A wrong read step has written
  nothing on the instruction side; a wrong write step has written up to
  three words there (they are data slot words, so the wipe right after
  clears them).
- **Every load**: the loader writes the spill words, then the data and
  zeroes, then reads the spill again and compares its checksum (GAME CODE
  OVERWRITTEN if it changed). `ChipSlots` also stops the chip (assert) if
  data would be written into the part of the data slot this load's code
  took.
- **Every wipe** (start-up, game end, failed load): the data slot is
  zeroed as before, which is also the spill; then the spill is read from
  the instruction side and the words not zero are counted (0 is right).
  This read is skipped only when the mirror check's read step failed
  (then the instruction side is not known to be the data slot).
- The timed screen paths are untouched; hot-check passes.

### Log lines

Start-up (measured on the board, 2026-11-22: the mirror check takes
1,919 us, the wipe 1,095 us):

```text
INFO - game slots ok: code 0x40092000 + 57344 bytes, spill 0x400a0000 + 49152 bytes, data 0x3fff4000 + 49152 bytes (linker = game_api::layout)
INFO - game slots: spill mirror ok, 12288 words, I 0x400a0000 = D 0x3ffffffc, in N us
INFO - game slots wiped in N us; spill words not zero after wipe: 0
```

If the order is not the expected one, instead of the second line (for
example, if whole 16 KB blocks were reversed but the words inside a
block were not):

```text
WARN - spill mirror WRONG: I 0x400a0000 shows D 0x3fffc000 (expected 0x3ffffffc); N of 12288 words wrong; nothing written on the instruction side
WARN - game slots: spill OFF; games with code past the code slot will be refused
```

A mirror that is not reversed at all (I 0x400a0000 = D 0x3ffe0000, the
start of SRAM1, below the data slot) prints `... shows memory outside the
data slot (word ..., expected D ...)` instead; a wrong write step prints
`... writing I ... did not change D ...; up to 3 words written on the
instruction side`.

Games (numbers MEASURED ON THE PC from the files; times are the board's):

```text
INFO - game SPILL: started, file 70180 bytes in N us (code 16672 words, 2336 past the slot, data 3456 bytes, zeroed 36324), entry 0x... opened in N us
INFO - game SPILLMAX: started, file 101884 bytes in N us (code 24476 words, 10140 past the slot, data 3944 bytes, zeroed 1028), entry 0x... opened in N us
WARN - game SPILLBAD: GAME TOO BIG FOR SLOTS (CodeMeetsData { spill: 9344, data: 39812, room: 49152 }); N us; slots written: 0 code words, 0 data bytes
WARN - game SPILLBIG: GAME CODE TOO BIG (CodeTooBig { size: 106500, room: 106496 }); N us; slots written: 0 code words, 0 data bytes
INFO - game ended; slots wiped in N us; spill words not zero after wipe: 0
```

Serial letter `z` (`boardctl text z`):

```text
INFO - game slots: C code words, D data words and S spill words not zero (running=...; at home the data slot holds home's list and pictures)
```

At home the data slot (and so the spill) holds home's list and pictures,
so D and S are not zero there; while no game runs C must be 0.

### The probe on the board (done 2026-11-22)

All measured on the CYD with the owner's two-core build with the
gamepad and `CYD_DEMO_EXTRA` (a test build; the board was left on the
build without it). Every step below passed.

- Start-up: `spill mirror ok, 12288 words, I 0x400a0000 = D 0x3ffffffc,
  in 1919 us`; wiped in 1,095 us with 0 spill words left; the pairing
  area's CRC unchanged; `fastest_read_us=17 calibrated=true`.
- SPILL (2,336 words past the slot) and SPILLMAX (10,140 words past it)
  start and pass: 62 check lines, all `check=PASS ... data=PASS`, the
  screen shows BAD 0. Loads take 30,204 us and 49,681 us.
- SPILLBAD and SPILLBIG are refused in 495 us and 414 us with their
  notices on the screen and `slots written: 0 code words, 0 data bytes`.
- CAMBER and MERCATOR start as before (loads 22,604 us and 29,813 us),
  also right after SPILLMAX and SPILL. After every game `spill words not
  zero after wipe: 0` (wipe 1,103 to 1,109 us); `text z` at home: 0 code
  words.
- Speed, `step_avg_us` of the same loop over 10 s (231 to 246 frames):

  | | A slot, no data | B spill, no data | D slot, data | C spill, data |
  |---|---|---|---|---|
  | SPILL | 12,578 | 12,569 | 16,067 | 16,172 |
  | SPILLMAX | 11,430 | 11,439 | 14,935 | 15,440 |

  Code in the spill against code in the slot: -0.07 % and +0.65 %
  (SPILL), +0.08 % and +3.4 % (SPILLMAX).
- Seen once and not looked into: the last statistics window (after 15
  screenshots and three games in a row) shows `late=1` of 286 presents;
  every earlier window shows `late=0`.
- The log cuts lines at 96 characters, so `bad_words=` and `fails=` of
  the probe's line are not in the log.

The steps as they were run:

The probe is `experiments/spill-probe` (its README has what it proves
and the PC results). Build a test firmware with the four files on the
demo card. Never hand this build over for use (`CYD_DEMO_EXTRA` is a
measuring switch):

```bash
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh
cd boards/cyd
GAMES_TARGET=$PWD/target ../../games/build-chip.sh
F=$PWD/target/spill-probe/chip-games/files     # experiments/spill-probe/README.md builds them
CYD_DEMO_EXTRA="SPILL=$F/SPILL.BIN;SPILLMAX=$F/SPILLMAX.BIN;SPILLBAD=$F/SPILLBAD.BIN;SPILLBIG=$F/SPILLBIG.BIN" \
  cargo build --release --offline --no-default-features --features demo-card,touch,link,store,core2,pad,boot-gather
python3 hot-check.py target/xtensa-esp32-none-elf/release/pocketme-cyd --cores two
```

Home sorts by name, so the row is (index from 0): CAMBER 0, HAIRPIN 1,
KESTREL 2, LANDER 3, LANTERN 4, MERCATOR 5, PADTEST 6, ROOFTOP 7,
SKIMMER 8, **SPILL 9, SPILLBAD 10, SPILLBIG 11, SPILLMAX 12**, STARDUST
13, TUNDRA 14. The four test tiles have TAG TEST FILE. After RST the row
starts on CAMBER; check the chosen tile with `boardctl shot` before A.
Every boardctl call under the board lock
(`flock <checkout>/.local/board.lock timeout 150 boardctl ...`).

1. Flash (with `--partition-table partitions.csv --flash-freq 80mhz`), `boardctl log
   --reset`: the three start-up lines above, `pairing area crc32=`
   unchanged, `panel:` with `fastest_read_us=17` and `calibrated=true`.
2. SPILL: `key right` 9 times, `key a`. The start line above, then the
   game's `spill: fns=228 below=219 above=9 check=PASS ... data=PASS
   bad_words=0 fails=0` once a second and a big PASS on the screen. The
   phases: `spill phase=A fn=0x40093ef0 data=no`, B `fn=0x400a0a6c`,
   C (B with data reads), D (A with data reads), 10 s each (addresses
   MEASURED ON THE PC from SPILL.elf). For speed: `text r` at a phase's
   start, `text m` near its end, `perf ... step_avg_us`. Leave: `text h`,
   `key down`, `key down`, `key a` (BACK TO HOME); `game ended ... spill
   words not zero after wipe: 0`. Then `text z` while at home.
3. SPILLBAD (`key right` to index 10, `key a`): GAME TOO BIG FOR SLOTS on
   the screen, the WARN line above with `slots written: 0 code words, 0
   data bytes`; the console stays usable.
4. SPILLBIG (index 11): GAME CODE TOO BIG, the same `slots written: 0`.
5. SPILLMAX (index 12): as SPILL with `fns=350 below=201 above=149`;
   phase A `fn=0x40094e28`, B `fn=0x400a8464`.
6. SPILL again after SPILLMAX, and a normal game (for example STARDUST,
   13) after both: each must start and pass (loads over each other).
7. End with the newest playable build on the board (without
   `CYD_DEMO_EXTRA`).

## Game flash parts (game-flash, 2026-11-22)

**On the board since 2026-11-23** (MERCATOR's part; see "On the
board" below). The numbers in "What moved" are MEASURED ON THE PC (the
ELF, the linker, `espflash save-image`, which writes a file and opens
no port) on 2026-11-22, with MERCATOR's first flash part.

### In plain words

A game may keep part of its code and constant data in the firmware's
flash instead of in the 56 KB code slot and the 48 KB data slot. The
game lists those functions and constants in `games/<name>/flash.list`;
`games/build-chip.sh` links it twice, cuts the listed parts out of the
game file and writes them as `flash/<NAME>/CODE.BIN` and `RO.BIN`
(`games/README.md`, "Flash parts"). The firmware built with `game-flash`
puts those bytes at fixed addresses (the game's **place** in
`game_api::layout::FLASH_PLACES`) inside two **windows** at the very
start of its flash code and data:

| window | address | size | what |
|---|---|---|---|
| code | 0x400D0020 (`layout::FLASH_CODE_ADDR`) | 128 KB | flash code, read by the instruction fetch through the cache; whole aligned words only |
| data | 0x3F400120 (`layout::FLASH_RO_ADDR`) | 64 KB | flash read-only data, any byte |

MERCATOR's place (the only one today) is code 80 KB (0x14000) at offset
0 and data 32 KB (0x8000) at offset 0 (raised from 32 KB and 16 KB on
2026-11-23 for the planet landing, and the data again from 24 KB in step
G4 the same day; the windows did not change, so a
firmware's windows and its layout are the same as before; only the
largest part build.rs accepts grew).

0x400D0020 is esp-hal 1.2.2's `irom_seg` origin and 0x3F400120 its
`drom_seg` origin plus the 256-byte app descriptor. Both numbers are
copied into `layout` by hand; `game-flash.x` stops the link if the
windows do not land there. Both sizes are multiples of 16 KB, so the
firmware's own code and data keep their flash cache sets relative to
each other (the hot block is checked by `hot-check.py` as before).

The file says which part it needs: a "GAMF" file has a 32-byte
extension after the header (addresses, sizes and CRC-32s of the code and
data halves, covered by the file's CRC). Before the loader writes
anything, `ChipSlots::flash_part` (src/games.rs) looks for a part this
firmware holds at those addresses and sizes, and computes the CRC-32 of
the bytes **as they lie in flash** (aligned 32-bit loads). The game runs
only when both CRCs match the file's and the ones built in.

The console never writes, erases or remaps that flash. The windows are
part of the firmware image that `espflash flash` writes; nothing else
touches them.

**The accepted limit: a game from the SD card works only when the
firmware holds its part, byte for byte.** Otherwise the console refuses
it with FLASH PART MISSING (no part at those addresses, or a firmware
without `game-flash`) or FLASH PART DIFFERS (a part there, other bytes).
A new MERCATOR build therefore needs a new firmware.

### What build.rs does

With `game-flash`, `game_flash()` in build.rs:

1. writes `game-flash-addresses.x` (the windows, from
   `layout::write_flash_window_symbols`);
2. for every game in `layout::FLASH_PLACES` whose
   `<game files>/flash/<NAME>/CODE.BIN` exists (the game files folder is
   `CYD_GAME_FILES` or `target/chip-games/files`, whether or not
   `demo-card` is on): checks the sizes against the place, and when
   `<NAME>.BIN` lies beside them, that its extension names exactly these
   bytes, else the build stops with "Build the games again". No files: a
   warning, empty windows, and the game is refused;
3. writes `game_flash.rs` (included by src/games.rs): one `#[used]`
   static per half in the input section `.game_flash_code.<NAME>` or
   `.game_flash_ro.<NAME>`, and `HELD`, what the console compares with;
4. writes `game-flash.x`, passed before `hot-block.x` and `linkall.x`:
   `.game_flash_code` in `irom_seg` before esp-hal's `.text` and
   `.game_flash_ro` in `drom_seg` after `.flash.appdesc`, each the whole
   window, the parts at their places, zeros between, and an end mark in
   the last word ("GFCW", "GFRW") so the window has real contents to its
   end and espflash keeps it, and the firmware's own flash code or data
   after it, as one flash segment. ASSERTs stop the link when a window is
   not at the layout's address, does not run straight into `.text` or
   `.rodata_merge`, or a part is not whole at its place.

The demo card's check uses `read_game_header`, so a GAMF file passes it.

**Linker warnings.** The script must name esp-hal's real regions
`irom_seg`/`drom_seg` (its aliases `ROTEXT`/`RODATA` are not declared
yet when the script is read, and naming them is a fatal "redefinition of
memory region alias"). So every build with `game-flash` prints one rustc
warning (`linker_messages`) holding four linker lines: `memory region
'irom_seg' not declared`, the same for `drom_seg`, and `redeclaration of
memory region` for both in esp-hal's `memory.x`. They are harmless: the
ELF is laid out as intended (checked by `game-flash-check.py` and the
ASSERTs).

### What moved (the owner's two-core build with the gamepad, 2026-11-22, MERCATOR's first flash part)

| | before (no `game-flash`) | with `game-flash` |
|---|---|---|
| `.game_flash_code` | none | 0x400D0020 + 131,072 (MERCATOR 19,224) |
| `.text` | 0x400D0020 + 621,581 (base) | 0x400F0020 + 623,733 |
| hot block | 0x40167C40..0x4016CDFC (base) | 0x401884A0..0x4018D65C (20,924 bytes, the same) |
| `.game_flash_ro` | none | 0x3F400120 + 65,536 (MERCATOR 10,524) |
| `.rodata` | 0x3F400120 + 942,576 (base) | 0x3F410120 + 914,096 |
| image (`espflash save-image`) | 1,675,632 bytes, 41.24 % of 4,063,232 | 1,845,952 bytes, 45.43 % |
| code RAM end (`_cyd_iram_end`) | 0x4008FFF4 | 0x4008FFF4 (unchanged) |

"Base" is the firmware of the commit this work started from. `.rodata`
is smaller because the demo card's MERCATOR.BIN is 71,600 bytes instead
of about 100 KB. The flash part check adds 788 bytes of flash code (the
same set without `game-flash`: `.text` 622,945). One core with
`game-flash` builds too (hot-check one-core: ok).

Today's MERCATOR part (2026-11-23) is 81,076 of 81,920 bytes of code
and 25,732 of 32,768 of constants (`games/README.md`, "Flash parts");
the two windows, and so every address above, did not change.

### Log lines

```text
game flash: code 0x400d0020+131072, data 0x3f400120+65536, parts held: 1
game flash: MERCATOR code 81076, data 25732 bytes
game MERCATOR: flash part ok, code 81076 data 25732 bytes, checked in 17049 us
game MERCATOR: flash part DIFFERS: crc32 code 0x........ data 0x........
game flash: no part held at code 0x........+N, data 0x........+N
game flash: windows 0x........ 0x........ DIFFER from layout; flash parts refused
game flash: off; games with a flash part are refused        (without game-flash)
```

The check reads MERCATOR's whole part through the flash cache at each
start of the game: 106,808 bytes today, 17.0 ms (MEASURED ON THE BOARD,
2026-11-23, one start, the line above). `StartReport` keeps it as
`flash_check_us`.

### Build and check (PC)

```bash
GAMES_TARGET=$PWD/target ../../games/build-chip.sh
cargo build --release --offline --no-default-features --features demo-card,touch,link,store,core2,pad,boot-gather,game-flash
python3 hot-check.py target/xtensa-esp32-none-elf/release/pocketme-cyd --cores two
python3 game-flash-check.py target/xtensa-esp32-none-elf/release/pocketme-cyd
```

`game-flash-check.py <elf> [<game files>]` checks the windows (address,
size, straight into `.text` and `.rodata_merge`/`.rodata`), the end
marks, each part byte for byte against `flash/<NAME>/`, and zeros
elsewhere; `--absent` checks that a build without the feature has no
trace of it. `check.sh` runs it on the owner's builds.

### On the board (2026-11-23)

Seen on the board with the owner's two-core build with the gamepad and
`game-flash`: the boot log's `game flash:` lines and `panel:` line
(`fastest_read_us=17`, `calibrated=true`, with the hot block moved),
the `pairing area crc32=` line unchanged, MERCATOR starting with
`flash part ok` and playing: quiet flight in space 4.1 ms a step at
59.99 pictures a second (at the settings the game started with then,
MID detail and the cockpit view; the run at today's starting settings
is in "At the starting settings" below), and the planet's whole visit
(most of it flash code) timed at LOW, MID and HIGH at 30 pictures a
second (those numbers are in working notes kept outside the
repository). The planet's inner loops had to go back to RAM: from
flash they took twice the time (MEASURED ON THE BOARD;
`games/mercator/flash.list` says which). Not checked: every other game
one by one against a firmware without the feature (their files are
byte for byte the same), and one core.

## Flash code probe (xip-probe, measuring only, 2026-11-22)

**Phase A-i was run on the board on 2026-11-22** (one board round,
with the probe build of this tree flashed at 80 MHz and once
at 40 MHz for A1). This section says what the probe build does and how
it was checked on the PC. The board's logs and results are not in the
repository; they are not published, and this file
quotes none of their numbers.

### In plain words

Games today run from RAM (the code slot). A later stage might run game
code straight from flash, through the same flash cache that the second
core's timed screen code uses. Three things decide whether that can
work, and only the board can show them:

- what one flash cache miss costs (A1);
- whether flash code that the first core runs during a frame makes the
  second core's timed screen windows late (A2);
- how much of this the flash bus shared by the two cores adds (A3).

The probe build puts 128 KB of plain probe code into the firmware's own
flash code. The first core runs it on letters from the serial port, so
these questions get measured without a game and without any change to
the game slots. Never hand this build over for use.

### What is in the build

- `src/xip_probe.rs` holds the block, its two trampolines and the
  letters. `app.rs` calls it at two places, both under
  `#[cfg(feature = "xip-probe")]`: once per console step, right after the
  picture is handed to the second core (`frame`), and in the letter
  match (`letter`). Without the feature nothing of it is compiled. A
  build without the feature (the owner's build
  `demo-card,touch,link,store,core2,pad,boot-gather,game-flash` too)
  carries nothing of it; `xip-check.py --absent` checks that in
  `check.sh`.
- The block is 4096 lines of 32 bytes, one flash cache line each, all
  the same: `addi.n a2,a2,-1; bnez.n a2,+4; ret.n` and 13 `nop.n`
  (written as bytes), and one more `ret.n` after the end. It has no
  literal, no call and no absolute jump, so it never reaches out of
  itself. It can be entered at any line, with `a2` = the number of lines
  to run, and every line it runs is fetched whole. It sits in `.text`
  (esp-hal's catch-all), right before the hot block (`.text_hot`, which
  starts where `.text` ends). Its lines go round the 512 cache sets 8
  times.
- The two trampolines (`cyd_xip_probe_run`, `cyd_xip_probe_run_quiet`)
  are in RAM (`.rwtext`). Each reads the cycle counter, calls the block
  with `callx0`, and reads the counter again, so only the block is
  timed. The `quiet` one masks the first core's interrupts (level 5)
  while the block runs. It never masks the second core's. Every timed
  run (A1 and the loads) goes through the `quiet` one in chunks of 1 KB
  (32 lines), so interrupts are masked for one chunk at a time
  (ESTIMATED: about 122 us at 40 MHz, from 32 misses of 3.8 us) and run
  between chunks; the summed chunk times leave the interrupts out, and
  the part's whole time with them (`wall`) is kept beside it.
- No cache flush is used. A part is made cold by running other probe
  code in between: between two runs of any probe line the whole rest of
  the block runs (for every load: parts of 16 KB and more follow on
  without a gap, 8 KB parts are 16 KB apart), that is 7 other lines on
  each of its two-way sets, as in `a`.
- Caches are per core. The first core's probe competes with the first
  core's own flash code (`App::step` alone is about 57 KB and covers
  every set), not with the hot block, which with `core2` is the second
  core's code in the second core's cache. So the start offsets do not
  aim at "the console's sets"; they only show whether where an 8 KB
  part sits changes anything (parts of 16 KB and more touch all 512
  sets whatever the start).
- `xip-check.py <elf>` checks a built firmware on the PC (python3, no
  extra modules; the esp toolchain's binutils on `PATH`). It checks:
  - the block is 131072 bytes, starts on a cache line, lies in `.text`
    and in flash code, and ends before the hot block;
  - its bytes and its disassembly (only the four instructions, 4097
    `ret.n`, every `bnez.n` to its own address + 4);
  - no other function starts inside it;
  - the trampolines are in RAM, instruction by instruction, operands
    included;
  - the layout table `CYD_XIP_PROBE_LAYOUT` ("XIPP", 4096 lines, 32
    bytes, 512 lines a way, the four start lines);
  - nothing in the hot block or in `core2` calls a trampoline.

  It prints each start offset's entry address and first cache set, the
  sets of the first core's main code (`App::step`, the letter path,
  `join`, the probe's own Rust code) and those of the hot block's parts
  (the second core's code). `xip-check.py <elf> --absent`
  checks that a build without the feature has no trace of the probe.
  `check.sh` runs both. `hot-check.py --cores two` must still pass on
  the probe build.

### Letters (first core only; `boardctl text X` or `boardctl log --send X`)

- `a`: A1, the cost of one miss. It first waits until the second core
  holds no job (`joined .. us`), then runs at the current start offset,
  for parts of 4 KB and of 16 KB, 8 times each. Each time: the part once
  (to warm it), the part timed warm, the rest of the block, then the
  part timed cold. Both timed runs are quiet chunks. Per line =
  (cold - warm) / lines. The loop is blocked meanwhile (ESTIMATED: about
  a quarter of a second; the log line `done in .. ms` says how long it
  really took). A run counts only when `presents during it=0`; void it
  if a radio error or a panic shows in its output.
- `0`..`5`: from then on, a load of 0, 8, 16, 32, 48 or 64 KB of probe
  code every console step (A2/A3), run right after the step has handed
  its picture to the second core, in quiet chunks. Each step runs the
  next part of the block. Parts are at least 16 KB apart, so each part
  starts at the same cache set (the start offset's). The letter first
  measures the warm time for that size, then clears the probe's
  counters. The warm time is the fastest of 3 runs; a part over 16 KB
  is worked out from a 16 KB run (`est`).
- `j`: the next start offset, 0, 4, 8 or 12 KB (first sets 149, 277,
  405 and 21 in this build), so an 8 KB part's sets shift against the
  first core's own code. It measures warm again and clears the counters.
- `d`: the next delay, 0, 2, 4 or 8 ms: after a step that handed a
  picture, the part waits that long (a spin) before it runs, to move it
  over the second core's send. It measures warm again and clears the
  counters.
- `r` also clears the probe's counters (`xip: counters reset`), and `m`
  also prints its five lines before the console's own report. Both
  keep doing what they did before.

These letters change only RAM: a restart (`boardctl log --reset`) puts
them back to load 0, start 0, delay 0. Send them after the restart.

### Log lines (each under 96 characters with the `INFO - ` prefix)

```text
xip: block 0x401692a0+131072 (4096 lines of 32 B) set0=149, letters a j d 0-5
xip: load: load 16 KB/frame, start 0 KB (set 149), step 16 KB, delay 0 ms
xip: warm=.. cyc (measured|est from 16 KB), counters reset
xip a1: start 0 KB (set 149), parts 4+16 KB, 8 repeats, joined .. us
xip a1 4K warm_cyc=lo/hi cold_cyc=lo/hi (min/max)
xip a1 4K line_ns min= avg= max= = (cold-warm)/128 lines
xip a1 16K ... (the same for 16 KB, 512 lines)
xip a1: done in .. ms, presents during it=0
xip load=16K start=0K step=16K delay=0ms next=.. frames=.. handed=..
xip parts=.. notcold=.. (below .. cyc)
xip cpu1=.. c2busy=../.. (parts in a send job at start/end)
xip cyc min=.. max=.. warm=..(est) a1_line=..
xip us avg=.. max=.. wall=avg/max line_ns=..
```

The longest of them, with its largest possible numbers, is 94
characters with the prefix (MEASURED ON THE PC: a python3 count of the
format strings filled with the largest values).

The first line comes once, at the first console step. Its address is
the probe build's (MEASURED ON THE PC with `xip-check.py`) and must match
on the board.

The fields of the `m` lines:

- `frames`: console steps seen. `handed`: steps whose picture went to
  the second core.
- `parts`: parts run (must equal `frames` while a load is on).
  `notcold`: parts faster than warm plus, per line, half of A1's 16 KB
  miss cost (`a1_line`), or 120 cycles before any `a` (ESTIMATED: half
  of the fastest miss estimate, 1.05 us at 80 MHz QIO). It must be 0,
  or the parts were not cold. `cyc min` allows a stricter rule by hand.
- `cpu1`: calls on the second core. It must be 0.
- `c2busy=start/end`: parts that began / ended while the second core
  still held the picture job. They show whether the load overlapped the
  send at all; `d` moves it.
- `cyc`: a part's summed chunk times in cycles; `a1_line` is A1's 16 KB
  cost per line in cycles (0 before any `a`).
- `us avg`/`max`: the same in microseconds (interrupts left out).
  `wall`: the part's whole time with the calls and the interrupts
  between chunks, average and largest.
- `line_ns`: (average - warm) / lines.

Read them beside the console's own `perf`, `send` (`late`, `crossed`,
`lead_us`, `wait_us`, `send_us`) and `core2` lines of the same `m`.
`perf`'s `step_avg_us`/`step_max_us` leave the probe out (it runs after
the step's end time is taken); they show only its indirect effect (the
console's own code made cold). The probe's own cost to the frame shows
in fps and in the `send` lines.

What A-i cannot show: the second core's timed path is the hot block,
which by design has no three lines in one set, so during these runs the
second core runs warm. A clean A2 shows that the first core missing
does not hurt a warm second core; it does not show that both cores
missing at the same time is harmless. That case needs a later phase.

### Build and check (PC)

```bash
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh
cd boards/cyd
GAMES_TARGET=$PWD/target ../../games/build-chip.sh
cargo build --release --offline --no-default-features \
  --features demo-card,touch,link,store,core2,pad,boot-gather,xip-probe --target-dir target/xip-probe
python3 hot-check.py target/xip-probe/xtensa-esp32-none-elf/release/pocketme-cyd --cores two
python3 xip-check.py target/xip-probe/xtensa-esp32-none-elf/release/pocketme-cyd
```

Sizes, MEASURED ON THE PC (`xtensa-esp32-elf-size -A`, 2026-11-22,
compared with the owner's build):

- `.text` grows by 140,012 bytes: the 131,074-byte block and about
  8.7 KB of probe code; `.rodata` by 904 bytes.
- `.rwtext` grows by 72 bytes (the trampolines), `.data` by 32 and
  `.bss` by 80.
- The main stack shrinks by 112 bytes (29,088 instead of 29,200).
- The hot block keeps its size (654 lines) but moves, so its cache sets
  differ from the owner's build (start set 246 instead of 479). Compare
  the probe's loads only with the probe build's own load 0, never with
  the owner's build.

Flash with `--flash-freq 80mhz` like every build. For the A1 comparison,
flash once more at 40 MHz (leave the option out). Every board command
goes under the board lock.

### Not done

What ran on the board in phase A-i (2026-11-22): `a` (A1) at 80 MHz
and at 40 MHz; the loads `0`..`5` on the home screen, in KESTREL and in
LANTERN at 60 pictures a second, each at least twice; three start
offsets (`j`: 4, 8 and 12 KB) with the 8 KB load in LANTERN; and three
delays (`d`: 2, 4 and 8 ms) with the 16 KB and the 64 KB load in
KESTREL. Its numbers stay outside the repository (see the top of this
section). Not done: both cores missing at the same time (a later
phase). The probe uses no flush and does not measure the second core's
misses directly; it shows them through the console's `send` and
`core2` lines.

## The gamepad radio (pad, round 6)

`pad` replaces `radio-fit`. It puts the owner's gamepad (GuliKit
KingKong 2 Pro, classic Bluetooth, Switch mode saved) into the console
through crates/pad-host, the reconnect-only gamepad host:

- At start the saved pairings are read, **read only**, from the
  pairing area (0x9000..0xB000, the first 8 KB of "nvs") and handed to
  the host; the radio starts in dual mode (the patched esp-radio of
  experiments/bt-classic-probe); the host sets the controller up and
  turns on page scan only: connectable for a saved pad, not
  discoverable. One log line per stage (`pad: pairings loaded`, `pad:
  radio up`, `pad: host starting`, `pad: waiting for a saved pad`),
  never a key, an address at most as its last two bytes.
- The firmware never searches (no inquiry, no inquiry scan), never
  starts a connection, accepts no pairing, and accepts a call or
  answers with a key only for a saved pad's address. hci-rules' gate
  checks every packet before it reaches the radio.
- **A pad switched off is gone within about a second** (round 6 fix,
  below: "A pad switched off and on").
- The pairing area is never written or erased: its CRC is logged at
  every start, and stays the same on the dev board (see src/pairing.rs
  for why no code path can write it).
- The pad is `board_api::Pad` (`CydPad` in src/board.rs): buttons,
  sticks, triggers and motion from the host while a pad is connected,
  with the cable keys (the debug link's `key`, the BOOT button) always
  ORed in; `take_pressed` keeps a press shorter than a frame. The
  console wakes only when what a game sees changes (buttons, sticks
  out of the dead zone, connect, disconnect): a resting pad's 100
  reports a second never wake it. Battery goes only to `stats` and the
  log.

### Where things are

- `hci-rules/` (its own crate: no_std, no unsafe, PC tests, checked by
  check.sh): packet lengths, the **gate** (only what the reconnect-only
  host needs; an accept, a key reply or a name request only for a saved
  address; a supervision timeout only for the handle of a saved pad's
  link, which the gate notes from the controller's events, `observe`),
  and `BdAddr`, which prints only its last two bytes.
- `src/hci.rs`, `HciPort`: the byte path. `receive()` hands out one
  whole packet or `None`, never waits; `send(packet)` passes the gate
  and gives the packet to the controller, or says `Busy` (nothing
  sent) or `Refused`; `wait()` sleeps until the controller has
  something (safe to drop in a `select`).
- `src/pairing.rs`: the pairing area's CRC and `load` (feature `pad`):
  a reader that can only read, and only inside 0x9000..0xB000; the gate
  filled with the saved addresses.
- `src/pad_radio.rs`, `PadRadio`: runs pad-host from the console's
  loop. `poll` (once per turn, before the input is looked at) sends what
  waited, shows every queued packet to the gate (`observe`) and hands
  it to `PadHost::packet`, runs the
  host's timers (every 15 ms while busy, once a second while it only
  waits), sets the pad's wake flag, logs connect, gone and battery, and
  the `pad` line every 5 s when it changed. With `link` it also plays an
  **injected pad** (`boardctl pad`, below).
- `src/main.rs`: the host (19,704 bytes in the build of 2026-11-23) is
  copied onto the heap from a
  constant in flash (never built on the stack) before the radio starts.
- `rtc-fast-phy.x`: the linker part that frees code RAM (below).

### The `pad` line (stats and the log)

`boardctl stats` and the 5-second log show

```text
pad state=waiting mode=- rps=0 drop=0 gap_ms=0 wait_us=0 tries=0 fail=0/0/0/0 lost=0 bat=- inj=0 heap_min=10364
```

`state` off / waiting / connecting / starting / ready; `mode` switch,
xinput, android or dinput; `rps` reports in the last second; `drop`
packets that could not go out; `gap_ms` the longest gap between two
reports; `wait_us` the longest a packet (or an injected report) waited
for the loop; `tries` calls taken up and `fail` the tries that ended
early, by the furthest stage reached (accepting the call / security,
key and encryption / opening the HID channels / waiting for the first
understood report); `lost` links lost;
`bat` percent (`+` charging); `inj` injected reports. The log's line
adds the counters behind them (`ok silent gate_links reports undecoded
wakes in out queued refused ready_ms`; `silent`: pads dropped by the
silence guard, `gate_links`: saved pads' links the gate knows are up). Letter `r` zeroes them with the rest of the stats.
The stats frame is at most 1,024 bytes: the `pad` line (about 120
bytes) comes before the `perf` lines; when many scenes have numbers the
last `perf` lines are cut (`(cut: frame full)`), as before `pad`.

### Playing a pad from the PC (`boardctl pad`, with `link`)

`boardctl pad connect switch`, `tap right`, `press east`, `tilt 2048
-1024`, `spin 0 224 0`, `repeat 10` (a resting pad's report every 10 ms
until `repeat 0`), `off`, or `boardctl pad script FILE` (one step per
line, see tools/boardctl/src/pad.rs). The reports are real Switch full
reports, built by `pad_host::inject` and decoded by the same decoder,
press latch, wake rule and motion mapping as the real pad's. Nothing
goes over the radio. A real pad that connects ends the injection.

### Where the host runs: in the console's loop

The controller runs in its own esp-rtos task (the driver's), from its
interrupts. Each packet it has for the host is copied to the heap and
queued by the driver at once, so the controller never waits for the
host. The host itself (`PadRadio::poll`) is called by the console's
loop, once per turn, before the input is looked at, and the loop also
wakes when a packet comes.

Why there and not in a task or interrupt of its own:

- Nothing is lost while a frame is drawn: the driver's queue holds the
  packets (measured below: 0 lost at 100 a second, at most 4 waiting).
- The console reads the pad once per step. A report read earlier than
  the step before it gains nothing: whatever came during a frame is
  read just before the next step looks at the pad.
- No second task or interrupt can cut into the present's timed windows
  (tear-free timing): the host's work happens between frames.
- esp-rtos 0.4 has no public way to start another thread; an
  interrupt executor would run the host in an interrupt, inside the
  present's timing.

The radio's own times (a set-up command, the load test's sends) bring
the loop round to `poll` but never make the console step.

### Code RAM

The radio's libraries need about 47.9 KB of executable RAM, which left
only 1,212 bytes below the game's code slot with `store` (round 5). The
slots stay where they are (code 56 KB at 0x4009_2000, data 48 KB at
0x3FFF_4000); 0x4007_8000 (the second core's cache area) is not used.
Instead, `rtc-fast-phy.x` moves the radio chip's set-up code (libphy's
`.phyiram` code and the `.iram1` code of phy_chip_v7.o, 7,636 bytes)
into the RTC fast memory: 8 KB of executable RAM at 0x400C0000 that
nothing else here uses. Only the first core can run code from it; the
console, its interrupts and the radio task all run on the first core.
The bootloader loads it like any other part (`segment ... vaddr=400c0000
load` in its log). Two ASSERTs stop the build if the part grows past
8 KB or if esp-hal's own `.rtc_fast.*` sections (same memory) are ever
used.

Executable RAM, 0x40080000 up to the code slot (73,728 bytes), default
features with `pad` (and `store`), from the linker map and the symbols:

| part | bytes | what | must it be in RAM? |
|---|---|---|---|
| `.vectors` | 1,024 | interrupt vectors | yes (the CPU jumps there) |
| `.rwtext` | 24,076 | all of the following: | |
| - esp-hal gpio | 3,707 | light-sleep wake hooks 3,267 (`gpio::wakeup`), GPIO interrupt handler 440 | the hooks are not used (no light sleep) but esp-hal places them; moving them needs a patched esp-hal |
| - esp-hal spi | 2,553 | the panel's SPI driver (`.cargo/esp-config.toml`) | for the scanline reads: from flash, a cache miss can stretch every read (measured below); the `SpiDma` wrappers above it are in the hot block (flash) |
| - the scanline read | 436 | `Lcd::scanline_stamped_raw` (new this round, see below) | yes |
| - esp-rtos | 5,489 | timer 2,552, task switch 1,563, run queue 1,197, scheduler 177 | yes: runs in interrupts, also while the flash is written with the cache off (`store`) |
| - esp-radio | 3,175 | the controller's OS glue: ble 1,601, common adapter 1,295, interrupt dispatch 279 | yes: called by the controller library from its interrupts |
| - esp-phy | 773 | critical sections for libphy | yes: called with interrupts off |
| - flash writes | 2,713 | ROM flash routines 1,950 (C), esp-storage 763 | yes: run with the flash cache off |
| - interrupt entry, context save, the rest | about 3,200 | xtensa-lx-rt and esp-hal trampolines, console 235 | yes |
| `.rwtext.wifi` | 40,368 | the controller library btdm (about 33,100: baseband interrupts, event scheduling, link control) and the rest of libphy (7,000, used at calibration) | btdm: yes (vendor-placed, timing-critical); the rest of libphy would fit nowhere else (RTC fast has 556 bytes left) |
| RTC fast (not in this RAM) | 7,636 | libphy `.phyiram` and phy_chip_v7.o `.iram1` | moved |
| **left before the code slot** | **8,260** | was 1,212 | |

Without `pad`: 52,608 left (owner's build and default build; 53,044
before the scanline read moved to RAM).

What moving costs, measured on the board (tour of 5 minutes, then
STARDUST 10 s; late / crossed are windows; step and present are
average / longest, us):

| build | radio | code RAM left | tour presents, late, crossed | row scroll step; present | STARDUST step; present; late | first frame |
|---|---|---|---|---|---|---|
| A: default, no radio | off | 53,044 | 2,140; 34; 1 | 17,501 / 34,784; 14,948 / 32,326 | 17,462 / 19,136; 16,848 / 18,505; 2 | 24,172 |
| B: radio, old placement (radio-fit) | started, idle | 1,212 | 2,154; 36; 4 | 15,720 / 35,369; 13,339 / 32,936 | 17,349 / 18,945; 16,684 / 18,276; 2 | 24,249 |
| C: radio, phy code in RTC fast (radio-fit) | started, idle | 8,696 | 2,153; 56; 7 | 16,514 / 35,734; 14,136 / 33,348 | 17,355 / 19,000; 16,690 / 18,330; 2 | 24,171 |
| E: default, SPI driver in flash, 60 s tour | off | 55,800 | 432; 3; 0 | 17,754 / 35,747; 15,126 / 33,077 | 17,223 / 18,868; 16,548 / 18,185; 1 | 23,997 |

The RTC fast move costs nothing that shows: the radio started in 393 ms
(375 to 386 before), STARDUST's times are the same within 10 us, and
the tour's differences are of the size seen between two runs of the
same build (round 5: 33 to 35 late, 6 to 9 crossed). The phy code runs
at the radio's start and calibration, not while frames are drawn.

The SPI driver in flash (E) worked in this build (fastest read 18 us)
and would free 2,756 bytes, but whether it works depends on where the
linker puts things: once before (sd-card build) every read took 41 us
or more. It stays in RAM.

**Found this round: the scanline read must not be in flash either.** In
the first `pad` build, `Lcd::scanline_stamped_raw` (in flash) and its
constants (the literal pool, also in flash) fell on the same lines of
the flash cache: every counter read took 29 us or more, the survey kept
none of 4,279 reads, and the console ran without tear-free timing
(`calibrated=false`, two boots).
In RAM (436 bytes), the same build kept all 8,467 reads, 17 us. The
SPI wrappers it calls (esp-hal's `SpiDma` read and write) stayed in
flash, and later builds showed that their place mattered too (25 us
reads in some layouts); since 2026-11-18 they are in the hot block
("The timed paths and the flash cache"). The boot log's `panel:` line
(`fastest_read_us=`, `calibrated=`) shows at once if a later layout
hits the read anyway.

### Data RAM and start-up (pad build)

| | with `pad` | without (owner's build) |
|---|---|---|
| main data RAM taken by the radio for itself | first 64 KB (0x3FFB0000) | 0 |
| `.data` + `.data.wifi` | 12,548 + 1,012 | 10,100 |
| `.bss` | 74,260 | 68,848 |
| main stack (deepest use seen) | 43,252 (14,032) | 117,656 (12,896) |
| heap 49,616: radio's share, least free seen under load | 19,580; 29,940 | 0 |
| radio start | 380 to 388 ms (six boots) | |
| radio start to page scan on | about 1,047 ms (the set-up only starts with the console's loop, after the panel survey and the console's start) | |
| start: pairing CRC (read only) | the same | the same |

The driver's queue is on the heap: at 100 packets a second the most
found waiting at once was 4, and the heap's low point moved by 96
bytes (30,036 free after the radio's start, 29,940 at the lowest).

Room for the host's own buffers: the stack is whatever data RAM is
left, so every static buffer comes out of it. 43,252 - 14,032 = about
29 KB of the stack were never used; with a margin of 8 KB, about 21 KB
can go to the host's static buffers (the byte path itself has one
1,028-byte packet buffer in `HciPort`, and `poll` copies each packet to
a 1,028-byte stack buffer, both counted above).

### The radio while frames are drawn

The load test (letter `p`, measuring only, removed with the host) sent Read Local Version
every 10 ms, 100 packets a second each way, as many as a pad's reports.
"Wait unread" is from when the command went out to when its answer was
read by `poll` (an upper bound for how long a packet sat unread; the
controller itself needs about 0.7 ms of it). Final build
(default features with `pad`; 5-minute
tours, then STARDUST 10 s; step and present average / longest, us):

| | tour, radio on, no load | tour, load 100/s | STARDUST, load | STARDUST, no load |
|---|---|---|---|---|
| packets sent / answered / lost | 5 / 5 / 0 (set-up) | 30,047 / 30,047 / 0 | 1,024 / 1,024 / 0 | |
| wait unread: least / average / longest | | 725 / 1,322 / 36,949 | 902 / 1,433 / 3,476 | |
| answers that waited over one frame (34 ms) | | 26 of 30,047 | 0 | |
| most packets waiting at once | 1 | 4 | 4 | |
| presents; late; crossed; untimed | 2,158; 23; 1; 0 | 2,122; 49; 9; 0 | 366; 3; 0; 0 | 307; 2; 0; 0 |
| row scroll step; present | 16,463 / 33,316; 13,872 / 30,743 | 17,775 / 36,666; 15,000 / 33,737 | | |
| game step; present | | | 17,556 / 19,248; 16,796 / 18,464 | 17,164 / 18,828; 16,547 / 18,195 |
| loop wakes; early | 2,331; 40 | 2,274; 10 | | |
| heap least free | 29,988 | 29,940 | 29,940 | |

Without the radio (build A above): tour 34 late, 1 crossed; STARDUST
17,462 / 19,136 and 16,848 / 18,505, 2 late. So:

- No packet lost at 100 a second, while frames were drawn and during a
  game.
- A packet waits at most about one frame: the longest wait (36.9 ms) is
  a present of the tour's widest scroll (up to 33.7 ms) plus the step;
  during STARDUST, never more than 3.5 ms.
- Tear-free timing: no window untimed, the survey and every anchor
  worked (no anchor read too slow). Late and crossed with the load
  (49, 9) are at the top of what tours of the radio-less build showed
  (round 5: 33 to 35 late, 6 to 9 crossed; A: 34, 1); without load
  (23, 1) they are like A. STARDUST: 3 late with load, 2 without, 0
  crossed either way; its step is about 390 us (2 percent) longer
  with the load.
- Two things found and fixed on the way: the first `pad` build had lost
  the timing to a flash cache clash (above), and the radio's send times
  made the console step about 1,800 times in 5 minutes for nothing
  (`early` 1,776 with the load; now 10).

Anchor reads not used (the `screen` log line's `anchor_miss`: too slow /
bits / near the range ends / failed): 1 / 0 / 599 / 0 over the whole
run; the third kind is the normal one (a read near the refresh's ends is
not trusted).

The tables above are from before the host (round 6, the radio and a
load test only). With pad-host in, measured below.

### With the host (pad-host), measured

The logs are kept outside the repository. M0 = default
features, M1 = default with `pad` (both with the tour); G0 / G4, G5 =
the owner's build (`demo-card,touch,link,store`) without / with `pad`.
"Waiting" = radio up, page scan on, no pad (the pad was off all round).
"Injected 100/s" = `boardctl pad connect switch` and `repeat 10`: a
resting Switch pad's full report every 10 ms through the real decoder.

| | no `pad` | `pad`, waiting | `pad`, injected 100/s |
|---|---|---|---|
| code RAM left below the game slot | 52,608 | 8,260 | |
| main data RAM taken by the radio for itself | 0 | first 64 KB | |
| `.data` + `.data.wifi`; `.bss` (owner's build) | 10,100; 68,880 | 15,604 + 1,012; 76,364 | |
| main stack: size (deepest use seen) | 117,624 (12,912) | 38,092 (18,560) | 38,092 (18,560) |
| heap 49,616: host; radio; least free seen | 0; 0; 49,616 | 19,640; 19,580; 10,356 | 10,364 |
| loop begins after the clock started (3 boots) | 573 ms | 1,061 to 1,069 ms | |
| radio start; pairings read (386 reads) | | 374 to 391 ms; 8.8 to 9.0 ms | |
| radio start to "waiting" | | 1,057 to 1,067 ms | |
| tour 5 min: late; crossed (M0 / M1) | 19; 0 | 46; 5 | 42; 5 |
| tour: scroll step; present (avg / max, us) | 16,444 / 34,747; 14,030 / 32,298 | 17,262 / 35,235; 14,580 / 32,625 | 17,198 / 35,824; 14,240 / 32,834 |
| tour: reports a second, dropped, console wakes by the pad | | | 98, 0, 0 |
| STARDUST 10 s: late; crossed | 0; 0 (M0) | 2; 0 (M1), 0; 0 (G5) | 2; 0 (G5) |
| STARDUST step; present (avg / max, us) | 17,082 / 18,772; 16,444 / 18,132 | 17,240 / 19,010; 16,595 / 18,081 | 17,723 / 19,277; 16,754 / 18,315 |
| STARDUST: reports a second; longest wait | | | 100; 17.4 ms |
| home, 20 s still: frames | | 11 (G4) | 12 (resting pad) |
| home, left alone: steps a second; heap, stack over 30 min | 0.52 (G0b, 2 min) | 0.52 (G5, 30 min); 10,364 and 18,560 all along | |
| stats frame at home (text bytes) | | 971 (G5) | |

- The pad's cost in a game is about 300 to 640 us of step per frame at
  100 reports a second (2 to 4 percent), with no window missed.
- The tour's late and crossed with `pad` (46 and 5, 42 and 5) are above
  this M0 run (19 and 0) and above round 5's 33 to 35 late; crossed is
  within round 5's 6 to 9. One run each: whether the radio's page scan
  (an 11 ms window every 1.28 s) makes some windows late is **not
  checked**.
- During the tour's widest scroll a report can wait one whole turn of
  the loop (step and present back to back): 75 ms at most seen.
- At home with nothing moving the console sleeps with or without a
  resting pad (11 and 12 frames in 20 s, the stats calls included).
- The radio is all behind `pad`: without it no radio, controller,
  pad-host, pad-proto or hci-rules symbol is in the ELF (0 of them; 332
  with `pad`). The owner's build grows by 323,040 bytes of code in
  flash, 60,804 of constants and 44,348 of code RAM.

Injected pad on the board: home moves by the pad (3 short taps right
and one left:
0 to 2; 5 taps right then 5 left in one script, each press shorter than
a frame: 2 to 5, capped at the last tile, and back to 0); STARDUST and
PADTEST start by the pad (first frame 25.4 / 25.7 ms after the press);
Home opens the game menu; down, down, A leaves; motion reaches PADTEST;
the pad going away shows `pad: gone (reason 0x13)` and "no pad". PADTEST
with east held, tilted and turning is the simulator's picture exactly (0
of 76,800 pixels); after a second move only its pictures-drawn counter
differs (11 / 12: when the 250 ms motion look falls); home differs only
in the uptime clock.

### The connection, against the test firmware's logs

The test firmware (experiments/bt-gamepad-test, logs live1 to live3)
ran the same pad-proto host, but searching (inquiry) as well, and read
the radio in its own task at once. In the console the host is
reconnect-only and is read between frames. Compared:

- **The first second after the pad is switched on** (live3, masked):
  the pad calls (class 0x002508), the host accepts, the pad switches
  the role (we become central, as in every logged link), asks for the
  saved key itself, encryption comes on, the pad opens HID control and
  interrupt while the host reads its SDP records, then the Switch
  start-up talk (device info, full reports, motion on, light) wins. The
  same accept role (0x01, "stay peripheral", the pad then switches), the
  same set-up commands except the search, the same waits (0.6 s before
  asking for authentication if the pad does not, 1 s for the pad to
  open HID itself).
- **Once in live3 the pad's first link after switching on ended (0x13)
  with the channels open and no report, and it called again at once**;
  the second call worked. The host takes such a call at once (test
  `the_pad_calling_again_at_once_after_a_failed_start_is_taken`).
- **Read between frames**: every packet waiting 40 ms (more than the
  slowest frame, 35 ms) still connects with the four logged outputs
  exactly and no failed stage (test
  `a_reconnect_works_when_every_packet_waits_a_frame`); by hand: 100 ms
  the same; 150 ms and more still connect but the start-up talk sends
  some subcommands again (it retries after about 100 ms; the note is
  kept outside the repository).
- **Page scan**: the gate allows no Write Page Scan Activity, so the
  controller's default is used (R1: an 11.25 ms window every 1.28 s);
  the test firmware used the same default. Not measured here: how long
  the pad takes to get through (the pad was off all round).
- Found and fixed: the start log said "any gamepad that answers is
  called" in reconnect-only mode (pad-proto; test
  `the_start_log_never_says_it_calls`); the injector's gyroscope
  inverse was off by one for most numbers (pad-host inject; test
  `every_reachable_gyro_number_comes_back`); the injected repeat now
  hands over every report that fell due during a long frame (up to 8),
  as the driver's queue does, so a game sees 100 a second (70 before).

Still uncertain: whether the real pad's first second goes the same way
with this firmware (the owner's test); whether it hangs up when the
console's answers come one frame late during a game (the tests say no
up to 100 ms); the radio blobs' own behaviour (closed code).

### A pad switched off and on (round 6 fix)

The owner's test: switched off, the pad stayed "connected" for about
20 s, and switched on again before that it could not come back. The
log (padcheck.log) showed why: every loss ended with reason 0x08
(connection timeout) about 20 s later, the controller's default link
supervision timeout (0x7D00 slots = 20 s); and the console only
noticed then. Now:

- **Supervision timeout**: once the role change makes the console the
  link's central (it does on every logged call, right after the link
  comes up), the host sends Write Link Supervision Timeout for that
  handle: the pad's own wish from SDP (0x020C, the GuliKit asks
  0x0C80 = 2 s) kept between 1 and 5 s, 2 s without one, and never
  under three sniff intervals (sniff was never seen in the logs). Sent
  once per change; a refusal (the console is not central after all)
  is logged and nothing else happens. The gate lets it through only
  for the handle of a saved pad's link, at least 1 s.
  How the controller uses it is closed code: its ROM has the LMP
  supervision-timeout routines (`r_lc_send_pdu_lsto`,
  `r_ld_acl_lsto_set`, `lmp_supv_to_handler`), so it is inferred, not
  seen, that it sets its own timer and tells the pad.
- **Silence guard** (pad-host, `SILENCE_MS` = 1 s): a pad in Switch
  mode that streams full reports (0x30, about every 10 ms; the longest
  real gap logged was 366 ms) and sends nothing for 1 s is gone: the
  console sees no pad at once (buttons released, the "gone" event),
  the host sends Disconnect and waits for the next call at once. Only
  after the first full report (never during the start-up talk), never
  for a mode that sends only on change (XInput, Android, DInput, the
  simple 0x3F reports), never for an injected pad. A loop that did not
  look for over 250 ms (`STALL_MS`) restarts the count, so a long
  frame is never taken for a silent pad.
- **The old link**: after the console dropped it, a new call from the
  pad is taken at once, whether the old link's Disconnection Complete
  comes at once, late, after the new call, or never (forgotten after
  10 s); data still coming on the old handle is dropped quietly. If
  the pad we are linked to calls again (switched off and on before
  anything noticed), the old link is dropped and the call taken.

What the owner should see: switched off during a game or at Home, the
pad is gone in about 1 s (Switch mode; up to the 2 s supervision
timeout in the other modes), "PAD DISCONNECTED" shows, a game sees no
pad. Switched on again later than about 2 s after the switch-off, it
is taken as soon as it calls. Switched on sooner, its call can come
through only once the controller has let the old link go, 2 s after
the switch-off (the console cannot see or speed that up; the console
has already dropped its side), so it is back a few seconds after the
switch-on, or after one more key press if the pad stopped calling.
Tests: crates/pad-host/tests/link_loss.rs, the round-6 tests at the end
of crates/pad-proto/tests/replay.rs, hci-rules tests/rules.rs,
tests/host_through_gate.rs and tests/hostile_radio.rs
(`a_pad_switched_off_and_on_again_comes_back_quickly`: a model of the
controller that holds the dead link until its supervision timeout).

Second reader (round 6) on this fix:

- In that model: a streaming pad is shown gone 1.0 s after the
  switch-off, the old link is freed at 2.0 s, and the pad is connected
  0.7 s after it calls once the link is free; switched on 0.3 s after
  the switch-off it was connected 3.9 s after the switch-on (the
  model's pad waits 1.5 s between calls). With the controller passing
  the pad's calls on while it still holds the old link, the host takes
  the call and drops the old link at once (gone 0.3 s), and the new
  link comes when the controller frees the old one. What the real
  controller does with such a call is not known: it is the owner's
  test.
- Fixed: a supervision timeout that waited behind other commands while
  its link ended was still sent, for a handle that was gone; the gate
  refuses that, nothing answers, and every later command waited the
  3 s command time-out (a pad switched off in the first moments of a
  link, then on again, could meet it). The host now drops it (test
  `a_queued_supervision_timeout_is_not_sent_after_its_link_ended` in
  replay.rs, fails without). Found by tests/hostile_radio.rs.
- Fixed: the hex of bytes the other side sent (an SDP request or a
  stray message on the SDP channel the pad opened, L2CAP configure
  options, information data) was logged as is; a Switch device-info
  answer there showed the pad's address. Now the pad's address is
  hidden as in the HID lines (test `hex_of_their_bytes_hides_the_pad_address`,
  fails without). Found by tests/hostile_radio.rs.
- Left as it is: someone who copies the saved pad's address can call
  while the pad is linked; the host then drops the real pad's link
  (the "same pad calls again" rule) and the caller fails at the key
  check. The pad then calls again by itself or after a key press. It
  needs the address and a radio near the console; no key leaks.
- Left as it is: the radio driver (esp-radio) keeps each received
  packet in its own heap box until the loop reads it, with no limit.
  With about 10 KB of heap free and a pad sending 100 reports a
  second, a loop that does not look for about 1 s or more could run
  the heap out (the chip stops). Nothing in the owner build stops the
  loop that long (a screenshot over the cable holds it about 0.8 s).
- The silence guard (1 s) is shorter than the supervision timeout
  (2 s). The longest real gap seen close by was 366 ms; at the edge of
  range longer gaps may come, and then the console drops a pad that
  the radio still holds; it calls again by itself or after a key
  press. Not seen yet.

## One core or two

Since 2026-11-18 the default and the owner's builds send the pictures
from the chip's second core (`core2`): the owner chose two cores. One
core stays supported: leave `core2` out (`check.sh` builds and checks
both). This section is the short version; the details are in "The
second core sends the pictures (core2)" below.

### Board numbers

MEASURED ON THE BOARD, scripted keys, a gamepad connected, the 60
setting unless said. Seven games (owner's set with `pad`, one core
against `+core2`); TUNDRA (eight games, four-player interface). The
reports are kept outside the repository.

| game, detail | one core fps | two cores fps | late / crossed, one core | late / crossed, two cores |
|---|---|---|---|---|
| LANTERN level 4 LIGHT | 59.9 | 60.0 | 42 / 5 | 6 / 0 |
| LANTERN level 4 HEAVY | 53.2 | 59.8 | 14 / 0 | 1 / 0 |
| HAIRPIN NORMAL | 55.0 | 59.7 | 92 / 2 | 13 / 0 |
| HAIRPIN HEAVY | 45.7 | 58.7 | 22 / 1 | 1 / 0 |
| KESTREL HIGH | 59.5 | 59.8 | 17 / 0 | 6 / 0 |
| TUNDRA, two players | 59.77 | 59.99 | 17 / 2 | 1 / 0 |

- Console step, average: one core 9.2 to 20.4 ms, two cores 3.0 to
  15.2 ms (seven games); TUNDRA 4.7 against 3.1 ms, longest 19.2
  against 3.7 ms.
- Two cores had 0 crossed windows (a possible tear) in every run.
- Picture check (letter `V`): 830 + 1,201 pictures, 0 mismatches.
- Lead time (send call to first pixel byte): one core 55 to 121 us
  average, 543 at most; two cores 38 to 47 us, 97 at most.

### At the starting settings (2026-11-23)

Since 2026-11-23 the games start at their HIGH (or HEAVY) detail and at
60 (`games/README.md`, "Starting settings"). MEASURED ON THE BOARD at
those settings: the owner's two-core build with `pad`, `boot-gather`
and `game-flash` from that day's tree, flashed at 80 MHz, no gamepad
connected, keys sent over the debug link (`boardctl key`), one run
each; the counters were cleared with `r` once the game was running and
read with `m`. CAMBER already started at HIGH and 60 and was not run
again (its table is in `games/camber/README.md`).

| game, what ran | frames | fps | console step, average / longest (ms) | screen pixels sent a frame | late / crossed |
|---|---|---|---|---|---|
| MERCATOR, HIGH, CHASE: flight just after launch, 20 s | 1,262 | 59.99 | 4.51 / 5.21 | 949 | 3 / 0 |
| MERCATOR, the same, 20 s after 5 s of thrust | 1,264 | 59.99 | 4.43 / 4.80 | 390 | 0 / 0 |
| HAIRPIN, HEAVY: a race with the accelerator held, 20 s | 1,241 | 58.87 | 15.53 / 16.94 | 40,840 | 0 / 0 |
| KESTREL, HIGH: mission 1 with the boost held, until the hull broke | 688 | 59.99 | 8.00 / 11.37 | 5,194 | 0 / 0 |
| LANDER, STILL: a level-1 flight with three burns | 466 | 59.99 | 4.19 / 4.54 | 151 | 0 / 0 |
| LANTERN, level 1, HEAVY: turning on the spot, 15 s | 943 | 58.74 | 16.34 / 16.97 | 61,196 | 0 / 0 |
| LANTERN, level 1, HEAVY: walking forward, 6 s | 425 | 60.01 | 5.09 / 13.48 | 44,277 | 1 / 0 |
| ROOFTOP, STEP: room 1, running and jumping | 783 | 59.99 | 4.57 / 4.93 | 285 | 0 / 0 |
| SKIMMER: a match against the computer | 1,119 | 59.99 | 2.29 / 2.65 | 1,145 | 0 / 0 |
| TUNDRA: a match, one pad against the computer | 1,287 | 59.99 | 2.70 / 3.02 | 1,106 | 1 / 0 |

- The lengths (20 s, 15 s, 6 s) are how long the script waited; counted
  from frames and fps, the counters ran about one second longer.
- HAIRPIN's race and LANTERN's turn change most of the picture every
  frame, so they run at the panel's pace (about 58.7 refreshes a
  second) and the step fills nearly the whole frame.
- SKIMMER's fps is between frames back to back (`run_fps`); with one
  pause of over 0.1 s in the run the plain count was 59.73.
- The walking run probably ended against a wall (its picture was not
  checked frame by frame), so it stands for less than six seconds of
  walking.
- Each start of this firmware logged `SPI Speed      : 80MHz`,
  `calibrated=true` and the pairing area's CRC unchanged.
- Not in this round: a MERCATOR fight and its planet, LANTERN's other
  levels, ROOFTOP's wide rooms and SMOOTH, LANDER's FOLLOW view, more
  than one player, a connected gamepad, long play, one core.

### What it costs

MEASURED ON THE PC (ELF of the owner's set with `pad`, 2026-11-18):

| | one core | two cores |
|---|---|---|
| main stack (size) | 38,180 | 29,448 (8,732 less: the second core's 8 KB stack and its statics are in `.bss`) |
| main stack, deepest use seen on the board | 19,168 | 16,656 |
| code RAM left below the game slot | 8,260 | 8,204 (56 bytes less: `core2::parked`, which must run from RAM) |
| second core's stack: size; deepest seen on the board | - | 8,192; 1,888 |
| heap free (board) | 10,300 | 10,300 |

The hardening of this round (bounded waits, the resend after a failed
send) cost 8 bytes of `.bss` and no code RAM (two-core main stack
29,472 before, 29,448 after, by alignment); the one-core build did not
change (same section sizes; the only differing bytes are source line
numbers in panic and log locations).

### Hardened in this round

- **Bounded waits.** Every wait of the first core for the second
  (`join`, parking for the flash, letting it go) ends after 500 ms
  (the longest send measured is about 33 ms). Then the console stops
  with one message on the serial port, `core2: the second core still
  holds job N after 500 ms (...); console stopped, press RST`, as a game
  panic does. The panel keeps its last picture. The first core does not
  take over the sending: a hung send may hold the SPI driver half way,
  and nothing proves that could be taken over safely.
- **A panic on the second core** is the same case: esp-backtrace prints
  it and stops that core; the first core stops at its next wait with
  its own message.
- **A failed send** (a window that did not go out whole, `errors` in
  the counters) makes the next present send the whole screen
  (`resends=` in the `core2` log line). The one-core build keeps its
  old behaviour.
- **The rules are checked.** A picture handed over while the box is
  still full stops the console with a message instead of sending from
  a changed slot. The first core's side of the hand-over is the crate
  `handover`; its PC tests run it against a model of the second core
  in every order the two can take (76,518 states, with a hang and late
  timers at any point): never both cores on the screen, never the flash
  used while the second core runs, no lost wake-up, never a wait for
  good.
- **The wake time stamp** (`wake_us` in the log) is now written before
  the job, so the second core reads this job's stamp. Before, it could
  read the one before: the `wake_us_max` of about 17,000 us seen on the
  board was most likely that (ESTIMATE), not a slow wake.

### Risks left

- A hang of the second core inside a lock shared with the first core
  (the critical section, the log's lock) would freeze the first core
  before its 500 ms wait ends. The second core takes such a lock only to
  copy the counters (`SNAP`) and inside esp-hal's drivers; not seen.
- A game that keeps its picture pointer past its call (only possible
  with unsafe code, which games may not have) would draw into the
  picture being sent: one mixed frame.
- The watchdog, the resend and the hand-over check are built and
  checked on the PC only; not yet run on the board (plan in the round's
  report). The watchdog can be seen on the board with a `hang-test`
  build (measuring only) and the letter `H`.
- The start-up survey sometimes rejects half its samples as slow
  (`samples=3416 rejected=3417 slow=3417`). It runs on the first core
  before the second core starts, and the same pattern shows in one-core
  builds (2841 / 2842 in one of them): it comes
  from the code layout, not from the second core. In the timed part of
  the scanline read, esp-hal's `SpiDma` read and write run from flash
  (only the low-level driver is in RAM), so a cache miss can make
  every other read slow. Since 2026-11-18 they are in the hot block
  ("The timed paths and the flash cache"), which should end this. In
  the board runs since then (the "A runs") `rejected=`
  was not logged (the run script cut that part of the line), but the
  survey's spread shows every read within 1 us of the fastest, except
  one 5 to 8 us over, and only reads more than 8 us over are rejected
  as slow: so at most one read per survey was rejected as slow
  (MEASURED ON THE BOARD). When half the reads were rejected, the line
  time came out about 0.14 percent short (49.486 against about
  49.52 us; the period is right): ESTIMATE under half a line of error
  within a refresh, against a guard of 8 lines, and the runs with that
  start had 0 crossed windows (MEASURED ON THE BOARD). The survey's
  rules were not changed for this.
- Power (two cores busy about 7 percent of the time), long sessions and
  a real second pad are not measured.

## The second core sends the pictures (core2)

Feature `core2`, on by default and in the owner's builds since
2026-11-18 (until then a lab feature, off by default). Ported
from experiments/cyd-lab (`core2-swap`, its README "The second core
without the third picture"). Code: `src/core2.rs` (its file notes have
the same rules as below), the `core2` parts of `screen.rs`, `app.rs`,
`link.rs`, `main.rs`, two helpers at the end of `flash_store.rs`, and
the crate `handover` (the first core's side of the hand-over, tested
on the PC). The evidence (logs, and for the hardening its own round's
files) is kept outside the repository.

**Result: it works on the board, with limits.** The console draws on the
first core and hands each finished picture to the second core, which
sends it with the same tear-free timing. The first core no longer waits
for the panel: a STARDUST step went from 17.1 ms to 0.6 ms, a row scroll
step from 15.5 to 17.4 ms down to 2.0 to 2.4 ms, and with an injected pad
at 100 reports a second the longest wait for a report fell from about
33 ms to about 3.3 ms in the tour. The price: 8,676 bytes less main
stack (the second core's 8 KB stack and its statics are in `.bss`),
56 bytes less code RAM, 8.5 KB more flash. The owner chose two cores
(2026-11-18); one core stays supported.

### Who runs what

| first core (PRO) | second core (APP) |
|---|---|
| the console's loop (one task, `app::run`), the radio (its controller task is pinned to this core, its interrupts were set up here, its PHY code is in RTC fast memory, which only this core can run), the debug link, the log, settings saves, the game loader and the games | one esp-rtos thread with an embassy executor and one task, `worker`, which sends the handed pictures |
| panel set-up, clock check, clear, the scanline survey and calibration (in `main`, before the second core starts) | every timed send from the first picture on, with its anchor reads (Get Scanline) and windows |
| link shots, raw panel commands, the backlight, the letter `l`: only after waiting until the second core holds nothing (`join`) | nothing else: it never logs, never uses flash on purpose, never runs game code |

The second core runs its executor, `worker` and the screen's `present`
and `send` from flash (through its own cache), all in the hot block
("The timed paths and the flash cache"); esp-hal's low-level SPI
driver, the scanline read, `fill_fast` and `parked` run from IRAM,
which both cores can run. None of its code
is in RTC fast memory (checked in the symbols: `core2::parked` is the
only new IRAM function, 45 bytes, and it calls nothing).

The beam anchor is a cycle count, and each core has its own cycle
counter, so an anchor is only good on the core that read it. The survey
(first core) measures the line length, a rate that is the same on both
cores (both at 240 MHz); its anchor is forgotten when the second core
starts, and the first core forgets the anchor again after it used the
panel (shots, raw commands, the inline fallback).

### Shared memory: the rules

One flag, `JOB`, hands things over: 0 = the second core holds nothing.
The first core writes what the second core will use, then stores the
job number (Release); the second core loads it (Acquire), does the job
and stores 0 (Release) after its last use; the first core sees 0
(Acquire, `join`) before it touches any of it again. Only the first core
sets a job, and only while `JOB` is 0 (`hand_over` checks it and stops
the console if not). The hardware does not enforce this; the code does.
Each ordering is argued in the file notes of `src/core2.rs`; the first
core's side is the crate `handover`, tested against a model of the
second core in every order.

| memory | first core | second core | enforced by | if the rule is broken |
|---|---|---|---|---|
| the handed picture (19,200 B, one of the two in the `Console` static) | reads it only (catch-up copy) until its next `wait_free`; writes it again only after that | reads any part of it while `JOB` = present (a box send also reads pixels outside the changed areas: "Sending a picture") | console-video's `present_handing` order (find, wait_free, hand_over, catch up, swap); `frame_mut` and `game_buffers` never give the held picture | the panel shows a mix of two pictures for one frame (letter `V` checks sums) |
| the drawn picture, palette, rest of the console | reads and writes | never | the second core gets only a pointer to the handed picture; colours go over as the screen's wire table | - |
| `SLOT` (pointer, up to 16 areas, sum) | writes while `JOB` = 0 | reads while `JOB` = present | `JOB` | wrong areas, or a send from a stale pointer |
| the screen (`CydScreen`: SPI, DMA buffers, wire table, beam, counters, backlight) | only through `Core2Screen`, whose `Deref` waits (`join`) first, and in `hand_over` right after `wait_free` | only while `JOB` = present | `JOB`; `Core2Screen` | two cores on one SPI bus: garbage on the panel or a stuck transfer; a foreign anchor: torn windows |
| `SNAP` (copy of the screen counters) | reads inside a critical section | writes inside a critical section after each send | the critical section (a lock across both cores) | a torn number in a report |
| report counters (atomics) | reads, resets (`r`) | adds | atomics | a count off by one |
| `HANDED_US` (wake time stamp) | writes it before the job's Release store | reads it after the job's Acquire load | `JOB` | a wrong `wake_us` |
| `RESEND` (a send failed) | takes it before each step | sets it | a hint: seen one step later at worst | one more picture with a stale area |
| `HOLD`, `HELD` | sets and clears `HOLD` | sets and clears `HELD` in `parked` | Release / Acquire | flash used while the second core still runs from it |
| the flash and its cache (settings saves) | only while a `Held` guard lives | waits in IRAM with interrupts off up to level 5 | `Held::park()` in every read, write and erase of `EspFlash`; esp-storage's `multicore_auto_park` also stalls the core for each write and erase | a cache fetch from flash during a write or erase: a crash or a wrong instruction |
| the game slots (code in IRAM, data) | the game loader writes, the game runs | never | the second core gets no pointer into a slot and runs no game code | - |

Settings saves, step by step: `Held::park()` waits for the current send
to end (`join`), asks the second core to park, and waits until it says
it is parked in `parked` (in IRAM, interrupts off, spinning on a flag in
data RAM). Then the read, write or erase runs; for writes and erases
esp-storage also stalls the second core in hardware. When the guard
ends, the second core goes back to its executor. If the second core
does not answer within 100 ms, the save fails (`StoreError::Io`, the
store tries again later) and `hold_fails` counts it. A save therefore
waits for at most one send (up to about 35 ms) before it starts. If the
send in flight does not end within 500 ms, the console stops (below).

Screenshots, raw panel commands and the backlight: the first core waits
for the second core to finish (`join`, through `Core2Screen`), uses the
panel itself, then forgets the anchor; the next send takes a new one on
the second core. The game loader writes the code slot on the first core
only; the second core never reads the slots, so this needs no rule.

Start order: pairing read and the CRC log, the settings store, the
radio, the panel survey, then the second core (328 us to start). Every
flash read before that runs with one core only.

If the second core hangs or panics, the first core's next wait for it
ends after 500 ms with a panic that says so (`core2: the second core
still holds job N after 500 ms (join|park|unpark); console stopped,
press RST`); the console stops as for a game panic and the panel keeps
its last picture. The first core touches neither the screen nor the
flash on that way, so the two cores never meet on the SPI bus. A panic
on the second core prints its own message first (esp-backtrace stops
only that core). See "One core or two".

### Memory (built, and from the boot log)

Owner's set `demo-card,touch,link,store,pad`, one core against
`+core2` (sizes from the two ELF files; board numbers from the runs of
the same sets with `tour`):

| | one core | two cores (`core2`) |
|---|---|---|
| code RAM left below the game slot | 8,260 | 8,204 |
| heap free (after the radio; least seen in all runs) | 10,292; 10,300 | 10,292; 10,300 |
| main stack: size; deepest use seen | 38,052; 18,608 | 29,376; 16,528 |
| second core's stack: size; deepest use seen | - | 8,192; 1,712 |
| `.data` + `.data.wifi`; `.bss` | 15,612 + 1,012; 76,380 | 15,740 + 1,012; 84,928 |
| app image in flash | 807,472 | 816,016 |

Without `pad` (`demo-card,touch,link,store`): code RAM left 52,608 /
52,552, `.bss` 68,880 / 77,428, main stack 117,624 / 108,908 (built
only, not run on the board). The main stack's deepest use with two
cores is lower because the panel send no longer runs on it.

With `core2` off, the firmware is the one before: every loaded section
of the owner's build and of the no-pad build has the same size, and
`.rwtext`, `.data`, `.bss`, the RTC fast part and the radio parts are
byte for byte the same. The only bytes that differ are 23 constants in
`.text` and 32 words in `.rodata`; each is a source line number (of a
log line or of a panic location) that moved because lines were added
above it, and each was checked to point at the same source text.

### Measured on the board, one core against two

These runs (2026-11-17) were made before the new send plan; with `core2`
the second core now uses the same plan (windows or one box), which is
not measured on the board yet.

Both builds `demo-card,touch,link,tour,store,pad`, the second with
`core2`; the radio up and waiting for the saved pad (page scan on).
"Injected" = `boardctl pad connect switch`, then `pad repeat 10` (one
resting Switch report every 10 ms, 100 a second, through the real
decoder; the task asked for "repeat 100", but repeat takes the gap in
milliseconds). Tours of 180 s, twice each. Numbers from the `stats`
frame taken first and from the `m` report right after; the pad's
`gap_ms` and `wait_us` are from the `stats` frame, because the 1 KB
stats frame itself holds the first core for about 70 ms and that shows
in the `m` report's pad line. Step and present are average / longest,
us; present is what the first core spends in it (with `core2` only the
hand-over and any wait for the second core).

| tour 180 s | one core | two cores |
|---|---|---|
| scroll fps | 30.13, 30.13; injected 29.99, 30.02 | 30.03, 29.92; injected 29.96, 30.00 |
| scroll step | 15,484 / 35,681; 17,266 / 35,234; injected 16,580 / 36,209; 17,385 / 36,112 | 2,056 / 11,871; 2,005 / 2,867; injected 2,442 / 3,146; 2,446 / 7,017 |
| scroll present (first core) | 12,805 / 33,018 ... 14,588 / 32,796 | 15 / 62 ... 56 / 10,326 |
| presents; late; crossed (waiting) | 1,297; 24; 6 and 1,267; 34; 4 | 1,296; 3; 0 and 1,266; 2; 0 |
| presents; late; crossed (injected) | 1,276; 30; 9 and 1,277; 40; 13 | 1,277; 4; 0 and 1,280; 11; 0 |
| pad (injected): reports a second; dropped; longest gap; longest wait | 101, 100; 0; 43, 45 ms; 32,894, 34,811 us | 100, 100; 0; 13, 13 ms; 3,296, 3,261 us |
| second core: send average / longest; times the first core had to wait for it | - | 9,465 to 10,060 / 32,568; 0 to 4 per tour, longest 10,309 us |

| | one core | two cores |
|---|---|---|
| STARDUST 10 s waiting: fps; step; late; crossed | 29.82; 17,147 / 18,709; 1; 0 | 29.81; 596 / 946; 0; 0 |
| STARDUST 10 s injected: fps; step; late; crossed; pad gap; wait | 29.81; 17,581 / 19,262; 4; 0; 29 ms; 19,496 us | 29.80; 861 / 1,078; 0; 0; 10 ms; 258 us |
| PADTEST 10 s injected: fps; step; late; crossed; pad drop; gap; wait | 29.28; 2,775 / 10,881; 4; 1; 0; 20 ms; 10,056 us | 29.80; 1,375 / 1,816; 2; 0; 0; 12 ms; 1,524 us |
| 50 start/leave cycles (25 PADTEST, 25 STARDUST): came home; errors; late; crossed | 50 of 50; 0; 6; 0 | 50 of 50; 0; 0; 0 |
| heap free / main stack deepest, before and after the cycles | 10,300 / 18,608, the same | 10,300 / 16,528, the same |
| game slots after the cycles | 0 words not zero | 0 words not zero |
| game first frame after the press | 23.9 to 26.2 ms | 7.2 to 7.9 ms |
| WARN, ERROR or PANIC lines in the whole run | 0 | 0 |

Brightness saved while the tour scrolls (two cores): four times the
home menu was opened, the brightness moved and the menu closed while
the tour ran (the tour's own presses also move it while the menu is
open, so the value is read back, not assumed). Three saves were
written (2, 1, 4: 6,597, 8,766 and 2,024 us), the fourth found 4
already in flash. The second core was parked 6 times (`holds=6`,
`hold_fails=0`); once the current send had ended it was parked within
8 us. In that minute: 251 presents, 2 late, 0 crossed, 0 errors.
After a restart: `settings: brightness 4 (saved)`,
the value before the restart, and the `pairing area crc32=` line the
same as at every start of this work.

Soak of 600 s, two cores, tour plus the injected pad at 100 reports a
second: no stop, 4,351 presents, 12 late,
0 crossed, 0 errors, 0 untimed; 60,097 reports, 0 dropped, longest gap
18 ms, longest wait 8,472 us; heap free 10,300 from start to end; main
stack deepest 16,528 of 29,376 and the second core's 1,712 of 8,192
(the same as after the first minutes); no WARN, ERROR, panic, stack
guard or exception line in 420 log lines; `hold_fails` 0. The second
core was busy 42.2 s of the 600 (7 percent).

Picture check (letter `V`, 60 s tour): every one
of 395 handed pictures had the same sum after its send as when it was
handed over (`checked=395 mismatch=0`): the first core did not write
into a picture the second core was sending.

**Does the second core sleep when idle?** Yes, as far as the counters
show. Its executor calls the `on_idle` hook (counted as `idles`) each
time it runs out of work, and esp-rtos then stops the core with `waiti`
until an interrupt (read in the esp-rtos 0.4.0 sources for the lab,
inferred for this build). At home, left alone for 20 s, the second core
did 1 job, 2 polls and went idle once (the end of the two-core run);
it is not polling in a loop. Across the tours, `idles` equals
`jobs` and `polls` is twice `jobs`: one wake per picture. Its busy time
in a 180 s tour was about 12.3 s (7 percent). Whether esp-rtos's own
timer wakes it now and then without polling the executor is not
counted.

A two-core build of the owner's set (`demo-card,touch,link,store,pad,core2`,
no `tour`) was kept (sha256 2ac22e24...). It was started once on the
board: pairing CRC unchanged, `calibrated=true`,
second core started in 333 us, heap free 10,292, main stack 29,392
(deepest 16,528). After that the owner's one-core test build was put
back.

### Still unknown

- Whether the second core changes anything for a real pad (only the
  injected pad was used), or for the radio itself: the radio stayed on
  the first core and lost nothing, but its timing under a real link was
  not measured with `core2`.
- That the controller task stays on the first core is read from the
  driver (pinned to core 0, the second-core bit not set in its start
  mask) and seen in no failure; not traced on the chip.
- Whether a save ever has to wait the full 100 ms for the second core:
  `hold_fails` stayed 0 in every run, but only 6 parks were measured
  (three saves). A save during a game, and many saves in a row (the
  store's wear test `w`), were not tried with `core2`.
- How the 2-core build behaves for hours: the longest run was 10 min of
  soak plus about 20 min of other tests.
- Power: whether two cores cost battery is not measured (no meter).
- A hang or panic on the second core stops the console with a message
  after 500 ms (since the two-cores round); not yet seen on the board.
- With `pad`, `core2` and `wide-strips` together the build does not link
  (main stack under 8 KB); `wide-strips` is for measuring only.

## Not done yet (round 6, 2026-11-17)

- The touch filter was tuned on the owner's finger test (round 5,
  crates/touch-filter/README.md): one wild reading is now left out. The
  debug link cannot make a single wild reading, so whether short taps
  stay taps on the real panel, and whether a drag that starts one
  reading later feels slow, is for the next finger test. The
  calibration is still the first guess.
- A short quick flick now goes a little further than before (see
  "Touch values and why"). If the owner finds flicks too strong, one
  way is to leave the first jump after a held move out of the speed.
- `game first frame: N us after the press` in the log is only right for
  games started by a button: a game started by a tap shows the time
  since the last button press (seconds). Measuring only.
- The real BOOT button's bounce, and how the five light steps look
  (spacing, the dimmest one, flicker, whine), are for a person to
  check. Only the chip side of the light was measured.
- Games from the real SD card: not tried (the card has no games and is
  read only). The same loader path was tested with the demo card.
- A game that panics stops the chip until reset (RST button). No
  watchdog and no way back home yet.
- A GAME.BIN of size 0 is not listed at all (no notice).
- Torn windows ("crossed" in `stats`) still show up now and then around
  game start and leave and in the tour (12 in 204 start-and-leave
  cycles). The cause is not found.
- The gamepad (`pad`): the owner's real pad connected and played in
  Switch mode (padcheck.log); switched off, it showed as connected for
  about 20 s and could not come back before that. The fix ("A pad
  switched off and on") was tested only with the injected pad and on
  the PC; whether the real pad is now gone in about 1 s and back
  quickly, and what the controller does with the pad's call while it
  still holds the old link, is the owner's next test.
- With `pad` the tour showed more late windows than without (one run
  each, see "With the host"); not checked further.
- With `pad`, 8,260 bytes of code RAM are left below the game code slot.
  A host in Rust runs from flash and does not need it; it is for
  esp-hal or esp-rtos code that must be in RAM.
- The load test's crossed windows (9 in 5 minutes) are at the top of the
  spread of radio-less tours; whether a real pad's 100 reports a second
  do the same is for the real-pad test.
- Only the brightness and, with `boot-gather`, the boot animation
  choice are kept. The store's record has room for more settings, but
  nothing else uses it yet.
