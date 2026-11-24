# desktop-board: the simulator

The console's own code (console-core) running on the PC, on a "desktop
board". It is **not** an emulator of the ESP32 chip. The program is
called `pocketme-sim`.

It runs in two ways:

- **No window** (the default): it runs a number of frames, follows an
  input script, saves pictures, and stops. Same result every run, byte
  for byte. This is how workers and tests use it.
- **With a window** (`--window`): you play with the keyboard and the
  mouse.

## Running it

```sh
cargo run -- --help                                  # every option and key
cargo run -- --frames 60 --png shot.png              # no window: 60 frames, one PNG
cargo run -- --frames 90 --script walk.txt --shot 10,last --prints
cargo run -- --window                                # a window, 3x size, 30 fps
cargo run -- --window --scale 2 --fps 60 --overlay
cargo run -- --window --close-after 3                # closes itself after 3 s
```

The window needs the SDL3 library on the PC (on Arch: the `sdl3`
package). To build with no outside library at all:

```sh
cargo run --no-default-features -- --frames 60 --png shot.png
```

Then only the no-window mode exists, and the boot is the plain boot
screen (see "The boot animation" below).

`--close-after S` (0.5 to 5 seconds) is for quick looks by a worker.
The window then closes by itself, does not take the keyboard from what
you are typing in, and saves what it showed last as `window-last.png`
in the `--out` folder (default `shots`).

## Playing the games

Fourteen small programs are linked into the simulator: eleven games,
two prototypes (DRIVE3D and PROTO3D, working names, not finished games)
and BENCH (a speed test, a measuring tool and not a game). The sample
card has a tile for each, with its picture (ART.BIN), and the home
screen shows the 14 tiles in this order: BENCH, CAMBER, DRIVE3D,
HAIRPIN, KESTREL, LANDER, LANTERN, MERCATOR, PADTEST, PROTO3D, ROOFTOP,
SKIMMER, STARDUST, TUNDRA.

- **BENCH** is a speed test, not a game: fixed loads (fills, triangles,
  3D scenes, screen sends, a city street, a floating island) and a ramp
  that finds the largest load that fits 60 and 30 pictures a second.
  `Left` and `Right` pick the test, `Up` and `Down` the load, `A` (pad
  X) switches 30 or 60, `Z` (pad A) starts the ramp, `X` (pad B) stops
  it, `Return` leaves. The simulator says nothing about speed (it calls
  a game on time whatever the work costs); only the board does. All its
  keys are in `games/bench/README.md`.
- **CAMBER** is hover-ship racing on real 3D tracks of polygons (hills,
  banked turns, a jump, tunnels) against seven rivals: steer with the
  arrows, thrust with `Z` (or the right trigger `2`), brake (both air
  brakes) with `X` or the left trigger `1`, and hold `Q` or `W` (L or R)
  through a corner to carve, then let go for the sling; while
  racing `A` changes the detail, `S` the view and `Backspace` the
  pictures a second, and `Return` is its own pause menu. Its title menu
  has PLAY, TRACK, CLASS, MODE, OPTIONS (DETAIL, RATE, VIEW, CATCH-UP,
  BACKDROP) and LEAVE, over a live 3D backdrop. All its
  keys are in `games/camber/README.md`.
- **DRIVE3D** is a prototype, not a finished game: a rally car on
  gravel, grass and a ford along one short stretch with a hill, a crest
  and a drop, in 3D with a chase camera behind the car. It is played
  with the gamepad only (touch does nothing in it): steer with the
  arrows (or the stick on `I` `J` `K` `L`), throttle with `Z` (or the
  right trigger `2`), brake and reverse with `X` (or the left trigger
  `1`), handbrake `W`; `S` puts the car back on the road, `A` steps
  through the four details, `Return` opens its menu (LOW, MID, HIGH,
  MAX, RESTART; the run stands still while it is open) and `Backspace`
  shows a line of numbers. All its keys are in `games/drive3d/README.md`.
