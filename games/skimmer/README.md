# SKIMMER

Top-down air hockey on one screen, for 1 to 4 players. An original game:
its table, rules, looks and name are made for this console.

- Two sides: **CORAL** (left, round mallets with a round knob) and
  **TEAL** (right, eight-sided mallets with a square knob). The sides
  differ by shape as well as colour.
- One mallet a side, or two a side in **2 AGAINST 2**. A mallet stays on
  its own half of the table.
- A mallet a person holds shows that player's pad number on its knob; a
  computer mallet shows a small dot; the touch mallet's knob is plain.

## The rules

- A match starts with a serve: the puck waits on the serving side's
  spot ("CORAL SERVES") and comes free after a short moment. CORAL
  serves first in the first match; in a rematch, the loser of the last
  match serves first.
- Get the puck into the other side's goal slot: GOAL! The slot and the
  score flash, and the side that conceded serves next.
- A puck left on one half for 7 seconds is "TOO SLOW": it goes to the
  other side's serve spot. Dots in the top bar count down the last
  seconds.
- MATCH sets the length: FIRST TO 5, 7 (the default) or 11, or TIMED.
  A timed match lasts 3 minutes; when it ends level, a GOLDEN GOAL of
  at most 1 minute follows (the next goal wins); nobody scores in that
  minute: DRAW.
- At the end the winner is shown with the score and the series (matches
  won since the title). REMATCH plays again with the same settings,
  TITLE goes back to the title.
- TABLE: PLAIN, or WILD with two bumpers on the centre line that kick the
  puck away.

## Controls

Every pad (players 1 to 4):

| Pad | What it does |
|---|---|
| left stick or d-pad | move the mallet |
| A or B | dash: a short burst of speed (then a short rest) |
| START | pause menu |

In the menus: up and down pick a line, left and right change a value,
A or START chooses, B or START closes the pause menu.

The **touch screen** (finger or, in the simulator, the mouse) drives one
mallet: it follows the finger, one finger only, no faster than 5 pixels
a tick, and never off its own half. A tap on the top bar pauses. All
menu lines can be tapped; on a value line, a tap on the left half steps
back, on the right half forward.

## Who plays which side

The title has a line for each side: AUTO, PAD 1, PAD 2, TOUCH,
COMPUTER. A pad or the touch screen can drive only one side at a time.

- **CORAL, AUTO** (the default): pad 1 when it is connected, else the
  touch screen. If pad 1 drops out while a finger already holds TEAL,
  the finger keeps TEAL and the computer plays CORAL until pad 1 is
  back.
- **TEAL, AUTO** (the default): pad 2 when it is connected. With no pad
  2, the first finger put down on the right half during a match takes
  TEAL for the rest of that match ("touch plays teal" in the log).
  Until then the computer plays it.
- The title shows what AUTO means right now ("AUTO: TOUCH").
- TEAMS: 1 VS 1 or 2 VS 2. In 2 VS 2, pad 3 plays CORAL's second
  mallet and pad 4 TEAL's; a missing pad is the computer. The back
  mallet stays near its goal, the front one stays away from it.
- COMPUTER: EASY, NORMAL (the default) or HARD.
- RATE: 60 (the default) or 30 frames a second. The game plays the same
  at both; only the picture is smoother.

Pause: RESUME, COMPUTER, RATE, RESTART (the same match from 0-0), TITLE,
LEAVE (back to the console's home). A tap outside the panel resumes.

## How it is built

- `src/fixed.rs`: whole-number maths. Positions are in 1/256 of a
  pixel; there is no floating point anywhere.
- `src/table.rs`: one match in fixed ticks of 1/60 s. The puck moves in
  sub-steps of at most 1.5 pixels against every mallet, so it never
  passes through a mallet or a rail at any speed or frame rate. A
  squeezed puck makes a mallet give way (it slides at most 4 pixels
  round the puck in one sub-step). Mallet speed adds to the puck;
  a little drag; a top speed.
- `src/cpu.rs`: the computer's mallets (no level dashes). A puck lying
  against a rail is hit from the nearest spot the mallet can reach, as
  long as that sends it forwards; a shot that would run flat along the
  rail goes into the rail instead, so the puck comes off at an angle
  (without this, two EASY computers could pass the puck along a rail
  for ever).
- `src/app.rs`: the screens, the menus and who drives which mallet.
- `src/draw.rs`: the palette (28 colours) and the painting; the game
  repaints the whole 160x120 picture each call and the console sends
  only what changed.
- `src/lib.rs`: the console side. When nothing moves (the title, the
  pause, the result screen) it asks to be called only when something
  happens, so the console sleeps.

Uses only the game kit. No heap, no floating point, no `unsafe`.

## Numbers

All MEASURED ON THE PC on 2026-11-18, the chip size on 2026-11-23:

- Chip size (`games/build-chip.sh`): code 35,308 of 57,344 bytes (62%),
  data 7,620 of 49,152 bytes (16%).
- Changed pixels a frame over a minute of play (the console's own diff
  rule; the picture has 19,200 pixels; `tests/pixels.rs`): one player at
  60 fps average 540 (2.8%), 99th percentile 2,055 (10.7%), worst 4,668
  (24.3%); 2 against 2 at 60 fps average 1,384 (7.2%), 99th percentile
  3,261 (17.0%), worst 5,761 (30.0%). The worst frames are the one frame
  after each goal, when the pieces go back to the serve. Never more than
  the 16 areas the console allows.
- Computer against computer over 80 minutes of play: NORMAL beats EASY
  339-159, HARD beats EASY 415-38, HARD beats NORMAL 409-262 (the
  ignored test `cpu::tests::print_duels`).

On the board (MEASURED, 2026-11-23, two cores, the starting 60
setting, one match against the computer driven by scripted keys, 1,119
pictures): 59.99 pictures a second between pictures back to back (59.73
counting one pause of over 0.1 s), the console's step 2.3 ms on average
and 2.7 ms at most, 0 crossed windows (`boards/cyd/README.md`, "At the starting settings").
Not measured: more than one player, the WILD table, and how the touch
screen feels.

## Tests

`cargo test --offline` here: every mode reaches a final (pad against
computer, pad against pad, pad against touch, touch against computer,
2 against 2, WILD, every length), the conceding side serves, the touch
mallet stays on its half and under its top speed, pause and title menus,
AUTO, pads leaving and coming back, the series, sleeping when still, the
same match at 30 and at 60 frames a second, uneven frame times, the same
input giving the same match, and pictures of every state
(`tests/shots.rs`). `tests/fuzz.rs` plays long random matches (random
pads, sticks, fingers, frame times) and checks after every call that the
puck is on the table, never inside or through a mallet, and that every
rally and every computer match ends:
`SKIMMER_FUZZ=4000 cargo test --release --offline --config
profile.release.overflow-checks=true --config
profile.release.debug-assertions=true --test fuzz`.
