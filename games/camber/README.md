# CAMBER

Hover-ship racing on real 3D tracks made of polygons: hills and dips,
banked turns, a jump, tunnels, open edges you can fall off. Eight ships,
three laps, three tracks. It is the console's 3D showpiece. An original
game: its name, ships, entrants, tracks, looks and screens are made
for this console and copy no other racing game.

(The name is the owner's choice. The picture shows `NAME` from
`src/lib.rs`; the name is also spelled in `LOG`, in `Cargo.toml` and
`Cargo.lock`, in the tests' `use camber::`, in `art.txt`'s header, in
`INFO.TXT` and here.)

## How to play

You fly a hover ship a hand's width above the road. It has no wheels:
the nose turns, but the ship keeps sliding the way it was going, and a
turn the road takes under you carries you to the outside unless you
steer into it. B is the brake (both **air brakes**). L and R are the
**carve**: hold one through a corner and the ship swings its nose into
the turn and keeps its speed; the turn heats the vanes, and letting go
pays the heat back as a **sling**, a short push above top speed.

### The rules

- A race is **three laps** against seven rivals; you start at the back
  of the grid. Every ship is its own entrant with its own colours:
  yours is magenta with yellow edges (YOU), the rivals are TIDEWAY
  (blue), CINDER (red), SAFFRON (yellow), VERDANT (green), CHALK
  (white), BASALT (dark grey) and MARIGOLD (orange); no rival has
  yellow edges. A count of 3, 2, 1, GO starts it. The results show every
  ship's place and time; the others go on racing behind the panel
  until they are over the line.
- **TIME TRIAL** is the same track alone; the results show the total
  and the best lap. The title shows the best lap of this session for
  the chosen track and class.
- **The shield** (the bar bottom left) wears down when you hit a wall
  (harder hits cost more; in STEADY a wall costs half) or another ship. At zero the ship **limps**
  (LIMP blinks; 60 % of its top speed) until the shield is back above
  a quarter.
- **The recharge lane** is the coloured strip on the left of the road
  after the start line: fly along it and the shield fills again.
- **Boost pads** (two arrows on the road) push you to 130 % of your top
  speed for a moment and speed you up faster.
- **Falling**: open edges and the jump's gap have no walls. A ship that
  leaves the road falls and is put back on the road after a moment,
  slowly and passing through the others for a moment ("ghost").
- **Bumps**: ships push each other apart side by side, and the one
  behind gives speed to the one in front; both lose a little shield.
- The rivals follow the racing line, each in its own lane, steer round
  a ship ahead to pass, take the pads, go to the recharge lane when
  their shield is low, and brake for what each turn allows. Three of
  them fly a ship equal to yours, the other four a slower one (see
  "The rivals" below). They
  **carve** with your buttons and rules (see "The rivals' carve" below)
  and, like you, pull both air brakes or none. They do not ram: a
  rival closes on a ship right in its track only gently until it has
  found room beside it. A ship that has finished touches nobody. They can
  be bumped, they hit walls and they can fall. A rival steers by the
  way it travels (the slip of a drift counts), but below 30 m/s the
  slip counts less, so two ships bumped nose to nose at a crawl turn
  away instead of holding each other. Before that rule, rivals on
  GLACIER could stay under 8 m/s for up to 53 s; now the longest such
  stretch is under 2 s (MEASURED (PC), random-key races, 2026-11-18).
- **How a rival holds its line** (repaired 2026-11-22, not yet seen on
  the board). The nose's turn rate moves only a quarter of the way to
  the stick's each tick, so the pilot used to lag, overshoot and swing
  from side to side about once a second (the weave the owner saw, also
  on your own ship after the finish, where the pilot takes over). Now
  the pilot asks for four times the change it wants, plus one unit past
  the rounding, so the rate lands on its wish in about a tick (since the
  rivals' rebuild also while carving, where it steers by the carve's own
  model of the slip). It aims at a pad as
  soon as the pad lies between the ship and the point it looks at, not
  only once that point is on the pad. And the baked racing line moves
  at most 0.5 m per 8 m segment on each side (`LINE_STEP`, reaching
  `LINE_REACH` = 10 segments: the move starts earlier and ends later),
  so the aim no longer jumps up to 5.75 m between two segments; where
  the line crosses from one side to the other both sides move, so the
  largest move between two segments is now 1 m (LAGOON, FURNACE) and
  0.5 m (GLACIER). MEASURED (PC), `tests/steer.rs` and `tests/laps.rs`
  `handover`, 2026-11-22: alone on a straight started 3 m off the line,
  the first overshoot fell from 0.46 to 0.93 m to 0.01 to 0.11 m (58 to
  126 m/s) and the swing is gone after about 1.3 s instead of 4 to over
  30 s; in races the rivals hit fewer walls and finish 2 to 10 % sooner
  with the same difficulty numbers. The steps and every table are in
  a report kept outside the repository.

### The carve and the sling

- **Start**: at speed (108 km/h or more), on the ground, press L or R.
  The way is the way the stick points as you press; with the stick in
  the middle, L carves left and R carves right. The nose starts turning
  smoothly (no kick), so a press alone costs no speed.
- **Hold**: while L or R stays held (either one), the nose keeps turning
  the carve's way: about 47 degrees a second with the stick in the
  middle, up to 94 into the turn, down to 3 out of it. The stick only
  makes it tighter or wider; it never turns it back. The ship's path
  follows the nose with a lag (about 16 degrees behind it in the middle,
  31 at full lock), so the nose points further into the turn than the
  ship goes, and the speed is kept: a carve costs 1/700 of the speed a
  tick, while a normal turn scrubs 1/12 of the slip off every tick.
  One carve at a time: the other button starts nothing new.
- **Heat**: the vanes heat with how far the ship **really turns**: the
  road's own turn under the ship plus how far the path points into the
  carve's side of the road, weighted by speed (full from 216 km/h). Steps
  at 30, 55 and 85 degrees: most corners reach step 1, the long ones 2,
  the hairpins and the biggest sweepers 3. The tail throws sparks under
  the hull (grey dust, then orange, red, white-hot; each step longer),
  and three marks low in the middle of the screen fill (step 3 blinks).
  In the NOSE view two small sparks rise from the bottom edge beside the
  marks. Holding on a straight or wiggling the stick does not heat
  faster: on a straight the path can only point so far across the road
  before the wall (crossing the whole road at full lock can just reach
  step 1, which does not pay: see below).
