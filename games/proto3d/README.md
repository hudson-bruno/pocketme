# proto3d: PROTO3D

A prototype, not a finished game. It tries out the movement, the camera
and the 3D picture of the showcase game (a 3D platformer on floating
islands). A small hero with a crimson top and a white cap runs, jumps
and lands, followed by a camera. The picture is made of flat-shaded 3D
polygons, drawn by the shared library `crates/game-3d`.

The specs the sources name (PROTOTYPE-SPEC.md, AREAS-SPEC.md) and the
review notes the tests name (REVIEW.md, FINAL.md, FINDINGS.md) are
working notes kept outside the repository; every number the specs set
is restated in `src/tune.rs`.

The game opens on the AREAS menu. There are four areas:

| Area | The idea | How to finish it |
|---|---|---|
| ISLAND | the first test island: run, jump, fall, land; four burrs and three tuftlings live there (see Creatures) | collect the gold nuts (six; four at LOW). The counter is at the top right |
| MACHINES | weight | pick up the brass bob, stand on the plate with it until its three pips latch it (the bridge rises), carry the bob over the bridge and up the plank, set it down on the plank's left half (the plank tips), walk up to the raised right end and jump to the ledge with the beacon |
| SAIL | the glide, against the clock | glide over four gaps, each longer than the one before, to the goal pad. The clock starts when the hero leaves the first pad; the best time is kept while the game runs |
| STONE | build your own step | the hero carries one slate slab. Raise it to climb the cliff, place it in the air to bridge the gap, and place and raise it again to reach the goal 4 m up |

Reaching an area's goal shows the DONE panel with the time, under the
top line, or at the bottom when the hero's cap would be behind it (as
on MACHINES' last ledge). MACHINES,
SAIL and STONE keep a best time while the game runs (nothing is saved
yet).

## Controls

| Action | Gamepad | Simulator keys | Touch |
|---|---|---|---|
| run | left stick or d-pad | `I` `K` `J` `L`, or the arrows | |
| jump (hold for a full jump) | A | `Z` | |
| turn the camera (and tilt it) | right stick | `T` `G` `F` `H` | |
| turn the camera 45 degrees (on release; both: behind the hero) | L, R | `Q`, `W` | |
| MACHINES: pick up or set down the bob (standing) | X | `A` | |
| SAIL: glide (hold while falling) | B | `X` | |
| STONE: place the slab (ahead on the ground, or under the feet in the air) | B | `X` | |
| STONE: raise the slab (hold while standing on it) | B held | `X` held | |
| STONE: call the slab back (in hand after half a second) | X | `A` | |
| pause menu | START | `Return` | |
| line of drawing numbers on or off | SELECT | `Backspace` | |
| DONE panel: play the area again (after half a second) | A | `Z` | |

