# kestrel: KESTREL

You fly KESTREL, a small courier craft, down a drifting belt of rock and
old wrecks. Fly through every beacon gate on the way (each one adds time
to the clock) and dock at HALDEN station before the clock runs out.
Three missions, each harder than the last.

The game is also the console's study of **simple 3D with real
polygons**: every rock, wreck, cargo pod, gate, the station and the
craft itself are 3D models made of flat-coloured triangles, turned and
seen in perspective, in whole-number maths. The polygon code knows
nothing about the game: it was moved into the shared game library
`crates/game-3d` (2026-11-19), which other games can use too.
`src/poly.rs` and `src/fixed.rs` now only pick the library's standard
choices and KESTREL's list sizes (48 corners, 112 triangles per model).

All of it is original: the names, the craft, the station, the belt, the
missions, the models and the screen layout were made for this game.

Words used below:

- **polygon, triangle**: the 3D models are made of flat triangles. Each
  is filled in one colour; its brightness comes from which way it faces
  the light ("flat shading").
- **back face**: a triangle on the far side of a model, facing away from
  you. It is hidden behind the model's front, so it is never drawn.
- **painter's order**: things are drawn from far to near, so near ones
  paint over far ones. There is no memory for a "depth buffer" (one
  number per pixel saying how far it is), so this is the only way.
- **clipping**: cutting a triangle that sticks out of the view (or
  comes too close to the eye) down to the part that can be seen.
- **send**: the console copying the changed parts of the 160 x 120
  picture to the screen. On this board, sending takes far longer than
  drawing.

## How to play

Start KESTREL from its tile. The title has a menu (the craft turns
slowly above it, so you see the model from every side):

