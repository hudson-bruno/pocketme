# Testing

PocketMe is checked in two places. Everything that does not need the
real chip (the console's logic and pictures, the games, the protocols,
the flash formats, the timing rules) is tested on the PC, most of it
deterministically, with the same result on every run. What only the
real board can judge (memory, speed, tearing, the radio, real touch) is
measured on the board, and a PC number is never reported as a board
number.

- [Who judges what](#who-judges-what)
- [The simulator as a test tool](#the-simulator-as-a-test-tool)
- [The kinds of tests](#the-kinds-of-tests)
- [Checks on built files](#checks-on-built-files)
- [The two check scripts](#the-two-check-scripts)
- [Measuring on the board](#measuring-on-the-board)
- [Where this comes from](#where-this-comes-from)

## Who judges what

| Question | Judged on the PC | Judged only on the board |
|---|---|---|
| Does the console draw the right picture, and the same one every time? | yes: simulator, golden pictures | |
| Do the games play as scripted? | yes: golden records, scripted runs | |
| Does a game file fit its memory slots and load byte for byte? | yes: the chip build and its checks | that the code really runs there |
| Does the gamepad host survive bad packets, and keep its key secret? | yes: scripted sessions and fuzz runs | the real radio and the real pad |
| Does the settings store survive a power cut? | yes: every cut point tried | the real flash timings |
| Are the two cores' rules kept in every order of events? | yes: a model of the second core | the real hand-over speed |
| How fast is it, does it tear, does it fit in RAM? | no | yes |
| Does real touch feel right? | the filter's rules | the finger on the panel |

The simulator's own README lists what it cannot tell: memory (the PC has
plenty, and a linked-in game has no slots), speed (the PC is much
faster, and the send time is only a model), tearing (the simulator
shows each send at once and whole), the radio, real touch, the real
card's quirks, and colours
([its list](../boards/desktop/README.md#what-the-simulator-cannot-tell-us)).

Behind most PC tests stand two stand-ins. board-api's feature `testing`
adds `FakeBoard` ([fake.rs](../crates/board-api/src/fake.rs)): every
part is a struct with public fields, so a test sets the input, moves
the clock, calls `step`, and looks at the picture, the log or the link.
For games, game-kit's feature `testing` adds `TestConsole`
([testing.rs](../crates/game-kit/src/testing.rs)), a stand-in console
that drives a game through its real entry function.

## The simulator as a test tool

The simulator ([boards/desktop](../boards/desktop/README.md)) runs the
console's own code on the PC. Without a window it runs a number of
frames, follows an input script, saves pictures, and stops.

**Determinism.** Time is simulated in fixed ticks (33,333 µs at 30
frames a second), and the clock reads exactly the time the console
asked for. So a run with no window gives the same pictures every time,
byte for byte, and the window shows the same pictures for the same
input. The test
[`same_result_every_run`](../boards/desktop/tests/headless.rs) runs the
simulator twice and compares.

**Fingerprints.** A fingerprint is a short number made from every pixel
of the picture. Two runs with the same fingerprint showed the same
picture. `--prints` writes `frame N fingerprint X` after every frame, so
"this change does not alter what is drawn" is checked by comparing the
fingerprints before and after. Window-only effects (the backlight
dimming, the F1 overlay) never go into PNGs or fingerprints.

**The input script.** A plain text file, one action per line, with the
frame number in front:

```text
# press start, then walk right for a second
0   press plus
2   release plus
10  press right
40  release right
50  touch 160 120        # a finger on the middle of the screen
52  lift
80  card out             # pull the SD card
88  pad off              # the gamepad goes away
89  shot                 # save this frame as PNG
120 p2 pad on            # player 2's pad connects
```

Scripts can also tilt and spin the pad's motion sensor, connect up to
four pads, raise a console notification, and check a value
(`backlight N` fails the run if the backlight is not N). A mistake in a
script stops the run before it starts and names the line
([the full list](../boards/desktop/README.md#the-input-script)).

**Failing loudly.** The simulator fails a run (exit code 1) on anything
the real board would not take: more than 16 rectangles in one send, a
rectangle outside the 320x240 panel, two sends in one console step, a
log line over 200 bytes, or more than 8 console steps in one frame
without time passing. Other numbers, such as the send time, are only
estimates: they are shown, never refused.

**Golden pictures.** A game's golden test keeps a record of every
different picture (as a hash of the pixels and the palette) and every
log line of a scripted run; the console's own golden test keeps the
picture's fingerprint at fixed moments of a scripted tour. Each compares
a new run with the stored record. When a
look changes on purpose, the rule is: look at the new pictures first,
then write the new record:

```sh
cd crates/console-core && UPDATE_GOLDEN=1 cargo test --test golden
```

In a game, `GOLDEN_SHOTS=<folder>` also writes one PNG at each named
mark of the script, so the pictures can be looked at before the record
is replaced. A test that fails prints the first line that differs.


## The kinds of tests

One real example of each kind. The test name follows the file.

| Kind | Example | What it shows |
|---|---|---|
| Golden pictures of the console | [crates/console-core/tests/golden.rs](../crates/console-core/tests/golden.rs), `fingerprints_match_the_stored_ones` | a scripted tour of the console's screens gives the stored fingerprints (with the boot animation built in, two tours, from the home screen on) |
| Golden pictures of a game | [games/camber/tests/golden.rs](../games/camber/tests/golden.rs), `pictures_and_log_match_the_stored_record` | CAMBER, driven through its entry function by a fixed script, draws the stored pictures and logs the stored lines |
| A game in the simulator | [boards/desktop/tests/games.rs](../boards/desktop/tests/games.rs), `scripted_play_is_the_same_every_run` | a scripted session through the whole console gives the same pictures each time |
| Scripted gamepad session | [crates/pad-host/tests/scripted.rs](../crates/pad-host/tests/scripted.rs), `switch_reconnect_from_waiting_to_buttons_motion_and_battery` | a saved pad's whole reconnect, against a scripted controller and pad, ends with buttons, motion and battery |
| Secrets | the same file, `the_link_key_never_reaches_any_output` | the pairing's link key never shows in any output |
| Fuzz run | [crates/pad-host/tests/fuzz.rs](../crates/pad-host/tests/fuzz.rs), `random_packets_never_break_the_host` | random and mutated packets never make the host panic, send a malformed or forbidden packet, or leave the game's view out of range |
| Radio allow-list | [boards/cyd/hci-rules/tests/rules.rs](../boards/cyd/hci-rules/tests/rules.rs), `the_event_mask_leaves_out_search_results_only` | the event mask the firmware gives the controller leaves out the search results, keeps the connection, disconnection, link key and hardware error events, and asks for nothing past the controller's default set |
| Power cuts | [crates/settings-store/tests/cuts.rs](../crates/settings-store/tests/cuts.rs), `every_cut_point_from_a_blank_area` | after a cut at any byte of any save and a restart, the store gives the old value or the new one, never anything else, and the next save works |
| Two cores in every order | [boards/cyd/handover/tests/interleave.rs](../boards/cyd/handover/tests/interleave.rs), `every_order_keeps_the_rules_even_with_a_hang` | every reachable state of the first core's machines against a model of the second core keeps the rules |
| Send-plan model | [boards/cyd/send-plan/tests/model.rs](../boards/cyd/send-plan/tests/model.rs), `one_small_area_waits_at_most_two_refreshes_and_is_never_late` | in simulated time, from 200 random starting points of the panel's refresh, one small changed area is never sent late or across the drawing line and waits at most about two refreshes |
| Damaged game files | [crates/game-loader/tests/real_files.rs](../crates/game-loader/tests/real_files.rs), `real_files_with_random_damage_are_never_accepted` | the real chip game files, damaged at random, are always refused |

A few of these deserve a sentence more.

**The gamepad tests** run the whole host (protocol, decoder and mapping)
against a scripted controller and pad. The pad's answers and reports in
the fixtures come from recorded sessions with a real pad; the radio
steps around them are written from the Bluetooth specification. Test
fixtures use made-up device addresses and keys, and tests fail if a key
or a full address reaches any output. For a longer fuzz run:

```sh
cd crates/pad-host && PAD_FUZZ=200 cargo test --release --test fuzz
```

**The power-cut tests** use a pretend flash that loses power after any
byte of a write or any 4-byte word of an erase, and can leave the byte
it was changing half done. They try every cut point of every save, not
a sample: in the default run, 808 saves from a blank area, done twice
(once with half-done bits). For a longer run,
`CUT_SAVES=3000 cargo test --release --test cuts` in
`crates/settings-store`.

**The two-core test** is an exhaustive search. The first core runs the
crate's own machines (wait until the job box is empty, hand over, park
the second core for a flash write, let it go); the second core is a
model of the firmware's `worker` and `parked`, step for step. Each step
touches at most one shared word, so the search can put the other
core's steps between any two of them. On top of that the second core
may hang at any point, and the time may jump past every limit. The test
checks that the first core never touches the screen while the second
sends, never uses the flash unless the second core is parked, never
puts a job into a full box, never loses a wake-up, and with a hung
second core always ends at a limit instead of waiting for good.

**The send-plan model** (feature `model` of
[boards/cyd/send-plan](../boards/cyd/send-plan/src/model.rs), PC only)
plays the firmware's picture send and the console's frame loop over
recorded frames in simulated time, using this crate's timing rules (the
ones the firmware itself calls) and a copy of the rest of the firmware's
send, and counts what the board counts (late windows, windows that cross
the panel's drawing line). The CPU time of the plan and the loops is an
estimate in the model; the board gives the real one.

## Checks on built files

Some mistakes only show in the built output, so the build scripts read
what the compiler and linker made.

- **Game files load byte for byte.**
  [games/chip/check_load.py](../games/chip/check_load.py) loads each
  `GAME.BIN` the way the console does, into a model of the chip's
  memory, and compares every byte the linker placed. It exists for one
  quiet mistake: the linker may leave a small gap before a section that
  needs 8-byte alignment, a gap that is in memory but not in the file,
  so everything after it would land too early. No other check noticed
  it.
- **Stack frames.**
  [games/chip/check_frames.py](../games/chip/check_frames.py) reads the
  stack frame size of every function of a linked game from its
  disassembly and stops on one over its limit. The limits are
  estimates and a guard against one known mistake (a game's whole state
  built inside the frame of its update call, a 25.8 KB frame found on
  the board on 2026-11-22), not a proof that a game fits the console's
  stack. The reader has its own self-test,
  [test_check_frames.py](../games/chip/test_check_frames.py), run before
  any game is built.
- **Flash parts.** For a game that keeps part of itself in the
  firmware's flash, [games/chip/flash_split.py](../games/chip/flash_split.py)
  checks the linked game (`check`) and is then run with two deliberately
  wrong lists, which it must refuse (`selftest`), so the checker is seen
  to bite on a real game.
- **The hot block.** The firmware links the functions on the timed
  screen path together in flash, the hot block, so that no three of
  its cache lines share a flash cache set.
  [boards/cyd/hot-check.py](../boards/cyd/hot-check.py) checks a built
  firmware: every listed function found and in its part of the block,
  no flash call out of the block from the timed paths except panics,
  the literals inside it, and its size in cache lines. The last line of
  its output is the summary and ends `ok` or `FAILED`.
- **The firmware's flash windows for games.**
  [boards/cyd/game-flash-check.py](../boards/cyd/game-flash-check.py)
  checks that the windows sit where `game_api::layout` says and hold
  each game's flash part byte for byte; with `--absent`, that a build
  without the feature has no trace of them.
- **Game slots in the simulator.** The simulator's
  `chip_game_files_pass_the_header_check` runs the console's own header
  check on every built chip file, and
  `no_file_on_the_sample_card_pretends_to_be_a_game` checks that the
  sample card holds no file with a game header (the sample card's
  `GAME.BIN` files are deliberately a line of text).

Tests that need the built game files read them from `GAME_FILES_DIR`.
The PC check script sets it; by hand, run `games/build-chip.sh` first
and export the folder. Without it, the simulator's chip-file tests print
a note and pass without checking anything.

## The two check scripts

There are two scripts, one for each side. Neither needs the board, and
neither opens the serial port or flashes anything.

| | [boards/desktop/check.sh](../boards/desktop/check.sh) | [boards/cyd/check.sh](../boards/cyd/check.sh) |
|---|---|---|
| Builds the games for the chip and checks they fit | yes | yes (the firmware's demo card embeds them) |
| Format check, clippy with warnings as errors, tests | 25 folders: the shared crates, the simulator and every game, each with default and all features; the simulator also with no window | the three PC-tested firmware crates (hci-rules, send-plan, handover); the firmware itself gets the format check |
| Builds for the chip | the shared crates, compile only, for the ESP32 and for a bare PC target with no standard library | the firmware: clippy on 11 feature sets, then 154 builds |
| Checks on built files | game files (load, stack frames, flash parts) | the same game files, plus the firmware's hot block, game slots and game flash windows on 7 of the builds |
| Runs | the simulator once, three frames to a PNG; checks the no-window build pulls in no SDL3 | nothing |
| Takes | about 75 seconds after a change (its README) | about 22 minutes (the script's own header) |
| Last line | `check: ok` | `all 154 builds ok` |

**The PC script** checks, in its own folder, each crate on its list:
`game-api`, `game-kit`, `game-3d`, `board-api`, `console-video`,
`console-core`, `game-loader`, `settings-store`, `touch-filter`,
`pad-host`, the simulator and all fourteen game folders. It does **not**
cover `crates/debug-link`, `crates/test-card`, `crates/pad-proto`,
`crates/pad-view`, `crates/pad-store` or `tools/boardctl`; those are
tested with `cargo test` in their own folders when they change. Window
tests use SDL's "offscreen" driver, so no window appears.

**The firmware script** builds every combination of the seven plain
features (`demo-card`, `sd-card`, `touch`, `link`, `tour`,
`wide-strips`, `store`): 128 one-core builds. Then 9 builds with the
gamepad (`pad`), a measuring switch or the boot animation, 10 with the
second core (`core2`), and 7 builds that are checked after linking, all
with the gamepad: two cores and one, each with and without the boot
animation; the same two with the boot animation and the game flash
parts; and the two-core one with the boot animation and the measuring
switch `xip-probe`. Each of those 7 must contain the game slots'
start-up log lines (the two with game flash parts also the flash part
check's), and must pass `hot-check.py`, `xip-check.py` and
`game-flash-check.py` (the last two with `--absent` on the builds
without that feature). A failure
prints `FAILED (build)`, `FAILED (game slots)`, `FAILED (game flash)`,
`FAILED (hot-check)` or `FAILED (xip-check)`. It builds into its own
folder, so the firmware ready to flash is left as it was, and it refuses
to run while one of the measuring variables of `build.rs`
(`CYD_LAYOUT_PAD`, `CYD_DEMO_EXTRA`) is set.

The firmware's three PC-tested crates must be run from the repository
root with their own toolchain, or cargo applies the chip settings of
the firmware folder:

```sh
cargo +1.99.0 test --offline --manifest-path boards/cyd/handover/Cargo.toml
cargo +1.99.0 test --offline --manifest-path boards/cyd/send-plan/Cargo.toml --all-features
```

## Measuring on the board

The board is judged through its own serial port.

- **The log.** At start the firmware logs what matters for the
  layout: the `panel:` line (after any change to the memory layout it
  must say `fastest_read_us=17` and `calibrated=true`), the line
  `game slots ok: code 0x40092000 + 57344 bytes, ...` that compares
  the linker's slots with the loader's, and, with the settings store or
  the gamepad, the CRC-32 of the saved pairing area, which shows the
  area unchanged without ever reading it to the PC. Every 30 seconds it
  logs its measurements (`perf`, `card`, `screen`, `mem`); the `send`
  report counts windows sent `late` and windows that `crossed` the
  panel's drawing line.
- **The debug link and boardctl.** With the feature `link`,
  [boardctl](../tools/boardctl/README.md) takes screenshots
  (`boardctl shot`, of the panel's memory or of the console's 160x120
  picture), presses buttons on any of four pads (`key`), taps
  (`tap`), plays a scripted finger (`touch`), reads the counters
  (`stats`) and prints the log (`log`). Measuring letters sent with
  `boardctl log --send X` log their result at once. Opening the port
  does not restart the board; only `reset` and `log --reset` do. Where
  the board is shared, every command that opens the port is wrapped in
  `flock <lockfile> timeout 150 ...`, because boardctl takes no lock
  itself.
- **Comparing pictures.** `boardctl compare a.png b.png` compares a
  simulator picture with a board screenshot on the panel's 16-bit
  colours and prints how many pixels differ, where, and which colour
  bits were wrong (exit code 0 the same, 1 different, 2 an error).
  `--scale-expected 2` compares a 160x120 picture with the 320x240
  panel. `boardctl testcard` draws the debug link's test pictures on
  the PC, so a screenshot of the firmware's test card can be checked
  pixel by pixel. A screenshot that arrives damaged is asked for again
  (up to three tries by default); in the measured runs on the board the
  checksums caught every damaged one.

**PC numbers are never board numbers.** The working READMEs label every
number by where it came from: measured on the board (often with the
date), a built size from the linker ("linker numbers, not measured on
the board"), or an estimate. BENCH, the speed test among the games,
shows the rule plainly: the simulator calls it on time whatever the
work costs, so only the board's numbers count. Board timings are
compared only between firmware flashed at the same flash speed: on
2026-11-19 the same firmware file stepped CAMBER in 17.8 ms at 80 MHz
against 18.3 ms at 40 MHz, measured on the board, so numbers taken
before the switch to 80 MHz are not compared with later ones. Measuring
switches (`send-ab`, `hang-test`, `wide-strips`, `xip-probe`,
`CYD_LAYOUT_PAD`, `CYD_DEMO_EXTRA`) never go into a build handed over
for use.

## Where this comes from

- [decisions.md](decisions.md)
- [boards/desktop/README.md](../boards/desktop/README.md), [boards/desktop/check.sh](../boards/desktop/check.sh), [boards/desktop/tests/headless.rs](../boards/desktop/tests/headless.rs), [boards/desktop/tests/games.rs](../boards/desktop/tests/games.rs)
- [boards/cyd/README.md](../boards/cyd/README.md), [boards/cyd/check.sh](../boards/cyd/check.sh), [boards/cyd/hot-check.py](../boards/cyd/hot-check.py)
- [boards/cyd/handover/src/lib.rs](../boards/cyd/handover/src/lib.rs), [boards/cyd/handover/tests/interleave.rs](../boards/cyd/handover/tests/interleave.rs)
- [boards/cyd/send-plan/src/lib.rs](../boards/cyd/send-plan/src/lib.rs), [boards/cyd/send-plan/src/model.rs](../boards/cyd/send-plan/src/model.rs), [boards/cyd/send-plan/tests/model.rs](../boards/cyd/send-plan/tests/model.rs)
- [boards/cyd/hci-rules/src/lib.rs](../boards/cyd/hci-rules/src/lib.rs), [boards/cyd/hci-rules/tests/rules.rs](../boards/cyd/hci-rules/tests/rules.rs)
- [crates/board-api/README.md](../crates/board-api/README.md), [crates/game-kit/src/testing.rs](../crates/game-kit/src/testing.rs)
- [crates/console-core/README.md](../crates/console-core/README.md), [crates/console-core/tests/golden.rs](../crates/console-core/tests/golden.rs)
- [games/camber/tests/golden.rs](../games/camber/tests/golden.rs), [games/README.md](../games/README.md), [games/build-chip.sh](../games/build-chip.sh)
- [games/chip/check_load.py](../games/chip/check_load.py), [games/chip/check_frames.py](../games/chip/check_frames.py), [games/chip/flash_split.py](../games/chip/flash_split.py)
- [crates/pad-host/src/lib.rs](../crates/pad-host/src/lib.rs), [crates/pad-host/tests/scripted.rs](../crates/pad-host/tests/scripted.rs), [crates/pad-host/tests/fuzz.rs](../crates/pad-host/tests/fuzz.rs)
- [crates/settings-store/README.md](../crates/settings-store/README.md), [crates/settings-store/tests/cuts.rs](../crates/settings-store/tests/cuts.rs)
- [crates/game-loader/tests/real_files.rs](../crates/game-loader/tests/real_files.rs)
- [crates/debug-link/README.md](../crates/debug-link/README.md), [tools/boardctl/README.md](../tools/boardctl/README.md)