- **Let go**: the **sling**, stronger and longer by step. With the
  reward (row B, chosen 2026-11-22, see below) step 1 pushes for 1.5 s towards
  123 % of your top speed, step 2 for 2.25 s towards 134 %, step 3 for
  3.25 s towards 146 % (even without A). Letting go before step 1 pays
  nothing. While an air brake is pulled the sling does not push, but
  its time still runs out (so braking into the next corner is not
  undone by the sling). And the **settle**: the slip a carve held
  at least 12 ticks (0.2 s) leaves is turned into the path over the next
  16 ticks without losing speed, so letting go does not scrub speed off.
  A shorter press (a tap) leaves no settle and pays nothing.
- **Lost**: a hard wall hit (more than 4 m/s across) ends the carve and
  its heat; a brush keeps it, but the wall's push turns the path back
  and takes that much heat away. More than 8 ticks (0.13 s) off the
  ground ends it too (a crest, the jump; a short hop keeps it, and in
  the air the carve has no grip of its own), and so does slowing below
  79 km/h.
- **Touch**: in the bottom strip of the screen (below y 100 of 120), a
  finger far left or far right starts a carve that way; anywhere in the
  strip the finger keeps it going (and steers it tighter or wider);
  lifting the finger or moving it up out of the strip lets go.

Every number of the carve is in one commented block at the top of
`src/race.rs` (`CARVE_*`, `HEAT_*`, `REWARD`):

| Constant | Value | What |
|---|---|---|
| `CARVE_START_SPEED` | 30 m/s | no carve starts slower |
| `CARVE_END_SPEED` | 22 m/s | a carve ends (heat lost) slower than this |
| `CARVE_PICK` | 40 / 256 | the stick decides the way when pushed more than this, else the button |
| `CARVE_WIDE` / `MID` / `TIGHT` | 8 / 120 / 240 (256ths of 100 deg/s) | the nose's turn: stick full out / middle / full in |
| `CARVE_GRIP` | 20 | the slip turns 1/20 of the way back to the nose a tick, keeping the speed (on the ground only) |
| `CARVE_DRAG` | 700 | the carve loses 1/700 of the speed a tick |
| `CARVE_SETTLE` / `_PART` / `_MIN` | 16 / 4 / 12 | after letting go: 16 ticks of settle, 1/4 of the carve's slip turned into the path a tick, only after a carve held 12 ticks |
| `CARVE_WALL_SPEED` | 4 m/s | a wall hit faster than this across ends the carve |
| `CARVE_FULL_SPEED` | 60 m/s | heat counts in full from this speed, less below |
| `HEAT_1` / `2` / `3` | 30 / 55 / 85 degrees | the heat's steps |
| `CARVE_AIR_TICKS` | 8 | ticks off the ground a carve survives |
| `REWARD` | top 1260 / 1370 / 1500, 90 / 135 / 195 ticks, pull 30 (row B below) | the sling of each step: top speed (1024ths of your own top), ticks of push, and the pull (1/30 of the gap to the sling's top a tick) |

### The carve's reward (row B, chosen 2026-11-22)

The owner asked for a bigger reward (2026-11-22): last round's sling
(row D) made a good carver only about 6 % faster on the best lap.
Three stronger rows (A, B, C) were built beside it, and the owner
played the four rows on the board (through a temporary title line,
since removed) and chose **row B** on 2026-11-22. Only row B is in the
game now: `REWARD` in `src/race.rs`. The table and the measurements
below are kept as the history of that choice. The heat steps stay at
30 / 55 / 85 degrees.

| Row | Top, steps 1 / 2 / 3 (1024ths of your top) | Push, steps 1 / 2 / 3 | BRISK: best lap / three laps | STEADY | WILD |
|---|---|---|---|---|---|
| A | 1140 / 1195 / 1260 (111 / 117 / 123 %) | 70 / 105 / 150 ticks (1.2 / 1.75 / 2.5 s) | 8.6 / 5.2 % faster | 7.6 / 6.8 % | 3.8 / 0.7 % |
| **B (chosen)** | 1260 / 1370 / 1500 (123 / 134 / 146 %) | 90 / 135 / 195 (1.5 / 2.25 / 3.25 s) | 11.2 / 7.1 % faster | 11.1 / 9.2 % | 5.6 / 4.1 % |
| C | 1450 / 1620 / 1820 (142 / 158 / 178 %), pull 1/24 | 180 / 230 / 255 (3 / 3.8 / 4.25 s) | 13.0 / 9.9 % faster | 17.5 / 15.7 % | 7.3 / 5.3 % |
| D (last round) | 1130 / 1180 / 1240 (110 / 115 / 121 %) | 30 / 45 / 65 | 5.3 / 1.6 % faster | 4.0 / 3.4 % | 1.2 / -1.8 % |

MEASURED (PC), 2026-11-22, a bench kept outside the repository (the same
model pilots as `tests/laps.rs` `reward_table`), TIME TRIAL, the
scripted good carver against the same pilot without L or R, the three
tracks summed, nine starts a quarter metre apart on each, with the
sling stopped by the air brake). Aimed at 9, 12 and 15 % at BRISK: A and B
are close to 9 and 12 %, C reaches 13 % (15 % at BRISK would need a
pilot that brakes less for the next corner; at STEADY C gives 17 %).
B was proposed as the default before the owner played the rows: close
to the 12 % aim, and the step 3 top (146 %) throws the ship into the
next wall less often than C's 178 %. The owner chose B on the board.

The highest speed in a sling (MEASURED (PC), same runs, BRISK): row B
85 / 95 / 104 m/s for steps 1 / 2 / 3 on GLACIER, row C 100 / 112 /
124 m/s. No row makes the good carver fall after a sling (0 falls in
every run).

For every row, at BRISK: carving from side to side on the straights
makes the three laps 0.7 to 4.2 % slower and its best lap at most
0.4 % faster; taps in the corners make the three laps 0.9 to 1.6 %
slower; carving edge to edge on the straights 9 to 12 % slower.
Letting go costs nothing (the settle), and the third step needs as
much turning as before. At WILD taps cost more on FURNACE (8.7 %
slower over three laps: a tap's small turn there knocks the ship into
the next wall; the same with every row).

At WILD the scripted good carver gains less (row B: best lap 13.7 %
faster on LAGOON, 1.7 % slower on FURNACE, 5.2 % faster on GLACIER;
three laps 13.4 %, 1.5 % and -1.4 %): at 86 m/s its slings throw it
into the next wall more often. A pilot who lets go earlier at WILD
would do better; the scripted one is kept as it was.