- **HAIRPIN** is a kart race on mountain roads (a pseudo-3D road, like
  old arcade racers): steer with the arrows, throttle with `Z` or `Up`,
  brake with `X` or `Down`, boost with `A`; `S` changes the view,
  `Backspace` the detail, `Return` is its own pause menu. Its title menu
  has PLAY, COURSE, DETAIL, RATE, VIEW and LEAVE. All its keys are in
  `games/hairpin/README.md`.
- **KESTREL** flies a small courier craft through a rock belt, gate by
  gate, drawn with real flat-shaded 3D polygons: steer with the arrows,
  boost with `Z`, brake with `X`, barrel roll with `Q` and `W`; `A`
  changes the detail, `S` the view, `Backspace` the pictures a second,
  `Return` is its own pause menu. Its title menu has PLAY, MISSION,
  DETAIL, VIEW, PICTURES and LEAVE. All its keys are in
  `games/kestrel/README.md`.
- **LANDER** is a moon lander: turn the craft, fire the engine (`Z`,
  `Up`, or the triggers `1` and `2`) and set it down on a flat pad,
  slowly and upright, before the fuel runs out. Its title menu has PLAY,
  LEVEL, VIEW, STEER, RATE and END. All its keys are in
  `games/lander/README.md`.
- **LANTERN** is a first-person walk through an old mine by lamplight
  (a ray-cast 3D view): walk with the arrows, side-step with `Q` and
  `W`, use with `Z`, flash the lamp with `X`, `A` changes the detail,
  `Backspace` shows the map, `Return` is its own pause menu. Its title
  menu has PLAY, LEVEL, DETAIL, RATE and LEAVE. All its keys are in
  `games/lantern/README.md`.
- **MERCATOR** is space trading in real 3D polygons: fly a small ship
  through a galaxy of 136 star systems, buy cheap, sell dear, fight
  raiders and buy better gear. In flight the arrows pitch and yaw, `Q`
  and `W` roll, `Z` fires the laser, `A` (or the right trigger `2`) is
  faster and at full speed cruise, `X` (or the left trigger `1`) is
  slower, `S` turns the auto-dock on or off, `Backspace` opens the
  chart and `Return` is its own pause menu. In the market `Z` or
  `Right` buys a tonne and `A` or `Left` sells one; hold the key to go
  on trading. Player 2's `Keypad 0` held fires the gunner's turret. Its
  title menu has PLAY (or CONTINUE), NEW GAME, GALAXY, DETAIL, VIEW, PICTURES,
  PITCH and LEAVE. In flight there are effects (engine trails, sparks,
  wrecks that fly apart, a nebula, the sun's corona), traffic (ships
  leaving and arriving at the station, a patrol, convoys with escorts
  or raiders), two hulls for each kind of ship in each system's own
  colours, and, far from the station, now and then a few rocks that
  burst when shot and leave a canister of ORE or ICE to scoop. All its
  keys are in `games/mercator/README.md`.
- **ROOFTOP** is a platformer of eight rooms: run with the arrows, jump
  with `Z`, dash with `A`; `Return` restarts the room, `Backspace` goes
  back to its title (START, ROOM, SCROLL, RATE, LEAVE). All its keys are
  in `games/rooftop/README.md`.

