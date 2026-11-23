# console-core

The console itself, the same on every board: the boot screen, the home
screen with the games on the card, the bar at the bottom, short
messages ("notifications"), a running game with its pause menu, and
the console menu with the screen brightness. It does one frame of work per call and never
waits. The board runs the loop around it.

```rust
let mut console = Console::new();      // const: can live in a static
loop {
    let wake = console.step(&mut board);
    // sleep until wake.deadline(), or until input if wake.on_input()
}
```

One step, in order: read the clock once. If nothing is due, return.
Read the input (the levels and the board's notes of short presses, see
below). Update the page on show (boot, home, or a game). Draw the
page, the bottom bar and the notifications. Hand the changes to the
board. Say when to come back.

## Modules

| Module | What |
|---|---|
| `lib.rs` | `Console`: the one state machine (Boot, Home, Game), and when to wake |
| `theme` | every colour, size and timing, in one place |
| `input` | buttons, stick, BOOT button and touch turned into menu actions; `Gesture`, the touch recogniser (down, tap, drag, release) any page can use |
| `boot` | the boot screen: a pixel-art handheld, then POCKETME, 1.5 s (with the feature `boot-gather`: `boot_gather.rs`, the GATHER animation) |
| `gather` | feature `boot-gather` only: the GATHER drawing, a pure function of the time (`gather/tables.rs` and `gather/name.rs` are built while compiling) |
| `home` | the games on the card as a row of tiles; no card / no games |
| `art` | the tile pictures (`ART.BIN`): the reader with its checks, the 6 kept in memory, drawing |
| `lent` | where home's game list and tile pictures sit in the RAM the board lends (the empty game data slot), as plain bytes |
| `bar` | the bottom bar: uptime, pad, battery, button hints, the menu's gear |
| `notify` | the queue of short messages that slide in and out |
| `game` | a running game: calls it, its rate, its requests, its log, the picture under the overlays |
| `gamein` | the pad, stick, triggers, touch and motion turned into the game's `Input` |
| `pause` | the console menu: over a game RESUME, BRIGHTNESS, BACK TO HOME; on the home screen BRIGHTNESS, CLOSE (with `boot-gather` also BOOT ANIM) |
| `gfx` | small drawing helpers, fixed-size text, the pixel art |

## What it does

- **Boot**: the handheld icon is drawn row by row, then the letters of
  POCKETME drop in, then the icon's screen lights up. 1.5 s in all. Any
  button, the BOOT button, or a tap skips it. With the feature
  `boot-gather` the GATHER animation plays instead (below).
- **Home**: one tile per game, 36x36, in a row. The chosen tile has a
  gold frame; its name and tag show below, with dots for where you are
  in the list. Left/right (pad, stick, or tapping a tile) moves the
  frame; the row scrolls smoothly when the frame would leave the screen.
  A finger on the row drags it, one to one, up to its ends. Let go, it
  glides to a resting place (a tile at the left margin, or the row's
  end); let go while moving fast (a flick), it goes on to the next
  resting place in that direction, further for a faster flick. The
  selection stays on a tile that is fully on screen, frame and all: if
  a drag pushes the chosen tile off, the nearest shown one is chosen.
  A button during or right after a drag works at once and ends the
  glide. A finger put on the row while it glides stops it where it is;
  that touch is no tap (lifted, the row settles on the nearest resting
  place; dragged, the row moves from where it stopped). A / Start, or tapping the chosen tile, asks the board to start
  the game (`Board::start_game(folder)`). A board that cannot start games
  shows "GAMES CANNOT START YET". A tile shows the game's picture
  (`ART.BIN`, 36x36, 16 colours of its own) when the card has one, and
  the letter tile otherwise; see "Tile pictures" below. With no card: a card picture and
  "NO CARD". With a card but no games: "NO GAMES". The card is looked at
  again every 2 s, so pulling it out or putting it in shows at once.
- **Bar**: uptime as H:MM, a pad picture (green when a pad is
  connected), the battery as an outline with "?" (not measured yet),
  the button hints, and a gear at the right end (gold while the menu
  is open).
- **Menu on the home screen**: Home on the pad, or a tap on the gear,
  opens a small box: BRIGHTNESS and CLOSE (and BOOT ANIM with
  `boot-gather`). B, Home, CLOSE or a touch on
  the gear close it. While it shows, the row does not move and no game
  starts. Notifications still come on top of it.
