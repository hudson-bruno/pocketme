# games: the console's games

Each folder here with a `Cargo.toml` is one game: a small `no_std`
library that uses only the game kit (`crates/game-kit`) and, if it
wants, the game library crates (today `crates/game-3d`, the shared 3D
polygons). The same source
runs in two places:

- **In the simulator**, the game is linked into the program
  (`boards/desktop/src/games.rs`) and starts from its tile on the home
  screen.
- **On the chip**, the game is a file on the SD card
  (`CONSOLE/GAMES/<NAME>/GAME.BIN`) that the console loads into two fixed
  memory slots and calls. `build-chip.sh` makes that file.

| Game | What it is |
|---|---|
| `bench/` | BENCH: a measuring tool, not a game. A speed test: fixed loads (fills, triangles, corners, 3D scenes, screen sends, CITY, ISLAND) and a ramp that finds the largest load that fits 60 and 30 pictures a second. Only the board's numbers count; the simulator calls it on time whatever the work costs. Moved in from a folder outside the repository on 2026-11-22 (its README, "Moving it in"); its measuring variants at other optimisation levels were not |
| `camber/` | CAMBER: hover-ship racing on real 3D tracks of flat-shaded polygons (hills, dips, banked turns, a jump, tunnels, open edges). Three tracks, three laps against seven rivals or a time trial, three speed classes; a shield, a recharge lane and boost pads; three details (LOW, MID, HIGH), three views, 30 or 60 frames a second; the touch screen alone can fly a race. Drawn by the shared `crates/game-3d`. 94 % of the code slot (overflow checks off; 2026-11-23, with the title's live backdrop) |
| `drive3d/` | DRIVE3D: a prototype with a working name, not a finished game. A rally car on gravel, grass and a ford along one short stretch with a hill, a crest and a drop, in 3D with a chase camera; played with the gamepad only (touch does nothing); four details (LOW, MID, HIGH, MAX), 30 frames a second. Drawn by the shared `crates/game-3d`; its code runs 4,252 bytes past the code slot (2026-11-23) |
| `hairpin/` | HAIRPIN: a kart race on mountain roads at dusk, drawn as a pseudo-3D road with sprites. Three courses, three laps each, four rivals; brake to charge the battery, boost to pass; three details (LIGHT, NORMAL, HEAVY), two views, 30 or 60 frames a second |
| `kestrel/` | KESTREL: fly a small courier craft through a drifting rock belt, gate by gate, and dock before the clock runs out. Real flat-shaded 3D polygons (drawn by the shared `crates/game-3d`); three missions, three details (LOW, MID, HIGH), three views, 30 or 60 frames a second |
| `lander/` | LANDER: a moon lander. Land on a flat pad, slowly and upright, before the fuel runs out; 20 levels, 3 craft, two views, 30 or 60 frames a second |
| `lantern/` | LANTERN: a first-person walk through an old mine by lamplight, drawn by ray casting. Four levels; find the crank, work the lift, keep creatures off with the lamp's flash; three details (LIGHT, NORMAL, HEAVY), 30 or 60 frames a second |
| `mercator/` | MERCATOR: space trading in real flat-shaded 3D polygons. Fly a small ship through a galaxy of 136 star systems (10 galaxies, each from one seed); buy cheap, sell dear (held keys go on trading), dangerous systems pay more; fight raiders with a laser whose range the view shows; better gear, auto-dock, a chart; a surveyors' charter to win; player 2 can be the gunner; three details, two views, 30 or 60 frames a second; effects, station traffic and convoys, two hulls per ship kind and rocks to mine (its README, "More life"). Before the planet its code ran 12,420 bytes past the code slot (2026-11-22). Since the planet steps G1 to G4 (2026-11-23) it also lands on planets: the pilot walks out, cuts seams, strips wrecks and fights drones against a site clock (its README, "The planet"); it is built with a flash part, and its RAM code runs 18,628 bytes past the code slot, 2,816 bytes of the slots left (2026-11-23; "Flash parts" below) |
| `padtest/` | PADTEST: shows every input (12 buttons, sticks, triggers, touch, motion of player 1, and which of the 4 players are connected with their held buttons). Sleeps when nothing changes |
| `proto3d/` | PROTO3D: a prototype with a working name, not a finished game. It tries out the showcase 3D platformer: a small hero runs, jumps and lands on floating islands, with a follow camera; played with the gamepad (touch works in its AREAS menu only); four test areas (ISLAND with creatures, MACHINES, SAIL, STONE); four details (LOW, MID, HIGH, MAX), 30 frames a second. Drawn by the shared `crates/game-3d`; its code runs 15,824 bytes past the code slot (2026-11-23) |
| `rooftop/` | ROOFTOP: a platformer. Eight rooms of run, jump, wall jump and one air dash; counts time and retries; played with the gamepad (touch works in its menus only); 30 or 60 frames a second |
| `skimmer/` | SKIMMER: top-down air hockey for 1 to 4 players on one table, a goal slot at each end. One mallet a side or two a side (2 VS 2); the computer (EASY, NORMAL or HARD) plays every side nobody holds; the touch screen can play one side; first to 5, 7 or 11, or a timed match; a PLAIN or WILD table; 30 or 60 frames a second, none while nothing moves |
| `stardust/` | STARDUST: a toy. A comet catches stars, at 30 frames a second |
| `tundra/` | TUNDRA: top-down arcade ice hockey for 1 to 4 players, the first multiplayer game. Two teams of two skaters and a goalie; the computer (EASY or HARD) plays every skater nobody holds; the touch screen can be player 2; 30 or 60 frames a second, none while nothing moves |
| `art/` | not a game: `make_art.py`, the tool that turns a game's `art.txt` into its tile picture `ART.BIN` |
| `chip/` | not a game: the chip build's linker scripts, packing tools, the flash-part splitter and checks (loading, stack frames, flash parts) |

Starting settings (the owner's decision, 2026-11-23): a game with a
detail setting starts at its level called HIGH (HEAVY in HAIRPIN and
LANTERN; HIGH, not MAX, in DRIVE3D and PROTO3D), a game with a 30 or 60
setting starts at 60, and MERCATOR starts in its CHASE view. Every
other option stays in each game's menus, and the settings are not saved:
each start of a game begins from these. STARDUST, DRIVE3D and PROTO3D
have no 60 setting. Each game that has one was run at least
once on the board at these settings on 2026-11-23: 58.7 to 60.0 pictures a
second, 0 crossed windows (`boards/cyd/README.md`, "At the starting
settings").

BENCH (`bench/`) is a measuring tool, not a game: it is built, checked
and put on the cards like a game so that it runs on the board the same
way, but it has nothing to play. DRIVE3D (`drive3d/`) and PROTO3D
(`proto3d/`) are prototypes: built, checked and put on the cards like
games, with working names, to try out a rally and the showcase
platformer before either becomes a game.

A game folder holds its crate, its `INFO.TXT` (the tile's name, tag and
colour, copied byte for byte to the card) and, if it has one, its tile
picture's source `art.txt` (see "The tile picture" below).

Rules for a game: no heap, no floating point, no `unsafe`, and nothing
but `game-kit` (the kit has the screen, input, time, log and video
settings) and the game library crates. A game library is compiled into
each game that uses it, never into the console, and follows the same
rules. Today there is one:

- `crates/game-3d`: flat-shaded 3D polygon models in whole numbers
  (triangle filler, clipping, back faces, painter's order, shading).
  KESTREL, CAMBER, PROTO3D and DRIVE3D use it; its README says how to draw a model and
  which choices a game makes while compiling.

Pixel art is drawn in code or in small tables. The console's
pause menu and notices go over any game; a game never draws its own
screen code.

## Reading the players

The console hands a game up to four players, one per gamepad (game
interface version 3). Player 1 is index 0. Touch is shared, not a
player's.

```rust
fn update(&mut self, ctx: &mut Ctx) -> Flow {
    // One player: the old shortcuts read player 1.
    if ctx.pressed(button::A) { /* ... */ }
    let (x, y) = ctx.left_stick();

    // Several: each player on its own. `player(n)` past 3, or a pad that
    // is not there, reads as nothing held and not connected.
    for n in 0..game_kit::PLAYERS {
        let p = ctx.player(n);
        if !p.connected() {
            continue;
        }
        let (x, y) = p.left_stick();
        if p.pressed(button::A) { /* player n + 1 pressed A */ }
        let _ = (p.held_bits(), p.triggers(), p.motion());
    }
    let _ = (ctx.has_pad(1), ctx.pad_count());
    Flow::Run
}
```

`pressed` and `released` are per player: the same button on two pads is
two presses. A pad that goes away arrives once as not connected with
everything it held as released (the loop above skips that call, so a
game that acts on a let-go reads `released` before checking
`connected`). Only player 1 opens the pause menu
(Home); players 2 to 4 reach only the game. In the simulator the
number pad is player 2, and scripts drive all four (`p2 press south`,
see `boards/desktop/README.md`). TUNDRA (`games/tundra/src/lib.rs`,
`keys_of`) and SKIMMER (`games/skimmer/src/lib.rs`, `keys_of`) read all
four players this way and let START on any pad open their own pause
menu.

## Add a new game in five lines

1. `cargo new --lib games/mygame`, then copy `rust-toolchain.toml` and
   `.gitignore` from `games/padtest` (the folder name, in capitals, is the
   card folder: at most 8 letters).
2. In its `Cargo.toml`: `game-kit = { path = "../../crates/game-kit" }`;
   in `src/lib.rs`: `#![no_std]` at the top.
3. Write a type with `impl game_kit::Game` (`start` and `update`) and end
   the file with `game_kit::game!(MyGame);`.
4. In `boards/desktop`: add `mygame = { path = "../../games/mygame" }` to
   `Cargo.toml`, and `unsafe { LinkedGame::new("MYGAME",
   mygame::game_entry) }` to `src/games.rs`.
5. Write `games/mygame/INFO.TXT` (`NAME=MYGAME`, `TAG=SHORT TAG`,
   `TILE=RRGGBB` lines) and, if you like, `games/mygame/art.txt`. Add
   `"mygame"` to `LINKED` in `boards/desktop/tools/make_sample_card.py`
   and run it (it copies INFO.TXT and makes ART.BIN), add `games/mygame`
   to `CRATES` in `boards/desktop/check.sh`, and the folder name to
   `TILES` in `boards/desktop/tests/games.rs` and to the list in
   `the_card_folder_lists_the_sample_games` in
   `boards/desktop/tests/board.rs`.

Then `cargo run -- --window` in `boards/desktop` shows the new tile, and
`games/build-chip.sh` says whether it fits the chip.

## Building for the chip: build-chip.sh

```sh
games/build-chip.sh
```

For every game it:

1. compiles it for the ESP32 (`xtensa-esp32-none-elf`, the "esp"
   toolchain) as one library file, small (`opt-level s`), in one piece
   (fat LTO), and with a panic that stops the game instead of unwinding;
2. links it with `chip/game.x` for the two slots: code at 0x4009_2000,
   at most 56 KB; data at 0x3FFF_4000, at most 48 KB. These numbers are
   not written here or in `chip/`: the script asks crates/game-api
   (`layout`, its example `slots_x`) and writes them to `slots.x` next
   to the results, which `chip/game.x` includes. The console's linker
   script is made from the same function, so the two cannot differ.
   The data is the constants, the starting values and the zeroed
   variables together. Code longer than its slot runs on into the top
   of the data slot (the spill, below). This is the way
   `experiments/native-loader/build-game.sh` linked its test game;
3. prints the sizes and **fails** when they do not fit (see "Code past
   the code slot" below), then reads every function's stack frame and
   **fails** when one is too big (see "Stack frames" below);
4. packs the result into `<NAME>.BIN` with `chip/pack.py`: the game-api
   header (the word GAME, version 3, the slots and sizes, the entry, a
   CRC-32) and then the code and data.

5. loads the file as the console does and checks every byte against
   the linked game (`chip/check_load.py`, with this run's `slots.x`).

The files land in `boards/desktop/target/check/chip-games/files/`
(`GAMES_TARGET` names another build folder), and each game also in a
card folder there: `card/CONSOLE/GAMES/<NAME>/GAME.BIN` with the game's
own `INFO.TXT` (the card's tile: name, tag, colour) and, when the game
has an `art.txt`, its tile picture `ART.BIN` (made with `art/make_art.py`;
a source the console would refuse stops the build). The CYD's demo
card is built from that folder (boards/cyd/README.md).
`games/build-chip.sh FOLDER...` builds only those game folders (a test
game kept elsewhere). For tests only, `GAMES_SLOTS_X=<file>` links for
other slots, to make a file the console must refuse. `boards/desktop/check.sh`
runs it first, and the simulator's tests then check every `.BIN` with
the console's own header check. Nothing is flashed and no board is
needed.

The sizes on 2026-11-23, all from one build of this tree (MEASURED ON
THE PC by `games/build-chip.sh`; they change a little with each edit).
Every game but CAMBER has its overflow checks on:

```text
BENCH    code  46428 of 57344 (+0 past the slot)   data  48836 (constants 42880, starting values 5952, zeroed 4)   room left 11232 of 106496
CAMBER   code  53824 of 57344 (+0 past the slot)   data  19540 (constants 18464, starting values 1072, zeroed 4)   room left 33132 of 106496
DRIVE3D  code  61596 of 57344 (+4252 past the slot)   data  36892 (constants 26360, starting values 10528, zeroed 4)   room left 8008 of 106496
HAIRPIN  code  37604 of 57344 (+0 past the slot)   data  12484 (constants 12040, starting values 440, zeroed 4)   room left 56408 of 106496
KESTREL  code  40868 of 57344 (+0 past the slot)   data  20796 (constants 17400, starting values 3392, zeroed 4)   room left 44832 of 106496
LANDER   code  28512 of 57344 (+0 past the slot)   data   9572 (constants 7856, starting values 1712, zeroed 4)   room left 68412 of 106496
LANTERN  code  32040 of 57344 (+0 past the slot)   data  40036 (constants 33992, starting values 6040, zeroed 4)   room left 34420 of 106496
MERCATOR code  75972 of 57344 (+18628 past the slot)   data  27708 (constants 18640, starting values 9064, zeroed 4)   room left 2816 of 106496
MERCATOR flash part: code  81076 of 81920   constants  25732 of 32768
PADTEST  code   8000 of 57344 (+0 past the slot)   data   2036 (constants 1952, starting values 80, zeroed 4)   room left 96460 of 106496
PROTO3D  code  73168 of 57344 (+15824 past the slot)   data  28428 (constants 23600, starting values 4824, zeroed 4)   room left 4900 of 106496
ROOFTOP  code  19460 of 57344 (+0 past the slot)   data   8556 (constants 8288, starting values 264, zeroed 4)   room left 78480 of 106496
SKIMMER  code  35308 of 57344 (+0 past the slot)   data   7620 (constants 7104, starting values 512, zeroed 4)   room left 63568 of 106496
STARDUST code   6556 of 57344 (+0 past the slot)   data   2348 (constants 2200, starting values 0, zeroed 148)   room left 97592 of 106496
TUNDRA   code  34500 of 57344 (+0 past the slot)   data   8004 (constants 7264, starting values 736, zeroed 4)   room left 63992 of 106496
```

### Code past the code slot (the spill, stage 1)

A game's code may be longer than the 56 KB code slot. The rest runs on
past the slot's end (game_api::layout::CODE_SPILL_ADDR) and lands in
the **top** of the 48 KB data slot: the same memory, seen from the
instruction bus in reverse word order (game_api::layout; quoted from
ESP-IDF, seen to hold on the CYD on 2026-11-22). The game's data still
starts at the bottom of the data slot and grows up, so code past the
slot and data share one room:

```text
code past the slot + constants + starting values + zeroed  <=  48 KB
code                                                       <=  104 KB
```

`game_api::layout::fits` is the rule. Four places hold it and refuse a
game that breaks it: the linker (`chip/game.x`, an ASSERT that says
"GAME TOO BIG"), `build-chip.sh` (with the numbers), `chip/check_load.py`
(it loads code, data and zeroes into one model of the memory and fails
with "data over code"), and the console's loader ("GAME TOO BIG FOR
SLOTS"). The header does not change and the version stays 3: an old
console refuses such a game as too big. A game that fits its code slot
is the same file as before, byte for byte.

The size line shows one "room left" for both: the code slot plus the
data slot (104 KB) minus code and data. Code past the slot is only
reached as whole 32-bit words, like the code slot; constants and
variables stay in the data slot as before.

Three games use the spill today: MERCATOR (18,628 bytes past the slot,
2,816 bytes of shared room left, with its flash part), PROTO3D (15,824
past, 4,900 left) and DRIVE3D (4,252 past, 8,008 left), sizes above. Code there first ran
on the board at the speed of code in the slot in a probe game
(2026-11-22). PROTO3D, MERCATOR and DRIVE3D then ran from the spill on
the board in the firmwares of 2026-11-22 (MEASURED ON THE BOARD, step
time of the game's own work; MERCATOR had 12,420 bytes past the slot
then): MERCATOR with all four "More life" steps at 59.99 pictures a
second in flight, its worst single step 7.86 ms (firmware 1122p-own);
PROTO3D's ISLAND at about 30 a second at all four details, MAX 23.74 ms on
average (1122q-own); DRIVE3D at 30 a second, MAX 26.36 ms standing
(1122n). The details, and what was not measured, are in each game's
README.

CAMBER has 3,520 bytes of code slot left (2026-11-23, with the title's
live backdrop; 10,392 on 2026-11-22; with its
overflow checks off; with them on it had 5,664 left on 2026-11-19, not
measured again since). It first ran on the board from a test build
(MEASURED on the board, 2026-11-18, LAGOON, MID, CHASE: 30 frames a
second held, a step took about 18 ms on average and 20 ms at most).
The build from this tree was in the firmwares from 1122i on
(2026-11-22): the first race on LAGOON took 10.92 ms a step on average
standing at the start (11.49 ms worst) and 10.30 ms over 8 s of thrust
(11.32 ms worst), at 30 frames a second (MEASURED ON THE BOARD,
firmware 1122i). At its default 60 setting and HIGH detail it showed
59.69 pictures a second over 8 s of thrust and 58.94 over 20 s of
driving (MEASURED ON THE BOARD, 2026-11-23, firmware 1123n; the table
is in `games/camber/README.md`). CAMBER's game feature `rec` (it records races over the
log; `games/camber/README.md`) is for measuring only: a `CAMBER.BIN`
built with it is never handed over for use.

### Flash parts (MERCATOR, 2026-11-22; on the board since 2026-11-23)

A game may keep code and constants that are not needed every picture in
the firmware's flash instead of in its slots. It says which in
`games/<name>/flash.list` (only `games/mercator/flash.list` today; the
file's own comment explains the lines `hot`, `cold` and `ram`). A game
without that file is built exactly as before: same commands, same bytes.

For a game with a `flash.list`, build-chip.sh:

1. links it once as always (pass 1, with a map; for MERCATOR its ELF was
   seen to equal the plain build's, MEASURED ON THE PC, not checked by the
   script);
2. runs `chip/flash_split.py split`: a walk from the `hot` roots through
   direct calls, `callx` targets loaded with L32R and vtables, which stops
   at `cold` functions; every function it does not reach (and that no
   `ram` line pins) goes to flash, and every constant only flash code or
   flash constants point at goes with it. It writes `flash-code.x` and
   `flash-rodata.x` (input section lists) to `<files>/flash/<NAME>/`;
3. writes the game's place (`slots_x --flash <NAME>`, from
   `game_api::layout::FLASH_PLACES`) as `flash-place.x` and links again
   with `chip/game-flash.x` in front of `chip/game.x`: `.flash_text` and
   `.flash_rodata` at the place, everything else as before;
4. runs `chip/flash_split.py check` on the result (`check.txt`): no call
   or L32R from a `hot`-reachable function into flash, no hot function in
   flash, every literal in the same memory as its code, the parts inside
   the place, nothing else there. A self test refuses two wrong lists
   (`chip/test_flash_split.py` has its unit tests, run on every build);
5. cuts `.flash_text` and `.flash_rodata` out as `CODE.BIN` and `RO.BIN`
   and the rest out as the payload, and packs a **"GAMF"** file: the
   usual 36-byte header (magic GAMF) and a 32-byte extension (its own
   size, 32; code address, size, CRC-32; data address, size, CRC-32; a
   reserved 0), covered by the file's CRC; `chip/check_load.py --flash` then loads
   it as the console does and compares every byte, the flash halves too.

The firmware built with `game-flash` holds the parts
(boards/cyd/README.md, "Game flash parts"). A console without them
refuses the game ("FLASH PART MISSING", or "FLASH PART DIFFERS" when its
part has other bytes): **a new build of a game with a flash part needs a
new firmware.** The simulator links the game directly and has no flash
part.

MERCATOR, MEASURED ON THE PC (2026-11-22, this tree):

| | before | with its flash part |
|---|---|---|
| code in RAM | 69,764 (12,420 past the slot) | 51,236 (none past the slot) |
| constants in RAM | 23,744 | 13,224 |
| room left in the slots | 5,912 of 106,496 | 34,960 of 106,496 |
| flash code | none | 19,224 of 32,768 at 0x400D0020 |
| flash constants | none | 10,524 of 16,384 at 0x3F400120 |
| MERCATOR.BIN | about 100 KB | 71,600 bytes |

MERCATOR's place (`FLASH_PLACES`) was 32,768 bytes of code and 16,384
of constants then. Since step G1 (2026-11-23, the planet landing) it is
81,920 bytes of code (0x14000), both at offset 0, so the planned
planet part fits without moving it again; its constants place was
24,576 bytes (0x6000) until step G4 raised it to 32,768 (`ro_max`
0x8000 in `crates/game-api/src/file.rs`, 2026-11-23; the code place is
unchanged). After step G3 (out of the ship on foot) MERCATOR used
64,572 bytes of that code and 21,356 of the constants, with 5,240 bytes
of room left in the slots. After step G4 (2026-11-23, the planet's
slice 1 finished: drones, the scan, sparks, props, the wreck's smoke,
the pay) it uses 81,076 of the 81,920 bytes of code (844 left) and
25,732 of the 32,768 of constants (7,036 left), with 2,816 bytes of room
left in the slots; the code in RAM is 75,972 bytes (18,628 past the
slot) and the data 27,708 (MEASURED ON THE PC; `mercator/README.md`,
"Chip build", keeps the current lines). A firmware with `game-flash`
for this build must come from a tree with the new `ro_max`.

In the first split (2026-11-22) 84 functions (18,688 bytes) and 167
constants (10,493 bytes) moved; 34 flash functions were called from RAM
code, all through `cold` lines or not reached from a `hot` root (the
biggest was `chart::draw`, 2,326 bytes, `cold` in the list: it runs
from flash, through the cache, on every picture while the chart is
open; the docked screens' `hud::docked`, `hud::market` and
`hud::outfit` likewise while docked). Today 142 functions and 630
constant sections are in the part, and 45 of the functions are called
from RAM code (`<files>/flash/MERCATOR/split.txt` and the build's
`flash check` lines). On the board (2026-11-23) the planet's whole
visit, most of it flash code, was timed at LOW, MID and HIGH at 30
pictures a second (`mercator/README.md`); the chart and the docked
screens were not timed on their own.

### Stack frames: a guard against a big frame

After each game is linked, `chip/check_frames.py` disassembles it
(`xtensa-esp32-elf-objdump -d -C`) and reads the first instruction of
every function, `entry a1, N`: N is the bytes that function takes on the
stack for its own frame (a frame over 32,760 bytes uses `entry a1, 32`
and a `movsp`; that form is read too, and a frame it cannot read counts
as too big). The table of every frame, biggest first, goes to
`<NAME>.frames.txt` beside the `.BIN`; the three biggest are printed
under the size line. The build fails, and packs no file for that game,
when `game_kit::rt::restart` (where the kit builds the game's state)
has a frame over 16,384 bytes, or any other function one over 4,096.
These limits are ESTIMATES, to be confirmed on the board: the console's
stack is 29,200 bytes and the console itself used 16,752 of it at its
deepest before a game ran (both measured on the CYD), which leaves about
12,400 for a game. They guard against one mistake found on the board on
2026-11-22 (a 10.8 KB game state built inside the update call made that
frame 25.8 KB; the kit now builds it in `restart`, a frame of its own
that is given back before the first update). They do not prove that a
game fits: frames add up along a chain of calls, and the check reads
each one alone. The biggest `restart` frames today (MEASURED ON THE PC,
2026-11-23): PROTO3D 13,792 bytes, DRIVE3D 10,576,
MERCATOR 9,104, BENCH 6,000. `chip/test_check_frames.py` tests the reader on a
made-up disassembly (no chip tools needed); `build-chip.sh` runs it
before it builds anything.

### Overflow checks: on, except where a game turns them off

The chip build has overflow checks **on** by default, as the PC tests
(debug builds) do: a sum that runs over (200 + 100 in a `u8`) stops the
game, and the console is told the line number. A new game gets this without
doing anything.

A game can have them **off** in its chip build: its folder name goes
into the list `OVERFLOW_CHECKS_OFF` at the top of `build-chip.sh`
(only folders inside `games/` are matched; a name that is not a game
folder there stops the script with an error). The script then says
`build-chip: <NAME>: overflow checks OFF` before the sizes. With the
checks off, the same sum quietly wraps round on the chip: a range
mistake shows as a wrong picture or a wrong number instead of a stop.
The code is smaller and faster (CAMBER: 9,488 bytes less code and
5,776 bytes less data, COUNTED, 2026-11-19). Division by zero, an index
outside a list and `i32::MIN / -1` still stop the game: those checks
are always on. The game's PC tests (debug builds) keep the checks on,
so its tests still find a range mistake.

It is a chip-only setting, so it is in the script and not in the
game's `Cargo.toml` (that would change the simulator's build too).
Today only **CAMBER** has them off (the owner's decision, 2026-11-19,
for speed; `games/camber/README.md`). Turn a game's checks off only
after its scripted plays have run with the checks on without a stop.

Notes on the link:

- The game calls nothing in the console's firmware: everything it needs
  comes through the console's table at run time. Only a few chip ROM
  functions (`memcpy`, `memset` and the like) are named in `chip/game.x`,
  by their fixed ROM addresses, as in the experiment.
- `chip/game.x` throws away the unwinding tables (`.eh_frame`,
  `.gcc_except_table`). A panic on the chip never unwinds; the kit's
  panic handler ends the game through the console.
- The card in the simulator (`boards/desktop/card`) has only a short
  note in each `GAME.BIN`, not these files: the simulator runs the
  linked-in code. Its `ART.BIN` files are the same bytes as the ones
  made here (a simulator test compares them). Copying a `.BIN` from here
  onto a real card as `CONSOLE/GAMES/<NAME>/GAME.BIN` is the board
  track's next step.

## The tile picture: art.txt and ART.BIN

The home screen shows a game's tile as a 36 x 36 picture when the game's
card folder has an `ART.BIN`, and as the old letter tile (the name's
first letter on the `TILE` colour) when it has none or the file is
refused. A refused file is logged once (`home: picture refused: LANDER
(length)`), never shown half.

The source is `games/<game>/art.txt`, plain text with one letter per
pixel:

```text
# A comment, to the end of the line.
size 36 36
colour . 0B0F1A     # a letter and its colour as RRGGBB; at most 16
colour a F7D154
pixels
....aaaa....        # then 36 lines of 36 letters
```

Tools (python3, no outside packages):

```sh
python3 games/art/make_art.py --check games/lander/art.txt             # only check it
python3 games/art/make_art.py games/lander/art.txt OUT.BIN --png P.png  # the file and a 4x preview
python3 games/art/make_art.py --show OUT.BIN P.png                      # a preview of any ART.BIN
```

`build-chip.sh` makes the chip card's `ART.BIN`; `boards/desktop/tools/
make_sample_card.py` makes the simulator's. **After changing an
`art.txt` or `INFO.TXT`, run `python3 boards/desktop/tools/make_sample_card.py`**
(it writes `boards/desktop/card/CONSOLE/GAMES/<NAME>/`, outside the game's
folder); `boards/desktop/check.sh` fails until it is run.

`ART.BIN`, version 1 (every number little-endian):

| Offset | Size | What |
|---|---|---|
| 0 | 3 | `ART` |
| 3 | 1 | version: 1 |
| 4 | 1 | width: 36 (the console takes only its tile size, `theme::TILE`) |
| 5 | 1 | height: 36 |
| 6 | 1 | colours `n`: 1 to 16 |
| 7 | 1 | bits per pixel: 4 |
| 8 | 2n | the colours, RGB565 |
| 8 + 2n | 648 | 36 rows of 18 bytes; two pixels a byte, the left one in the high 4 bits; each below `n` |

The file is exactly `8 + 2n + 648` bytes (688 with 16 colours). The
console checks the size first and reads at most 688 bytes. The full
description is at the top of `art/make_art.py`; the console's reader is
`crates/console-core/src/art.rs`.

Each picture brings its own colours: the home screen gives each picture
it keeps 16 palette entries of its own (104 to 199), so pictures never
share or fight over colours. Those entries belong to the game while it
runs.