- **PADTEST** shows every input: the 12 buttons, both sticks, both
  triggers, where the finger is, and a level bubble for the motion
  sensor (all of player 1's pad), and at the right a small line per
  player 1 to 4: a dash while that pad is not connected, otherwise 12
  marks lit while its buttons are held. When nothing changes for a second it asks the console to call
  it only when something happens (the rate at the top says ON DEMAND).
- **PROTO3D** is a prototype, not a finished game: a small hero in 3D
  (a crimson top and a white cap) with a follow camera. It opens on the
  AREAS menu (ISLAND, MACHINES, SAIL, STONE; arrows move, `Z` or
  `Return` chooses, or tap a line). ISLAND is a floating island with six
  gold nuts to collect (four at LOW), four burrs (land on one from above
  to squash it; running into one throws the hero back) and three
  tuftlings that turn and hop when he comes near. MACHINES is about
  weight: pick up the brass bob and set it down with `A`, load the plate
  to raise the bridge, tip the plank and jump to the ledge. SAIL is a
  glide course against the clock: hold `X` while falling to glide.
  STONE is a course you build your own step for: `X` places the slab
  (ahead on the ground, or under the feet in the air), `X` held while
  standing on it raises it, and `A` calls it back. Run with the arrows
  or the left stick (`I` `K` `J` `L`), jump with `Z` (hold it for a
  full jump), turn the camera with the right stick (`T` `G` `F` `H`),
  and `Q` or `W` turn it by 45 degrees when let go (both: behind the
  hero). `Return` opens the menu (LOW, MID, HIGH, MAX, RESTART, AREAS:
  the four picture details, HIGH at the start; arrows move, `Z` or
  `Return` chooses, `X` closes; this menu takes no tap; in an AREAS
  menu opened from there `X` goes back to play). `Z` on the DONE panel
  (after half a second) starts the area again. `Backspace` shows or
  hides a line of drawing numbers. All its keys are in
  `games/proto3d/README.md`.
- **SKIMMER** is top-down air hockey for 1 to 4 players: one mallet a
  side (CORAL left, TEAL right), or two a side in 2 VS 2; the computer
  plays every side nobody holds. Player 1 moves with the arrows, `Z` or
  `X` dashes, `Return` is its own pause menu. Player 2 is the number pad
  (`8 4 2 6`, `0` or `.` dash, `+` pause), or the mouse as the touch
  screen (with no pad 2, the first click on the right half takes TEAL;
  the mallet follows the mouse while the button is held). Its title
  menu has PLAY, CORAL, TEAL, TEAMS, COMPUTER, MATCH, TABLE, RATE and
  LEAVE. All its keys are in `games/skimmer/README.md`.
- **STARDUST** is a toy: steer a comet with the d-pad, the left stick or
  your finger, and catch stars. Start (`Return`) ends it.
- **TUNDRA** is top-down ice hockey for 1 to 4 players, two skaters a
  team plus a computer goalie each; the computer plays every free
  skater. Player 1 skates with the arrows, `Z` passes (tap) or shoots
  (hold, then let go), `X` checks, `A` or `S` takes the other skater,
  `Return` is its own pause menu. Player 2 is the number pad (`8 4 2 6`,
  `0` shoot, `.` check, `1` switch, `+` pause), or the mouse as the
  touch screen (hold to skate there, click to shoot or check). Its
  title menu has PLAY, PLAYER 2, COMPUTER, MATCH, RATE, ICE MARKS and
  LEAVE. All its keys are in `games/tundra/README.md`.

```sh
cargo run -- --window          # then: Right to pick a tile, Z to start
```

On the home screen, `Left` and `Right` pick a tile, and `Z` (bottom face
button) or `Return` starts it. In a game, `Tab` (home) opens the pause
menu: RESUME, BRIGHTNESS, or BACK TO HOME. On the home screen, `Tab` or
a click on the gear at the right end of the bar opens the console menu:
BRIGHTNESS and CLOSE (and BOOT ANIM, see below). On the BRIGHTNESS line, `Left` and `Right` change
the step (five steps), and a click on a bar sets it. The window shows
the picture darker at a lower step; screenshots and fingerprints do
not change. With `--store FILE` the step is kept: it is saved 2 s after
the last change (as the board will save it, see
`crates/settings-store`) and read back at the next start. A run that
ends sooner than that does not keep the change. The sources are in `games/` (see
`games/README.md`, which also says how to add a game); `src/games.rs`
lists them.

## The boot animation (GATHER)

The simulator is built with console-core's feature `boot-gather` (on in
the default features): at power-on the GATHER animation plays (points
in a turning ball stream into the name POCKETME, which shrinks into the
home screen's title), about 2.7 s at 60 frames a second. Any key, click
or button skips it. The console menu on the home screen then has a
third line, BOOT ANIM, between BRIGHTNESS and CLOSE: `Z`/`Return` or a
click goes to the next value, `Left` and `Right` to the one before or
after. FULL is the whole cut (the default), SHORT only its end (the
solid name, its gold gleam and the shrink, under 1 s), OFF goes
straight to home. The choice counts from the next start and is kept
with `--store FILE` like the brightness.

`--boot full|short|off` sets it at start, over the saved value. With
`--store FILE` it is then written with the next save of the settings
(after a change in the menu), as if chosen there. The tests that are about home and the games, not
the boot, run with `--boot off` (or `settings::set_boot(.., Boot::Off)`),
so their frame numbers stay as they were; `tests/cli.rs` checks the
three values themselves. `--no-default-features` builds the simulator
without the feature: the plain boot screen, no BOOT ANIM line, and
`--boot` is refused. On the real board the feature `boot-gather` is
not in the default set, but it is in the owner's builds since the
owner approved the animation on the board (2026-11-19).

## The settings app

The simulator is built with console-core's feature `settings` (on in
the default features). Placement E puts a `GAMEPADS` tile before the
games and a `SETTINGS` tile after them (plus shortcuts in the quick
menu): `SETTINGS` holds brightness, the boot animation (with
`boot-gather`), and About; `GAMEPADS` shows Player 1 (and up to four
entries when flash holds older pads), `+ PAIR PLAYER 1`, and forgetting.
Selecting `+ PAIR PLAYER 1` or confirming `FORGET` acts directly from
the UI (auto-search without a BOOT hold) to open the search with pads
found, confirm `CALL IT AND PAIR?`, pair, or forget.
Without `--settings-entry` the console is as without the app.