- **Brightness**: five steps, never off, the brightest the default
  (`theme::BACKLIGHT`: 16, 42, 87, 156, 255; evenly spaced in lightness,
  CIE L* 30 to 100). The BRIGHTNESS line shows a sun and five bars,
  filled up to the step in use. While it is chosen, Left and Right
  change the step; OK (or BOOT) goes one step up, and from the brightest
  to the dimmest; a touch on a bar sets that step, and a finger sliding
  on the bars takes the step under it. Each change shows at once. The
  console calls `Screen::backlight` once just after its first picture
  was sent (never before), and then once per change, never with 0. It
  saves nothing: a board gives the saved step with
  `Console::set_brightness` before the first step, and saves when
  `Console::take_settings_changed` is true (once per change by the
  player).
- **Notifications**: slide down from the top (0.2 s), stay 2.5 s, slide
  back up. Up to 4 wait in a queue. Used for: pad connected, pad
  disconnected (pad 1 says `PAD CONNECTED` / `PAD DISCONNECTED` as
  before; pads 2 to 4 say `PAD 2 CONNECTED` and so on), game cannot start, card removed, card ready, game ended,
  game stopped. They wait during the boot screen. Over a game they are
  drawn on top, and the game's picture under them is put back after
  each frame is sent.
- **Game**: see below.

## A running game

The contract is `crates/game-api`; games are written with
`crates/game-kit`. The console:

- asks the board for the game by folder name. If that fails it stays
  home and says why: "GAMES CANNOT START YET" (the board cannot start
  games), "GAME NOT FOUND", "GAME FILE IS DAMAGED" (bad file, or a
  table too small), "GAME IS TOO OLD" (contract version 1),
  "GAME NEEDS A NEWER CONSOLE" (the game refused the console's table);
- clears the picture, gives the game its start colours (0..239), keeps
  240..255 for itself, and calls `update` once per frame at the game's
  rate: 30 a second (the default), 60, or on demand (only on input
  change, the game's timer, a start or a resume; motion alone does not
  wake it; a stick, a trigger or a moving finger wake it at most 60
  times a second, buttons and touch at once);
- answers video requests at the next call (only mode 0 is offered;
  hints and colour settings are kept but change nothing yet);
- prints the game's log lines as `game: ...`;
- Home, or the board's BOOT button, opens the pause menu over the
  game: RESUME (also B or Home again), BRIGHTNESS (see above) or BACK
  TO HOME (calls the game's `exit`). RESUME is chosen when it opens, so
  BOOT alone opens and resumes, as before; BOOT and a touch on the bars
  change the brightness. Time stops while paused. The picture under the
  menu is put back on resume. The menu has no title any more: with the
  BRIGHTNESS line, a title would need 720 more bytes under it;
- goes home when the game ends: "GAME ENDED" when it says it is done,
  "GAME STOPPED: ERROR" when it aborts (a panic on the PC), nothing when
  the player chose BACK TO HOME. The game's colours (0..239) are put
  back to black and the tile colours set again, so home looks exactly
  as before the game (`tests/game.rs` checks the palette, the picture
  and the panel). Then it calls `Board::end_game`.
- calls the game only with a `game_api::LiveFrame` (real buffers, sizes
  filled in by `game-api`), so this crate needs no `unsafe`.

## A game that panics

On the PC the game kit catches a panic. `tests/panic.rs` proves, for a
panic in the first call, in a later call and in `exit`: the console goes
home with "GAME STOPPED: ERROR" (no notice after a panic in `exit`, as
for any BACK TO HOME), logs the panic, has no game and nothing paused,
tells the board (`end_game`), drops the game's state exactly once, puts
home's colours back, and a second start is fresh (new state, frame 0,
time 0, the default rate, the start picture and colours).

On the chip a panic cannot be caught: there is no unwinding (a game is
built with `panic = "abort"`), and the game's panic handler calls
`ConsoleApi::abort`, which must never return. Today the chip's `abort`
(boards/cyd, games.rs) panics: the panic handler prints "game aborted:
panic, source line N" and a backtrace on the serial port, then the chip
STOPS with interrupts off. It does not restart: esp-hal turns every
watchdog off at start. Only a reset (`boardctl reset`, the RST button)
brings it back (measured 2026-11-17 with a test game). For it to come
back home instead, the board must:

1. Give the console its own `update` and `exit`: small functions of the
   board that save a recovery point (the stack pointer, the return
   address and the saved registers; on the ESP32 first spill the
   register windows, as ESP-IDF's `setjmp` does) and then call the
   game's real function. It wraps them with `unsafe { UpdateFn::new(..) }`
   in the `GameCallbacks` it returns from `start_game`. The console
   itself does not change.
2. Make `abort` jump back to that recovery point, so the board's
   `update` returns `status::ABORTED` (and `exit` simply returns). Only
   the game's code and the board's small function are skipped, and
   neither holds anything that must be cleaned up. The console's own
   code is never in between.
3. Refuse a jump when no game call is running (an `abort` at any other
   time is a real fault: stop as today), and after a jump, answer
   every later call with `ABORTED` until the next `start_game`.
4. In `end_game`: forget the game's entry and table, and clear the two
   game slots (code 56 KB at 0x4009_2000, data 48 KB at 0x3FFF_4000)
   so nothing of the first game is left for the next one. boards/cyd
   does this part already (about 1.1 ms).
5. Optionally treat a CPU fault (a bad memory read or write, an illegal
   instruction) the same way when the faulting address is inside the
   game's code slot.

What cannot be promised on the chip: the ESP32 has no memory guard
between the game and the console, so a game that wrote over the
console's memory before it panicked can still break the console; a
game that never returns from `update` hangs the console (no watchdog
runs today; a watchdog would restart the chip); a game that uses up the stack overwrites what is below it.

## The card

```text
/CONSOLE/GAMES/<FOLDER>/GAME.BIN   the game (must be there, not empty)
/CONSOLE/GAMES/<FOLDER>/INFO.TXT   optional, lines KEY=VALUE:
    NAME=LANDER       shown name (12 letters at most; else the folder name)
    TAG=MOON LANDER   shown under the name (16 letters at most)
    TILE=2B3A67       tile colour, RRGGBB (else one of 4 default colours)
/CONSOLE/GAMES/<FOLDER>/ART.BIN    optional: the tile picture (version 1,
                                   36x36, 1 to 16 colours, 4 bits a pixel;
                                   format in games/README.md)
```

Other keys are skipped. A folder without `GAME.BIN` is not a game. Games
are sorted by name, and by folder name when two names are the same.
Games with no `TILE` get the default colours in that sorted order. So
the screen looks the same whatever order the card lists its folders in.
Only the first 32 folders the card lists are looked at; a folder that is
not a game still uses one of those 32 places. A game with no `ART.BIN`
(or one that is refused) shows its letter tile: its colour and the
first letter of the name.

### Tile pictures

- **Format**: `ART.BIN` version 1, at most 688 bytes (games/README.md;
  the tool is `games/art/make_art.py`). The size is asked first and
  must be exactly what the header says; the file is read with at most 3
  bounded reads; every pixel must name one of the file's colours. Any
  fault (cut, too long, other version, size, colour count, depth,
  pixel, a card error) and the tile stays the letter tile; the reason
  is logged once (`home: picture refused: <FOLDER> (<reason>)`). No
  `ART.BIN` is normal and logs nothing.
- **Memory**: home keeps `theme::ART_SLOTS` = 6 pictures (680 bytes
  each: 16 colours and 648 bytes of pixels), every tile on screen (at
  most 5) and the next one off it. They are in the lent RAM (the empty
  game data slot, see "Home lives in the game data slot"), 4,080 bytes
  there; the console itself keeps only which game is in which slot
  (12 bytes, `art::Art`).
- **Colours**: picture slot `k` owns palette entries `104 + 16 k` to
  `+ 15` (`theme::ART_FIRST`), so pictures never share colours. A game
  owns them while it runs; home sets them again when it ends.
- **When the card is read**: only while the row is still: no finger on
  the row, not held, no glide, no scroll left to do. Never during a
  drag, a glide or a button scroll (a test counts the card calls). Then
  every wanted picture not yet known is read in the same step, nearest
  the selection first, so the palette changes once and the screen is
  sent whole once (about 17 ms on the CYD) per batch. Read at the first
  home picture after boot, after a scroll comes to rest on tiles not
  seen before, and after the card is read again (put back, or a game
  ended). A refused or missing picture is not tried again until the
  card's games are read again. The card check every 2 s also waits
  while the row is not still.
- **Before it is read**: the tile shows its letter tile.

### Home lives in the game data slot

The game list and the tile pictures (5,396 bytes) are not in the
`Console`. They are in RAM the board lends while no game is loaded
(`board_api::Board::parts_and_lent`; on the chip the 48 KB game data
slot, which is empty while the player is at home). Layout in
`src/lent.rs`: a 4-byte stamp, 32 games of 41 bytes, 6 pictures of
680 bytes. The console reads and writes it as plain bytes (no
`unsafe`); a letter that is not printable reads as `?`, so rubbish can
never crash it.

- **Starting a game** ends the loan, whether the start works or not:
  home forgets its list first (`Home::lose_list`), then asks the board
  to start the game. The loader writes the game over the slot (or wipes
  it after a failed load). From then on nothing reads the old list.
- **Back home** (the pause menu, the game ending, a game that panics, a
  bad or missing game file, a board that cannot start games, a card
  pulled or changed during the game): home reads the card again before
  it is shown. It takes three steps, at the frame rate:
  1. the step the game ends (or fails to start): the board gets the slot
     back only at the end of that step; nothing is drawn or sent;
  2. the next step reads the list (one folder listing, then the size of
     each `GAME.BIN` and each `INFO.TXT`); nothing is drawn or sent;
  3. the next step reads the tile pictures on screen, then draws home.
  So the screen keeps the last picture it had (the game, or its pause
  menu) for two more frames (about 67 ms at 30 frames a second) and then
  shows home whole, pictures and all. Home is never shown half-read:
  never with no games, never with letter tiles where pictures will be.
  Splitting the list and the pictures over two steps keeps each step
  about as long as the boot screen's single card scan, cut in two
  (`Console::home_pending` is true in steps 1 and 2).
- **The stamp**: each time home reads the list, it writes a new number
  at the start of the lent RAM and keeps the same number itself. If the
  RAM ever holds another number (a board that broke the rules), home
  reads the card again instead of showing rubbish.
- **A board that lends nothing** (the default) still runs; home then has
  nowhere to keep a list and shows "NO CARD".

Room left in the slot for home: 49,152 - 5,396 = 43,756 bytes (not used
yet; more pictures or a longer list could go there later).

## Input map (the console's menus)

| Input | Action |
|---|---|
| D-pad, left stick | move (held: again after 0.4 s, then every 0.12 s) |
| bottom face button, + (Start), board's BOOT button | OK (start) |
| right face button | back (not used yet) |
| Home | opens the console menu (over a game: the pause menu); it no longer goes to the first game |
| Left / right on BRIGHTNESS | one step dimmer / brighter |
| tap | choose a tile; tap the chosen one to start. A tap counts when the finger lifts (believed one frame late, `theme::TOUCH_GRACE`), if it moved less than 4 picture pixels (`theme::DRAG_START`) and was held at most 0.5 s (`theme::TAP_MAX_US`); a tile is started only if it was already chosen when the finger went down |
| long press (over 0.5 s) on a tile or the gear | nothing (not a tap) |
| drag on the row | scrolls the row; let go: it glides to rest (a flick goes further) |
| finger on the row while it glides | stops it where it is. Lifted: the row settles on the nearest resting place, and nothing is chosen or started. Dragged: the drag goes on from there |
| tap the gear (bar, right end) | opens the console menu (on the lift, like a tile) |
| tap in a menu | acts when the finger goes down (as before), so the tap time limit does not apply; on the bars it sets the step, and a finger sliding there follows. A finger already down when a menu opens does nothing in it |

The stick counts once pushed past 13,000 (of 32,767), and stops once back
under 9,000. All of it is in `input::BUTTON_MAP` and `theme`.

## Input map (a game)

In `gamein` and `game`. Which physical button is "confirm" is not
decided by the owner yet; this follows the console's menus (bottom =
confirm).

**Four players** (game interface version 3). The console asks the board
for pads 0 to 3 every step; board pad `n` is game player `n + 1`
(`Input::players[n]`). Each player has its own held, pressed and
released bits, sticks, triggers and motion, worked out from that pad
alone, so nothing leaks from one player to another and the same button
on two pads is two presses. A pad that goes away gives held 0, so
everything it held arrives once as released. Buttons held on any pad
when a game starts or resumes stay hidden until let go there. A press,
a stick past its rest or a connection on any pad wakes an on-demand
game. Only pad 0 (player 1) drives the console's menus (home, the
pause menu, Home opens it); players 2 to 4 reach only a running game.
Touch is shared (not a player).

