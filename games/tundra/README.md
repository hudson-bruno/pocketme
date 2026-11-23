# TUNDRA

Top-down arcade ice hockey on one screen, for 1 to 4 players. It is the
console's first multiplayer game. An original game: its rink, rules,
looks and name are made for this console.

- Two teams: **AMBER** (round players, attacks to the right) and
  **BLUE** (square players, attacks to the left). The teams differ by
  shape as well as colour, so they can be told apart in any light.
- Each team has two skaters and a goalie. The goalies are always played
  by the computer. Every skater nobody holds is played by the computer
  too (EASY or HARD).
- Players 1 and 3 play for AMBER, players 2 and 4 for BLUE. A skater a
  person holds shows that player's number; a computer skater shows a
  small cross.

## The rules

- A match starts with a face-off at the centre spot. The puck drops
  after a short wait ("FACE-OFF").
- Get the puck over the other team's goal line, inside its net: GOAL!
  The nets flash, the score goes up, and there is a new face-off.
- A goalie stops a slow puck and holds it a moment, then passes it
  out; a fast one bounces off.
- MATCH sets the length: 2, 3 or 5 minutes on the clock, or FIRST TO 5
  goals. A timed match that ends level goes to overtime: sudden death,
  the next goal wins at once. Overtime lasts at most 3 minutes of game
  clock (the top bar counts it down, `OT 3:00` to `OT 0:01`); if nobody
  scores in that time, the match ends as a DRAW. A FIRST TO 5 match
  always has a winner, so it has no overtime.
- At the end the winner is shown with the score and the series (matches
  won since the title). A draw shows DRAW and the score, with no team
  badges; the series goes up for neither team (draws are not counted
  anywhere). REMATCH plays again with the same settings, TITLE goes back
  to the title.
- The ice keeps marks from fast skating and checks: the newest 160,
  older ones go first; a new match starts on clean ice. ICE MARKS on
  the title turns them off.

## Controls

Every pad (players 1 to 4):

| Pad | What it does |
|---|---|
| d-pad or left stick | skate |
| A (bottom) or R | with the puck: tap to pass to your team mate; hold to wind up a shot and let go to shoot (longer hold, harder shot, straight ahead) |
| B (right) | without the puck: a check, a short dash that knocks the puck off a carrier and can stun them |
| X or Y | a player alone on a team takes the other skater (the one the computer had) |
| START | pause menu |

A pass to a team mate the computer plays hands your control to that
mate, so you keep the puck.

In the menus: up and down pick a line, left and right change a value,
A or START chooses, B or START closes the pause menu.

The **touch screen** (finger or, in the simulator, the mouse) can be
player 2, the right team:

- Hold a finger on the ice: your skater skates to it.
- Tap (short, without moving): with the puck, a shot towards the tap
  (or at the goal when the tap is right next to the skater); without
  the puck, a check towards the tap.
- A tap on the top bar pauses. All menu lines can be tapped; on a
  value line, a tap on the left half steps back, on the right half
  forward.

In the simulator, player 1 is the keyboard and player 2 the number pad:

| Simulator key | Player 1 | Player 2 (number pad) |
|---|---|---|
| skate | arrows (or `I J K L` for the stick) | `8 4 2 6` |
| pass / shoot | `Z` (or `W`) | `0` (or `9`) |
| check | `X` | `.` |
| take the other skater | `A` or `S` | `1` or `3` |
| pause | `Return` | `+` |

`Tab` (home) opens the console's own pause menu over the game as in
every game. Scripts drive all four players (`p3 press south`, see
`boards/desktop/README.md`).

## Who plays the right team: PLAYER 2

The title line PLAYER 2 says who drives player 2's skater:

- **AUTO** (the default): a second pad, when one is connected. With no
  second pad, the first finger on the ice during a match takes player 2
  for the rest of that match ("touch plays player 2" in the log). Until
  then, and when nobody touches, the computer plays it.
- **COMPUTER**: always the computer, the touch screen does nothing on
  the ice.
- **TOUCH**: the touch screen, from the start.
- **PAD 2**: the second pad only (the computer while it is not
  connected).