| Option | Does |
|---|---|
| `--settings-entry tiles\|on\|off` | where the app is opened from: `GAMEPADS` and `SETTINGS` tiles in the home row plus quick-menu shortcuts (`tiles`/`on`), or nowhere (default `off`) |
| `--pair WHAT` | what the stand-in pairing radio does: `found` (default: finds pad A after 3 s, pairs it 2 s after YES), `two` (pads A and B), `none` (nothing in 60 s), `old-pin`, `lost`, `refused`, `store` (the call ends with that failure), `linked`, `damaged`, `not-held`, `busy` (refused at once), `off` (a board that cannot pair) |
| `--pair-saved N` | pads already saved, 0 to 4 (default 1, shown as connected) |
| `--pair-slots N` | free records in the pad memory, 0 to 255 (default: not known) |

There is no radio on the PC: the stand-in is board-api's scripted fake
radio (`crates/board-api/src/fake_radio.rs`, compiled into the simulator
by path because board-api exports it only with its dev-only `testing`
feature). The names and tails it shows are made up. Built with
`--no-default-features` (or without `settings`), these options are
refused.

## Keys in the window

The same table drives the window, `--help` and this list (a test
checks they agree). Keys are by their place on the keyboard, so a
non-QWERTY layout uses the same spots.

| Key | Does |
|---|---|
| `Up` `Down` `Left` `Right` | d-pad |
| `Z` | bottom face button (south) |
| `X` | right face button (east) |
| `A` | left face button (west) |
| `S` | top face button (north) |
| `Q` | left shoulder (L) |
| `W` | right shoulder (R) |
| `1` | left trigger (ZL), pulled all the way |
| `2` | right trigger (ZR), pulled all the way |
| `Return` | start (+) |
| `Backspace` | select (-) |
| `Tab` | home |
| `C` | capture |
| `I` `K` `J` `L` | left stick up, down, left, right |
| `U` | left stick pressed in |
| `T` `G` `F` `H` | right stick up, down, left, right |
| `Y` | right stick pressed in |
| `B` | the board's own BOOT button |
| mouse | touch: hold the left button, drag to move |
| `V` | motion: hold, and move the mouse to tilt the pad (right tilts right, up tilts away); let go to lay it flat |
| `M` | motion sensor on or off (it starts on) |