**The air brake stops the push.** Before this rule the sling pushed
on while the scripted pilot braked for the next corner, and with row B
it hit walls more often in the 2 s after a let-go (MEASURED (PC),
before and after the change; the run files are kept outside the
repository: hard wall
hits in the 2 s after a let-go with the sling against the same runs
with the sling taken away, STEADY +25.9 % before and +4.1 % after,
WILD +18.1 % and +4.7 %, BRISK -3.0 % and -0.4 %).

### The rivals' carve

The rivals carve with the same rules as you: a press with the stick in
the middle, the stick held for the arc the pilot wants, letting go
where the corner ends, a wall comes close, the stick is pulled hard
against the carve or the class's heat step is reached. Which corners:
a fixed hash of the ship, the lap and the corner's place, so a race
plays the same every time. They throw the same sparks as you, at any
distance: beyond 16 m from the eye the sparks grow with the distance
(up to 4 times at 64 m) so a carving rival far ahead still shows a few
pixels. The skill is one row a class, `SKILLS` in `src/race.rs`:

| Class | Corners carved | Lets go at | Room it wants to both walls | Least turn over the next 64 m | Rest after a let-go |
|---|---|---|---|---|---|
| STEADY | 52 / 256 (about one in five) | step 1 | 3 m | 6000 (33 degrees) | 40 ticks |
| BRISK | 170 / 256 (two in three) | step 2 | 3 m | 6000 (33 degrees) | 30 ticks |
| WILD | 240 / 256 (nearly all) | step 3 | 2.5 m | 4000 (22 degrees) | 20 ticks |

They never press with less than that room to either wall, nor slower
than 40 m/s (144 km/h; you can from 30 m/s), nor in a corner that
turns less than the class's least turn over the next 64 m (a short
kink pays no sling, so a press there is wasted). Overall a bit more
than half of the rivals' carves pay a sling, but not in every class
and track (MEASURED (PC), a bench kept outside the repository, all eight
ships computer-driven, seeds 0 to 6, so all 7 grids): 3,634 of 6,144
let-gos paid, 59.1 %. On LAGOON / FURNACE / GLACIER: STEADY 38 / 27 /
78 %, BRISK 64 / 46 / 72 %, WILD 64 / 58 / 68 %. Under half on LAGOON
STEADY (38 %), FURNACE STEADY (27 %) and FURNACE BRISK (46 %). Before
the turn rule, with rooms of 4 / 5 / 2 m, it was as low as 6 %.
`tests/rivals.rs` asks for at least a quarter in every class and track
(since the rebuild: of the carves they pick, with STEADY on FURNACE let
through at 12 %, where it now presses more short carves; it measured
15 % on the PC).

The one-sided air brake the rivals used to pull in tight turns is gone:
you cannot pull one. Against last round (MEASURED (PC), the same bench,
7 grids, before and after the change; the run files are kept outside
the repository; last
round had the one-sided brake, no rival carve, WILD 1024 and row D, so
the change is not the brake's alone), hard wall hits per race, before /
after:

| Track | STEADY | BRISK | WILD |
|---|---|---|---|
| LAGOON | 23.6 / 36.6 | 55.3 / 116.7 | 91.7 / 137.9 |
| FURNACE | 62.1 / 37.3 | 99.4 / 104.3 | 131.1 / 156.3 |
| GLACIER | 4.4 / 1.4 | 37.3 / 53.6 | 58.3 / 100.3 |

The rivals now carve and hit more walls in most cells (fewer only on
FURNACE STEADY and GLACIER STEADY). A clean driver (no L / R) won WILD
1 / 0 / 1 times in 7 on LAGOON / FURNACE / GLACIER before and 0 / 0 / 0
after (measured on the PC before and after the change; the run files
are kept outside the repository).

(These numbers are from before the rivals' rebuild below. Since then
every class also carves each corner it reaches over its plan's speed,
for the grip; `tests/rivals.rs` counts those carves apart.)

### The rivals (rebuilt 2026-11-22)

The owner beat every class easily, so the rivals were rebuilt against
the owner's own three recorded LAGOON races (one per class, replayed
on the PC with the console's race check matched). Nothing is given to
a rival that your ship does not have: a rival tick, put into your ship
with the same controls, moves it to the very same state
(`tests/rivals.rs`).

**Ships.** `PACE` in `src/race.rs`: the top speed and the planned
corner speed of each rival's ship, in 1024ths of the class's (your ship
is 1024). The seed turns the list round over the grid.

| Rivals | Ship | Slower than yours |
|---|---|---|
| three "fronts" | 1024, 1024, 1024 | 0 % (your own ship) |
| four "pack" | 968, 952, 936, 920 | 5.5, 7.0, 8.6, 10.2 % |

**Skill: one row a class** (`SKILLS`, the same code for every class and
track, so it carries to every track). No deliberate mistakes: the easy
class is only slower.

| Class | Corner speed it dares, of the plan (`dare`) | Both air brakes when over that by | ... while a sling pushes (`sling_brake`) | Carving, plans with this share of the tightest carve's turn (`carve_rate`) | Carves every corner it reaches over its plan (`need`) |
|---|---|---|---|---|---|
| STEADY | 940 / 1024 (8 % under) | 1/12 | 1/3 | 700 / 1024 | yes |
| BRISK | 1100 / 1024 (7 % over) | 1/12 | 1/5 | 700 / 1024 | yes |
| WILD | 1350 / 1024 (32 % over) | 1/12 | 1/5 | 700 / 1024 | yes |

Less over the planned speed it only lifts off the thrust; the one-side
air brake is gone. While carving the planned speed is what the carve's
turn rate can still follow (no grip runs out in a carve), so the rivals
brake later and corner faster than before. The aim keeps clear of the
room a carve needs and of a road narrowing ahead.

**No ramming.** A ship ahead in a rival's own track (within 24 m, and
across less than the bump box's width plus 0.3 m) caps its speed at that ship's
speed plus 1/30 of the gap a tick (`FOLLOW_CLOSE`), so it closes in
about half a second, until the steering has found room beside it. The
steering looks for that room sooner the faster it closes (from 16 m
plus 60 ticks of the closing speed, `PASS_LOOK`).

**Against the owner's recorded races** (MEASURED ON THE PC with
`examples/refghost`, LAGOON, catch-up OFF, the owner's path as a ghost
the rivals steer round): the leading rival finished, before / after:
STEADY +27.8 % / +12.0 % (owner 0:58.93, leader 1:06.00), BRISK
+14.2 % / +2.5 % (owner 0:49.20, leader 0:50.43), WILD +23.5 % /
-0.04 % (owner 0:41.51, leader 0:41.50). The other fronts finished
3.3 to 6.9 s behind the leader in these three races (rivals alone on
LAGOON over the seven grids: 1.6 to 5.4 s), and some pack ships finished
less than 1 % behind the fronts' mean: the fronts lose time in traffic,
passing the pack from where the grid put them. A person who did not race those
three races may find it otherwise; that is for the board to show.