| Pad | Game sees |
|---|---|
| D-pad | UP, DOWN, LEFT, RIGHT |
| bottom / right / left / top face button | A / B / X / Y |
| L, R | L, R |
| + / - | START / SELECT |
| ZL, ZR (buttons only) | trigger at 32,767 |
| sticks | -32,767..32,767 (y down) |
| analog triggers (0..1023) | 0..32,767 |
| touch | the picture's pixels |
| motion (each pad, when it sends it) | 4096 per g, 16 per degree per second |
| Home | never: opens the pause menu |

## Short presses

A press or a tap can be shorter than a frame. The board may note it
(`take_pressed`, `take_tap` in `board-api`); the console takes the notes
once per step, next to what is held now, and treats a noted press as
pressed in that step:

- the menus move or choose once, as for a normal press;
- a game sees the button as pressed and released in the same call, and
  a short tap as touch start and touch end in the same call;
- an on-demand game (one that runs only when something happens) is
  woken at once;
- the press that started a game, or that closed the pause menu, is not
  handed to the game (the same as a held button, which the game sees
  only once it is let go and pressed again).

A board that notes nothing works as before.

A noted tap is a finger down and up together, so it is a tap even
though touch now acts on the lift. It acts at once, in its own frame
(no grace frame: it is already over). A note that comes while a finger is
already down is ignored (it is the same finger).