These are player 1's keys. The number pad is a second, smaller gamepad,
**player 2**. Its first key plugs pad 2 in (the window's title then
says `P2 pad`), and it stays plugged in. It has no sticks, home,
capture or motion. Players 3 and 4 have no keys; scripts drive them
(see below). A laptop with no number pad can only use scripts for
player 2.

| Key | Does (player 2) |
|---|---|
| `Keypad 8` `Keypad 2` `Keypad 4` `Keypad 6` | d-pad up, down, left, right |
| `Keypad 0` | bottom face button (south) |
| `Keypad .` | right face button (east) |
| `Keypad 1` | left face button (west) |
| `Keypad 3` | top face button (north) |
| `Keypad 7` | left shoulder (L) |
| `Keypad 9` | right shoulder (R) |
| `Keypad /` | left trigger (ZL), pulled all the way |
| `Keypad *` | right trigger (ZR), pulled all the way |
| `Keypad +` | start (+) |
| `Keypad -` | select (-) |

Keys for the simulator itself (the console never sees them):

| Key | Does |
|---|---|
| `P` | pause, or go on |
| `N` | one frame on, while paused |
| `F1` | overlay on or off |
| `F12` | screenshot of the panel (PNG in the `--out` folder) |
| `Escape` | quit |

The **overlay** draws a yellow box round each area the console sent
in the last send, and a line of numbers: the frame, the number of
areas, pixels and bytes sent, and the time the send would take on the
real panel against the 33.3 ms of one frame, and `light`, the backlight
value the console last asked for (`-` before the first). The line
turns red when the send would take longer than a frame. The overlay is
drawn on the window only, never into the picture or the PNGs.

Which buttons a game will really get is not decided yet. The keyboard
gives every button the board layer knows.

The **motion sensor** is a stand-in for the real one: it reports the pad
lying still and flat (gravity straight down) until you tilt it with `V`
and the mouse. Moving the mouse also turns the pad for one frame (the
gyro). The units are the game contract's: 4096 for one g, 16 for one
degree a second; x is to the right, y away from the player, z up.

## The input script

A plain text file, one action per line. The number in front is the
frame the action happens on (frame 0 is the first). Lines may come in
any order; lines with the same frame keep their order. `#` starts a
comment.

```text
# press start, then walk right for a second
0   press plus
2   release plus
10  press right
40  release right
50  touch 160 120        # a finger on the middle of the screen
52  lift
54  touch 260 80         # a finger on the home row...
55  touch 240 80         # ...drags it left, 10 picture pixels a frame
56  touch 220 80
57  lift                 # let go while moving: a flick
60  stick left 32767 0   # left stick fully right
70  stick left 0 0
75  trigger right 1023   # right trigger pulled fully
76  trigger right 0
80  card out             # pull the SD card
85  card in
88  pad off              # the gamepad goes away
89  shot                 # save this frame as PNG
90  tilt -2048 0         # right side down: x reads minus half a g
91  spin 0 320 0         # turning at 20 degrees a second
95  tilt 0 0             # flat again
96  motion off           # the motion sensor stops answering
100 notify HELLO THERE   # a console notification slides in
110 backlight 255        # check: the screen is at full brightness
120 p2 pad on            # player 2's pad connects
121 p2 press south       # player 2 holds its bottom face button
130 p4 pad on            # player 4's too
131 p4 stick left 0 -32767
```

**Players.** The game interface carries four players, one per
gamepad. A pad line (`press`, `release`, `stick`, `trigger`, `pad`,
`motion`, `tilt`, `spin`) may name its player right after the frame
number: `p1`, `p2`, `p3` or `p4`. Without it the line is player 1's, so
scripts written before there were four mean what they did. Player 1's
pad starts connected with motion on; players 2 to 4 start not connected
with motion off, so plug them in with `pN pad on` first (presses on a
pad that is not connected reach nothing). `boot` is the board's own
button, nobody's pad, and the lines that are no pad's (`touch`, `lift`,
`card`, `shot`, `notify`, `backlight`) take no player: both are
mistakes. Only player 1 drives the console's own menus (home, pause);
players 2 to 4 reach only a running game.