Hard wall hits and rival-to-rival contacts (MEASURED ON THE PC,
`tests/contacts.rs`, rivals alone, seven races on each of the three
tracks, before / after): wall hits STEADY 116 / 30, BRISK 1,009 / 254,
WILD 1,734 / 1,130; rival contacts in all 63 races 112 / 91, fewer in
STEADY (58 / 17) but more in BRISK (24 / 30) and WILD (30 / 44), where
the fronts now pass the pack far more often. The test asks for fewer
wall hits in every class and no more contacts in all; the table is
printed by `contacts_table`.

### The catch-up

A title setting, CATCH-UP: LIGHT (the default) or OFF. With LIGHT a
rival far behind you along the race flies a little faster and one far
ahead a little slower, both ways the same. Its size (`CATCH_FREE`,
`CATCH_FULL`, `CATCH_MAX` in `src/race.rs`): nothing while the rival is
within 40 m of you, then the rival's pace (its top speed and the corner
speed it plans) changes evenly with the gap, up to 20 / 1024 (2.0 %) at
200 m and beyond, never more. At BRISK's 72 m/s top that is at most
1.4 m/s. Only while the race runs and neither you nor that rival has
finished; never in a time trial. OFF leaves every ship as it is. A
recording's header carries the setting, so a replay knows it.

### After your finish

