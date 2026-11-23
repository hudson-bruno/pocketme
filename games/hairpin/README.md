# hairpin: HAIRPIN

A kart race on mountain roads at dusk. Small electric karts race the
service roads of a mountain dam: round the lake below it, through the
pine forest above it, and up the hairpin bends to the radio mast on the
summit. Three courses, three laps each, four rivals. Finish in the top
three to go on to the next course; win all three for the cup.

Braking and rolling charge your kart's battery; the BOOST spends it for
a higher top speed. So the race is about when to brake (a bend has a
top speed, and you slide to its outside if you go faster) and where to
spend the charge (on a straight, to pass).

All of it is original: the name, the karts, the three courses, the
pictures and the rules were made for this game. It is a racing game
like many, drawn the way old arcade racers drew their roads, but it
copies no existing game's names, courses, characters or look.

Words used below:

- **pseudo-3D road**: the road is not a real 3D model. It is a list of
  short pieces ("segments", 200 world units long), each with a bend and
  a height. The game works out, piece by piece, how far away each one is
  and how much the bends have pushed the road sideways by then, and
  fills the screen rows between two pieces with grass, kerb and road.
- **sprite**: a flat picture standing beside the road (a tree, a post, a
  sign, another kart), drawn smaller the further away it is.
- **send**: the console copying the changed parts of the 160 x 120
  picture to the screen. On this board sending takes far longer than
  drawing, so the game is built around sending little.
- **game pixel**: one pixel of the 160 x 120 picture; the panel shows it
  as 2 x 2 = 4 panel pixels.

## How to play

The title has a menu (the course you chose is drawn behind it):