## Touch

`input::Gesture` turns the finger into four reports, in picture
pixels:

- **down**: a finger touched (the boot skip and the pause menu use it);
- **tap**: the finger lifted without moving `DRAG_START` (4) pixels on
  either axis, held at most `TAP_MAX_US` (0.5 s); reported at the
  point where it went down, one frame after the lift (the grace below);
- **drag**: the finger moved that far, then every frame it moves (the
  first one carries the whole way from the down point);
- **release**: a drag ended, with the finger's speed in pixels a
  second (0 if it rested 0.1 s, `TOUCH_STILL_US`, before the lift).

The finger may vanish for `TOUCH_GRACE` (1) frame (the board's touch
is noisy); the touch goes on: a press stays a press, a drag a drag. So
a tap, and a drag's release, come one frame after the lift. A missing
finger that the board noted touching again since the last frame is not
missing (one bounce). A press is timed from the read that saw it go
down to the first read that missed it, so a finger resting while the
console sleeps is still timed right. When the page changes (boot ends,
a game starts or ends, a menu opens or closes) the touch is forgotten
until the finger lifts, so the finger that skipped the boot or ended a
game taps nothing; a forgotten touch gets the same grace, so one lost
read does not make it a new touch. Speeds and
flicks use whole numbers only (`theme::FLICK_*`). The numbers are not
tuned on the board yet.

## Few changed pixels

The look is calm on purpose: a still star field, the frame moves instead
of tiles growing, the clock shows minutes only, no colour cycling. The
renderer sends only what changed. Bytes on the wire (2 per panel pixel,
11 per changed area; measured by `tests/shell.rs`, sample card):

| Moment | Bytes |
|---|---|
| a full screen (for scale) | 153,611 |
| home screen doing nothing, 1 s | 0 |
| moving the frame one tile (one frame) | 34,756 |
| a notification sliding in (worst frame) | 10,494 |
| ... the whole slide in (about 7 frames) | 43,797 |
| row scrolling (worst frame; 9 games) | 53,530 |

## Waking

While something moves (boot animation, row scrolling, a notification
sliding, a finger dragging the row), the console asks for the next
frame, 30 a second. A finger counts as moving for 0.1 s after it went
down or last moved, and while it is missing in its grace frame (a
press, a drag or a forgotten touch); so a lift is seen at the next
frame even if the board does not wake the console for it, and the tap
comes one frame after the lift. A finger that rests on the screen lets the console
sleep again. Otherwise it
asks to be woken on input, or at the next timed change: the bar's next
minute, the next card check (2 s), a notification leaving, a key repeat.
An idle home screen sends nothing.

A step woken early by input (before the next frame was due) keeps that
frame time. It does not push the next frame further away, so a stick or
a finger that wakes the console often cannot stall the screen.

While the menu over a game shows, a finger on it is looked at every
frame in the same way, so a finger sliding on the brightness bars is
followed. The open menu on the home screen asks for nothing more than
the home screen: it sleeps and sends nothing while left alone.

