# lantern: LANTERN

A first-person walk through an old mine by lamplight. You carry a lamp
through dark tunnels, pick up crystals, keep the lamp filled with oil,
find the lift's crank and ride the lift up. Moths drink your oil,
crawlers bite, and digging machines roll along their tracks. The lamp's
flash stops all of them for a while.

The game is also a test of how much 3D the console can draw. It has
three detail settings with very different screen costs, and it counts
its own work (see [Numbers](#numbers)).

All of it is original: the names, the mine, the creatures, the maps,
the textures and the screen layout were made for this game.

Words used below:

- **ray casting**: the 3D view is drawn one screen column at a time. For
  each column the game follows a line (a "ray") from the player across
  the grid map until it hits a wall. The wall's distance sets the height
  of that column.
- **texture**: a small picture painted onto a wall, floor or ceiling.
- **sprite**: a flat picture that stands in the 3D world and always
  faces you (creatures, crystals, oil cans).
- **send**: the console copying the changed parts of the 160 x 120
  picture to the screen. On this board, sending takes far longer than
  drawing.

## How to play

Start LANTERN from its tile. The title has a menu:

| Line | What it does |
|---|---|
| PLAY | start at the chosen level (START also plays, from any line) |
| LEVEL | the level to start on, 1 to 4 (left/right) |
| DETAIL | LIGHT, NORMAL or HEAVY (default), see [The three details](#the-three-details) |
| RATE | 30 or 60 (default) pictures a second |
| LEAVE | back to the home screen |

All three details and both rates can always be chosen; the game starts
on HEAVY and 60 (the owner's choice of 2026-11-23; before that it
started on LIGHT and 30). The detail can also be changed while you walk (X, see
[Controls](#controls)). The rate is chosen on the title only: to change
it during a level, open the pause menu, pick TITLE, change RATE and
PLAY again (the level starts from its beginning).

Then you walk. The bottom line shows your hearts (3), the oil bar, the
crystals found and the three touch boxes FLASH, MAP and II (pause). A
short message under the view says what just happened ("A CRYSTAL",
"LOCKED: FIND ITS SWITCH", "OUCH!").

### The rules

- **The lamp** burns all the time. A full lamp lasts 200 seconds of play.
  The fuller it is, the farther it lights: from 7 map cells (full) down
  to 2.5 (empty). An empty lamp does not end the game, but you see very
  little.
- **Oil cans** fill the lamp by 2500 of its 6000 units.
- **The flash** costs 700 units of oil and works only with at least that
  much left. It stops every creature in front of you within 5 cells:
  crawlers curl up for 5 s, diggers stop for 3 s, moths fly off and
  leave the level. A flash needs 0.7 s before the next one.
- **Moths** fly to your lamp and drink oil while they are close. They
  never hurt you.
- **Crawlers** walk toward you and bite: one heart. After a bite you
  cannot be hurt again for 1.5 s, and you are pushed back.
- **Diggers** roll back and forth on a fixed track, east-west or
  north-south, and cost a heart if you are in the way.
- **Flasks** give back one heart. A flask stays where it is until you
  have lost a heart.
- **Doors** open when you face one and press the use button; a door
  slides sideways as it opens (about half a second). **Gates**
  open only by their **switch** (a lever on a wall, also worked with the
  use button); a switch opens every gate of its colour.
- **The lift** is the way out. It works only once you carry the
  **crank**. Using it ends the level, with a card that shows the
  crystals found, the moths sent away, the creatures stopped and the
  time.
- With no hearts left you faint ("THE LAMP FALLS"), and the level starts
  again from its beginning. After level 4 a last card shows the crystals
  of the whole run and how often you fainted.

Crystals are only for points: you can leave a level without them.

### The levels

Counted from `src/level.rs`:

| Level | Name | Map size (cells) | Crystals | Moths | Crawlers | Diggers | Doors |
|---|---|---|---|---|---|---|---|
| 1 | OLD ENTRANCE | 12 x 12 | 6 | 1 | 0 | 0 | 2 |
| 2 | TIMBER HALLS | 16 x 16 | 8 | 1 | 3 | 0 | 3 |
| 3 | CRYSTAL DEEP | 24 x 24 | 12 | 4 | 3 | 2 | 7 |
| 4 | DEEP WORKINGS | 32 x 32 | 13 | 5 | 5 | 3 | 11 |

### Controls

Every control works with the gamepad alone. The keys are the
simulator's (`boards/desktop/README.md` has the whole key table).

| Gamepad | Simulator key | While walking |
|---|---|---|
| d-pad up / down | `Up` / `Down` | walk forward / back |
| d-pad left / right | `Left` / `Right` | turn |
| left stick | `I` `K` `J` `L` | walk forward / back and step sideways, as far as the stick is pushed |
| right stick left / right | `F` / `H` | turn, as far as the stick is pushed (twin sticks: walk with the left, look with the right) |
| L / R shoulders | `Q` / `W` | step sideways |
| left / right trigger | `1` / `2` | step sideways, as far as the trigger is pulled |
| A | `Z` | use: open a door, work a switch or the lift. With nothing to use ahead (open floor, a plain wall, a switch already on) it does nothing: it never flashes and never costs oil |
| B or Y | `X` or `S` | flash |
| X | `A` | next detail (LIGHT, NORMAL, HEAVY); a short note shows the new one |
| SELECT (-) | `Backspace` | the map |
| START (+) | `Return` | the game's pause: RESUME, RESTART LEVEL, TITLE |
| HOME | `Tab` | the console's own menu (the game is not called; its time stops) |

**Touch** (mouse button in the simulator):

| Where | What it does |
|---|---|
| the top third of the view | walk forward; left or right of the middle also turns |
| the bottom third of the view | walk back (and turn the same way) |
| the middle third, left or right of the centre | turn on the spot |
| a tap in the centre of the view | use (like A: with nothing to use ahead, nothing happens) |
| FLASH, MAP, II boxes | flash, the map, the pause menu |

The stick has a dead zone (a middle range that counts as zero) of 4000
of 32767, so a stick that does not quite centre does not make you
drift. In menus: up/down (d-pad or stick) moves, left/right changes, A
chooses, a tap on a line chooses it. In the pause menu START or B
closes it. On the map, SELECT, A, B or a tap goes back to the walk;
START goes to the pause menu.

While the map or the pause menu is open, the world waits. The level
time on the card counts only time spent walking.

### The three details

| Detail | 3D view | Rays (columns) | Floor and ceiling |
|---|---|---|---|
| LIGHT | 120 x 80 window, in a frame | 60, each 2 pixels wide | plain, shaded by distance |
| NORMAL | 160 x 104, the whole width | 160 | plain, shaded by distance |
| HEAVY (default) | 160 x 104 | 160 | textured |

The walls, the lamp light and the sprites are the same in all three.
LIGHT exists because sending the picture is the expensive part on this
board: a smaller view means fewer pixels to send.

## How it is built

A `no_std` library that uses only `game-kit`: no heap, no floating
point, no `unsafe`, no panic handler of its own. All maths is in whole
numbers:

- **fixed point**: whole numbers that stand for fractions. Positions are
  "Q12": 4096 means one map cell. The sine table is "Q14": 16384 means
  1.0.
- **DDA** ("digital differential analyser"): the way each ray walks the
  grid, one cell edge at a time, with only adds and compares per step.

| File | What is in it |
|---|---|
| `src/render.rs` | the 3D view: rays, walls, floor and ceiling, sprites sorted far to near against a per-column depth list, and the work counter (`Cost`) |
| `src/world.rs` | one level in play, moved in fixed ticks of 1/60 s: walking with wall sliding, doors, gates, switches, things to pick up, creatures, the flash |
| `src/level.rs` | the four maps, as tables of letters (the file's top lists every letter) |
| `src/art.rs` | the palette (28 materials x 8 shades + 16 colours for text and boxes = 240), 14 wall/floor textures of 32 x 32, 13 sprites of 16 x 16, all built while compiling |
| `src/fixed.rs` | the sine table, angles, distances, a small random-number maker |
| `src/app.rs` | the screens (title, play, map, pause, level cards, the end) and the controls |
| `src/hud.rs` | what is drawn around the view: the status line, menus, the map, touch areas |
| `src/lib.rs` | the console side: input, palette, frame rate, the video hint, log lines |

The game asks the console for 60 (or 30) pictures a second while you
play, and for "only when something changed" on the title, the map, in
the pause menu and on the cards, so a still screen costs nothing. Every
240 ticks (4 s) of play it logs one line with the work of the last
picture (one taken at LIGHT: `lantern: cost LIGHT rays 60 steps 206 mul
447 div 353 px wall ...`).

The home tile (`art.txt`, 36 x 36, 16 colours) is a mine tunnel with
timber frames, rails, a crystal and the lamp; `games/art/make_art.py`
turns it into `ART.BIN`.

## Numbers

Every number says how it was found:

- **COUNTED**: counted exactly, by the game's own counter or by the
  tests, in the simulator, or from the chip build's machine code.
- **MEASURED (simulator)**: what the simulator did.
- **ESTIMATE**: worked out from counts and the board's measured send
  speed. The board has since measured this game (level 4, one core
  against two, 2026-11-18): `boards/cyd/README.md`, "One core or two".
  The numbers below were not redone from it.

**Size on the chip** (COUNTED, `games/build-chip.sh`, 2026-11-23): code
32,040 of 57,344 bytes (56 %); data 40,036 of 49,152 bytes (81 %: 33,992
bytes of constants such as textures, maps, the palette and the game's
6,040-byte starting state, 6,040 of starting values, 4 zeroed). The
starting state is kept as a constant so the game's start copies it
straight into place instead of building it on the stack.

**Stack on the chip** (COUNTED from the chip build's machine code, the
`entry` of each function): the game's `update` takes 6,480 bytes, of
which about 6 KB is the game kit's copy of the game's state at the
start; `App::draw` adds 528 and nothing it calls more than 112. The
console's own deepest stack use was 18,560 of 38,092 bytes in the
owner's build with `pad` (MEASURED on the board, boards/cyd/README.md;
not measured while a game runs); the owner's build now has 33,892.

**Work for one picture while walking** (COUNTED by `tests/cost.rs` over
2,040 pictures each on levels 1 and 2):

| Detail | Changed pixels, mean / worst | Pixels sent, mean / worst | Ray steps, mean / worst | Multiplies / divides, mean |
|---|---|---|---|---|
| LIGHT | 4,306 / 9,667 | 8,215 / 10,020 | 161 / 417 | 394 / 349 |
| NORMAL | 7,463 / 16,707 | 14,361 / 17,060 | 430 / 1,114 | 999 / 861 |
| HEAVY | 9,446 / 16,640 | 15,979 / 17,060 | 430 / 1,114 | 1,161 / 942 |

"Pixels sent" is more than "changed", because the console sends whole
rectangles (at most 16 a picture). Standing still changes 0 to 6
pixels a picture (COUNTED): the lamp burns down slowly, so the oil bar
and the light change a little now and then.

**Machine instructions in the inner loops** (COUNTED from the chip
build's disassembly, the instructions run each time round the loop):
about 16 a wall pixel, about 32 for a 2-pixel LIGHT wall row, 7 a plain
floor or ceiling pixel (NORMAL), 25 to 26 a textured one, 22 a sprite
pixel, about 85 a ray step.

**Time on the board** (ESTIMATE; the frame rates measured on the board
on 2026-11-18 are in `boards/cyd/README.md`, "One core or two"). Drawing: `tests/cost.rs`, counted work x
instruction counts, 30 % extra for stalls, 240 MHz. Sending, two ways:
"wire" scales the board's measured 15.5 ms for a whole 160 x 120
picture on the wire alone (what `tests/cost.rs` prints); "present"
scales what the board measured for a game that sends the whole screen
every frame (STARDUST: present 16.3 to 16.9 ms average, 18.6 ms
worst), which includes waiting for the panel's refresh. The console's
own work per frame besides the send was about 0.6 to 1.0 ms with
STARDUST (step minus present, MEASURED on the board).

| Detail | Drawing, mean / worst | Sending, wire, mean / worst | Sending, present, mean / worst | Whole frame, mean / worst |
|---|---|---|---|---|
| LIGHT | 1.0 / 1.1 ms | 6.7 / 8.1 ms | 7.2 / 9.7 ms | about 9 / 12 ms |
| NORMAL | 2.0 / 2.2 ms | 11.7 / 13.8 ms | 12.6 / 16.5 ms | about 15 / 20 ms |
| HEAVY | 2.3 / 3.0 ms | 13.0 / 13.8 ms | 14.1 / 16.5 ms | about 17 / 21 ms |

So at 30 pictures a second (33.3 ms each) every detail should fit on
average. One thing can still make a frame late: when only part of the
screen changes, the console waits for the panel's refresh before
sending, and on the board a present of a moving home-screen row took up
to 32.7 ms (MEASURED, boards/cyd/README.md; 20.9 ms in another run). Add a wait like
that to a NORMAL or HEAVY frame and it misses 33.3 ms now and then. At
60 (16.7 ms each) LIGHT may fit (about 9 ms on average, 12 at worst,
before any such wait); NORMAL and HEAVY need about 15 to 17 ms on
average, so by this estimate they would not hold 60. The board has
since shown otherwise with two cores: level 4 HEAVY at 60 ran at 59.8
pictures a second (53.2 on one core; MEASURED, 2026-11-18,
`boards/cyd/README.md`, "One core or two"). At the starting settings of
2026-11-23 (level 1, HEAVY, 60) it ran at 58.74 pictures a second
turning on the spot for about 15 s, which is the panel's own pace, and
at 60.01 walking forward for about 6 s, with 0 crossed windows
(MEASURED, two cores, no gamepad connected, one scripted run each; the
same README, "At the starting settings").

**In the simulator** (MEASURED (simulator), 30 pictures a second,
LIGHT): the autopilot's moves from `tests/play.rs`, replayed as
simulator scripts, clear every level:

| Level | Frame cleared | Crystals | Game time |
|---|---|---|---|
| 1 | 779 | 6/6 | 23 s |
| 2 | 1,393 | 8/8 | 44 s |
| 3 | 3,039 | 12/12 | 99 s |
| 4 | 4,136 | 13/13 | 135 s |

The simulator's own send model is not the board's: it reckons a whole
screen at 30.96 ms (the board measured about 15.5 ms on the wire and
16.3 to 16.9 ms for a whole present), so its send times are about
twice too long. In it, every send in these four runs
fit in a 33.3 ms frame.

## Tests

41 tests, all in `tests/` (`cargo test --offline --locked`, about 17 s on the PC used):

| File | Tests | What they check |
|---|---|---|
| `world.rs` | 18 | maps closed all round, walking and wall sliding, doors, gates and switches, the use button doing nothing with nothing to use ahead, the crank and the lift, oil, the flash, each creature, hurting and fainting |
| `controls.rs` | 13 | every menu line, pause, map, detail change, twin sticks (left walks and steps sideways, right turns), triggers, shoulders, touch steering per detail, A never flashing while B, Y and the FLASH box do, presses kept between calls |
| `play.rs` | 2 | an autopilot finishes every level, once with only the crank and once collecting everything |
| `look.rs` | 1 | every screen draws with the game's own colours only (numbers below 240) |
| `fuzz.rs` | 2 | 12,000 calls of random input and odd time gaps never panic (overflow checks on); the view drawn from every free cell, 16 directions, every detail (35,232 views) |
| `faces.rs` | 2 | walls read left to right whichever way you look (the drawn texture columns, read back from the picture); a half-open door shows its gap on the same side of the map from both sides |
| `golden.rs` | 2 | a fixed script through the game's `game_entry`: every different picture (as a hash) and every log line match the stored record in `tests/data/golden.txt`, and two runs give the same record |
| `cost.rs` | 1 | the work and send counts above; LIGHT sends less than NORMAL, which sends no more than HEAVY; standing still sends almost nothing; it writes `.local/games/3d/cost.txt` |

`LANTERN_FUZZ=<calls>` makes the random-input test longer.

## What could come next

- Measure on the board: drawing and sending time at NORMAL, and at
  every detail at 30 pictures a second (HEAVY and LIGHT have run at 60
  there: "Time on the board" above and `boards/cyd/README.md`; the
  `perf` log lines already give step and present times).
- Sound for the flash, bites and the lift, once the console has sound
  for games.
- Saving the best crystals per level, once games can save.
- A second creature look per level.
