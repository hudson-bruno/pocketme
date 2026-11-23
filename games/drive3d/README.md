# DRIVE3D: the rally prototype

A working name and a prototype, not a finished game. One car slides on
gravel, grass and a ford along one short stretch of road (344 m) on a
height map: a hill, a crest that throws the car, a drop and two corners,
with trees, rocks, a hut, posts, a banner over the split straight and a
finish gate beside and over the road. Grass grips far less than the
road and holds the car to 12 m/s (the owner's rule since 2026-11-22: the
road is the best way through a corner); the grass inside the second
corner is cut low (a lighter green) and drives the same. The
spec is kept outside the repository.

It has the **rules** (everything a test can check without a picture)
and the **3D picture**, which reads the rules only through `src/look.rs`
(`Look`), a small read-only interface. The picture is drawn far to near
with no depth buffer: a sky with haze bands, the land as fine 8 m cells
near the camera and 16 m blocks farther out, in four haze steps, the
road ribbon on gravel, grass and the ford, the things beside the road,
then the car's shadow, the car seen from behind, and dust (or spray in
the ford). A dust puff stays where the car threw it and fades after
about half a second, so dust stays in the air when the car flies over
the crest; reversing throws none. In the ford each front wheel (and,
from MID, each rear one) pushes a flat sheet of water aside, bigger
with the speed and on the side the car slides toward (drawing only).
The speed is at the bottom left, the
level at the top right, the run's time at the bottom right. When the car
is pushed against something and cannot move for a second, the picture
says "B: REVERSE  Y: ROAD".

Four detail levels (LOW, MID, HIGH, MAX; `src/detail.rs`) change how
far the land and the things reach, how many dust puffs there are (the
newest 2, 4 or 8), and at LOW the road pieces, car, banner and puffs are
plainer. MAX is the level above HIGH, sized for about 24 pictures a
second on the board: the land to 352 m and the
things to 192 m (HIGH: 240 m and 112 m; MID 144 m and 56 m; LOW 80 m
and 32 m). On the board it holds 30 (see "On the board" below). The
levels change the picture
only; the driving rules are the same at all four (`tests/max.rs`). The
game starts at HIGH (the owner's decision of 2026-11-23; it started at
MID before). The levels are picked in the START menu, where "<"
marks the level in use (X steps through them too, round from MAX to
LOW). The menu also restarts the
run; while it is open the run stands still. An A (or B) still held from
taking a line counts for nothing until it is let go once, so it neither
drives the car nor starts the run clock (`tests/menu_hold.rs`).

## Controls

Player 1; the simulator's keys in brackets.

| Control | Pad | Simulator |
|---|---|---|
| steer | left stick, or the d-pad left and right | arrows; the stick on `I` `J` `K` `L` |
| throttle | right trigger, or A held | `Z`; the trigger on `2` |
| brake, and reverse once stopped | left trigger, or B held | `X`; the trigger on `1` |
| handbrake | R held | `W` |
| back to the road (after 1.5 s) | Y | `S` |
| detail LOW, MID, HIGH, MAX (next one) | X | `A` |
| the menu: LOW, MID, HIGH, MAX, RESTART (UP, DOWN, A or START takes, B closes) | START | `Return` |
| the line of numbers | SELECT | `Backspace` |

The rally is played with the gamepad only (the owner's rule,
2026-11-22): touch does nothing in this game. Home (`Tab`) is the
console's pause menu.

The d-pad gives full lock at once (after the steering's ramp of about
0.2 s), which spins the car at speed; the stick gives part lock and is
the better way to drive.

## On the board

Step time, average / worst (MEASURED ON THE BOARD, firmware 1122n,
2026-11-22, the build before the ford spray; the spray changes the
drawing only):

| Detail | standing | 6 s drive from the start |
|---|---|---|
| LOW | 6.58 ms / 6.83 ms | not measured |
| MID | 10.49 ms / 10.78 ms | not measured |
| HIGH | 17.21 ms / 17.54 ms | 18.93 ms / 20.80 ms |
| MAX | 26.36 ms / 26.67 ms | 28.29 ms / 30.19 ms |