The pause menu has LOW, MID, HIGH, MAX (the picture detail, HIGH at the
start since 2026-11-23, the owner's decision; MID before), RESTART
(the area from its start) and AREAS. UP and DOWN (or the left stick)
move, A or START chooses, B closes it and keeps the old
detail. The pause menu takes no tap.

The AREAS menu: UP and DOWN move, A or START starts the chosen area. A
tap on a line chooses it. Opened from the pause menu, B (or a tap
outside its box) goes back to play.

The line of drawing numbers: `T` triangles filled, `O` objects, `F`
front faces and polygons, `P` pixels painted, `S` steps run in this
update.

## Creatures

ISLAND has two kinds of creature, the same at every detail (only the
model's face count changes). The other areas have none. Every number is
a first guess, the owner's to change (`src/life.rs`, all ESTIMATED):

| Rule | Today |
|---|---|
| burr (the enemy): a slate dome with gold eyes, 60 cm wide, 51 cm high | four, each walking a fixed straight patrol on a flat top at 0.6 m/s, standing half a second at each end |
| running into a burr | the hero is thrown back 2.1 m/s and up 2.9 m/s (straight up when the ground does not go on 89 cm that way), no input for half a second, no new knock for one second. No lives |
| landing on a burr from above (feet at most 12 cm under its top) | it is squashed flat, gone half a second later until RESTART or a new start; the hero bounces (a small hop, higher with A held) |
| tuftling (the friend): a pale body under a teal cap, 72 cm wide | three, each at its home; it touches nothing |
| the hero within 3 m of a tuftling | it turns to face him and hops (19 cm) every 2.5 s; when he leaves it turns back |
| pause menu, AREAS menu, DONE panel | the creatures stand still |

Where they stand (cm, ISLAND's records in `src/island.rs`): burrs from
(-60, -140) 3 m along +x, from (-520, 260) on the 1 m terrace 3 m along
+z, from (-90, 1250) 1.8 m along +x, and from (-560, -590) 1.8 m along
+x; tuftlings at (-180, -300), (440, 380) and (-70, 540).

## The four details

| Detail | What it draws |
|---|---|
| LOW | 3 grass tiles across 14 m, the simplest models (a four-sided tree, a box hero, nuts without their six sides), fewer props and nuts |
| MID | 8 tiles, seven-sided trees, more props |
| HIGH | 14 tiles, two-cone trees, twelve-sided rock points (the main island's rock narrows from the island's square edge), the most props of the first three (the start) |
| MAX | everything HIGH has, and: a finer checker of half tiles near the hero, a rounder head and a domed cap on the hero, trees of three ten-sided tiers, bushes and the islets' rock points drawn twice (turned) for a rounder outline, a bevel strip on the sides of the raised blocks with a rim (the floating block, the step and ramp, the resting bob, the stone), four more grass and bush patches on ISLAND, and grass hanging over the side edges of the flat grass tops near the eye. No meadow in this build (`LOOK` 1, see below) |

MAX is sized for an average of 24 pictures a second (41.67 ms, the
owner's decision of 2026-11-22), an average and not a ceiling; the board
decides the time, the tests only hold the triangle counts. Its numbers
to trim it by, all in `src/draw.rs`, cost no code:

- `LOOK`: the ground decoration (1 none, today; 2 eight one-colour
  flower patches on ISLAND; 3 a light meadow, one near band at a sixth
  of R6's pieces). Looks 2 and 3 cost chip bytes this build does not
  have (see Numbers);
- `LIP`, `LIP_FULL`, `LIP_END`: the hanging grass's teeth and reach;
- `FINE_REACH` (cm, now 200): how far from the hero's feet the grass
  tiles get the finer checker;
- `MAX_TILES` (now 14): the grass tiles across 14 m at MAX.

The two tables below were measured before the meadow and the hanging
grass came (2026-11-22, MAX then about 27 ms on its busiest view); they
still show what the two older numbers do, not today's totals. Hero on a
50 cm grid, 32 camera turns; T and P MEASURED on the PC with the game's
own counts, the time ESTIMATED with the owner's CYD model
`0.48 + 0.01473 T + 0.000291 P` ms, NOT CHECKED on the board:

| FINE_REACH (MAX_TILES 14) | 0 | 50 | 100 | 150 | **200** | 250 | 300 | 350 | 400 | 450 | 550 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| busiest est (ms) | 25.55 | 25.63 | 25.93 | 26.33 | **27.28** | 27.87 | 28.82 | 30.19 | 30.26 | 32.08 | 33.29 |

| MAX_TILES (FINE_REACH 200) | 3 | 4 | 5 | 7 | 9 | 10 | 12 | 13 | **14** |
|---|---|---|---|---|---|---|---|---|---|
| busiest est (ms) | 24.03 | 22.06 | 23.63 | 23.72 | 23.94 | 25.09 | 25.51 | 27.24 | **27.28** |

`MAX_TILES` does not fall evenly: fewer tiles can paint more pixels.

How MAX grew on 2026-11-22, step by step (ISLAND at MAX, the hero on a
100 cm grid with 32 camera turns at the usual pitch, and the same tilted
fully down; T MEASURED on the PC, est ESTIMATED with the model above,
which was too low at MAX by up to 3 ms on the board last time):

| Step | What it adds | mean T | mean est | busiest T | dearest est |
|---|---|---|---|---|---|
| R0 | (MAX before) | 660 | 17.44 ms | 1,029 | 28.47 ms |
| R1 | the near meadow (100 cm cells) | 873 | 20.73 ms | 1,459 | 34.51 ms |
| R2 | hanging grass; near meadow 75 cm | 1,062 | 23.61 ms | 1,810 | 39.24 ms |
| R3 | near meadow 60 cm | 1,283 | 27.01 ms | 2,287 | 46.59 ms |
| R4 | the far meadow band (115 cm) | 1,478 | 29.95 ms | 2,603 | 51.32 ms |
| R5 | three-tier trees; near meadow 54 cm | 1,649 | 32.63 ms | 2,918 | 56.17 ms |
| R6 | far meadow 90 cm | 1,769 | 34.43 ms | 3,087 | 58.87 ms |

The busiest views are all tilted fully down (40 degrees). Around the
start, where the player looks most, the eight turns average 43.17 ms
estimated at R6 (2,255 triangles), a little over 41.67.

The owner found R6's meadow too busy (2026-11-22). The meadow is gone
(`LOOK` 1); the three-tier pines (R5) and the hanging grass (R2) stay,
and the creatures came. Same sweep, on the build with the creatures
but before the two repairs F1 and F2 (see Numbers): mean T 774, mean
est 19.28 ms, busiest T 1,261 (tilted fully down), dearest est
29.13 ms.

## Numbers

The busiest view per area and detail (`tests/max_level.rs`,
`max_level_report`; T and P MEASURED on the PC, est ESTIMATED with the
model above, NOT CHECKED on the board):

| Area | LOW | MID | HIGH | MAX |
|---|---|---|---|---|
| ISLAND | 15.52 ms | 18.99 ms | 24.36 ms | 29.13 ms (T 1,177, P 38,902) |
| MACHINES | 10.12 ms | 11.31 ms | 12.68 ms | 14.51 ms |
| SAIL | 9.79 ms | 9.77 ms | 10.23 ms | 11.63 ms |
| STONE | 12.19 ms | 12.85 ms | 13.43 ms | 17.18 ms |

(Hero on a 100 cm grid, 32 camera turns, `MAX_STEP=100`, 2026-11-22,
this build, that is after the repairs F1 and F2: F1 drew the LOW nut
with fewer sides, closed a sky line under the islands and moved the
DONE panel off the hero; F2 fixed the draw order around the hero.
Against the build before them, only ISLAND LOW (15.83 to 15.52 ms) and
ISLAND MAX's P (38,898 to 38,902) moved by more than 10 pixels or 1
triangle.) The most triangles in one view (MEASURED, same sweep), MAX:
ISLAND 1,230, MACHINES 397, SAIL 284, STONE 310; HIGH: 894, 272, 193,
174; ISLAND at LOW 218. Tilted fully down, ISLAND at MAX reaches 1,261.

The size on the chip (MEASURED, `games/build-chip.sh` with this folder,
2026-11-23, this build):

```text
PROTO3D  code  73168 of 57344 (+15824 past the slot)   data  28428 (constants 23600, starting values 4824, zeroed 4)   room left 4900 of 106496
```

The code runs 15,824 bytes past the code slot into the data slot's top
(the spill, see `games/README.md`); spill, data and zeroed variables
together use 44,252 of the data slot's 49,152 bytes. Keep at least
4,000 bytes of room: that is what stops MAX growing further in code.
`LOOK` 2 (flower patches) left 3,348 and `LOOK` 3 (light meadow) 1,960
(MEASURED, chip builds, 2026-11-22), both under that floor; that was on
the build before F1 and F2, which had 4,172 bytes of room (F2 has 724
more, mostly because `maths::clamp` replaced `i32::clamp`), and they
have not been built again since. The biggest stack frame is the start
(`restart`), 13,792 bytes (limit 16,384).

The play's state is laid out so that its small fields stay near its
start: when they moved past byte 511 the code grew by about 850 bytes
(MEASURED, chip builds; Xtensa reaches such fields with shorter
instructions). Any new field in `Play` can cost hundreds of bytes for
that reason; read the size line after each change.

## How it is built

- `src/tune.rs`: every starting value, with its unit. `src/maths.rs`:
  units and angle helpers.
- `src/island.rs`: ISLAND's records (blocks, props, nuts) and the
  collision. `src/area/`: MACHINES, SAIL and STONE, each with its own
  records and rules (each file's doc has them).
- `src/life.rs`: the creatures' rules (ISLAND's are placed in
  `src/island.rs`).
- `src/hero.rs`, `src/camera.rs`, `src/collect.rs`: the rules of each
  part. `src/play.rs`: one whole step. `src/step.rs`: the fixed 30 a
  second step clock.
- `src/pause.rs`: the AREAS and pause menus.
- `src/draw.rs`: the picture. `src/models.rs`: the models, made while
  compiling. Draw order: far islets, then the floors, then the raised
  blocks and everything else far to near; the blocks and the hero's
  body (not his arms) are boxes, and whatever lies behind another by a
  plane between them is drawn first, so a wall behind the hero never
  covers him and a pad over him hides him (a floor over him is drawn
  after him).

No heap, no floating point, no unsafe; it depends only on `game-kit`
and `crates/game-3d`. Overflow checks are on for the chip.

## Tests

`cargo test --offline --locked --all-features` in this folder: 199
tests pass, 12 are ignored on purpose (report, picture and measuring
runs that assert nothing; run them with `--ignored`). Among them:

- the routes of MACHINES, SAIL and STONE reach DONE at MID, LOW and MAX;
- every detail stays in its triangle budget (LOW 160, MID 430, HIGH
  940, MAX 1,324; on ISLAND LOW 256 and MID 500, raised by the
  creatures' share) in camera sweeps and in long random play;
- MAX stays within 5 % of its measured busiest view on ISLAND (1,261
  triangles, at the usual pitch and tilted fully down), and is richer
  than HIGH in every area (`tests/max_level.rs`); the time is the
  board's to decide, the tests only print the estimate;
- few crack pixels (sky between two parts of one surface) at every
  detail;
- golden pictures (hashes in `tests/golden/`) of fixed moments at each
  detail;
- a long random soak that presses every button and opens every menu;
- the creatures (`tests/life.rs`): each patrol stays on its top, a
  knock-back never goes through a wall or off the island, the squash
  works from above only, RESTART brings every creature back, the pause
  menu freezes them, no fixed list overflows, the rules give the same
  play at every detail, and a play with creatures differs from one
  without only once the hero meets one.

`PROTO3D_REPORT=<folder> cargo test --release --offline --all-features
--test max_level -- --ignored` writes `busiest-views.txt` and
`start-views.txt` there; with `PROTO3D_LADDER=<folder>` as well, the
`ladder_report` test writes `ladder.txt` (the table above) and eight
views as PNG. `PROTO3D_VIEWS=<folder> cargo test --release --offline
--test views -- --ignored` writes eight fixed ISLAND views at every
detail (start, tilted down, next to a burr, next to a tuftling, a
squash, an edge, the far view, the trees) with their counts.

## On the board

This build (F2) was on the board in the owner's test firmware 1122q-own
(2026-11-22), ISLAND only, standing at the start with eight camera
turns. Step time, average / worst over the eight views (MEASURED ON
THE BOARD):

| Detail | average | worst |
|---|---|---|
| LOW | 7.22 ms | 8.56 ms |
| MID | 11.41 ms | 13.87 ms |
| HIGH | 18.24 ms | 21.94 ms |
| MAX | 23.74 ms | 29.50 ms |

All 32 views held 29.99 to 30.01 pictures a second, except one at
29.63. The build before the repairs F1 and F2 (with the creatures) was
timed at MAX in firmware 1122l: standing at the start 29.30 ms average,
29.71 ms worst. Against the PROTO3D in firmware 1122f, the same eight
views cost 0.8 to 3.9 ms more each; that is two different firmwares,
not a clean comparison.

## Not checked

Not measured on the board: MACHINES, SAIL and STONE, and MAX with the
camera tilted down and moving. The estimates in Numbers come from a
model that counts triangles and pixels only. How the creatures feel to
play (the knock-back, the squash, the greeting), and the repairs F1 and
F2 as a player sees them, are checked only by tests and still pictures
on the PC, not by a person.
