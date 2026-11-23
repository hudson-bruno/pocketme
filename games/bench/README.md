# bench: BENCH

A speed test for the console, not a game to play. It finds the real
limits of the board: how many fills, triangles, corners and 3D objects
fit one frame at 30 and at 60 pictures a second, and what sending a
changed picture costs. Its last two tests are showcases: CITY, a
street through a city with traffic, and ISLAND, a figure running round
a floating island followed by a third-person camera (from the showcase
game's study, kept outside the repository), the kinds of scene a game would
draw.

Each **level** is a fixed scene at one load. It repeats for ever and is
the same every run. The **ramp** raises the load by itself and finds the
largest load that fits, first at 60, then at 30.

Uses only `game-kit`. No heap, no floating point, no `unsafe`. Since
2026-11-22 it is in `games/bench` and on the simulator's card, like a
game (see "Moving it in"); it is still a measuring tool, not a game.

## The tests

The picture is 160 x 120. Rows 0 to 19 are the text band. Rows 20 to
119 (16,000 pixels) are the work area. LEFT and RIGHT go through the
tests in this order. Each test starts at its second load.

| # | Test | The load is | Loads (UP / DOWN) | Ramp |
|---|------|-------------|-------------------|------|
| 0 | `FILL` | full fills of the work area per call (16,000 bytes each) | 1, **16**, 64, 256, 1024 | yes |
| 1 | `TRI 20` | triangles of exactly 20 pixels per call | 256, **1024**, 4096, 16384 | yes |
| 2 | `TRI 100` | triangles of exactly 100 pixels | 128, **512**, 2048, 8192 | yes |
| 3 | `TRI 400` | triangles of exactly 400 pixels | 64, **256**, 1024, 4096 | yes |
| 4 | `VERT` | corners turned, moved and projected, nothing drawn (50 per object) | 256, **1024**, 4096, 16384 | yes |
| 5 | `SCENE 12` | balls of 12 triangles flying at the eye | 8, **32**, 128, 512 | yes |
| 6 | `SCENE 32` | balls of 32 triangles | 4, **16**, 64, 256 | yes |
| 7 | `SCENE 96` | balls of 96 triangles | 2, **8**, 32, 64 | yes |
| 8 | `SEND ROWS` | per cent of the picture's rows changed every call (from the bottom; at 100 the band too) | 0, **25**, 50, 75, 100 | no |
| 9 | `SEND DOTS` | per cent of 240 squares of 8 x 8 changed every call, spread out | 5, **10**, 25, 50 | no |
| 10 | `TRI SPIN` | triangles of about 100 pixels, each turning round its own middle every call, on a cleared work area | 128, **512**, 1024, 2048, 4096 | yes |
| 11 | `CITY` | buildings of a city street (`BLDG`); cars to a distance that grows with them | 8, **32**, 64, 96, 128 | yes |
| 12 | `ISLAND` | a detail level, shown by name: LOW, MID, HIGH, XL, XXL (loads 1 to 5) | LOW, **MID**, HIGH, XL, XXL | no |

The scenes are the headline: objects culled against the view, sorted
far to near, turned, back faces dropped, cut where they cross the near
plane, flat shaded with one light. Triangles drawn per frame is the
number that counts; the band shows it.

The static tests (FILL, TRI, VERT) draw the same picture every call, so
the console sends nothing but the band. They measure drawing alone.
The SEND tests do almost no drawing. They measure sending.

TRI SPIN is TRI 100 made to move: the same spread of triangles of
about 100 pixels (99.4 by area), but each one is turned round its own
middle every call (one turn in 64 calls, every other one the other way;
corners turned with the sine table), and the work area is cleared
first. So the whole work area changes every call, like a game's. The
band counts the pixels really painted (between 99 and 100 a triangle).
Against TRI 100 at the same load it shows what turning, clearing and
sending a changed picture add.

### Game time (round 3)

The moving tests (the SCENEs, TRI SPIN, CITY, ISLAND) move by game
time, not by calls: the same speed at 30 and at 60, and at 60 twice
as many pictures (before round 3 they moved one step a call, so at 60
everything ran twice as fast). The console hands the game `delta_us`,
the measured time since the last call. It wobbles round the console's
grid (33,333 or 16,667 µs) by the board's wake-up, but the sum stays
on the grid. `app::advance` turns it into ticks of 1/60 s, counted in
half microseconds with a tick of exactly half the console's frame
(33,333 half µs = 16,666.5 µs at 30, 33,334 half µs = 16,667 µs at
60; `app::tick_half_us`): each call takes the nearest whole number of
ticks and keeps all the rest for the next call. So a steady 30 gives
exactly 2 ticks a call and a steady 60 exactly 1, wobble up to about
8 ms changes nothing, a late call catches up, a gap of more than 15
ticks (250 ms, a stall) moves 15 and drops the rest, and when every
step takes longer than the frame (an over-loaded test) game time still
runs at real speed, the same at 30 and at 60 (MEASURED ON THE PC,
`tests/time.rs`: 0.99998 to 1.00002 for steps of 8 to 90 ms). The
picture at 60 after `2k` calls is the picture at 30 after `k` calls;
the calls in between show the positions in between.

(Round 3's first rule used one tick of 16,667 µs at both rates and cut
the rest to a quarter tick each way, because at 30 the console's 33,333
µs is 1 µs short of two such ticks and a whole rest would drift. That
cut lost or added time when the steps were longer than the frame: 0.80
to 1.11 of real speed, found by the round-3 check. Fixed as above.)

CAMBER's rule (add the delta, take every whole tick, keep the floor
rest) stutters under the same wobble, how often depending on the launch
offset: how late the call that started its clock came. Calls of 20,000
that take a number of ticks other than the steady 1 (at 60) or 2 (at
30), with the board waking 0 to J ms late (MEASURED ON THE PC,
`tests/time.rs`, test `camber_rule_stutters_depending_on_the_launch_offset`;
CAMBER itself is not changed here):