`Console::notify` adds a message from outside; call `step` soon after,
as the wake time given before does not know about it.

## Memory

No heap. Fixed sizes. PC numbers from `tests/memory.rs`; ESP32 numbers
from a compile-only build for `xtensa-esp32-none-elf`.

| What | PC | ESP32 |
|---|---|---|
| renderer (`Video`: 2 pictures, palette) | 39,064 | 39,056 |
| home (selection, scroll, card state, the stamp; of it 12 for which picture is in which slot) | 72 | not measured alone |
| notification queue (4 x 32 letters) | 160 | 160 |
| running game (`game::Run`: 2,560 under the notices, 2,880 under the pause menu, 176 the last input, 128 the rest) | 5,744 | 5,736 |
| input (72, with the touch recogniser), boot, bar, clock times, brightness and the home menu, the rest | 152 | not measured alone |
| **`Console`** | **45,192** | **45,168** |
| not in the `Console`: in the lent RAM (the game data slot), stamp 4 + game list 32 x 41 + tile pictures 6 x 680 | 5,396 | 5,396 |

Home moved into the game data slot (2026-11-18): `Console` 50,472 to
45,056 on the PC (-5,416), 50,456 to 45,032 on the ESP32 (-5,424,
`pocketme_cyd::CONSOLE`, `nm`, in a `demo-card,touch,link,store,pad`
build). The list and pictures now take 5,396 bytes of the 49,152-byte
slot, which is empty while home is shown; 43,756 bytes of the slot are
still free for home. See "Home lives in the game data slot".

Four players (2026-11-18, game interface version 3): the last input grew
from 52 to 176 bytes and `Run` keeps a hidden-buttons word per player
(+12), so `Run` went from 5,608 to 5,744 on the PC (+136) and `Console`
from 45,056 to 45,192. On the ESP32 `pocketme_cyd::CONSOLE` went from
45,032 to 45,168 (+136; `nm`, `demo-card,touch,link,store,pad` build);
the ESP32 `Run` figure is that growth added to the old one.

Tile pictures (games and splash art step): `Console` 46,376 to 50,472
on the PC (`tests/memory.rs`), 46,352 to 50,456 on the ESP32
(`pocketme_cyd::CONSOLE` in a `touch,link,store` firmware build, `nm`;
the home row's ESP32 number is that difference added, not measured on
its own). Before that:

Measured again for the owner decisions D1, D4, D5 (2026-11-17; with an
ESP32 probe): the touch recogniser keeps the time a press went down and
a "held too long" flag (+8 bytes, 48 to 56); home's "a finger stopped
the glide" flag fits in its padding.

Measured again in step 4 (drag scrolling; with an ESP32 probe): the
touch recogniser added 40 bytes to the input state, and the drag state
16 bytes (8 on the ESP32) to home.

Measured again for the brightness setting (with an ESP32 probe):
`Console` 46,200 to 46,368 on the PC (46,168 to 46,336 on the
ESP32), +168 bytes. The menu box over a game grew from 80 x 34 to 80 x
36 for the third line (+160 in `Run`); the step, the last backlight
value, the "changed" flag and the home menu take 5 bytes (8 with
padding).

A running game adds 5,744 bytes (5,736 on the ESP32) that are there
all the time, game or not. Most of it keeps the picture under the
notification band (160 x 16) and under the pause menu (80 x 36), so the
game never has to redraw after them.

Stack, measured again in step 2 (read from the compiled ESP32 code with
the fake board, `entry a1, N` lines; the probe is kept outside the
repository): `step` 256 bytes, `Run::step` 96, `Run::call_update` 480
(the frame table, the input and the 256-byte log buffer), so about 0.9
KB of the console's stack is in use when a game's `update` starts. The
game's own use comes on top (PADTEST and STARDUST: under 0.5 KB).
`present` adds 224, plus what the board's
own `present` uses. The CYD sets aside about 117 KB of stack.

Fixed limits: 32 games, names 12 letters, tags 16, messages 32 letters,
4 messages waiting, `INFO.TXT` read up to 192 bytes, paths 64 bytes.

### Starting as zeros

`Console::new()` is all zero bytes (since step 3). Why that matters:

- A `static` that starts with any nonzero byte goes in `.data`. The
  whole value is stored in the flash, and copied to RAM at every
  start.