Players 1, 3 and 4 are always pads 1, 3 and 4 when those are
connected, and the computer otherwise.

## Title and pause menus

Title: PLAY, PLAYER 2, COMPUTER (EASY, HARD), MATCH (2 MIN, 3 MIN,
5 MIN, FIRST TO 5), RATE (30 or 60 frames a second; 60 at the start),
ICE MARKS (ON, OFF), LEAVE (back to the console's home).

Pause: RESUME, COMPUTER, RATE, RESTART (the same match from 0-0), TITLE,
LEAVE (back to the console's home). A tap outside the panel resumes.

## How it is built

- `src/fixed.rs`: whole-number maths. Positions are in 1/256 of a
  pixel; there is no floating point anywhere.
- `src/play.rs`: one match in fixed ticks of 1/60 s (skating, the
  puck, the boards with cut corners, goal mouths and nets, goalies,
  checks, goals, the clock, overtime). The same inputs always give the
  same match.
- `src/cpu.rs`: the computer's skaters.
- `src/app.rs`: the screens, the menus and who drives which skater.
- `src/draw.rs`: the palette (26 colours) and the painting; the game
  repaints the whole 160x120 picture each call and the console sends
  only what changed.
- `src/lib.rs`: the console side. When nothing moves (the title, a
  menu, the pause, the result screen) it asks to be called only when
  something happens, so the console sleeps.

Uses only the game kit. No heap, no floating point, no `unsafe`.

## Numbers

All MEASURED ON THE PC on 2026-11-18, the chip size on 2026-11-23:

- Chip size (`games/build-chip.sh`): code 34,500 of 57,344 bytes (60%),
  data 8,004 of 49,152 bytes (16%). On 2026-11-18 it was code 36,348
  and data 8,532 (after the overtime cap; 36,176 before it).
- Changed pixels a frame over a scripted minute of play at the RATE 30
  setting (the console's own diff rule; the picture has 19,200 pixels):
  one player average 531
  (2%), worst 1,676 (8%); four players average 504 (2%), worst 2,973
  (15%); never more than the 16 areas the console allows
  (`tests/pixels.rs`).
- Those minutes replayed through the screen send model
  (`boards/cyd/send-plan`, feature `model`): 30.00 frames a second,
  0 crossed. The model's drawing time is its default guess, not a
  measured TUNDRA time, so this is an ESTIMATE of the board.
- Computer against computer over 3 minutes: EASY about 4 to 11 goals a
  match, HARD about 9 to 14 (the ignored test `cpu::stats::print_stats`).

Measured on the board (2026-11-18, two players, the 60 setting, a
gamepad connected; `boards/cyd/README.md`, "One core or two"): 59.77
pictures a second on one core and 59.99 on two. At the starting
settings of 2026-11-23 (60, one pad against the computer, no gamepad
connected, one scripted run of 1,287 pictures): 59.99 on two cores, the
console's step 2.7 ms on average and 3.0 ms at most, 0 crossed windows
(`boards/cyd/README.md`, "At the starting settings"). Not
measured: how the touch screen feels for steering.

## Tests

`cargo test --offline` here: the rink is symmetric, a puck through the
mouth is a goal, the face-off, overtime (it counts down and a level one
ends as a draw exactly at 3 minutes, a goal in it still wins at once,
FIRST TO 5 never goes to overtime; the draw screen's picture goes to
`.local/four-players/hockey/overtime/`), computer teams score on both
levels, a one-player match from the drop to the final whistle and a
rematch, two and four players each moving only their own skater, touch
as player 2 (steer and tap-shot, tap-check, pad 2 wins over touch in
AUTO), the pause menu (resume, title, leave), sleeping when still, the
same input giving the same match, and the changed-pixel minute.
`tests/fuzz.rs` plays long random matches; it checks, among other
things, that every computer-only match ends and that no overtime runs
past its cap (`TUNDRA_FUZZ=10 cargo test --release --offline --test
fuzz` for a longer run).
`boards/desktop/tests/games.rs` plays it through the whole simulator.