| Action | Meaning |
|---|---|
| `press NAME`, `release NAME` | a button down or up |
| `touch X Y` | a finger at panel pixel X (0 to 319), Y (0 to 239); also moves a finger already down |
| `lift` | the finger leaves the screen |
| `stick left\|right X Y` | a stick at X, Y, each -32767 to 32767 (up is negative Y) |
| `trigger left\|right V` | a trigger at V, 0 to 1023 |
| `pad on\|off` | the gamepad connects or goes away (a gone pad lets go of every button it held) |
| `pN` before a pad line | the line is player N's (1 to 4); without it, player 1's |
| `card in\|out` | the SD card goes in or comes out |
| `motion on\|off` | the motion sensor answers, or not (a pad with no sensor) |
| `tilt X Y` | the pad tilted: the accelerometer's x and y, each -4096 to 4096 (4096 is one g); z follows. Right side down makes x negative, far side down makes y negative. `tilt 0 0` is flat |
| `spin X Y Z` | the gyro: turning speed around x, y, z, 16 for one degree a second; it stays until the next `spin` |
| `shot` | save this frame as PNG (like `--shot`) |
| `backlight N` | a check, not an input: after this frame the backlight must be N (0 to 255). Anything else, or no backlight yet, is a problem, and the run fails |
| `notify TEXT` | a console notification, as the console's own news (`Console::notify`): printable ASCII, 32 letters at most, no `#`. It waits behind notifications already queued; a full queue is a problem, and the run fails |
| `press` then `release` on one frame | a press shorter than a frame: the board notes it, so the console sees it once |

Button names (any case): `south`, `east`, `west`, `north`, `l`, `r`,
`zl`, `zr`, `minus`, `plus`, `home`, `capture`, `lstick`, `rstick`,
`up`, `down`, `left`, `right`, `boot`. Face buttons are
named by place (south = bottom), because "A" is at the bottom on one
make of pad and on the right on another.

A press shorter than one console step is still seen once.

A mistake stops the run before it starts and names the line, for
example ``script line 3: unknown word `pres`: `3 pres south` ``.

## Testing with no window

What a worker uses to check a change without looking at a screen:

| Option | Gives |
|---|---|
| `--frames N` | how many frames to run (default 60) |
| `--shot 0,10,last` | PNGs of those frames, `shots/frame-00010.png` and so on |
| `--png FILE` | the last picture as FILE |
| `--prints` | `frame N fingerprint X` after every frame |
| `--trace` | `frame N sent ...`: areas, pixels, bytes and send time of each send; `frame N backlight V` when the backlight changes |
| `--quiet` | no console log lines |

The **fingerprint** is a short number made from every pixel of the
picture. Two runs with the same fingerprint showed the same picture.
To check that a change did not alter what is drawn, compare the
fingerprints before and after.

The last line is a summary: frames run, console frames, sends, sends
over the frame time, the simulated clock, the backlight value last
asked for (`-` if none) and the last fingerprint.

The **exit code** is 0 when all went well and 1 on any problem. The
problems are listed on the error output. A problem is:

- a mistake in the options or the script;
- a picture that cannot be saved;
- script lines or `--shot` frames after the last frame;
- anything the real board would not take (see below).

Log lines from the console start with the frame number:
`[frame    12] console: started`.

## Strict, and what is only a guess

Some limits are exact on the real board too. The simulator **fails the
run** when the console breaks one:

- more than 16 areas in one send;
- an area outside the 320x240 panel;
- a log line over 200 bytes (it is also cut);
- two sends in one console step;
- a console that asks to run again and again without time passing
  (more than 8 steps in one frame).

A card path that breaks the 8.3 rule gets an error back, as on the
real card; the console has to cope with it.