Every one of these held 29.99 to 30.09 pictures a second; MAX was also
timed in the firmwares 1122h, 1122j and 1122m, always at 30 a second.
This build, with the spray, was on the board in the owner's test
firmware 1122o-own: the builder's MID drive played in five parts gave
averages of 10.85 to 11.87 ms a step, the worst single step 13.79 ms,
30.00 pictures a second in every part. Whether the spray was on screen
in those parts is NOT CHECKED. HIGH and MAX with the spray have not
been timed on the board.

The size on the chip (MEASURED, `games/build-chip.sh` with this folder,
2026-11-23, this build):

```text
DRIVE3D  code  61596 of 57344 (+4252 past the slot)   data  36892 (constants 26360, starting values 10528, zeroed 4)   room left 8008 of 106496
```

The biggest stack frame is the start (`restart`), 10,576 bytes (limit
16,384).

## Files

- `src/tune.rs`: every starting value, with its unit and reason, and the
  rules marked DEFAULT where the spec was silent.
- `src/maths.rs`, `src/road.rs`, `src/land.rs`, `src/things.rs`: units,
  the stretch from records (baked while compiling), the land's heights
  and surfaces, the things beside the road.
- `src/car.rs`, `src/camera.rs`, `src/clock.rs`, `src/play.rs`: the car's
  step, the chase camera, the fixed step (30 a second, at most 2 a
  frame), one whole step.
- `src/detail.rs`: LOW, MID, HIGH, MAX as values. `src/menu.rs`: the START
  menu.
- `src/look.rs`: the drawing's read interface. `src/draw.rs`: the
  palette, sky, words and menu box; `src/scene.rs`: the land, road,
  things, shadow, car and dust, far to near; `src/models.rs`: the fixed
  models; `src/order.rs`: the land's drawing order (a quarter of it is
  stored and turned back into the whole while drawing).
- `art.txt`: the tile picture's source (`games/art/make_art.py`).

## Tests

```bash
cargo fmt --check
cargo clippy --offline --locked --all-targets --all-features -- -D warnings
cargo test --offline --locked --all-features
cargo test --offline --test measure -- --ignored --nocapture   # the measurements
```

`tests/max.rs` guards MAX: its triangles against its budget over the
scripted drive and the sweep, no lost triangle, the same rules as HIGH
step by step, and the land walk's far edge (`MAX_NUMBERS=<file>`
writes the counts).

`tests/draw.rs` sweeps the drive at each level (triangles against the
budget, pixels drawn in the wrong order, cracks in the land) and keeps
golden hashes of eight moments per level: `DRIVE3D_PICTURES=<folder>`
writes them as PNG files (1x and 4x), `DRIVE3D_NUMBERS=<file>` writes the
sweep's numbers, `UPDATE_GOLDEN=1` stores new hashes (after looking).
`DRIVE3D_SCRIPT=<file> cargo test --offline --test drive
the_scripted_drive_as` writes the test driver's drive as a simulator
input script (home to past the finish). It waits on the home screen
until the default boot animation has ended, so it works with the
default boot and with `--boot off`.

`tests/speed_baseline.rs` hashes every picture of a set of drives and
compares them with `tests/speed_baseline.txt`, pinned before the speed
work, so that work could not change a pixel. The menu fix above changed
its `game` section on purpose (eight picture lines and the total); that
section was pinned anew in speed round 2b, and nothing else in the file
changed. The section as pinned before the fix is kept as history in
`tests/speed_baseline_fixes.txt`; `tests/speed_baseline_fixes.rs` checks
that the pictures before the menu opens are the old ones and the first
one after the first menu choice is not. MAX changed the file's `menu`
and `game` sections on purpose (the menu's box is 8 pixels taller for
its fifth line, and X from HIGH now goes to MAX); nothing else in it
changed. `tests/speed_proofs.rs`
checks the speed work's exact replacements against game-3d (corner
planes and light in 32 bits, the visibility test, the block height
table, the land walk's first test of a block, the land's light
tables, the nearest road point in 32 bits, polygons with no pixel
rows), and `tests/speed_counts.rs` counts
the picture's work (`DRIVE3D_COUNTS=<file>` writes the counts).

`tests/final_look.rs` checks the picture reviewer's findings (dust,
the blocked hint, the cut's colour, the shadow on bent land, the finish
gate, words away from the edges, the menu's mark).

`tests/drive.rs` holds a fingerprint of a scripted drive; any change to
the rules changes it, and then it must be measured again and written in.