- A `static` that starts as all zeros goes in `.bss`. Nothing is
  stored. The start-up code just clears that RAM.

The CYD keeps the console in a `static`. Before step 3 it sat in
`.data`, because of 3 bytes: the renderer's "send everything" flag
(`true`), and the size and version of the last game input (then 52
and 2; 176 and 3 since four players).
Now the flag means the opposite, "the panel is in step" (`false` at
start). And the last input starts as zeros: no game was called yet,
and a game's start puts in a fresh one before the first call. So the
picture on the screen and what games get are the same as before.

On the CYD (default features, compile-only build): `CONSOLE` moved from
`.data` to `.bss`. `.data` went from 55,592 to 9,464 bytes, so the
firmware stores 46 KB less in flash.

`tests/zeros.rs` keeps it so. It puts a new `Console` in a `static` of
the test program, and reads the program's own file to see which section
the compiler chose. One nonzero byte, and the test fails.

## Boot animation GATHER (feature `boot-gather`, off by default)

The owner saw it on the board and approved it on 2026-11-19; the
firmware's owner builds turn it on (`boards/cyd` feature `boot-gather`,
not in its default set), and the simulator has it on by default.
Without the feature nothing changes: `boot.rs`, its timing and the
stored golden pictures are as before.

With it, `boot_gather.rs` is the boot page and `gather/` draws it:
points in a turning sphere stream into the console's name, the letters
heat up and turn solid, a gold gleam crosses the name, then the name
shrinks into home's title while the bar slides in. Its last picture is
home's background, stars, bar and title, so home only adds its own
things.

- **Time.** The picture is a pure function of the time since the boot
  page's first step (`gather::draw(t_ms, short, pixels)`), the time
  taken once per step from the board's clock. A late step shows a later
  picture; nothing is kept between frames.
- **Rate.** While it moves the console asks for a frame every
  `theme::BOOT_FRAME_US` (16,667 µs, 60 a second). 33,333 plays it at
  30 a second; every test passes at both rates except the stored boot
  fingerprints (`tests/data/golden_gather.txt`, written again then).
- **Setting.** `boot::Anim`: FULL (the whole cut, 2.72 s, the default),
  SHORT (joins 120 ms before the gleam: solid name, gleam, shrink;
  0.96 s), OFF (straight to home). The home menu's BOOT ANIM line
  changes it (OK or a tap: the next value, round; LEFT/RIGHT: stop at
  the ends); it counts from the next start. The board keeps it like the
  brightness (`Console::set_boot_anim` at start, saved when
  `take_settings_changed` says so; settings-store byte 6, blank or
  unknown is FULL). The numbers 0, 1, 2 are kept in step by hand with
  `settings_store::BOOT_ANIM_FULL`, `_SHORT`, `_OFF`.
- **Skip.** Any button, the BOOT button or a tap ends it in that step;
  the press does not act on home.
- **Name.** `theme::NAME` is the one name: the boot draws it and home
  writes it as its title. The font is console-video's (`glyph_for`).
  Names of 1 to 16 letters fit and finish in time (unit test).
- **Chime.** `gather::CHIME_MS` (1,880 ms of the animation's time) is
  the moment the gleam starts, for a sound later. No sound is played.
- **Colours.** The animation sets its own 256 colours
  (`gather::PALETTE`) on its first frame. At the switch to home the
  colours change through `Video::recolour_unseen`: none of the changed
  colours is on the panel's last picture, so the panel is not sent
  whole again. Measured on the PC (`tests/gather.rs -- --nocapture`):
  the switch sends 30,536 panel pixels in 10 areas instead of the whole
  76,800. A skip in the middle still sends the whole screen once.
- **Cost.** `Console` +8 bytes (45,192 to 45,200 on the PC; 45,168 to
  45,176 on the chip). The tables are built while compiling and live in
  flash. The CYD firmware with `pad` grows by about 16 KB of flash
  (two cores 15,992 bytes, one core 15,844), `.bss` by 16 or 8 bytes,
  and its code RAM not at all (measured from the ELF files,
  2026-11-18).
- **Tests.** `tests/gather.rs` (pure function, last frame is home,
  frame rate, skip at any moment with a button and a tap, SHORT, OFF,
  a slower board, the menu line, the stored value);
  `tests/golden.rs` with the feature checks that home and everything
  after it match the stored pictures exactly and stores the boot's own
  pictures in `tests/data/golden_gather.txt`.

## Rules

- No `std`, no heap, no `unsafe`, no floating point.
- It depends only on `board-api`, `console-video` and `game-api`. Never
  on a board or on esp-hal.
- It never waits and never reads a real clock. Time comes from `Clock`
  only, read once per step.
- Fixed-size lists, with their sizes as named settings.
- Features `bt-classic`, `passby`, `gps`, `sound` pass on to the board
  layer. The board program turns on the same ones.
- Feature `swap` (off by default) turns on console-video's `swap` and
  adds `Console::step_with_sender(board, sender)`: the same step, but
  the picture is handed to the board's sender (for example its second
  core) instead of sent at once. Before it sets the backlight, the
  console waits for the sender. `step` is unchanged and sends at once.
  The rule is in `crates/console-video/README.md`, "Swap: two cores,
  two pictures"; `tests/swap.rs` runs both ways side by side.