Other numbers are only estimates. They are shown, never refused:

- the **send time**: about 31 ms for a full screen, plus 0.1 ms per
  area. A guess from the panel's clock speed, not a measurement;
- the **bytes sent**: 2 bytes per pixel plus 11 bytes of commands per
  area;
- sends over the frame time are counted in the summary and shown in
  red in the overlay.

## Time

Time is simulated. The loop moves in fixed ticks: 33,333 µs at
`--fps 30`, 16,667 µs at `--fps 60`. A "frame" in scripts and options
is one tick. The console is run when the time it asked for falls in
the tick, or sooner when input changes and it asked to wake on input.
The clock then reads exactly the time it asked for.

With no window, ticks run as fast as the PC can. In the window, the
real clock only decides how many ticks to run (at most 4 to catch up).
So the window and the no-window mode give the same pictures for the
same input.

## The SD card

`--card DIR` makes a PC folder stand in for the card. It is read-only.
The card's rules hold: absolute paths, 8.3 names in capitals. Names
that break the rules are skipped, and the program warns about them.
Asking the size of a folder gives "not found", as a real card does.
`--no-card` runs with the slot empty.

With no `--card`, the simulator uses the sample card in `card/`:

```text
card/CONSOLE/GAMES/
  BENCH/     GAME.BIN  INFO.TXT  ART.BIN
  CAMBER/    GAME.BIN  INFO.TXT  ART.BIN
  DRIVE3D/   GAME.BIN  INFO.TXT  ART.BIN
  HAIRPIN/   GAME.BIN  INFO.TXT  ART.BIN
  KESTREL/   GAME.BIN  INFO.TXT  ART.BIN
  LANDER/    GAME.BIN  INFO.TXT  ART.BIN
  LANTERN/   GAME.BIN  INFO.TXT  ART.BIN
  MERCATOR/  GAME.BIN  INFO.TXT  ART.BIN
  PADTEST/   GAME.BIN  INFO.TXT  ART.BIN
  PROTO3D/   GAME.BIN  INFO.TXT  ART.BIN
  ROOFTOP/   GAME.BIN  INFO.TXT  ART.BIN
  SKIMMER/   GAME.BIN  INFO.TXT  ART.BIN
  STARDUST/  GAME.BIN  INFO.TXT  ART.BIN
  TUNDRA/    GAME.BIN  INFO.TXT  ART.BIN
```

All fourteen (the eleven games, the two prototypes and BENCH) are
linked into the program (`src/games.rs`); the
board finds each by its folder name. Their `GAME.BIN` is only a short
note ("NOT A GAME FILE"), which the chip's loader refuses as damaged (it
does not start with the word `GAME`); the real chip files come from
`games/build-chip.sh`. A test checks that no file on the card starts
with a game header. (A file with a correct header and filler would be
worse: the chip's loader would run the filler as code.)

The card used to have four made-up games as well (tiles that could not
start). They are gone; tests that need a game that is not linked in
("GAME NOT FOUND") or a long row of tiles make their own card folder in
the test build's temporary folder (`tests/games.rs`, `tests/drag.rs`,
`tests/attack.rs`).

`INFO.TXT` is a copy of `games/<game>/INFO.TXT` (a test checks they are
the same bytes). It has `KEY=VALUE` lines:

```text
NAME=LANDER
TAG=MOON LANDER
TILE=2E3550
```