Your ship is flown home by a quiet pilot of its own (`Race::cruise`),
not the rivals' racing pilot: both air brakes while it is over 3/4 of
the class's top, then no thrust down to half the top and just enough
to hold that (faster before GLACIER's jump, so it clears the gap), no
carve, and it eases back to the racing line along a long look with half
the racing pilot's pull, so the nose settles instead of swinging.

### The tracks

Every road is **10 % wider** than first built (`WIDEN` = 110 in
`src/track.rs`, the owner's wish, 2026-11-21): the pieces' half widths
of 5.5 to 7 m are scaled once while the tracks are baked, to 6.05 to
7.7 m (12.1 to 15.4 m wall to wall). Walls, tunnels, the jump, the
racing line (kept at least 2.5 m from the edges) and the scenery follow the
baked width; the grid stays two by two, 3 m either side of the middle.
CAMBER has no map.

| Track | Length (COUNTED) | What it has |
|---|---|---|
| LAGOON | 1,248 m (156 segments of 8 m) | wide and flowing beside the sea; for learning |
| FURNACE | 1,112 m (139 segments) | narrow and twisting in a red desert: two tunnels, a hairpin |
| GLACIER | 1,488 m (186 segments) | big climbs and drops through ice, a jump over a gap, open edges on the high road |

### Speed classes

| Class | Top speed | A wall costs |
|---|---|---|
| STEADY | 58 m/s (about 209 km/h) | half |
| BRISK (default) | 72 m/s (about 259 km/h) | all |
| WILD | 86 m/s (about 310 km/h) | all |

The rivals' ships and skill per class are under "The rivals" above
(since 2026-11-22 the class's own `rival_top` and `pace` are gone: the
fronts fly your ship, the pack 5.5 to 10.2 % slower ships, and the
class's skill row sets how hard they corner). The turn speeds the
rivals plan for are made once per track for a fixed sideways grip, the
same in every class.

The rest of this section is from before the rivals' rebuild (kept for
the record; the rebuilt rivals are much faster, so these wins no longer
hold). On 2026-11-18 (MEASURED (PC), scripted pilots over seven
seeds per track and class, all three tracks): a scripted beginner (reacts 0.1 s
late, wobbles, lets go of thrust now and then) won about half the
STEADY races and was mid-field or behind in BRISK and WILD; a clean
racing line without brakes won every STEADY and BRISK race and about
two WILD races in three; holding thrust alone never won.

Once the rivals carved, before their rebuild (MEASURED (PC), 2026-11-22, a bench kept
outside the repository, the same model players as `tests/laps.rs`
`rivals_and_places`, the live game with the reward (row B), the
player on the last grid place, seeds 0 to 6: the grid is the seed
modulo 7, so these are all seven grids), wins out of 7 for
LAGOON / FURNACE / GLACIER:

| Class | Clean driver (no L / R) | Good carver | Bad carver (taps, abuse) |
|---|---|---|---|
| STEADY | 5 / 6 / 7 | 7 / 7 / 7 | 5 / 7 / 6 |
| BRISK | 0 / 0 / 0 | 6 / 6 / 6 | 0 / 0 / 1 |
| WILD | 0 / 0 / 0 | 6 / 6 / 1 | 0 / 0 / 1 |

The aim (ESTIMATE of what feels right): in STEADY a driver who never
carves can win; in WILD winning needs good carving. It holds on LAGOON
and FURNACE. It misses on GLACIER at WILD (the good carver wins 1 of
7, places 2, 6, 3, 2, 1, 6, 8), where the scripted good carver gains
little even in a time trial (see above): the rivals' WILD row and
`rival_top` cannot fix that without letting the clean driver win too;
the carver (or the reward at WILD) would have to change. These scripted
players are not people; the owner beat every class easily, which is
why the rivals were rebuilt against the owner's own races.

### Controls

| Pad | What it does |
|---|---|
| left stick or d-pad left/right | steer |
| A or the right trigger | thrust |
| L / R | the carve (see above) |
| B or the left trigger | the brake (both air brakes) |
| X | next DETAIL (while racing) |
| Y | next VIEW (while racing) |
| SELECT | 30 or 60 pictures a second (while racing) |
| START | pause menu |

The **touch screen** alone can fly a whole race: a finger down thrusts
and steers towards itself (its distance from the middle of the screen
is how hard); in the bottom strip (the bottom sixth) and far to one
side it carves that way, and anywhere in the strip it holds the carve
(a finger never brakes: lift it to coast). A tap on the top strip
pauses. In
the menus a tap on a line chooses it or steps its value; a tap outside
the pause panel closes it.

In the menus: up and down pick a line, left and right change a value,
A or START chooses, B or START closes the pause menu.

### Settings

All on the title; DETAIL, RATE and VIEW also in the pause menu and on
X, Y, SELECT while racing. Every option stays selectable; the default
is a pick, not a limit.

| Setting | Options | Default |
|---|---|---|
| TRACK | LAGOON, FURNACE, GLACIER | LAGOON |
| CLASS | STEADY, BRISK, WILD | BRISK |
| MODE | RACE, TIME TRIAL | RACE |
| DETAIL | LOW (16 segments ahead, no trees), MID (24), HIGH (32, everything near) | HIGH |
| RATE | 30 FPS, 60 FPS | 60 FPS |
| VIEW | CHASE (behind and above), FAR (further back), NOSE (no ship in view) | CHASE |
| CATCH-UP | LIGHT (rivals far behind a little faster, far ahead a little slower, at most 2.0 %), OFF | LIGHT |
| BACKDROP | RACE (a made-up race, filmed), SHIP (your ship turning on the grid); title only | RACE |

The race runs in fixed ticks of 1/60 s, so the rate and the detail
never change the race: both rates run the same race (tested).

### The title

A column 72 pixels wide on the left holds the name and the menu; the
rest of the picture (about 55 %) is a live 3D backdrop on the chosen
track, in its theme, at your DETAIL. The column has two pages:

- **Main**: PLAY, TRACK, CLASS, MODE, OPTIONS, LEAVE, and the best lap
  of this session for the chosen track and class at the bottom.
- **OPTIONS**: DETAIL, RATE, VIEW, CATCH-UP, BACKDROP, BACK.

Up and down pick a line, left and right change a value, A or START
chooses (or steps the value), B on the OPTIONS page goes back to the
main page, on its OPTIONS line. A tap on a line (anywhere in its row of the column) chooses
it or steps its value; a tap on the backdrop does nothing. Every line
of both pages works by taps alone and by buttons alone (tested).

**BACKDROP** picks what moves behind the menu:

- **RACE** (default): a made-up race of the computer's pilots on the
  chosen track and class (`src/tv.rs`). It is a normal race stepped
  with no player, so it obeys the race's own rules; its events are
  dropped. A director cuts between four kinds of shot, in a fixed
  order: SIDE (a camera standing 3 m outside the road's edge that turns
  to follow the ships; also the start's), FAR (behind and above),
  FRONT (low, ahead, looking back; not in tunnels) and HIGH (high above
  and behind; not near tunnels). A shot follows the leader or now and
  then a ship in a close fight, and cuts after a few seconds, or sooner
  when the ship gets too far, enters a tunnel the camera is not in, or
  falls. When the race is over (at most 6 minutes) a new one starts.
- **SHIP**: your ship alone on the chosen track's grid, 12 m before
  the start gantry, turning once every 12 s, bobbing and swaying a
  little, at half its racing height so its shadow stays close. A still
  camera 6 m behind, 1.2 m to its left and 2.4 m up looks at it, with
  the road, the gantry and the track's sky and hills behind. The ship
  has your colours (magenta, yellow edges, cyan canopy and engine);
  the speed classes do not change a ship's colours, so CLASS changes
  nothing in this picture.

Both cameras aim a little left of the ship (`tv::AIM_LEFT`) so it
stands right of the column, at about x 116. Changing TRACK, CLASS or
BACKDROP starts the backdrop again at once on the new choice. Nothing
on the title writes a best lap or a log line, the recorder (`rec`)
never sees it, and PLAY starts the same race whatever ran behind the
menu (same options, same picture count: same race; tested).

**Cost.** The title asks for 30 pictures a second and never rests:
the backdrop always moves, so the console draws and sends 30 pictures
a second while the title is shown, as in a race. Per picture
(MEASURED (PC), `tests/cost.rs` `the_title_costs_no_more_than_a_race`,
all three tracks, BRISK; the race: the autopilot's whole race, CHASE,
30 a second):

| Detail | Backdrop | Corners turned mean / worst | Triangles mean / worst | Pixels painted mean / worst | Sent mean / worst |
|---|---|---|---|---|---|
| LOW | RACE | 277 / 555 | 166 / 303 | 29,347 / 40,678 | 5,370 / 10,900 |
| LOW | SHIP | 346 / 348 | 193 / 203 | 33,590 / 34,876 | 1,352 / 3,114 |
| LOW | a race | 306 / 529 | 193 / 319 | 31,062 / 42,119 | 12,855 / 18,568 |
| MID | RACE | 453 / 741 | 275 / 409 | 30,295 / 40,764 | 5,915 / 10,900 |
| MID | SHIP | 479 / 486 | 289 / 299 | 33,722 / 35,109 | 1,462 / 3,469 |
| MID | a race | 484 / 744 | 315 / 448 | 31,907 / 42,159 | 13,193 / 18,568 |
| HIGH | RACE | 639 / 909 | 396 / 619 | 30,486 / 41,141 | 5,981 / 10,900 |
| HIGH | SHIP | 640 / 686 | 455 / 483 | 34,035 / 35,308 | 1,586 / 3,564 |
| HIGH | a race | 690 / 946 | 466 / 661 | 32,103 / 42,666 | 13,204 / 18,568 |

The RACE backdrop is under a race's average in everything; its worst
picture turns a few more corners than a race's worst at LOW (555
against 529; the test holds the averages and the worst pixels
painted). SHIP is
nearly the same picture every time: every one of its pictures is less
work than a race's worst, and it sends about a tenth of a race's
pixels, but its average is over a race's average (LOW: corners +13 %,
pixels painted +8 %; the ship is big and close). The test holds SHIP
to the race's worst picture. Time on the chip: NOT CHECKED.

## How it is built

| File | What is in it |
|---|---|
| `src/lib.rs` | the console side: input to `Keys`, the palette, the rate it asks for (nothing while still), the log |
| `src/app.rs` | the screens (title, race, pause, results), the menus, the controls, ticks from the call's time |
| `src/race.rs` | one race in ticks: handling, walls, jumps and falls, pads, shield, lane, bumps, the rivals, laps, places |
| `src/track.rs` | the tracks as pieces, baked while compiling into 8 m segments with position, heading, climb, bank, width, racing line and safe speed |
| `src/poly.rs` | CAMBER's choices for the shared 3D library `crates/game-3d` (shared projected corners, polygons cut against five planes with seams that cannot open, the 32-bit lens, models sorted far to near, checked sums) and CAMBER's own counts (`Stats`); the drawing itself is game-3d's, shared with KESTREL |
| `src/scene.rs` | the backdrop (dithered sky, sun, stars, two rows of far hills, the sea or ground), the track drawn far to near one segment at a time, scenery, ships with their shadow and trail, the cameras |
| `src/hud.rs` | the race's numbers, the count, the notes, the title's column and pages, the pause menu, the results |
| `src/tv.rs` | the title's backdrops: the made-up race's director and cameras (RACE) and the turning ship on the grid (SHIP) |
| `src/art.rs` | the palette (materials in 4 haze steps and 2 to 3 light steps per track theme), the eight entrants' names and colours, the ship's model |
| `src/fixed.rs` | whole-number maths: angles in degrees, `atan2`, turning, scaling, hash; the sine table, matrices and square root come from `crates/game-3d` |

**3D**: the track is a real polygon mesh. Each boundary between two
segments is turned into the camera's view once and its corners (road
edges, stripes, wall and roof tops, the skirt underneath) are shared
by every polygon that touches them. Segments are painted from the far
end towards the camera, which is the right order for a track seen from
on it; inside a segment: road, inner walls, roof, the ships on that
segment, then outer faces and what stands on the boundary. Every ship,
the player's too, is drawn in the turn of the segment under its tail
(3 m behind its middle), so nearer walls, tunnel mouths and crests
cover it and the road under it never does. From CHASE and FAR a rival
passing the camera (from 8 m behind to 3 m ahead of it) is left out:
it would fill the picture and hide your ship. The race numbers have a
dark outline all round so they read over sky, snow, lamp posts and
wall stripes. The haze
grows in four steps with distance so the end of the view fades into
the sky. Corners are 1/256 m, matrices 1/16384, screen positions
1/16 pixel; no floating point anywhere.