| J | at 60, launch on the grid | at 60, launch J/2 late | at 30, on the grid | at 30, J/2 late |
|---|---|---|---|---|
| 1 ms | 0 | 9,915 | 650 | 482 |
| 2 ms | 0 | 10,014 | 1,282 | 992 |
| 4 ms | 0 | 9,949 | 2,580 | 1,974 |

At 60 a launch on the grid never stutters; at 30 it slips whatever the
offset, because two ticks of 16,667 µs are 1 µs longer than the
console's frame: even with no wobble it takes 1 tick on the first call
after the start and then once every 16,667 calls (9.3 min). Ours: none
in any case.

The ramp keeps a fixed advance per call (2 ticks; CITY 16 at 30, and
at 60 one tick a call in short runs, see CITY), so its steps see the
same pictures however late the calls come. The static
tests and SEND do not move.

### CITY

The eye drives 3 m above the middle line of a four-lane street at 10 m
a second (at 30 and at 60 calls a second), looking a little down and swaying 10
degrees left and right. A cross street every 32 m, a zebra crossing
before each junction, a yellow middle line, dashed lane lines. Each
side has four rows of buildings: low shops and houses at the street
(5 to 16 m), towers behind (up to 47 m). A building is a box with
none to three dark window bands, some with a smaller box on top; eight
wall colours, two glass colours, flat shaded by a sun from the upper
right (one side of the street is in shadow). Cars (a body and a glass
cabin, six colours) drive both ways in four lanes at 5 to 15 m a second
and cross the street at the junctions. The city is 256 m long and
repeats: every 25.6 s (768 calls at 30, 1,536 at 60) the picture is
the same again, cars included, so a reading over 26 s covers every
picture once at either rate.

The load is the number of buildings: the nearest ones ahead, both
sides, all rows. 8 is the first corner, 32 about 50 m of street, 64
about 100 m, 128 about 200 m. Cars are drawn to a fixed distance for
each load, about the stretch the buildings cover plus 16 m: 48 m at 8,
67 m at 32, 118 m at 64, 170 m at 96, 190 m at 128
(`city::car_reach`). So a car comes into sight or leaves it only by
crossing that distance; it never blinks (before the check it was the
far end of the nearest buildings, which moves back, and cars blinked).
The road is always drawn 190 m ahead. Farther buildings are mostly hidden behind nearer
ones (there is no depth buffer; everything is drawn far to near), so
from 64 on the picture changes little but the work grows: that hidden
work is what a game without its own culling pays.

The band: `OBJ` road pieces, buildings and cars drawn (in view),
`TRIS` triangles that painted at least one pixel, `PX` pixels painted,
a pixel painted twice counted twice (the sky and ground fills, 16,000
pixels a call, are not counted). In CITY a triangle whose box holds no
pixel middle of the view is not handed to the filler at all
(`Paint::ONLY_PAINTED`, `poly::no_pixel_middle`), and one that is
handed but paints nothing (thin, between pixel middles: 6 to 17 % of
those handed) is not counted. Before the check TRIS counted every
triangle handed to the filler, half of which painted nothing (33 to
74 %). The scenes keep the first round's rule (every triangle handed
over is counted, 8 to 18 % of them paint nothing), so their numbers
compare with round 1. These move along the cycle: per call on the PC
(average / most over one cycle):