`TILE` is the colour of the game's letter tile, as hex RGB (shown while
there is no picture), and `NEEDS=GPS` names an optional part the game
needs. `ART.BIN` is the tile picture, made from `games/<game>/art.txt`
by `games/art/make_art.py` (format: `games/README.md`); a test checks
it is the same file `games/build-chip.sh` makes for the chip card. The
file layout is a plan, not a final format. To remake the card (after a
game's INFO.TXT or art.txt changed, or a game was added to `LINKED`):

```sh
python3 tools/make_sample_card.py
```

## The other parts

| Part | On the PC |
|---|---|
| Screen | a 320x240 RGB565 picture. Only the areas the console sends change; old pixels stay elsewhere, as on the real panel |
| Touch | the mouse, or `touch`/`lift` script lines. A click shorter than a step is noted as a tap |
| Pad, Keys | four pads: the keyboard (player 1), the number pad (player 2), or `press`/`release` script lines (any player). A press and release before the next step (or on the same script frame) is noted, so the console still sees it once |
| Motion | flat and still; `V` and the mouse tilt it, or `tilt`/`spin`/`motion` script lines |
| Games | linked into the program (`src/games.rs`); started from their tile |
| Lent RAM | 48 KB, the size of the chip's game data slot (`board::LentRam`). The home screen keeps its game list and tile pictures in it while no game runs, as on the chip. Stricter than the chip on purpose: it starts as rubbish and is filled with other rubbish at every game start (good or failed) and every game end, so a home screen that showed old data after a game would show wrong names in a test. The rubbish is the same every run, so pictures stay repeatable |
| Clock | simulated, see above |
| Files | a PC folder, see above |
| Log | printed on the terminal with the frame number |
| Store | 16 KB that behaves like flash (erase sets bytes to FF). In memory, or in a file with `--store FILE`; a file of another size is refused. The first 8 KB hold the settings (`src/settings.rs`, crate `settings-store`); the log says what was read at start and each save |
| Link | the debug link, on a TCP address with `--link 127.0.0.1:7000`. One client at a time. With no client, bytes sent are thrown away |
| Radio, GPS, sound | absent |

## What the simulator cannot tell us

Only the real board can answer these:

- **Memory**: the PC has plenty. A game that runs out of the chip's
  RAM still runs here. The same goes for a game's size: here a game is
  linked in, so its code and data have no slots. On the chip, code and
  data together must fit in 104 KB, and code past the 56 KB code slot
  takes the top of the 48 KB data slot, shared with the data (stage 1;
  games/README.md, "Code past the code slot"). Only `games/build-chip.sh`
  and the console's loader judge that, and that such code runs was
  checked on the board, not here (2026-11-22, boards/cyd/README.md).
- **Speed**: the PC is much faster. A console step that takes too
  long on the chip looks fine here. The send time is only a model.
- **Tearing**: the real panel is drawn while it shows, so a picture can
  tear. The simulator shows each send at once and whole.
- **The radio**: Bluetooth pads, pass-by meetings and the like are not
  simulated.
- **Real touch**: the mouse is exact; the real touch screen is not, and
  it needs calibration.
- **The card**: the real card is slower, can fail, and has its own
  file system quirks.
- **Colours**: the PC screen shows the colours differently from the
  panel.

## Checking everything

```sh
./check.sh
```

It first builds every game in `games/` for the chip and checks that it
fits its slots (`games/build-chip.sh`: code at most 56 KB plus what
runs on into the data slot, and that part plus the data at most 48 KB,
from crates/game-api's `layout`; the sizes are printed). Then it runs, for every new crate and
every game: the format check, clippy with warnings as errors, and the
tests (with the default parts and with all of
them; the simulator also with no window). The window tests use SDL's
"offscreen" driver, so no window appears. It then builds the ten
shared crates for the ESP32 and for a bare PC with no standard library,
and runs this program once. No board is needed. It takes about 75
seconds after a change (most of it the random runs of
`tests/attack.rs`, three times: once per set of parts).

There is no shared Cargo workspace on purpose. A workspace above
`crates/` would take in the older crates and experiments too. So
`check.sh` checks each crate in its own folder, with one build folder
(`target/check`). Each folder pins Rust 1.99.0. The chip build names
the esp toolchain on the command line.

## Rules

- This is the only crate that may use `std`, the PC, and SDL3.
- The console must not know it runs here: nothing in console-core may
  depend on this crate.
- Tests run with no window (or the offscreen driver) and give the same
  result every run.
- A real window: at most 5 seconds, closing by itself, not taking the
  keyboard.