## Numbers

Every number says how it was found: **COUNTED** (exact, by the tests or
from the chip build's files), **MEASURED (PC)** (by the tests on the
PC, with the console's own rule for what it sends,
`console_video::diff`), or **ESTIMATE**. The board numbers were
estimated before the game had run on the board; the board
measurements since (2026-11-18 at 30, 2026-11-23 at 60) are under
"Pictures a second on the board".

**Size on the chip** (COUNTED, `games/build-chip.sh`, 2026-11-23, the
title with its column menu and both backdrops): code 53,824 of 57,344
bytes; data 19,540 (constants 18,464, starting values 1,072, zeroed
4); room left 33,132 of 106,496 (the title took 4,892 code and 384
data, 5,276 bytes in all); `GAME.BIN` 73,396 bytes. With the `rec`
feature: code 58,620 (1,276 past the code slot), data 36,204, room left
11,672.
Before (COUNTED, `games/build-chip.sh`, 2026-11-22 late,
the rebuilt rivals with the CATCH-UP setting, overflow checks off):
code 48,932 of 57,344 bytes; data 19,156 (constants 18,120, starting
values 1,032, zeroed 4); room left in both slots together 38,408 of
106,496 (before the rebuild 40,564, so 2,156 bytes more); `GAME.BIN`
68,120 bytes. With the `rec` feature: code 53,844, data 35,812, room
left 16,840 (before 19,360). Earlier (COUNTED, 2026-11-22, with
the chosen reward (row B only), the rivals' carve with the turn rule
and the sparks at any distance, the rivals' steering repairs (lead,
pads, racing line) and game-kit's `restart` change, overflow checks
off): code 46,952 of 57,344 bytes (82 %, 10,392 free); data
18,980 of 49,152 (38 %: constants 17,976, starting values 1,000,
zeroed 4); `CAMBER.BIN` 65,964 bytes. Before the steering repairs:
code 46,840; before the `restart` change too: code 46,828, data the
same, file 65,840. Earlier the same day, with the four reward
rows and the temporary title line to try them: code 47,224, data
19,356, file 66,612; before the review fixes: code 46,792, data 19,124,
file 65,948.
On 2026-11-21, with the
carve and the wider road: code 45,920, data 18,868, file 64,820.
Before the carve (2026-11-19, on the shared 3D library
`crates/game-3d`): code 42,192, data 18,620, file 60,844.
With the checks on it was 51,680 code and 24,396 data. Before the move
to game-3d it was 51,644 code and 24,052 data (and 53,124 code before
the games stopped pulling in `core::fmt` for plain log lines).

### Overflow checks: off in CAMBER's chip build