| Load | OBJ | TRIS | PX |
|---|---|---|---|
| 8 | 18 / 25 | 114 / 155 | 9,530 / 13,745 |
| 32 | 44 / 52 | 296 / 414 | 23,416 / 29,841 |
| 64 | 86 / 93 | 845 / 988 | 36,840 / 42,053 |
| 96 | 127 / 136 | 1,468 / 1,624 | 43,381 / 48,526 |
| 128 | 163 / 170 | 2,023 / 2,173 | 46,263 / 51,347 |

The result screen of a CITY ramp shows the triangles at FIT, like the
scenes'. CITY's ramp steps are longer than the others' and cover the
whole street loop (1,536 ticks, 25.6 s at the real speed), so FIT is
the whole loop's and `n` its average:

- At 30 the street moves 16 ticks a call and a step measures 96 calls:
  each step sees every 16th tick of the loop once.
- At 60 (since the ramp-60 change) a step is 196 calls: 4 not counted,
  then 16 short runs, one in each sixteenth of the loop. A run starts
  at that sixteenth's first tick and moves one tick a call (the real
  speed, like the fixed test at 60): 6 calls not counted (the jump to
  the run's start changes the whole picture; the console settles),
  then 6 counted. 16 x 6 = 96 counted calls. Why: at 16 ticks a call
  nearly every picture changed across the whole width, and on the two
  cores a picture that wide is sent behind the panel's own sweep (one
  every 17,018 us, 58.8 a second), so every step on the board averaged
  17,038 to 17,052 us and was judged over (round 3: "60: fit 1"),
  while the fixed CITY 32 at 60 held 60.01. Real-speed pictures change
  less per call, as in a game.

A step takes about 3.3 s at both rates when it fits (1.7 s at 60
before the change), longer when over. In the send model of `tests/pace.rs`
(draw times fitted to the board) a whole CITY ramp takes 55.5 s (the sum
of the times between calls), the old one 60.8 s (MEASURED ON THE PC, on a model: ESTIMATED for
the board). The fixed levels run at the real speed.

### ISLAND

The showcase game's study (kept outside the repository) drawn with this
renderer: an original box figure (1.26 m, with a pack) runs a loop
round a 14 m island in the sky. It jumps onto two floating platforms
and across the gap between them, runs through a stone gate, jumps up
onto the first terrace (1 m) and up a step to the second (2 m), and
runs down two ramps. A third-person camera follows 4.4 m behind and
0.9 m above the figure's middle, looking 22 degrees down, turning
after the figure smoothly. One loop is 768 ticks (12.8 s) at 30 and at
60; the picture of tick `t + 768` is that of tick `t`, and no step of
the eye or camera is larger at the loop's end than anywhere else.
Around it float islets (a grass top, earth and rock underneath, some
with a tree); gems spin. Grass in two shades of tiles, flat shading by
one sun, the figure's round shadow on what it is over (tilted on a
ramp).

