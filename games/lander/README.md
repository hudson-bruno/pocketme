# lander: LANDER

A moon lander. A small craft falls toward rough ground. Turn it, fire
its engine, and set it down on a flat pad: slowly, upright, and before
the fuel runs out. Each landing calls up a harder level.

The game is also a test of the console. It uses analog triggers and
sticks, motion, touch, the frame-rate and video-hint requests, and has a
calm view and a heavy view whose screen cost is counted (see
[Numbers](#numbers)).

## How to play

Start LANDER from its tile. The title has a menu:

| Line | What it does |
|---|---|
| PLAY | start a game at the chosen level (START also plays, from any line) |
| LEVEL | the first level, 1 to 20 (left/right) |
| VIEW | STILL (default) or FOLLOW, see below |
| STEER | STICK (default) or TILT: lean the craft by tilting the pad |
| RATE | 30 FPS or 60 FPS (default) |
| END | leave the game, back to the home screen |

Every option can always be chosen; the game starts on STILL, STICK and
60 FPS (60 since the owner's choice of 2026-11-23; before that it
started at 30). A "LEVEL n" card shows first (A or a tap skips it). Then you fly. The
top bar shows:

- **FUEL**: a bar; green, then amber under 25 %, red under 10 %.
- **V**: speed down, in pixels a second. **H**: speed sideways (minus is
  left). Each is green while it is safe for a landing, red when not.
- **L n**: the level. Small craft marks: craft left. The score, at the right.
- **WIND** with arrows, under the bar, on windy levels.

Each pad has a label: **X1**, **X2** or **X4**, its score multiplier.
Smaller pads pay more.

### Landing rules

The craft lands when both feet come down on the same pad and, at that
moment:

| Rule | Limit | Ending when broken |
|---|---|---|
| down speed | at most 7 px/s | CRASHED: TOO FAST |
| sideways speed | at most 4 px/s | CRASHED: SLIDING |
| lean | at most 8 steps of 256 a turn (about 11 degrees) | CRASHED: TILTED |
| both feet on one pad | | CRASHED: MISSED THE PAD |

Flying out of the sides of the world, or far above it, is **LOST**. A
crash or a loss costs one of the 3 craft and the level starts again.
With no craft left the game is over; the best score of this session is
kept (it is not saved when the game ends).

Points for a landing: (50 + the fuel left in % + 25 for a soft touch)
times the pad's multiplier. A soft touch is half the safe down speed or
less.

### Controls

Every control works with the gamepad alone. Keys are the simulator's.

| Gamepad | Simulator key | In flight |
|---|---|---|
| right trigger (ZR) | `2` (full) | engine, as much as the trigger is pulled |
| left trigger (ZL) | `1` | engine at half power: on a pad whose triggers are only on/off (Switch mode) this is a steady "hover" |
| A, d-pad up | `Z`, `Up` | engine at full power |
| left stick up | `I` | engine, as far as the stick is pushed |
| left stick left/right, right stick | `J` `L`, `F` `H` | turn, as fast as the stick is pushed |
| d-pad left/right | `Left` `Right` | turn, full speed |
| L / R shoulders | `Q` `W` | turn slowly (a quarter speed), for fine work |
| B | `X` | turn back to upright |
| tilt the pad (STEER: TILT) | hold `V`, move the mouse | lean follows the tilt: 30 degrees of pad is about 60 of craft |
| finger on the screen | mouse button | the craft leans toward the finger and fires at full power |
| START (+) | `Return` | the game's own pause: RESUME, RESTART LEVEL, TITLE |
| SELECT (-) | `Backspace` | switch the view (STILL / FOLLOW) |
| HOME | `Tab` | the console's own menu (the game is not called and its time stops) |

In menus: d-pad or left stick up/down to move, left/right to change, A
to choose, a tap on a line chooses it. In the game's pause menu START or
B closes it.

The stick has a dead zone of 4000 (of 32767) in the middle, so a stick
that does not quite centre does not drift.

### The two views

- **STILL** (default, calm): the camera stands still and jumps half a
  screen when the craft nears a side. Few pixels change per picture.
- **FOLLOW** (heavy): the camera follows the craft, and below 22 pixels
  over the ground it zooms in two times (out again above 30). The whole
  ground moves under the camera, so much more of the picture changes.

The game asks the console for the hint `MOSTLY_STILL` in STILL and
`SCROLLS` in FOLLOW, and for 30 or 60 pictures a second (RATE). When
nothing moves (a settled card, the pause menu) it asks for `ON_DEMAND`,
so the console calls it only on input.

### Levels

Each level number always gives the same ground: the ground comes from a
random number maker seeded with the level number. After level 12 the
numbers stop growing, but the ground keeps changing.

| Level | gravity px/s² | fuel (s of full burn) | world | wind px/s² | gusts ± | pads |
|---|---|---|---|---|---|---|
| 1 | 3.3 | 30 | 1 screen | none | none | 32, 24, 16 |
| 2 | 3.6 | 29 | 1 screen | none | none | 24, 16, 16 |
| 3 | 3.8 | 27 | 2 screens | 0.16 | none | 24, 24, 16 |
| 5 | 4.4 | 24 | 2 screens | 0.33 | 0.22 | 24, 24, 16 |
| 6 | 4.7 | 23 | 2 screens | 0.38 | 0.27 | 24, 16, 16 |
| 8 | 5.2 | 20 | 2 screens | 0.55 | 0.38 | 24, 16, 16 |
| 10 | 5.8 | 17 | 2 screens | 0.66 | 0.38 | 16, 16 |
| 12+ | 6.3 | 14 | 2 screens | 0.66 | 0.38 | 16, 16 |

The engine pushes 9.6 px/s² at full power. The wind blows one way per
level; gusts swing it slowly (an 8 s cycle). Wind and gusts together
stay below a fifth of gravity, so leaning as far as a landing allows
can always hold the craft still against them (a test checks this for
every level).

## How it is built

`no_std`, depends only on `game-kit`. No heap, no floating point, no
`unsafe`, no own panic handler. All maths is whole numbers, and safe for
any input (the chip build keeps overflow checks on).

| File | What |
|---|---|
| `src/lib.rs` | the console side: turns the `Ctx` into an `app::Input`, sets the palette, asks for rate and hint, logs |
| `src/app.rs` | the game's state and rules: menus, scenes, controls, scoring, camera. No `Ctx`, so tests drive it directly |
| `src/craft.rs` | one physics tick of the craft, and touching ground |
| `src/level.rs` | each level's numbers, and its ground from the seed |
| `src/fx.rs` | dust and broken pieces: a fixed list of 48 |
| `src/fixed.rs` | whole-number maths: a sine table, an angle-from-x-and-y table, a small random number maker |
| `src/draw.rs` | the picture, all colours and sizes as named constants |
| `art.txt` | the home tile's picture (36 x 36, 16 colours); `games/art/make_art.py` makes `ART.BIN` |
| `tests/` | `play.rs` (whole games, endings, menus, pauses), `controls.rs` (each control), `cost.rs` (screen cost), `fuzz.rs` (random and extreme input), `common/` (a stand-in console loop and an autopilot) |
| `examples/` | `trace.rs` prints an autopilot flight; `script.rs` writes a simulator script of one |

Numbers inside:

- Positions and speeds are Q16: 65536 is one pixel. Speeds are per
  physics tick.
- Physics runs in fixed ticks of 1/60 s taken from the game time
  (`ctx.time_us()`), not from the calls: 2 ticks a call at 30 pictures
  a second, 1 at 60, and the craft flies the same either way (a test
  checks it). After a long gap at most 4 ticks are run; the rest of the
  gap is skipped, so the craft never jumps.
- Angles: 256 steps a turn, kept with 8 more bits for smooth turning.
  0 is upright. The craft leans at most a quarter turn.
- The ground is a line through points 8 pixels apart, with one pixel of
  roughness; pads are flat runs of those points.
- The picture is drawn whole every call (ground column by column, then
  stars, pads, bits, craft, bar, cards). The console sends only what
  changed.
- The craft is a 9 x 9 picture, turned by looking up, for each screen
  pixel near it, which craft pixel lands there (no gaps when turned).

The game's state is 1,712 bytes (`SLOT` in the chip file).

## Numbers

Chip build (`games/build-chip.sh games/lander`, 2026-11-23), from the
build's own report:

| | used | slot | left |
|---|---|---|---|
| code | 28,512 bytes | 57,344 | 28,832 |
| data | 9,572 bytes (constants 7,856, starting values 1,712, zeroed 4) | 49,152 | 39,580 |

Screen cost per picture, from `tests/cost.rs` at 30 pictures a second
(RATE 30, which the tests' loop sets; game pixels; one picture is
160 x 120 = 19,200). "Sent" is what the console's diff rule sends
(at most 16 areas, which cover more than the changed pixels).

| Play | sent, mean | sent, worst |
|---|---|---|
| title (flame flickers) | 84 | 189 |
| level 1 flight, STILL | 66 | 7,221 (the LANDED card coming up) |
| level 9 flight, STILL | 112 | 9,918 |
| level 1 flight, FOLLOW | 477 | 9,661 |
| level 9 flight, FOLLOW | 1,738 | 11,457 |
| skimming the ground at top speed, STILL | 556 | 9,988 (a page jump) |
| the same, FOLLOW at 40 px up | 4,855 | 9,507 |
| the same, FOLLOW zoomed, 10 px up | 8,191 | 13,988 |
| crash, 3 s after | 112 | 5,244 |
| the game's pause opening, then | 3,840, then 0 | |

The worst picture found in play is FOLLOW, zoomed in, the craft flying
sideways at top speed low over the ground: the camera moves about 4
pixels a picture at 2x, so the ground's edge and its bands shift across
the whole width; the diff covers that with 16 areas, 13,988 of 19,200
pixels (73 %). It is never the whole screen, because the sky is one
plain colour. The only whole-screen sends are the console's own (the
game starting, the pause menu closing).

In the simulator's send model (`--trace`): a level-1 flight in STILL
averages 312 us of sending per picture (worst 835 us); a level-3 flight
in FOLLOW averages 4.2 ms (worst 15 ms). These are the simulator's
model, not the board.

On the board (MEASURED, 2026-11-23, two cores, the starting settings
STILL and 60, scripted keys, one run): 466 pictures of a level-1 flight
with three burns at 59.99 pictures a second, the console's step 4.2 ms
on average and 4.5 ms at most, 0 crossed windows
(`boards/cyd/README.md`, "At the starting settings"). FOLLOW has
not been measured there.

Tests: 13 in the library, 23 in `play.rs`, 11 in `controls.rs`, 3 in
`cost.rs`, 2 in `fuzz.rs`, 2 in `golden.rs` (54 in all). The autopilot
lands levels 1 to 9 one after the other, and 16 of the 20 title levels
on their own (it is a simple pilot; the other 4 are not proven
impossible, only harder for it).

## Running it

```bash
cargo test --offline                                 # all tests
cargo test --offline --test cost -- --nocapture      # the cost table
LANDER_FUZZ=150000 cargo test --offline --test fuzz  # a longer fuzz run
cargo run --offline --example trace 5                # watch the autopilot on level 5
cargo run --offline --example script -- 130 > land1.txt  # a simulator script
```

In the simulator (from `boards/desktop`):

```bash
cargo run --no-default-features -- --fps 60 --frames 1680 --script land1.txt --out ../../.local/shots
```

The script flies level 1 exactly as the test autopilot does, at the
game's starting 60 pictures a second (hence `--fps 60`). The
simulator's log line `lander: landed on level 1, 294 points` comes on
the frame the script's last comment names; that shows the simulator and
the tests run the game the same way.

The game logs one line per event, starting with `lander: ` (level
start, landed, crashed and why, lost, game over, view change, the end).

## What to add next

- Save the best score (the game interface has no storage yet).
- Sound: engine, touch-down, crash (no sound in the interface yet).
- A smoother FOLLOW camera at 60 pictures a second (it moves in whole
  pixels now, so slow drifts step).
- More level ideas: moving pads, a ceiling (caves), fuel pick-ups.
- A "ghost" of your best flight per level (needs storage).
- Two-player turns (pass the pad).
