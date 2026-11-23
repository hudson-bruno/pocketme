# rooftop: ROOFTOP

A precision platformer. A night courier with a white helmet, an orange vest and a yellow lamp
crosses the city's roofs, one room at a time, to deliver a parcel. Run,
jump, slide down walls and jump off them, and dash once in the air.
Broken glass and the street below send the courier back to the start of
the room at once. There are no lives: the end screen counts the time and
the retries.

## How to play

On the title screen choose:

| Item | What it does |
|---|---|
| `START` | a run from the chosen room to the last one |
| `ROOM n` | where the run begins (1 is a full run; a later room is practice and sets no best time) |
| `SCROLL STEP` / `SMOOTH` | how the wide room's view follows: in half-screen steps (default, calm) or every pixel (heavy) |
| `RATE 30` / `60` | 30 calls a second or 60 (default, heavier) |
| `LEAVE` | back to the console's home screen |

Reach the lit door in each room. The bar on top shows the room, its
name, the time and the retries.

### Controls

| Action | Gamepad | Simulator keys | Touch |
|---|---|---|---|
| run | d-pad left/right, or the left stick (analog: slower near the middle) | arrows, or I J K L (stick) | |
| jump (hold for higher) | A, B, L, or ZL past half | Z, X, Q, 1 | |
| dash (towards the held direction, 8 ways) | X, Y, R, or ZR past half (on each new pull) | A, S, W, 2 | |
| restart the room (counts as a retry) | START | Return | |
| back to the title | SELECT | Backspace | |
| console pause menu | HOME | Tab | |
| title menu | d-pad or stick, A or START to choose, B to leave | arrows, Z / Return, X | tap a line; left or right half changes a value |
| end screen | A or START: title, B: leave | Z / Return, X | left half: title, right half: leave |

The Switch-mode triggers are on/off only, so ZL and ZR count as pressed
past half way. The stick runs at a speed between a walk and full speed
(dead zone 6,000, full from 26,000 of 32,767); the d-pad always runs at
full speed. The dash with no direction held goes the way the courier
faces.

### Rules

- One dash between landings. Landing, or touching an awning (`=`), gives
  it back. The lamp on the courier's back is yellow while the dash is ready and dark
  grey once it is used; it flashes when it comes back.
- Glass (`^ v < >`) cuts from any side; falling off the bottom is a fall.
  Either sends the courier to the room's start at once, with the dash
  back, and adds one retry. A small puff shows where it happened.
- The hit box is fair: the courier's box is 5 by 8 pixels, the part that
  glass can cut is the middle 3 by 5, and only the sharp half of a glass
  tile cuts.
- START restarts the room and counts a retry. Time spent in the
  console's pause menu does not count.
- The best full run is kept until the game is left (nothing is saved).

## The rooms

Each of the first six teaches one idea; the last two put them together.

| # | Name | Idea | Script ticks |
|---|---|---|---|
| 1 | FIRST ROOF | run and jump; the gaps drop to the street | 142 |
| 2 | BROKEN GLASS | glass cuts; a short press makes a short hop under the low duct | 142 |
| 3 | CHIMNEY | slide down a wall and jump off it; climb between two walls | 122 |
| 4 | DASH | one dash in the air; landing gives it back | 116 |
| 5 | UPDRAFT | dash up, and up and across | 164 |
| 6 | AWNING | an awning gives the dash back: dash to it, then dash again | 138 |
| 7 | LONG ROOF | everything so far, three screens wide: the view scrolls | 474 |
| 8 | LAST ROOF | all of it, tight: a low hop with a dash, a narrow landing, glass on the wall | 158 |

A full run by the scripts takes 1,456 ticks (24.26 s) with 0 retries.
Every room has a recorded input script in `tests/data/roomN.txt` that
finishes it (`tests/rooms.rs`).

## Handling

Measured by `tests/handling.rs` (1 tick = 1/60 s, 1 pixel = 256 sub-pixels):

| Jump button held | Height | Ticks in the air |
|---|---|---|
| 1 tick | 7 px | 18 |
| 4 ticks | 11 px | 23 |
| 8 ticks | 16 px | 27 |
| 12 ticks | 19 px | 30 |
| to the top | 20 px | 36 |

- A running jump crosses 36 px. A dash on the ground covers 29 px, a dash
  up 28 px, and a jump plus an up dash reaches 48 px.
- Coyote time: 6 ticks after leaving an edge a jump still works.
- Jump buffer: a jump pressed up to 6 ticks before landing happens on
  landing (a dash: 4 ticks).
- Dash: the world freezes for 3 ticks, then 10 ticks at 720 sub-pixels a
  tick (509 on each axis diagonally), with a short trail of 4 ghosts.