Every other game's chip build has overflow checks on: a sum that runs
over (200 + 100 in a `u8`) stops the game. CAMBER's chip build has
them **off** (the owner's decision, 2026-11-19, for speed): the same
sum quietly wraps round, so a range mistake shows as a wrong picture
or a wrong number instead of a stop. The switch is the list
`OVERFLOW_CHECKS_OFF` in `games/build-chip.sh`; nothing in this
folder's `Cargo.toml` changes, and the PC tests (debug builds) keep
the checks on, so the tests still stop at such a mistake.

What it changes (COUNTED, chip build, 2026-11-19): code 51,680 to
42,192 bytes (9,488 less), data 24,396 to 18,620 (5,776 less: the file
names and lines kept for the panics). The calls that stop the game
after an add, subtract, multiply, negate or shift (374 places) are
gone. What still stops the game: a division by zero, an index outside
a list, and `i32::MIN / -1`; those checks are always on. On the board,
before the game library, a step took 17,871 us with the checks and
10,418 us without (BOARD).

Before the switch was turned on, the scripted plays were run with the
checks on: the golden test (debug build) and the autopilot through a
whole race on every track, at every detail, view and rate, and every
speed class in RACE and TRIAL: 72 races, 220,825 pictures (MEASURED
(PC), 2026-11-19, about 3.5 minutes). None tripped a check.

**Stack on the chip** (COUNTED from the `entry a1, N` of each function
in the chip build, 2026-11-19): `App::draw` 3,136 bytes (it holds the
ship renderer's working lists, about 1.1 KB), the kit's `update` 1,280,
`Race::setup` 784, `scene::ship` 672, `set_palette` 608, everything
else 448 or less. Deepest chain without a panic: about 5.9 KB (update,
draw, ship, polygon, clipping).

**No cracks between segments.** Two neighbouring segments can cut their
shared boundary at different points (the stripes near the camera, the
recharge lane's edge, the plain road far away). Each road strip
therefore goes through every point either neighbour uses on that
boundary, so the edges match point for point and no pixel row can slip
between them. It costs about 2 triangles a picture (MEASURED (PC)). A
polygon can now have 10 corners before clipping, so `MAX_POLY` is 16.

**Work per picture** (MEASURED (PC), `tests/cost.rs`, 2026-11-18, the
autopilot's whole race at BRISK on all three tracks, CHASE at 30; means
weighted by pictures, worst of the three tracks; all settings are in
`.local/camber/cost.txt`):

| Detail | Corners turned mean / worst | Triangles mean / worst | Pixels painted mean / worst | Changed mean | Sent mean / worst |
|---|---|---|---|---|---|
| LOW | 315 / 529 | 198 / 316 | 31,030 / 43,680 | 2,920 (15 %) | 13,280 / 18,690 |
| MID | 488 / 706 | 316 / 448 | 31,870 / 43,750 | 3,330 (17 %) | 13,520 / 18,880 |
| HIGH | 687 / 950 | 462 / 662 | 32,070 / 44,400 | 3,390 (18 %) | 13,530 / 18,880 |

The wider road (2026-11-21, MEASURED (PC), all 36 settings, the
autopilot's whole race): the worst picture per track turned 929 / 851 /
829 corners (LAGOON / FURNACE / GLACIER; before 950 / 865 / 829), drew
643 / 591 / 567 triangles (before 666 / 609 / 567) and sent 18,640 /
18,903 / 18,370 pixels (before 18,719 / 18,878 / 18,712). The races are
not the same ticks as before (the road is wider, so the autopilot drives
other lines), so the worst painted pictures cannot be compared one to
one: FURNACE's worst rose from 44,404 to 48,211, but that is a
different picture. Drawn as the SAME scenes (the new race copied
picture by picture into the old game, places across scaled by 100/110;
a bench outside the repository), the
wider road paints +0.4 % on average over all 36 settings and -0.7 % at
the worst picture (45,193 to 44,893); the biggest rise of one picture is
+2.3 % (GLACIER, FAR, LOW). The carve's sparks add about 0.1 % on
average and at most 357 pixels to one CHASE picture (in NOSE, which
draws no ship, only the HUD's two small streaks, at most 44 pixels). Speed on the chip: not checked on the board yet.

Painted pixels include the full-screen backdrop (19,200) but not the
HUD. The HUD's outline draws each text 9 times: about 3,400 to 3,900
more pixel writes a picture than plain text (MEASURED (PC), glyph
pixels of a typical race HUD). FAR is within 2 % of CHASE and sends up
to 18,720; NOSE sends about 11,700 to 11,800 on average (18,580 at
most); 60 a second changes 8 to 10 % a picture and sends about 11,700
to 12,200. No model triangle was ever dropped.

**Pictures a second on the board** (ESTIMATE, two cores, the owner's
build): the second core sends a picture (a whole one, 19,200 game
pixels, in about 14 ms; it sends about 13,500 on average and
18,900 at most) while the first draws the next, so the step decides it:
60 a second needs a step under about 14 to 15 ms, 30 under about 30 ms.
KESTREL's board steps (6.1 / 7.0 / 8.0 ms at LOW / MID / HIGH, with
87 / 124 / 179 triangles) give about 21 us more a triangle, all work
included; taking 15 to 25 us a triangle and 1.5 to 4.2 ms for the rest
(race, per-pixel backdrop, HUD, the console):

| Detail | Triangles mean / worst | Step mean | Step worst | 60 | 30 |
|---|---|---|---|---|---|
| LOW | 198 / 316 | 4.5 to 9.2 ms | 6.2 to 12.1 ms | holds | holds |
| MID | 316 / 448 | 6.2 to 12.1 ms | 8.2 to 15.4 ms | most pictures; the worst may slip | holds |
| HIGH | 462 / 662 | 8.4 to 15.8 ms | 11.4 to 20.8 ms | not sure (about 45 to 60) | holds |

**On the board** (one test build, MEASURED, 2026-11-18, LAGOON, MID,
CHASE): a step took about 18 ms on average and 20 ms at most at 30
frames a second (30 held), above the MID estimate here.

**On the board at 60** (MEASURED, 2026-11-23, firmware 1123n: the
owner's two-core build with the gamepad feature and the game flash
parts, no gamepad connected, buttons sent over the debug link; LAGOON,
BRISK, RACE, HIGH, CHASE, the game's defaults):

| | pictures a second | step, average / longest | late / crossed windows |
|---|---|---|---|
| standing on the grid, 9 s | 59.99 | 11.6 / 13.3 ms | 0 / 0 |
| the first 8 s under thrust | 59.69 | 13.6 / 16.9 ms | 1 / 0 |
| 20 s of driving with steering | 58.94 | 15.9 / 32.9 ms | 1 / 0 |

While driving, about 45,000 of the 76,800 panel pixels changed a
picture. The panel refreshes about 58.7 times a second, and a picture
that changes nearly everywhere cannot be shown more often than that, so
at about 58.9 the game runs at the panel's pace, not below it. The driving was crude
(held buttons, the ship brushed the walls); a clean lap at full speed
was not measured. The title's live scene runs at 30 (30.00 over 10 s,
a step of 11.7 ms on average and 12.6 at most).

## Measuring: the `rec` feature and `refghost`

**`rec`** (OFF by default; never in a build handed over for use) records
every race's controls, tick by tick, and when the race ends sends them
as log lines (`camber: rec H/D/E ...`, each at most 80 characters, 93
with the firmware's prefix). The header carries the format `VERSION`
(2), the race `RULES` the game was built with (2: the rebuilt rivals),
track, class, mode, the CATCH-UP setting (0 OFF, 1 LIGHT), seed, steps
and the race check. `examples/rec` decodes a captured log, checks its
CRC and replays it; it refuses a recording of another format version
or other race rules with a message that says which. The format is in
`src/rec.rs`. Chip build:
`RUSTFLAGS='--cfg feature="rec"' games/build-chip.sh games/camber`.

**`refghost`** (`examples/refghost`, feature `rec`, PC only) turns a
replayed recording into the person's path (`path`), races today's rivals
against it as a ghost they steer round (`ghost`, `--contacts
none|oneway`), races the rivals alone on every track (`tracks`), gives
one line a class plus every track over seeds 0 to 6 (`brief`), or
flies an equal ship with the rivals' driving (`pilot`); `--set
CLASS.field=v` and `--pace` try other skill rows and ships. `--catchup
off|light` picks the setting. The rivals were tuned with it against the
owner's three LAGOON races; it says how the rivals do against those
races, not how hard the game is for anyone else.

## Tests

From this folder:

```sh
cargo fmt --check
cargo clippy --offline --locked --all-targets --all-features -- -D warnings
cargo test --offline --locked --all-features
```

| File | What it checks |
|---|---|
| `tests/play.rs` | rivals finish every track at every class (two seeds), a time trial alone, a whole race through the keys on every track at both rates to the results and back, the same keys give the same race and pictures, both rates run the same race, every detail and view gives the same race on every track |
| `tests/controls.rs` | the title, pause and results menus by keys and by touch (every line of both title pages), every race control, a whole race by touch alone on every track |
| `tests/title.rs` | the RACE backdrop's cameras keep the followed ship in sight right of the menu, never stand on the road or in a tunnel's wall; three minutes of title write no best lap and no log line |
| `tests/backdrop.rs` | both backdrops: settings keep their values across OPTIONS and back (by buttons and by taps); PLAY starts the same race (RACE and TIME TRIAL, 70 s compared call by call: race, camera, picture, logs) after RACE, SHIP, or a track changed and back; the same keys give the same title pictures; ten minutes of title switching track, class and backdrop: no panic, no log, no best, no camera in the road or a tunnel's wall, the SHIP camera on the road behind the ship |
| `tests/carve.rs` | the carve: a press at speed starts it with no kick (not slower, not in the air, not a held button), the way (stick, then button; both buttons in the middle: none), one at a time and never turned back, heat never above the real turn (path and road) and wiggling no faster than holding, a straight alone never past step 1 (every track, straight and corner), the sling by step (stronger and longer) and nothing below step 1, letting go costs no more than coasting (the settle), a tap pays nothing and costs little, a hard wall hit ends it and a brush does not, no carve grip in the air, the air and a crawl end it, B brakes and L / R do not, the touch strip; the reward (each step stronger and longer, row B's numbers, stronger than last round's D at every step, the heat steps unchanged); the air brake stops the sling's push at every step while its time runs on |
| `tests/laps.rs` | with the reward, a good carver's best flying laps at least 9 % faster than the clean driver's on the three tracks, and abuse on the straights and taps in the corners no faster over three laps (runs every time); and, with `-- --ignored`: the reward table for six scripted drivers (`reward_table`; `CAMBER_DRIVERS`, `CAMBER_PATHS`, `CAMBER_CLASS`), the rivals and the three model players' places (`rivals_and_places`; `CAMBER_SEEDS`, `CAMBER_PLACE_SEEDS`), lap times and pictures (`CAMBER_DRIFT_OUT`, `CAMBER_DRIFT_TAG`), and the computer's pilot taking over your ship after the finish (`handover`: swings, stick and roll flips, knocks, 150 ticks after the finish) |
| `tests/steer.rs` | measurements only (`-- --ignored`; `CAMBER_STEER_OUT`, `CAMBER_STEER_TAG`): the pilot alone on a GLACIER straight with a flat racing line, started 3 m off it at 58 to 126 m/s (`straight_bench`: overshoot, time to settle, swing left, stick flips); whole races with eight computer-driven ships (`rivals_steer`: finish times, walls, falls, pads, stick and roll flips, distance from the line on plain road); the PC's time for a race tick (`pilot_time`); how far the baked racing line moves from one segment to the next (`line_steps`) |
| `tests/rivals.rs` | the rivals carve in every class up to their class's step (STEADY only step 1, BRISK up to 2, WILD to 3) and the harder class carves more corners; a rival presses only at speed, with room to both walls and after its rest; it presses with the stick in the middle, only in a corner that turns enough over the next 64 m (or, since the rebuild, a shorter one it comes to over its plan's speed), and holds or lets go exactly by the rules (corner ended, a wall near, step reached below step 3, stick against: each seen); at least a quarter of the carves the rivals pick pay a sling in every class and track (STEADY on FURNACE: at least 12 %; the carves for the grip counted apart); a rival moves exactly as the player's ship would with the same controls (over 165,000 ticks compared); the reward gives its sling time to every ship (four grids of every track); rivals pull no air brake on one side; `carve_stick` gives back the carve's rate; a carving rival shows the sparks, and still shows at least 4 pixels of them at 20 and 40 m on every track and detail (`CAMBER_SPARK_PICS` saves the pictures) |
| `tests/pace.rs` | no rival flies faster than the player's ship could (its pace never over 1024, never over the highest sling's top on the flat); the catch-up stays inside its size (exactly 2.0 % at full distance, nothing within 40 m, both ways, nothing outside the running race); the same race runs the same every time; the classes and the fronts and the pack come in order; the player's ship eases round after its finish on every track, class and grid (no carve, no air brake alone, no wall, no fall, at most 2 stick swings in 20 s, cruising at about half the top); a long run over every track, class, grid and both catch-up settings: no rival stuck, none held at a wall 1.5 s, at most 16 falls in the 126 races (GLACIER's gap); the PC's time for a race tick (`step_time`, `-- --ignored`) |
| `tests/contacts.rs` | hard wall hits and rival-to-rival contacts, rivals alone, seven races a cell, against the numbers before the rebuild: fewer wall hits in every class, no more contacts in all (the table: `contacts_table`, printed with `--nocapture`) |
| `tests/pictures.rs` | four pictures of the rebuilt rivals (grid, a fight in a corner, results, the CATCH-UP line; `-- --ignored`, `CAMBER_PICTURES=<dir>`); the backdrops (`backdrop_pictures`, `CAMBER_BACKDROP_PICTURES=<dir>`: SHIP at 8 angles on each track, the OPTIONS page at both values, 4 moments of RACE) |
| `tests/rec.rs` | (feature `rec`) every track, class and mode replays step by step, falls, noisy and slow races, a full buffer, damaged captures refused, a version 1 header and other rules refused, a race left at its first tick does not hide the next one, the game itself logs a recording that replays |
| `tests/golden.rs` | the golden record through `game_entry` (`tests/data/golden.txt`) |
| `tests/fill.rs` | game-3d's triangle filler, as CAMBER uses it (checked sums, rows counted), against the slow one, shared edges painted once |
| `tests/fuzz.rs` | random keys, triggers, touches and pauses on every track at both rates (`FUZZ_MIN`, default 3 minutes; the long run is 60), part of the time L and R mashed with the stick swinging or a finger in the carve strip: no panic, numbers (the carve's too) in range, no ship off the track for good or stuck; L / R mashing repeats exactly; and the computer's pilot driving the player with random L / R holds and stick overrides (4 seeds a `FUZZ_MIN` minute, every class): slings of every step happen, the numbers stay in range, the same seed repeats exactly, every sling of any ship lasts the reward's time for its step; the rivals carve in all of these races, their carve numbers are checked every tick, no rival slings above its class's step, and rivals' slings of every step their class allows happen |
| `tests/cost.rs` | the work and the sent pixels of every setting (the table above) |
| `tests/shots.rs` | pictures to look at, each moment from all three views (`-- --ignored`; written to `.local/camber/shots`) |
| `tests/diag.rs` | where the rivals lose their shields, the tracks' lengths (`-- --ignored`) |

The long fuzz run, with overflow checks and debug assertions:

```sh
CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true \
FUZZ_MIN=60 cargo test --release --offline --test fuzz -- --nocapture
```

The home tile (`art.txt`, 36 x 36, 16 colours) is the player's ship
from behind on LAGOON with rivals ahead; `games/art/make_art.py` turns
it into `ART.BIN`.