The load is a **detail level**, fixed (there is no ramp for ISLAND,
`kmax` 0: the levels change the scene, not a count). LOW, MID and HIGH
are the study's three levels: the floor in 6 x 6, 7 x 7 or 9 x 9
tiles, trees from four-sided pyramids to cones to two tiers, the
tower's sides, bushes from MID, fences and grass tufts from HIGH, the
figure as one model (LOW, MID) or two (legs and body, from HIGH),
posed every tick (legs and arms swing; a jump pose). XL and XXL are
HIGH made heavier the way a richer game would: a finer floor (28 x 28
and 56 x 56 tiles instead of 14 x 14; made while drawing, only the
strips in view, so it takes no room in the game's data), finer trees
(three tiers of ten sides) and grass (16 blades a patch), more trees,
bushes and grass, and more islets round it (11 more at XL, 36 more at
XXL, placed while compiling). They are there so that the limits fall
inside the range: on the cost estimate below, LOW to HIGH fit 60, XL
is at 60's edge, XXL is over 30 for about half the loop.

Per tick over one loop on the PC (average / most; MEASURED ON THE PC,
`cargo test --all-features --test island pictures -- --nocapture`):

| Level | OBJ | TRIS | PX | Corners |
|---|---|---|---|---|
| LOW | 18 / 24 | 93 / 144 | 14,382 / 20,954 | 224 / 277 |
| MID | 27 / 36 | 192 / 298 | 14,410 / 19,314 | 461 / 563 |
| HIGH | 40 / 53 | 402 / 648 | 15,893 / 21,714 | 978 / 1,231 |
| XL | 65 / 87 | 923 / 1,621 | 17,605 / 27,146 | 2,109 / 2,794 |
| XXL | 164 / 205 | 2,197 / 3,628 | 19,735 / 29,588 | 6,625 / 8,203 |

The loop on the board, ESTIMATED from these counts with round 2's
cost model fitted to the board (2,435 µs + 1.381 x (1.28 µs a corner
+ 6.25 µs a triangle + 0.074 µs a pixel)): LOW 5.1 /
6.3 ms, MID 6.4 / 8.0, HIGH 9.3 / 12.3, XL 15.9 / 24.0 (349 of 768
ticks over 16.7 ms), XXL 35.1 / 50.3 (418 of 768 ticks over 33.3
ms). The board decides.

No triangle is dropped at any level. TRIS counts triangles that
painted a pixel (`Paint::ONLY_PAINTED`, as in CITY); PX does not count
the sky's fill. The band's first line shows the level's name instead
of a number.

**Draw order** (there is no depth buffer; the study's rules): the far
islets first, far to near; then the island's underside and floor in
list order, then the figure's shadow when it is on the floor; then
every other object far to near by the depth of its middle. Three rules
on top: an object that stands on another is drawn after it (its key is
kept below its support's: the "parent" rule; the figure stands on its
shadow, the shadow on what the figure is over); a ramp leaning on a
terrace's side is drawn before or after the terrace by which side of
that side's plane the eye is on; the gate's beam is drawn after both
pillars when the eye is below its underside, before them when above.
A slow exact checker on the PC (feature `check`: every triangle handed
to the filler is recorded with its depths and its object; per pixel,
the nearest triangle's object against the object shown) finds per
picture (192 pictures a level, every 4th tick; average / most): LOW 1 /
11, MID 2 / 19, HIGH 5 / 33, XL 6 / 39, XXL 8 / 39 wrong pixels of
16,000. What is left: the fence's own faces (one model that is not
convex, sorted by face) and the shadow's edge at the foot of the
second ramp. The checker's pictures `island-check-*.png` mark them.

**The near-bush fault** (the study: a bush or tree between the camera
and the figure was cut by the near plane and showed as a big dark slab
at the picture's edge) is fixed this way: small things (trees, bushes,
fences, grass, gems) are not drawn while their ball reaches nearer
than the near plane (50 cm) to the eye; a thing that close covers
the view and hides the figure anyway, and it pops back as soon as it
is clear. The solid things (floor, terraces, ramps, tower, gate,
platforms) are never hidden: the route and the camera are laid out so
that the eye stays at least 60 cm from all of them (closest 62 cm, the
second ramp; a test checks every tick at every level). That 62 cm is a
straight-line distance to the nearest corner, not the depth along the
view: when the camera passes beside the stone gate, the gate pillars'
faces do cross the near plane in depth (ticks 370 to 388 and 452 to
460, and the beam at tick 461; MEASURED ON THE PC by the round-3
check, whose report is kept outside the repository). The cut faces
show at most 1,450 pixels (9 % of the picture, tick 454) as the
pillar's own face at the left edge: it
looks like a pillar passing the camera, not a fault.

**Known cosmetic issue: seam cracks.** Where a big face is cut by a
side plane of the view, its two triangles compute the cut point on
their shared edge from opposite ends, and the shared renderer's
`clip_polygon` (`src/poly.rs`, unchanged since round 2) divides with
truncation, so the two points can differ by one unit: single pixels of
sky or grass show through along the seam. Worst 18 to 23 pixels on the
near gate pillar at tick 362; 719 crack pixels over the LOW loop (654
to 4,934 over the levels; MEASURED ON THE PC by the round-3 check).
They do not change the timing (the same triangles, the same counts).
Not fixed in BENCH: the renderer is shared with SCENE, TRI SPIN and
CITY, whose pictures at 30 must stay those of round 2 byte for byte.
To fix before ISLAND becomes the showcase: compute the cut from the
edge's two corners in one fixed order (for example the lower corner
number first), so both triangles get the same point.

## Keys

Player 1's pad (on the board: `boardctl key NAME`).

| Key | Level screen | Ramp | Result |
|-----|--------------|------|--------|
| LEFT / RIGHT | previous / next test (wraps round) | | |
| UP / DOWN | larger / smaller load (stops at the ends) | | |
| X | switch 30 / 60 pictures a second | | |
| A | start the ramp (not for SEND) | | run the ramp again |
| B | | stop | back to the level screen |
| START | leave BENCH | leave | leave |

The game starts at FILL, load 16, 30 a second. Every press is one step,
so a level can be reached by counting: SCENE 32 at load 64 at 60 is
RIGHT x6, UP x1, X. To be sure of a load, press DOWN x8 after
picking the test (each test keeps its own load; DOWN stops at the
smallest).

The order wraps round: RIGHT from SEND DOTS goes to TRI SPIN, RIGHT
from ISLAND to FILL, LEFT from FILL to ISLAND. From the start: ISLAND
is LEFT x1 (at MID), CITY LEFT x2 (at 32), TRI SPIN LEFT x3 (at 512).
CITY at 8 is LEFT x2, DOWN; each UP then goes 32, 64, 96, 128. ISLAND
at LOW is LEFT, DOWN; each UP then goes MID, HIGH, XL, XXL.

## Reading the numbers

### The level screen (the band)

    SCENE 32  16 OBJ  AT 30
    FRAME 33333 MAX 33333 LATE 0
    OBJ 14 TRIS 211 PX 5819

- Line 1: the test, its load and unit, the rate the console runs at
  now. `WAIT` follows while the console has not yet taken the rate X
  asked for.
- Line 2: the time between calls in µs over about one second: average,
  longest, and how many calls came more than 1/8 of a frame late.
  `FRAME ...` until the first second is measured.
- Line 3: what was drawn per call, on average over that second.

**How to read FRAME.** A game cannot read a clock. The console reads the
time once per step and tells the game how long since the last call.
While the work fits, that is the frame: 33,333 at 30, 16,667 at 60. So
FRAME at the frame means **it fits**, and says nothing about how much
time is left. When the work does not fit, the console calls again at
once and FRAME becomes the whole loop: the game's work plus the
console's own (input, sending the picture, the log). So FRAME above the
frame means **it does not fit**, and then it is the real time.

The numbers in the band change at most once a second, when a second
ends. When they do not change, the band is not drawn again.

### The ramp

    RAMP SCENE 32  AT 60
    LOAD 38 OBJ
    LAST 32: 16667 US FITS

Each step: 4 calls not counted (the load settles), then at least half a
second and 4 calls measured. A step **fits** when the average stays
within 1/64 of the frame and at most one call in 16 is late. A step is
**over** when the average is more than 5/4 of the frame.

The search: the load doubles until a step does not fit, then goes up in
quarter doublings (about 19 % a step) from the last that fit. Then it
measures loads above until it has two over steps (at the top step:
the untried steps just below it). Then it does the same at 30, starting
from the load that fit at 60. An over loop takes the same time at both
rates, so the line at 30 uses the two largest over steps of either
rate. Against made-up costs a
ramp takes 7.5 to 12.3 seconds of game time (MEASURED ON THE PC,
`a_ramp_takes_well_under_a_minute`); on the board expect 10 to 20
(ESTIMATED). CITY's steps are whole cycles (see CITY above).

**Near panel pace** (CITY's ramp at 60 only; a label, not a verdict).
The panel (the screen) redraws itself line by line about 58.7 times a
second; one such **sweep** takes about 17,018 us (MEASURED ON THE
BOARD, round 3). When pictures change across the whole width, the
console sends them in step with the sweep, and the calls can be held to
one sweep each, about 17.0 ms, just over the 60-a-second limit. BENCH
only sees the time between its calls, never a clock inside a call, so a
drawing that takes about 17.0 ms looks exactly the same. A CITY step at
60 that does not fit, averages at most 17,283 us (the sweep plus 1/64,
the same margin a fit has) and has no more late calls than a fit may
have (one in 16) is **labelled** "near panel pace": the band says
`OVER, NEAR PANEL`, the step's log line ends `over, near panel pace`.
It still counts as **over**: the search goes on exactly as without the
label, so FIT stays the strict answer (the largest load that kept the
full 60). The console's own `perf` reading (letter `m`), taken while
such steps run, tells the panel from the drawing: when the panel holds
the pace, `fps` is about 58.8 (the panel's own rate) and
`present_avg_us` is several ms (the console waited for the screen); the
work without that wait (`step_avg_us` minus `present_avg_us`) is well
under 16.7 ms. Round 3's held steps: fps 58.83 to 58.86,
present_avg_us 9,322 to 11,471, work 4.6 to 6.0 ms; a real overload
(load 108): fps 47.17, present_avg_us 65, work 20.0 ms (MEASURED ON THE
BOARD, round 3). All 20 held steps of
round 3's board readings get the label (`tests/ramp60.rs`). The other
ramps never show it.

### The result screen

    AT 60:
    FIT 32          400.00 US EACH
                    +2000 US
    SAFE 28         415 TRIS

- **FIT**: the largest load measured that fit the frame. `FIT 512+`
  (log: `fit 512+`): even the top step fit, so the real limit is higher.
- **SAFE**: the load that leaves a fifth of the frame free (the stated
  margin). It comes from a straight line through the two over steps:
  the time one more unit costs (`US EACH`) and the time of a loop with
  no load (`+... US`: the console's own work and the game's fixed
  part). SAFE is never more than FIT. `-` when the line could not be
  made (fewer than two over steps at either rate): that happens when
even the top step's loop stays under 5/4 of the frame (for CITY, load
128 under 20.8 ms at 60); FIT is then the answer. On the board CITY 128
takes about 37 ms, so CITY has a SAFE there. SAFE is a guide:
  a scene's cost per object is not constant, and at 60 the line reaches
  far below its two points. Against made-up scene costs it was within
  5 % at 30 but up to 55 % low at 60 (MEASURED ON THE PC,
  `tests/sched.rs`). Check a SAFE load as a fixed level with the
  firmware's `step_avg_us`.
- For a scene: the triangles drawn at FIT. For FILL: bytes per µs.
- CITY only, when steps at 60 were near panel pace (above), three small
  lines above the bottom one (how many steps, the last one's average,
  the lowest load that had the label):

      60: 3 NEAR PANEL PACE, 17018 US
      PANEL OR DRAWING? COUNTED OVER
      FIRST AT LOAD 19

- The bottom line names the unit. It is red when nothing fit.

The result stays until B or A, and is logged every 2 seconds.

### The log

Every line starts with `bench:`. On the board, `boardctl log` shows
them.

    bench: started
    bench: FILL load 16 at 30                       (a level was picked)
    bench: FILL load 16 at 30: frame avg 33333 max 33333 late 0 of 30; tris 0 px 256000 objs 0
                                                    (every 4th second)
    bench: ramp SCENE 32 starts
    bench: ramp 60 k 20 load 32: avg 16667 max 16667 late 0 of 30 n 415 fits
                                                    (each ramp step; n: triangles,
                                                     corners or pixels per call)
    bench: result SCENE 32; 60: fit 32 n 415 safe 28 ns 400000 fixed 2000; 30: fit 76 n 995 safe 61 ns 400000 fixed 2000
    bench: ramp 60 k 21 load 38: avg 17018 max 17036 late 0 of 96 n 393 over, near panel pace
                                                    (CITY at 60: over, at about one
                                                     panel sweep a call)
    bench: result CITY 60: 3 near panel pace (counted over), 17018 us; first at load 19
                                                    (the call after a CITY result
                                                     that had such steps)
    bench: ramp stopped
    bench: leave

`ns` is the cost of one unit in nanoseconds, `fixed` the loop with no
load in µs. The console cuts a game's log line at 96 letters, so the
end of a long result line (`...; 30: ...`) does not reach the board's
log; the result screen has all of it.

## What the band costs the screen

MEASURED ON THE PC with the console's own rule (`console_video::diff`,
`cargo test --test send -- --nocapture`):

- Static tests: nothing is sent while the band does not change. In the
  PC's steady run the band never changed after the first second. On the
  board the numbers vary, so the band changes at most once a second: at
  most 3,200 pixels a second (the whole band), about 107 a frame at 30
  (ESTIMATED, an upper bound; 0.6 % of the picture). VERT's check sum
  changes every second: 185 to 195 pixels.
- SEND ROWS sends exactly what it says: 4,800, 9,600, 14,400 and
  19,200 pixels a call for 25, 50, 75 and 100 %.
- SEND DOTS sends more than it changes, because the console joins
  areas (at most 16 a picture): it changes 768, 1,536, 3,840 and 7,680
  pixels and the console sends 3,840, 7,808, 12,160 and 13,632. The
  band shows what changed. The board's `perf` line (`px_per_frame`)
  shows what was sent.
- The scenes change most of the work area: 900 to 14,100 pixels a call
  on average, often in 16 areas.
- TRI SPIN sends 13,100 to 15,300 pixels a call on average (most of
  the work area, 16 areas). CITY sends 4,400 a call at load 8 (the road
  and sky change little at the first corner) and about 10,700 from 32
  on, at most 14,307 (over 90 calls, at the real speed). In the CITY
  ramp the street moves 8 times as fast, so at load 8 more changes per
  call; from 32 on the real speed already sends most of the work area
  (the board sent 61,145 screen pixels a present at CITY 32 at 60 and
  kept the frame).

## What the simulator cannot tell

Nothing here about speed comes from the PC. The simulator calls a game
on time whatever the work costs, so FRAME always shows the frame, and a
ramp would find that every step fits. The PC tests check the logic:
the pictures, the counts, the ramp against made-up times, overflow. The
board decides the speed, the sending cost, tearing and how the radio
copes.

## Size

Round 3 (with ISLAND), MEASURED (chip build, a variant script kept
outside the repository, which builds exactly what `games/build-chip.sh` builds at its own
settings; slots: code 57,344, data 49,152):

| build | opt | overflow checks | code | data (constants, starting values, zeroed) |
|---|---|---|---|---|
| BENCH (today's settings) | s | on | 45,884 | 48,612 (42,688, 5,920, 4) |
| BENCH SN | s | off | 41,604 | 46,668 (40,744, 5,920, 4) |
| BENCH O2 | 2 | on | 49,940 | 48,532 (42,608, 5,920, 4) |
| BENCH O3 | 3 | on | 54,060 | 48,548 (42,624, 5,920, 4) |
| BENCH O3N | 3 | off | 48,240 | 46,284 (40,360, 5,920, 4) |

After the ramp-60 change (MEASURED, chip build, same script):

| build | opt | overflow checks | code | data (constants, starting values, zeroed) |
|---|---|---|---|---|
| BENCH (today's settings) | s | on | 46,480 | 48,828 (42,872, 5,952, 4) |
| BENCH SN | s | off | 42,224 | 46,868 (40,912, 5,952, 4) |
| BENCH O2 | 2 | on | 50,768 | 48,756 (42,800, 5,952, 4) |
| BENCH O3 | 3 | on | 55,008 | 48,772 (42,816, 5,952, 4) |
| BENCH O3N | 3 | off | 49,080 | 46,484 (40,528, 5,952, 4) |

All five fit; BENCH's data has 324 bytes to spare (556 before the
ramp-60 change, 540 in the first table). ISLAND's tables are
most of the constants. The first round-3 build did not fit (data over
by 11,996), so:

- the workloads' memory (`load::Work`: renderer and island work space)
  is kept beside the app, not in it, and its start value is all zero
  bytes, so starting is a memory fill, not an 8 KB constant;
- the sort lists (`[_; MAX_OBJ]`, 2 KB) are locals of the CITY, ISLAND
  and SCENE workloads (`scene` is `#[inline(never)]` so they stay out
  of `run`'s frame); this costs a 2 KB zero fill a call, about 2 to 3
  µs (ESTIMATED);
- the fine floor strips are drawn after the floors, not kept in the
  thing table; normals and faces are kept for 92 faces (the largest
  flat floor), and the islands' origins are 16-bit.

The pictures did not change (goldens and the checker's counts are the
same). Game value: 5,928 bytes on the PC, 5,920 on the chip (was 6,640); 5,960 and
5,952 after the ramp-60 change.
Largest stack frames (chip build, BENCH): the kit's `update` 7,648
(was 8,352; 7,680 after the ramp-60 change), `island` 2,832, `city`
2,736, `scene` 2,640.

In `games/bench` (2026-11-22, MEASURED ON THE PC by
`games/build-chip.sh`, with the kit's `restart` change: the game's
state is built in a frame of its own): code 46,428, data 48,836
(42,880, 5,952, 4), so 10,916 bytes of code slot and 316 bytes of data
slot to spare. Largest stack frames (the build's own check,
`games/chip/check_frames.py`): the kit's `restart` 6,000, `island`
2,832, `city` 2,736; the kit's `update` is no longer among them.

Earlier: round 2 code 27,476, data 27,348; before TRI SPIN and CITY
code 20,636, data 19,700.

## Tests

    cargo test --offline                    # 100 tests and 1 ignored (102 with --all-features; 2026-11-22)
    cargo test --release --test sched       # one more, skipped in a debug build
    UPDATE_GOLDEN=1 cargo test --test levels every_level_has_a_fixed_picture
    cargo test --test send -- --nocapture   # the send table
    cargo test --test shots                 # pictures into .local/bench/shots/

- `levels`: each shape paints exactly its pixels anywhere; triangle,
  fill and corner counts match the load; a lone ball paints what is
  seen; a fixed picture per level (`tests/data/golden.txt`); the band's
  text; the keys.
- `ramp`: the ramp against a made-up console (`fixed + load x unit`)
  finds the step a search over every step finds, and the cost line;
  300 random cost models; late calls; the whole flow in the app.
- `limits`: the largest loads, odd times (0, `u32::MAX`), ticks that
  wrap, the longest log line. Tests build with overflow checks on, like
  the chip build.
- `send`: what each level costs the screen (above).
- `city`: TRI SPIN and CITY: the same picture for the same call
  whatever came before, counts equal to what the slow filler paints
  (TRI SPIN) and at least every pixel seen (CITY), nothing drawn outside
  the work area, no triangle dropped, the city's picture at call `t +
  768` equal to call `t` and the step over the cycle's end no larger
  than any other, cars that never meet (every call of the cycle), the
  ramps' loads, the game value not grown. After the check: the box test
  never skips a triangle that paints (200,000 random ones against the
  slow filler), CITY's TRIS are triangles that painted, the cars'
  distance is fixed per load and grows with it, a CITY ramp step is one
  whole cycle (96 calls, every 8th call) and `n` at FIT is that
  average, TRI SPIN's step is as before. It writes the pictures
  `city-load-*.png`, `city-064-second-*.png`, `city-064-cycle-sheet.png`
  and `tri-spin-*.png` into `.local/bench/shots/` and, with `--nocapture`, the
  count table above.
- `ramp60` (ramp-60 change): CITY's 60 schedule (16 real-speed runs
  over the whole loop, 96 counted calls, the 30 half's ticks where they
  were), its pictures the same however late the calls come, the
  screen-pace verdict and its words (band, log, result screen,
  `.local/bench/shots/ramp60-*.png`), and on the send model below: the old 60
  pictures held to the panel's sweep, the new ones and the fixed test's
  not, the new ramp's FIT the largest ladder load the fixed test's
  pictures keep the frame at, TRI SPIN's and the scenes' ramps not held
  more than their real-speed pictures.
- `pace` (ramp-60 change): the whole app driven through a model of the
  two-core send (`tests/common/pace.rs`, ported from the round-3
  checker's `lock_sim.py`; ESTIMATED, a model): fixed CITY at both
  rates and the CITY ramp. It builds against the BENCH before the
  change too, to show the old ramp's numbers.
- `kit`: through `game_entry` with a stand-in console: start, palette,
  rate requests, START, the state's size (5,960 bytes on the PC since
  the ramp-60 change, 5,928 in
  round 3, was 6,640).
- `time` (round 3): the game-time rule (`advance`) against wobbling and
  late calls and over-loaded steps (real speed at both rates, on a
  model of the console's call grid), CAMBER's floor rule as a model (it
  stutters, depending on the launch offset), the picture
  at 60 after `2k` calls equal to the one at 30 after `k`, CITY's cycle
  25.6 s at both rates, ramps that step per call.
- `island` (round 3): five fixed loads, the same tick gives the same
  picture, nothing drawn outside the work area, the loop closes, the
  camera and the figure keep clear of solid things, pictures and counts
  per load.
- The `check` feature (PC only, never in a game build; in
  `--all-features`) records every triangle handed to the filler, so
  `island` can replay a picture: the counts are what was painted, and
  the draw order is wrong on only a few pixels per picture.

## Moving it in

It was made in a folder outside git so it did not touch
`games/` or the simulator. On 2026-11-22 it was copied to
`games/bench`, its two paths in `Cargo.toml` were changed to
`../../crates/...`, its test pictures moved to `.local/bench/shots/` at
the repository root (git-ignored), and it was added in the six places
CLAUDE.md names ("A new game touches six places"). So
`games/build-chip.sh` builds it for the chip with today's settings
(opt-level s, overflow checks on) and the CYD's demo card takes it too.
The measuring variants (other optimisation levels) and the round
reports, board logs and pictures named in this README were not moved:
they stay outside the repository, in the checkout where BENCH was
made.