- Wall slide falls at most 128 sub-pixels a tick; a wall jump pushes
  away for 8 ticks. Corners forgive 2 pixels on the head, ledges 3 pixels
  at the feet. Awnings are one-way: through from below, solid from above.

## How it is built

| File | What |
|---|---|
| `src/sim.rs` | the physics: whole numbers, 8 bits of sub-pixel, a fixed 1/60 s tick, pixel-by-pixel moves |
| `src/rooms.rs` | the rooms as tables of letters (`.` air, `#` brick, `M` metal, `=` awning, `^ v < >` glass, `S` start, `D` door, `a c` decoration); room 7 is built by a `const fn` |
| `src/draw.rs` | the 34-colour palette, the tiles, the courier, the sky and the far city, and the text line helper |
| `src/lib.rs` | the screens (title, play, end) as a plain state machine over `Keys` and `Out`; the `Game` impl at the bottom only turns `Ctx` into `Keys` and `Out` back into calls |
| `art.txt` | the home tile's picture (`games/art/make_art.py`) |
| `INFO.TXT` | the home tile's name, tag and colour |

Each call turns the time that passed into whole ticks (at most 6 a call,
so a stall never throws the courier through a wall), and presses count
in the call's first tick only. The same scripts give the same ticks at
30 and 60 calls a second. The game paints the whole picture each call;
the console sends only what changed. The palette is set once, so it
never forces a full send. No heap, no floating point, no `unsafe`; it
uses only `game-kit`.

## Numbers

Chip build (`games/build-chip.sh games/rooftop`, 2026-11-23): code
19,460 of 57,344 bytes (34 %), data 8,556 of 49,152 bytes (17 %:
constants 8,288, starting values 264, zeroed 4). Home tile art: 688 bytes.

Pixels per frame, in picture pixels (160 by 120 = 19,200), from
`tests/pixels.rs` with the console's own diff rule:

| Case | Median | p95 | Max | Pixels a second |
|---|---|---|---|---|
| one-screen rooms, 30 calls/s | 112 to 132 | 323 at most | 339 | 3,700 to 4,800 |
| one-screen rooms, 60 calls/s (default) | 100 to 110 | 289 at most | 332 | 6,000 to 7,600 |
| room 7, STEP, 30 calls/s | 110 | 310 | 12,804 (5 steps) | 13,520 |
| room 7, STEP, 60 calls/s (default) | 100 | 242 | 12,804 (5 steps) | 16,086 |
| room 7, SMOOTH, 30 calls/s | 8,409 | 11,630 | 12,218 | 173,468 |
| room 7, SMOOTH, 60 calls/s (heaviest) | 5,223 | 10,804 | 12,176 | 294,315 |

A one-screen room never sends more than half the picture in one frame,
and at most 4 areas. In the simulator, the console's model of the send
time for room 7 with SMOOTH at 60 calls a second averaged 8.8 ms a frame
(max 21.2 ms); STEP at 30 calls a second averaged 0.87 ms (STEP at 60,
the default since 2026-11-23, was not run through that model). These
times are the simulator's model, not the board.

On the board (MEASURED, 2026-11-23, two cores, the starting settings
STEP and 60, scripted keys, one run): 783 pictures of running and
jumping in room 1 at 59.99 pictures a second, the console's step 4.6 ms
on average and 4.9 ms at most, 0 crossed windows
(`boards/cyd/README.md`, "At the starting settings"). The wide
rooms and SMOOTH have not been measured there.

## Tests

`cargo test --offline` here (38 tests plus 1 ignored helper):

- `tests/rooms.rs`: every room's script finishes it, with no retry.
- `tests/hazards.rs`: glass on the floor, the ceiling and both walls, a
  dash into glass, and a fall each send the courier back; the hit box
  edges; retries counted.
- `tests/handling.rs`: the jump table, dash lengths, the dash not
  refilling in the air, coyote time and the jump buffer.
- `tests/play.rs`: a full run from the title, practice, restart and
  quit, leaving three ways, the title menu (keys and touch), every jump
  and dash button, the stick, 60 calls a second, delta clamps,
  determinism, and 20,000 calls of random input.
- `tests/pixels.rs`: the table above (written to
  `.local/games/platformer/pixels/`).

## What to add next

- A saved best time and per-room bests, when the console can save for a
  game.
- Sound for jump, dash, refill and glass, when the console has sound.
- More rooms: moving parts (a lift, a swinging sign), wind, crumbling
  roofs, and a few collectables off the main path.
- An assist option: slower game speed or an extra dash.
- A replay of the best run as a ghost, from recorded inputs.