- Feature `boot-gather` (off by default): the GATHER boot animation
  and the BOOT ANIM setting (see above).

## Testing

```sh
cargo test                       # in this folder
cargo test -- --nocapture        # also prints the sizes and the bytes
UPDATE_GOLDEN=1 cargo test --test golden   # after a change in the look
../../boards/desktop/check.sh    # every new crate, plus chip builds
```

The tests use the fake parts of `board-api` and a fake clock, with no
window. `tests/shell.rs` drives the screens with short input scripts;
`tests/golden.rs` keeps the picture fingerprints of two scripted tours:
`tests/data/golden.txt` (letter tiles) and `tests/data/golden_art.txt`
(tile pictures: good ones, a cut one, none, more games than pictures
kept). `tests/art.rs` tests the tile pictures: every cut length, header
faults, a lying card, 3,000 random files, a picture drawn pixel for
pixel, the letter tile for every bad file, one try and one log line per
bad file, no card read while scrolling, the pictures kept on a long
row, an idle home that sleeps. `tests/attack.rs` holds the second reader's
cases: a card that keeps listing after "stop", 40 games, odd names,
equal names, many input wakes in a row, two inputs in one frame, a held
key, a very late call. `tests/lent.rs` tests home in the lent RAM:
every way back home (pause menu, game ends, game panics, refused file,
game not found, a board without games, card pulled, card changed, a
board that changes the RAM), each checked to show exactly the picture
and list of before and to send nothing while home is read again, a game
started right after boot, a press during the reload, a board that lends
nothing, and the cost of the way back (`-- --nocapture` prints it).
`tests/drag.rs` drags the home row (one to one, ends, rest, flick,
1, 3, 9 and 32 games, the selection rule, buttons during a drag and a
glide, taps on lift, the tap note, a missing frame, waking, a notice,
the pause menu, a finger held when a game ends); `tests/gesture.rs`
tests the recogniser alone. `tests/game.rs` runs games (made with
`game-kit`, and raw tables) through the fake board: start, missing
game, no games, damaged and old tables, rates, on-demand sleep,
requests, notices over a game, pause, resume, quit, done, panic.
`tests/brightness.rs` tests the brightness steps, the backlight calls
(once per change, never 0, the first after the first picture), the
menu on the home screen (pad, gear, CLOSE, BOOT, bars, sliding), the
menu over a game (the game is not called while it shows; BOOT alone
still opens and resumes), notices with the menu open, sleeping, and
that nothing is written to the store. `tests/attack_step4.rs` holds the
second reader's cases for the drag and the brightness menu: random
gestures on 0, 1, 2, 7, 9 and 32 games (more runs with `FUZZ_RUNS`),
tap notes, a tap with a button, a game started or the card pulled
mid-drag, the menu opened mid-drag, the ends of the steps, all the ways
to close the home menu, the game clock in the menu, sleeping with a
finger in the menu, and the owner decisions of 2026-11-17: a lost read
during a press or under the finger that closed the menu (D1), the tap
one frame after a real lift, a finger stopping a glide at any frame
(D4), a long press on a tile, the gear, a bar and CLOSE (D5). The
random gestures check those rules too. For a long hunt:
`CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true FUZZ_RUNS=500 cargo test
--release --test attack_step4 random`.

Pictures in the simulator (from `boards/desktop`):

```sh
cargo run -- --script ../../crates/console-core/scripts/tour.txt \
  --frames 130 --shot 15,55,63,93,110 --out shots
```