| Line | What it does |
|---|---|
| PLAY | race the cup, starting at the chosen course (START also plays, from any line) |
| COURSE | 1 RESERVOIR, 2 PINEWOOD, 3 SUMMIT (left/right). The bottom line shows its best time |
| DETAIL | LIGHT, NORMAL or HEAVY (default), see [The three details](#the-three-details) |
| RATE | 30 or 60 (default) pictures a second |
| VIEW | CHASE (default: behind your kart) or NOSE (from the driver's seat) |
| LEAVE | back to the home screen |

Every setting can always be chosen; the game starts with HEAVY, 60 and
CHASE every time (since 2026-11-23, the owner's choice; before that
LIGHT, 30 and CHASE). DETAIL, RATE and VIEW can also be
changed in the race's pause menu, and DETAIL (SELECT) and VIEW (Y)
straight from the race.

The race starts with three red lamps, one a second, then GO. The top
bar shows the lap (L1/3), the race time and your place, and on the right
the pause box. Below the view: the speed, the battery bar and the two
touch boxes BRAKE and BOOST (the BOOST box is lit while the battery
holds charge). In LIGHT the frame round the view also shows the last and
best lap, a strip with every kart on the lap, and the order of the karts
(yours has a white box).

After the line, the race runs on for a moment, then a card shows your
place, time and best lap, and NEXT COURSE (top three), RACE AGAIN
(fourth or fifth) or SEE THE CUP (after course 3), and TITLE. The cup's
end lists each course's place and time and the total.

### The rules

- **Speed.** Top speed is shown as 120 KMH. Off the road (past the
  kerb) the top speed is less than half, and the kart shakes.
- **Bends** push the kart to their outside, more the faster it goes (with
  the square of the speed); steering moves it only in step with the
  speed. So each bend has a top speed: take a sharp one too fast and you
  run wide even with full steering. Brake before it.
- **Hills** slow you going up and speed you up going down.
- **The battery** starts a third full. Braking at speed charges it fast,
  rolling (no throttle, no brake) above a third of top speed charges it
  slowly. **Boost** raises the top speed by a quarter and pulls harder;
  a full battery lasts 3 seconds of boost. Braking stops the boost.
- **Crashes.** Trees, rocks, posts, poles, railings, signs, lamps, the
  radio mast and the start arch's feet beside the road stop the kart short (a quarter of its speed is
  left) and throw it back towards the road. Bushes are only looks. The
  lake is a crash too: a kart that comes in from far off the road outside
  the railing is stopped and put back on the road's side of it.
- **Rivals** keep to their lane, take the inside of bends, slow down for
  what is coming, and go round slower karts. Karts that touch push each
  other apart, and the one behind is held back.
- **Laps.** Each lap is timed; the best lap is kept. The finish place
  counts after three laps. The best race time per course is kept while
  the game runs (not saved when you leave).

### The courses

Counted from `src/track.rs` (a "segment" is 200 world units; the road is
2,000 wide). Bends are written from -16 to 16: 16 is a hairpin. The
rivals' top speed is in thousandths of yours (1,024 = as fast as you).

| Course | Name | Where | Segments a lap | Sharpest bend | Highest point | Rivals' top speeds |
|---|---|---|---|---|---|---|
| 1 | RESERVOIR | round the lake below the dam: gentle bends, small hills, water and a railing on one side | 1,230 | 10 | 2,400 | 880 860 840 820 |
| 2 | PINEWOOD | through the pine forest: rolling hills, sharper S-bends | 1,270 | 12 | 3,500 | 935 910 885 860 |
| 3 | SUMMIT | snow, rocks, three hairpins up to the radio mast, a long fast way down | 1,120 | 16 | 8,000 | 965 945 920 895 |

The rivals get faster and the bends sharper from course to course.
Signs with arrows stand before every bend of 9 or more and along its
outside.

How hard (MEASURED (simulator), `tests/play.rs`): the test's autopilot,
which drives close to the limit, finishes in about 63 s (course 1) and
66 s (courses 2 and 3), and wins each. The rival you must beat for third
place came in 13 s (course 1), 9 s (course 2) and 6.5 s (course 3)
after it. So course 1 forgives a lot, course 3 wants clean driving.

### Controls

Every control works with the gamepad alone, and the whole game (title to
cup) also works with a finger alone. The simulator keys are the ones
`boards/desktop/README.md` lists for each pad button; HAIRPIN is the
fourth tile on the simulator's home screen.

| Gamepad | Simulator key | While racing |
|---|---|---|
| left stick, left / right | `J` / `L` | steer, as far as the stick is pushed |
| right stick, left / right | `F` / `H` | steer too (when the left stick is in the middle) |
| d-pad left / right | `Left` / `Right` | steer fully |
| A, or d-pad up | `Z`, `Up` | throttle |
| right trigger | `2` | throttle, as far as it is pulled |
| B, d-pad down, or L | `X`, `Down`, `Q` | brake |
| left trigger | `1` | brake, as far as it is pulled |
| X or R | `A`, `W` | boost (while the battery holds charge) |
| Y | `S` | next view (CHASE, NOSE) |
| SELECT (-) | `Backspace` | next detail (LIGHT, NORMAL, HEAVY) |
| START (+) | `Return` | the game's pause menu: RESUME, DETAIL, RATE, VIEW, RESTART, TITLE |
| HOME | `Tab` | the console's own menu (the game is not called; its time stops) |

The sticks have a dead zone (a middle range that counts as zero) of
4,000 of 32,767, so a stick that does not quite centre does not steer.
In menus: up/down (d-pad or stick) moves, left/right changes a value,
A chooses (on a value line A steps it on). In the pause menu START or B
closes it.

**Touch** (the mouse button in the simulator):

| Where | What it does |
|---|---|
| anywhere on the view | drive: full throttle, and steer by how far from the middle of the screen the finger is (6 pixels either side of the middle go straight; 56 or more steer fully) |
| lift the finger | roll (no throttle): the battery charges slowly |
| the BRAKE box | brake |
| the BOOST box | boost (and full throttle, straight on) |
| the two bars in the top right corner | the pause menu |
| a line of a menu | chooses it; on a value line, a tap right of the middle steps it on, left of the middle steps it back |
| a line of the finish card | chooses it |

The board's touch screen feels one finger, so with touch alone you
cannot steer and brake at the same time: brake before the bend, then
steer through it. The tests race a whole course this way and win it.

### The three details

| Detail | 3D view | What it adds | Sent game pixels a picture, mean / worst (CHASE, 30) |
|---|---|---|---|
| LIGHT | 112 x 66 in a frame with lap strip, order and lap times | plain road and grass, a backdrop that stays put, no bushes | 4,597 / 7,834 |
| NORMAL | 160 x 90, the whole width | stripes on the road and kerbs, the backdrop turns in bends, near hills, bushes | 9,992 / 14,776 |
| HEAVY (default) | 160 x 110, the whole screen below the top bar | stripes on the grass too, edge lines, small BRK and BST boxes over the view | 13,227 / 17,950 |

Stripes that run towards you are the strongest feeling of speed, but
they change every road row in every picture, and that costs most of
the send. LIGHT keeps one colour each for road, kerb and grass and gets its sense
of speed from the dashed middle line, the things beside the road and
the rivals.

## How it is built

A `no_std` library that uses only `game-kit`: no heap, no floating
point, no `unsafe`, no panic handler of its own. All maths is in whole
numbers ("fixed point": whole numbers that stand for fractions; for
example a kart's place across the road is counted in 1/16,384 of half
the road's width).

| File | What is in it |
|---|---|
| `src/track.rs` | the three courses as short lists of sections (a length, a bend, a climb, what stands beside the road); each segment is worked out from that list when it is needed, so no big table sits in memory |
| `src/race.rs` | one race, in fixed ticks of 1/60 s: speed, grip, bends and hills, the battery and boost, crashes, the rivals' driving, bumps, laps and places |
| `src/render.rs` | the 3D view: segment ends projected to screen rows, the ground front to back (a hill top hides what is behind it), the backdrop, sprites back to front; and the work counter (`Cost`) |
| `src/art.rs` | the palette (48 shared colours and 48 for each course, with three steps of evening haze for the far road) and the sprites, made from letter tables while compiling |
| `src/hud.rs` | the top bar, the dashboard, LIGHT's frame, the countdown, menus and cards, and where a finger lands |
| `src/app.rs` | the screens (title, race, pause, finish card, cup) and the controls |
| `src/lib.rs` | the console side: input, palette, frame rate, the video hint, log lines |

The game asks the console for 60 (or 30) pictures a second while racing
and for "only when something changed" on the title, in the pause menu
and on the cards, so a still screen costs nothing to send. The palette
is set once and never changes (on this console a palette change sends
the whole screen). Every 4 s of racing it logs one line with the work
of the last picture (`hairpin: cost HEAVY segs .. rows .. sprites ..
sprite px ..`).

The game's state is built in place, never as a big value on the stack:
`App::blank()` is a `const`, and the title's course is set up on the
first `update` (the field `loaded`), as LANTERN does.

The home tile (`art.txt`, 36 x 36, 16 colours) shows the yellow kart
from behind on a road that bends towards the mountains at sunset, with a
bend sign and switchbacks on the mountain. `games/art/make_art.py`
turns it into `ART.BIN`.

## Numbers

Every number says how it was found:

- **COUNTED**: counted exactly, by the tests on the PC or from the chip
  build's files.
- **MEASURED (PC)**: what the tests measured on the PC, with the
  console's own rule for what it sends (`console_video::diff`).
- **ESTIMATE**: worked out from those and from what the board measured
  for LANTERN. The board has since measured this game (NORMAL and
  HEAVY, one core against two, 2026-11-18): `boards/cyd/README.md`,
  "One core or two". The numbers below were not redone from it.

**Size on the chip** (COUNTED, `games/build-chip.sh`, 2026-11-23): code
37,604 of 57,344 bytes (66 %); data 12,484 of 49,152 bytes (25 %:
constants 12,040, starting values 440, zeroed 4).

**Stack on the chip** (COUNTED from the chip build's machine code: the
`entry a1, N` instruction at the start of each function, which reserves
N bytes; `xtensa-esp32-elf-objdump -d HAIRPIN.elf`): the biggest frame
is `App::draw`, 2,576 bytes (it holds the projected segment ends of the
3D view); then the game's `update` 912, `Race::reset` 272; everything
else 208 or less. The deepest chain (update, draw, sprite drawing) is
about 3.6 KB.

The ESTIMATE column below turned out too low at 60: on the board NORMAL
ran at 59.7 and HEAVY at 58.7 pictures a second with two cores (55.0 and
45.7 with one; MEASURED, 2026-11-18, `boards/cyd/README.md`, "One core
or two"). HEAVY at 60 is the default since 2026-11-23, and was measured
again that day: 58.87 pictures a second over about 20 s of a race with
the accelerator held, the console's step 15.5 ms on average and 16.9 ms
at most, 0 crossed windows (MEASURED, two cores, no gamepad connected,
one scripted run; `boards/cyd/README.md`, "At the starting settings").

**Sent game pixels a picture** (MEASURED (PC), `tests/cost.rs`: the
autopilot races all three courses to the finish in each setting; "p95"
means 95 of 100 pictures send this much or less):

| Detail | View | Rate | Mean | p95 | Worst | ESTIMATE pictures a second on the board |
|---|---|---|---|---|---|---|
| LIGHT | CHASE | 30 | 4,597 | 6,859 | 7,834 | 30 |
| LIGHT | NOSE | 30 | 3,684 | 5,592 | 7,386 | 30 |
| NORMAL | CHASE | 30 | 9,992 | 13,903 | 14,776 | about 27 |
| NORMAL | NOSE | 30 | 10,560 | 13,647 | 14,460 | about 27 |
| HEAVY | CHASE | 30 | 13,227 | 16,804 | 17,950 | about 26 to 27 |
| HEAVY | NOSE | 30 | 13,215 | 16,727 | 17,735 | about 26 to 27 |
| LIGHT | CHASE | 60 | 4,048 | 6,344 | 7,681 | about 55 to 60 |
| LIGHT | NOSE | 60 | 3,095 | 5,003 | 6,891 | about 55 to 60 |
| NORMAL | CHASE | 60 | 8,852 | 13,294 | 14,841 | about 30 to 34 |
| NORMAL | NOSE | 60 | 8,780 | 12,426 | 14,282 | about 30 to 33 |
| HEAVY | CHASE | 60 | 11,244 | 15,454 | 17,526 | about 27 to 29 |
| HEAVY | NOSE | 60 | 10,744 | 14,750 | 17,318 | about 27 to 29 |

The whole picture is 19,200 game pixels. The rule of thumb from the
board says a setting must stay at or below about 6,000 on average and
8,000 at worst to hold 30 a second on one core; LIGHT CHASE 30 (the
default until 2026-11-23) does in this test (the test checks it), but
only just. The default HEAVY CHASE 60 sends about twice that (mean
11,244, worst 17,526) and leans on the board's two cores (58.7 a second on
the board, above). Per course LIGHT CHASE 30 is
mean 3,789 / 4,898 / 5,064 and worst 7,194 / 7,715 / 7,834, so the later
courses run closer to the limit. A check in the simulator (the same
autopilot, replayed with the simulator's own timing, MEASURED) gave
means of 3,993 / 5,123 / 5,309 and one picture of 8,046 on course 2 (of
about 2,000; rivals being passed beside a near sign), and a sloppier
driver on the PC a worst of 7,998 in 26,700 pictures. The other screens: a still title or pause menu sends 0, a
step in a menu about 2,000, opening the pause menu 8,700 once, going
from the title into the race 16,000 once.

How the estimate is made (`step_us` in `tests/cost.rs`): LANTERN on the
board sent about 6,000 game pixels in about 12 ms (30.0 a second, 58 at
the 60 setting) and about 11,000 in about 30 ms (26.6 a second). The
panel refreshes about every 17 ms and the console times each send to
it, so a send that does not fit one refresh window waits for the next.
The model takes a step of 1.5 ms plus 2 microseconds a sent game pixel
up to 8,000, and 37.6 ms above that, then counts each picture as at
least one period of the chosen rate.

**Drawing** (COUNTED pixels written, `tests/cost.rs`; time is an
ESTIMATE): in LIGHT the renderer writes about 7,800 pixels a picture on
average and 11,700 at worst (ground rows plus sprite pixels), in HEAVY
about 16,000 and 24,000. That is less work than LANTERN's ray casting,
which took 3 to 4 ms on the board, so drawing should take about 1 to
3 ms. With the player's kart far off the road among the trees (which a
crash normally prevents) a picture can write up to about 47,000 sprite
pixels (COUNTED, `tests/fuzz.rs`): ESTIMATE 4 to 5 ms of drawing.

## Tests

From this folder: `cargo test --offline --all-features`.

| Test file | What it checks |
|---|---|
| `tests/rules.rs` | the race rules: nothing moves in the countdown, top speed, bends have a top speed, the battery and boost, grass is slow, posts stop the kart, the lake cannot be driven on, three timed laps, places, only the top three go on, every course is a closed loop and each is harder than the last, a race left running for half an hour never overflows |
| `tests/controls.rs` | the defaults (course 1, HEAVY, CHASE, 60), the title and pause menus by pad, stick and finger, START pauses and the race stands still, SELECT and Y while racing, the finish card, the rate and hint the game asks for |
| `tests/play.rs` | replays: the autopilot wins the whole cup from the title; each course alone; the d-pad alone and a finger alone each finish a course |
| `tests/fuzz.rs` | random input (every button, stick, trigger and touch value, odd time gaps) never panics and keeps to the game's palette; the view drawn from every segment of every course in every detail and view. `HAIRPIN_FUZZ=<calls>` runs longer (default 12,000; 400,000 was run once) |
| `tests/cost.rs` | the send cost table above, written to `.local/next/racing/cost.txt`, with LIGHT CHASE 30's limits checked and pictures of its worst picture's areas |
| `tests/look.rs` | pictures of real play, written to `.local/next/racing/shots/`; a note (LAP 2, DETAIL: ...) covers neither LIGHT's lap times, the pause menu nor the finish card |