| Line | What it does |
|---|---|
| PLAY | start at the chosen mission (START also plays, from any line) |
| MISSION | the mission to start at, 1 to 3 (left/right) |
| DETAIL | LOW, MID or HIGH (default), see [The three details](#the-three-details) |
| VIEW | CHASE (default, behind the craft), WIDE (farther back and higher), NOSE (from the craft's nose) |
| PICTURES | 30 or 60 (default) pictures a second |
| LEAVE | back to the home screen |

Each mission starts with a briefing card. Press A (or tap) to launch.

### The rules

- The craft flies forward by itself. You steer it up, down and sideways
  inside a corridor, set the speed (boost, cruise, brake) and can throw
  it aside with a **barrel roll** (half a second, faster than steering;
  then a short wait before the next).
- **Gates** stand across the way. The next one is bright cyan, later
  ones dim blue, passed ones green, missed ones red. Through a gate:
  100 points and extra time. Past it: no time. Into its ring: it counts
  as missed and the hull takes a hit.
- **Hull**: 3 pieces. A rock, a wreck or a swinging cargo pod takes one
  and halves your speed; then the craft blinks and cannot be hurt for
  1.5 seconds. No hull left: the mission is lost.
- **Time cells** (green crystals): 50 points and 2 more seconds.
- The **dock gate** (gold) in front of HALDEN station ends the course.
  Through it: delivered, with 20 points for each whole second left and
  150 for each hull piece left. Past it, out of time, or out of hull:
  the mission failed; try again or go back to the title.
- After the third mission the end card shows the total score, gates,
  time cells, hits taken and tries.

### The missions

Every mission is the same course each time (it is worked out from a
fixed number, nothing is random while you play).

| Mission | Gates | Clock at start | Time per gate | Speed | What is new |
|---|---|---|---|---|---|
| 1 THE SHALLOWS | 8 | 24 s | 4.0 s | 22 m/s | still rocks, big gates |
| 2 HULL GRAVEYARD | 10 | 17 s | 4.0 s | 24 m/s | wrecks across the way, swinging cargo pods, drifting rocks, smaller gates |
| 3 THE SIEVE | 12 | 15 s | 3.6 s | 26 m/s | more rock, more pods, gates that slide to and fro, the smallest gates |

The tests' autopilot delivers each mission with 18 to 24 seconds left
(COUNTED, `tests/play.rs`); a person needs most of that time for
mistakes.

### Controls

With a gamepad:

| Input | While flying | In menus |
|---|---|---|
| left stick, d-pad | steer (up is up) | move, change a value |
| A, right trigger | boost | choose |
| B, left trigger | brake | back |
| right stick up / down | boost / brake | |
| L, R, or a flick of the right stick left / right | barrel roll left / right | |
| X | next DETAIL | |
| Y | next VIEW | |
| SELECT | 30 or 60 pictures a second | |
| START | the game's pause menu (RESUME, DETAIL, VIEW, PICTURES, RESTART, TITLE) | play (title) |

HOME opens the console's own menu, as in every game.

With touch alone (the CYD has no game buttons):

- **Steer**: put a finger on the view. The craft flies towards the
  finger: the farther from the craft, the harder it turns. In LOW, the
  frame round the small view steers too.
- **The five boxes along the bottom**: `<ROLL`, `SLOW`, `II` (pause),
  `FAST`, `ROLL>`. SLOW and FAST stay on until tapped again (the lit box
  shows the speed).
- Menus and cards: tap a line or a box. On the title and in the pause
  menu, a tap on a setting steps it to its next value. A tap beside the
  pause card goes back to flying; a tap on the card but on no line (its
  PAUSED heading, its edges) does nothing.

The tests fly all three missions with touch alone (`tests/controls.rs`).

### The three details

Every setting can be changed while flying (X, Y, SELECT, or the pause
menu). The game starts with HIGH, CHASE and 60 pictures a second every
time (since 2026-11-23, the owner's choice; before that LOW, CHASE and
30, the setting that holds 30 pictures a second on one core by the rule
of thumb in [Numbers](#numbers)).

| Detail | View | Drawn up to | Models |
|---|---|---|---|
| LOW | 128 x 80 inside a still frame | 65 m | rocks farther than 25 m with 8 faces, nearer with 20; a gate stops being drawn just before you reach it (see below) |
| MID | 160 x 96, the whole width | 90 m | 8 faces from 45 m |
| HIGH (default) | 160 x 96 | 120 m | 8 faces from 70 m, the two nearest rocks with 80 faces |

Why LOW lets a gate go early: a ring that fills the view changes the
ends of nearly every row, and the console would send almost the whole
view for each of the last few pictures before you pass it. LOW stops
drawing a gate while it still fits in the view (when it is closer than
2.2 times its own size); the gate's colour and the clock already tell
whether you will pass.

Each detail also has a **triangle budget** (LOW 160, MID 240, HIGH 320,
guessed as half of each model's faces): when a picture would go over
it, the farthest rocks, wrecks, pods and cells are left out of that
picture. The craft, the gates and the station never are.

## How it is built

| File | What |
|---|---|
| `src/poly.rs` | the 3D: takes `crates/game-3d` with its standard choices (`game_3d::Standard`) and sets the list sizes (48 corners, 112 triangles) |
| `src/fixed.rs` | whole-number maths: the sine table, 3 x 3 turning matrices (1/16384) and square root come from `crates/game-3d`; the hash for the courses and the dice are KESTREL's own |
| `src/models.rs` | the model tables (corners and faces) |
| `src/art.rs` | the palette (22 materials x 8 brightness steps, and the screen colours) and the still backdrop |
| `src/course.rs` | the three missions; every rock, wreck, pod and cell along a course is worked out from a hash of its 10 m slice, nothing is stored |
| `src/world.rs` | one mission in flight, in ticks of 1/60 s: the craft, the clock, the hull, gates and hits |
| `src/scene.rs` | the settings, the camera, what is drawn in which order and with which model |
| `src/hud.rs` | the bars, the touch boxes, the menus and cards, and where a finger lands |
| `src/app.rs` | the screens (title, briefing, flying, pause, result, end) and the controls |
| `src/lib.rs` | the console side: input, palette, picture rate, log lines |

### The way a model reaches the picture (`crates/game-3d`)

1. **Model tables**: corners in 1/256 m, faces as three corner numbers,
   a material and a "part". Each part is a convex piece (the body, a
   wing); while compiling, every face is turned so that its front looks
   away from its part's middle, and its direction for the light is
   worked out. So nobody has to wind a table by hand, and a test checks
   that every model is closed and every face looks outwards.
2. **Turning and moving**: one matrix (the camera's turn times the
   object's turn, scaled) takes each corner into the camera's view.
   Objects whose surrounding ball is fully outside the view are skipped
   first.
3. **Back faces** are dropped.
4. **Clipping**: a face that crosses the near plane (half a metre in
   front of the eye) or reaches far outside the view is cut in 3D
   (Sutherland and Hodgman's way: one plane after the other). Faces only
   a little outside are not cut: the filler simply stops at the view's
   edge (a "guard band").
5. **Perspective**: x and y divided by the distance, to 1/16 of a pixel.
6. **Painter's order**: the scene sorts objects far to near; inside one
   object the renderer sorts its triangles far to near.
7. **Filling**: row by row in one colour, following each edge exactly
   with whole numbers (no rounding drift), with the "top-left rule", so
   two triangles that share an edge never both paint a pixel on it and
   never leave a gap. `fill_tri_slow`, which tests every pixel on its
   own, is the reference the tests compare it with.

**The backdrop never moves.** The camera slides with the craft but
never turns, so the stars and the ringed planet stay on the same pixels
and cost nothing to send. Only where objects are, or were, changes.
The palette is set once (a palette change forces a full-screen send).

### The models

| Model | Corners | Faces | What |
|---|---|---|---|
| craft | 32 | 48 | body, two swept wings, a fin, engine glow |
| far rock | 6 | 8 | a lumpy eight-sided stone |
| rock (two looks) | 12 | 20 | lumpy twenty-sided stones |
| near rock (HIGH) | 42 | 80 | a finer stone |
| gate | 32 | 64 | an eight-sided ring with lights front and back |
| wreck | 22 | 32 | a broken hauler: hull, bridge, a torn plate |
| cargo pod | 12 | 20 | six-sided |
| time cell | 6 | 8 | a glowing crystal |
| HALDEN station | 24 | 40 | a drum and its hub, turning |

## Numbers

Kinds of numbers here:

- **COUNTED**: counted exactly, by the tests on the PC or from the chip
  build's machine code.
- **MEASURED (PC)**: what the console's own send rule
  (`console_video::diff`, at most 16 areas a picture) gives for the
  game's pictures on the PC.
- **ESTIMATE**: worked out from those and the board's measured send
  speed (LANTERN, 2026-11-17). The board has since measured this game
  (HIGH, one core against two, 2026-11-18): `boards/cyd/README.md`,
  "One core or two". The numbers below were not redone from it.

**Size on the chip** (COUNTED, `games/build-chip.sh`, 2026-11-23):
code 40,868 of 57,344 bytes (71 %, 16,476 free); data 20,796 of 49,152
bytes (42 %: 17,400 of constants such as the models and the palette,
3,392 of starting values, 4 zeroed). The game's state (3,392 bytes, most of it
the renderer's working lists) is built as a constant, so the start
copies it into place instead of building it on the stack.

**Stack on the chip** (COUNTED from the chip build's machine code: the
`entry` instruction of each function gives its frame): the biggest
frame is the game kit's `update` with 3,952 bytes, of which 3,392 is
the kit's copy of the game's state at the start (every game has this;
LANTERN's is 6,480). Below it, `scene::draw` takes 1,280 (the list of
things to draw, kept small: a course thing is kept as its slice and
number, not as a whole), `Renderer::draw` 752, `World::tick` 384,
`Mission::things` 240, and every other function 208 or less. Deepest
chain: about 6.1 KB (update, `scene::draw`, `Renderer::draw`,
`Mat3::mul`; about 6.5 KB down a panic's path). The kit's frame is only
144 bytes under 4 KB: a bigger game state pushes it over, because the
kit's `update` holds the state that `start` returns.

**Triangles** (COUNTED): the most in one picture over the autopilot's
flights through all three missions: LOW 135, MID 188, HIGH 281; over
150,000 random scenes in any setting: 285 (`tests/fuzz.rs`).

**What one picture costs the screen** (MEASURED (PC), `tests/cost.rs`,
the autopilot through all three missions, 3,371 pictures at 30 a
second, 6,753 at 60; game pixels, one game pixel is 4 panel pixels):

| Setting | Sent, mean | Sent, worst | Pictures over 6,000 | over 11,000 | Triangles, mean / worst |
|---|---|---|---|---|---|
| LOW CHASE 30 | 1,211 | 6,033 | 0.0 % | 0.0 % | 87 / 134 |
| LOW WIDE 30 | 1,161 | 10,246 | 0.2 % | 0.0 % | 86 / 135 |
| LOW NOSE 30 | 1,164 | 8,174 | 0.6 % | 0.0 % | 64 / 125 |
| MID CHASE 30 | 2,638 | 15,624 | 8.2 % | 3.3 % | 124 / 188 |
| MID WIDE 30 | 2,448 | 15,366 | 7.7 % | 1.1 % | 123 / 187 |
| MID NOSE 30 | 2,514 | 15,372 | 8.8 % | 3.8 % | 100 / 173 |
| HIGH CHASE 30 | 2,729 | 15,624 | 8.6 % | 3.3 % | 180 / 280 |
| HIGH WIDE 30 | 2,544 | 15,366 | 7.8 % | 1.1 % | 179 / 278 |
| HIGH NOSE 30 | 2,601 | 15,372 | 9.0 % | 3.8 % | 157 / 272 |
| LOW CHASE 60 | 968 | 5,405 | 0.0 % | 0.0 % | 87 / 135 |
| LOW WIDE 60 | 901 | 10,492 | 0.1 % | 0.0 % | 86 / 135 |
| LOW NOSE 60 | 933 | 8,218 | 0.4 % | 0.0 % | 64 / 127 |
| MID CHASE 60 | 2,264 | 15,408 | 7.2 % | 2.9 % | 124 / 186 |
| MID WIDE 60 | 2,011 | 15,612 | 5.8 % | 0.5 % | 123 / 185 |
| MID NOSE 60 | 2,151 | 15,372 | 7.8 % | 3.2 % | 100 / 174 |
| HIGH CHASE 60 (default) | 2,348 | 15,408 | 7.4 % | 2.9 % | 179 / 281 |
| HIGH WIDE 60 | 2,100 | 15,612 | 6.2 % | 0.6 % | 179 / 280 |
| HIGH NOSE 60 | 2,230 | 15,372 | 7.9 % | 3.2 % | 157 / 276 |

A rougher pilot (the stick thrown hard now and then, rolls, boost
bursts; `tests/cost.rs`, 1,908 pictures at 30 a second) sends a little
more: LOW CHASE mean 1,434, worst 10,241 (one picture in a thousand over
6,000: a wreck passing close by the camera); MID CHASE mean 2,520;
HIGH CHASE mean 2,631, worst 15,361.

MID and HIGH send much more than LOW mostly in the last pictures before
a gate: they keep drawing it until you pass, when the ring fills the
view.

Other screens (MEASURED (PC)): the title sends about 350 pixels a
picture (the turning craft); a menu step about 1,200; a card or the
pause menu that stands still sends nothing, and the game then asks the
console to call it only on input.

The estimate below turned out too low: on the board, HIGH at 60 ran at
59.8 pictures a second with two cores and 59.5 with one (MEASURED,
2026-11-18, `boards/cyd/README.md`, "One core or two"). HIGH at 60 is
the default since 2026-11-23, and was measured again that day: 59.99
pictures a second over 688 pictures of mission 1 with the boost held
(the hull broke after about 11 s), the console's step 8.0 ms on average
and 11.4 ms at most, 0 crossed windows (MEASURED, two cores, no gamepad
connected, one scripted run; `boards/cyd/README.md`, "At the starting settings").

**Frames a second on the board** (ESTIMATE, from LANTERN's board
measurements: about 6,000 sent game pixels take about 12 ms and hold 30
a second; about 11,000 take about 30 ms, 26.6 a second; sends grow in
steps of the panel's 17 ms refresh. Drawing time for polygons has not
been measured; at 90 to 280 triangles and about 900 to 1,800 filled
pixels a picture it should stay a few milliseconds, like LANTERN's 3 to
4 ms):

| Setting | ESTIMATE |
|---|---|
| LOW, 30 | a steady 30; a rare single slower picture when something big passes close |
| LOW, 60 | most pictures fit one panel refresh: about 50 to 58 |
| MID or HIGH, 30 | about 30 most of the time, dropping to about 20 to 26 for a few pictures at each gate |
| MID or HIGH, 60 (HIGH, 60 is the default) | about 40 to 55, less near gates |

## Tests

From this folder:

```sh
cargo fmt --check
cargo clippy --offline --all-targets --all-features -- -D warnings
cargo test --offline --all-features
```

| File | What it checks |
|---|---|
| `tests/poly.rs` | KESTREL's own models: never painting outside the view or overflowing, a rock half off the view cut at its edge, every model closed and facing outwards. The renderer's own tests (filler, clipping, mirroring, back faces, sine, square root) moved with it to `crates/game-3d/tests` |
| `tests/rules.rs` | gates, points and time, rings, hits and the safe time, wrecks and pods, time cells, the clock, the dock, the corridor, barrel rolls, speeds, replays, gates never blocked, missions get harder |
| `tests/controls.rs` | every control on the pad and by touch alone, menus, pause, settings while flying, the picture rate asked for, all three missions flown by touch alone |
| `tests/play.rs` | the autopilot delivers every mission; settings never change the flight; all three in a row reach the end |
| `tests/fuzz.rs` | random input from every screen never panics (overflow checks on), only the game's colours, the world keeps its rules; random scenes in every setting stay within the triangle limit. Length: `KESTREL_FUZZ=600000` (run once, 2026-11-18: passed, 600,000 calls and 150,000 scenes); another sequence: `KESTREL_FUZZ_SEED=<number>`. A `--release` run needs `CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true` to keep the overflow checks (checker, 2026-11-18: four seeds of 1,000,000 calls and 250,000 scenes each, with overflow checks, passed) |
| `tests/cost.rs` | the table above (`--nocapture` shows it); LOW CHASE 30 must stay at 6,000 sent pixels on average and 8,000 at worst |
| `tests/golden.rs` | every picture and log line of scripted play, as fingerprints in `tests/data/golden.txt`; they must not change |
| `tests/look.rs` | writes pictures of every screen and real play (every detail and view) |

Pictures and tables go to `.local/next/ship/` in the checkout
(git-ignored): `shots/`, `cost.txt`, `cost-rough.txt`,
`autopilot.txt`, `touch-only.txt`. With `KESTREL_COST_DEBUG=1` the cost
test also saves every expensive picture of LOW to `debug/`.

The tile picture is `art.txt` (check it with
`python3 games/art/make_art.py --check games/kestrel/art.txt`).

## In the simulator

KESTREL is in all five places listed in `games/README.md`:
`boards/desktop/Cargo.toml`, `boards/desktop/src/games.rs`, `LINKED` in
`make_sample_card.py` (the card's `INFO.TXT` is the same as this
folder's), `CRATES` in `boards/desktop/check.sh` and `TILES` in
`boards/desktop/tests/games.rs`.
