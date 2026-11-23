# game-api

The contract between the console and a game, version 3. Both sides are
built against this crate, and they talk only through its tables. No
dependencies: plain `core`, so the console (chip and PC), every game,
and the PC tool that packs game files agree byte for byte.

Games do not use this crate by hand. They use `crates/game-kit`, which
wraps it in a safe face. Boards use `open_game`. The console uses the
tables.

Version 2 started from the loader experiment
(`experiments/native-loader/game-api`, version 1). The file header and
the memory slots (`file.rs`) are copied from it unchanged, except that a
file must now say version 2 or later. Version 3 (2026-11-18) changed
`Input` to carry four players, so it also needs version 3 or later now.

## The tables

| Table | Who owns it | When | What is in it |
|---|---|---|---|
| `ConsoleApi` | console | given once, at the entry call | `abort(code, detail)` |
| `GameCallbacks` | game (a `static`) | returned by the entry function | `update(frame)`, `exit(frame, reason)` |
| `GameFrame` | console (on its stack) | one per call | pointers to the picture, the palette, the input, a log buffer; time, frame number, events; what is offered and in use; the game's wishes |
| `Input` | console (on its stack) | one per call | touch, then 4 × `PlayerInput`: 12 buttons, 2 sticks, 2 triggers, motion per player |

`GameEntry` is `unsafe extern "C" fn(*const ConsoleApi) -> *const
GameCallbacks`. On the chip the file header says where it is. On the PC
the simulator links the game crate and lists `the_game::game_entry`.
It is `unsafe` because whoever calls it trusts the pointer it returns:
safe code cannot call a game's entry, or `open_game`, with a made-up
pointer. Making a `LinkedGame` (`unsafe { LinkedGame::new(..) }`) is
the one promise; `LinkedGame::open` is then safe.

**Calling a game safely (step 3).** `update` and `exit` take a raw
pointer to a `GameFrame`, and the game writes wherever that frame
points. So in `GameCallbacks` they sit in two wrappers, `UpdateFn` and
`ExitFn`. Their only call, `call(&mut LiveFrame)`, takes a `LiveFrame`:
a `GameFrame` built from real buffers (the 19,200-byte picture, the 256
colours, the input, the log buffer), which it borrows during the call.
`LiveFrame::new` sets the pointers and the sizes itself; the caller
gives only the other numbers. So the console calls a game with no
`unsafe`, and safe code cannot hand a game a made-up frame. Making a
wrapper is `unsafe` (`UpdateFn::new`): the kit does it for each game,
and `open_game` for a table read from a game. `as_fn()` gives the bare
function back for tests (calling it needs `unsafe`).

The wrappers are `repr(transparent)`: in memory each is just the
function pointer, so the table's bytes did not change. The chip game
files built before and after this change are the same, byte for byte.

Sizes: `ConsoleApi` 12 bytes on the chip (16 on the PC),
`GameCallbacks` 16 (24), `GameFrame` 104 (120), `Input` 176 (176),
`PlayerInput` 40 (40). The layout is checked while compiling, for both
pointer sizes.

### `Input`, version 3

| Offset | Field | Type | What |
|---|---|---|---|
| 0 | `size` | u32 | 176 |
| 4 | `version` | u32 | 3 |
| 8 | `flags` | u32 | `input_flag`: `TOUCH`, `TOUCH_START`, `TOUCH_END` |
| 12 | `touch_x`, `touch_y` | i16 × 2 | where the screen is touched |
| 16 + 40 n | `players[n]` | `PlayerInput` | player n, n = 0..4 (`PLAYERS`) |

`PlayerInput` (40 bytes):

| Offset | Field | Type | What |
|---|---|---|---|
| 0 | `held` | u32 | `button` bits held now |
| 4 | `pressed` | u32 | went down since the last call |
| 8 | `released` | u32 | came up since the last call (also when the pad went away) |
| 12 | `flags` | u32 | `player_flag`: `PAD` (connected), `MOTION` |
| 16 | `left_x`, `left_y`, `right_x`, `right_y` | i16 × 4 | sticks |
| 24 | `left_trigger`, `right_trigger` | u16 × 2 | triggers |
| 28 | `accel` | i16 × 3 | accelerometer |
| 34 | `gyro` | i16 × 3 | gyroscope |

Player 0 ("player 1" on screen) is the pad that also drives the menus.
A player with no pad reads all zeros. The two flag sets use different
bits (`player_flag` bits 0 and 1, `input_flag` bits 2 to 4), so reading
one with the other's names gives "no", never a wrong "yes".

## Why it looks like this

**Only plain C types cross.** `repr(C)` structs, `u8`..`u64`, `i16`,
raw pointers, `extern "C"` functions. No `usize`, no `bool`, no `&str`,
no slices, no `Option`, no enums. Rust does not promise a layout for
those, and a game file may be built by another compiler version than
the console. A `u16` is a `u16` everywhere.

**Every table starts with `size` and `version`, and only grows at the
end.** A reader takes what it knows and treats anything past `size` as
missing. So an older game runs on a newer console, and a newer game on
an older console, as long as the old fields keep their place. The
console checks the game's `GameCallbacks` this way (`open_game`): a
table that ends after `update` gets an `exit` that does nothing. The
game kit checks the console's `GameFrame` and `Input` the same way:
fields the console did not give read as 0.

**Zero means "nothing" or "the default".** In every field a game
writes. A game that does not know a field leaves it at 0 and gets the
default.

**Versions 1 and 2 are refused.** Version 1 was the experiment's test
contract, and no game was written against it. Version 2 was the
one-player contract; version 3 put the four players in the middle of
`Input`, which breaks "only grow at the end" once, so every game was
rebuilt and `MIN_GAME_VERSION` is 3. From version 3 on, "only grow at
the end" is the rule again.

**The console calls the game, never the other way round** (except
`abort`). The game does one frame's work in `update` and returns. It
never loops for ever. So the console keeps the radio, the timing, the
notifications and the pause menu alive, and can stop a game at any
frame.

**Everything the game wants goes back as values in `GameFrame`.** Not as
calls into the console. The console reads them after `update` returns.
So the console holds no pointer from the game, and needs no `unsafe`
code at all. The only place that reads memory the game made is
`open_game`, written once, used by every board.

**The picture is drawn straight into the console's buffer.** `pixels`
points at the 160 x 120 picture (one byte per pixel, a palette number;
`stride` bytes per row; aligned to 4). No copy, no extra 19 KB. The
console sends only what changed, as it does for its own screens.

**Pointers are good for one call only.** The frame lives on the
console's stack during the call. A game must never keep a pointer from
it.

**The palette is split.** Numbers 0 to 239 are the game's
(`game_colours`). They start as a 6 x 8 x 5 colour cube
(`picture::start_colour`: 0 is black, 239 is white), so a game can
draw at once without setting any. Numbers 240 to 255 are the console's:
its notifications and pause menu are drawn with them, over the game's
picture. A game may draw with them, but what it writes there is put
back after each call. After changing colours the game sets
`request::PALETTE`.

**Video settings are asked for, not taken.** The game writes `ask_mode`,
`ask_rate`, `ask_hint`, `ask_colour` and sets `request::VIDEO`. The
console checks each against what it offers (`offered_*`, one bit per
value) and answers in `answer` at the next call: `OK`, or which one is
not offered (`NO_MODE`, `NO_RATE`, `NO_HINT`, `NO_COLOUR`). Any value it
does not know, even a huge one, gets a clean "not offered" and nothing
changes. Version 2 offers mode 0 (160 x 120), rates 30, 60 and
on-demand, hints 0..2, colour 0..1. Hints and colour settings are kept
but change nothing yet.

**On-demand rate.** The game is called only when input changes (a
button, a stick, a trigger, the touch; motion alone does not count,
because a pad in a hand never holds still), when its timer comes
(`wake_at_us`, in game time; set it again in every call), and after a
start or resume. In between the console sends nothing and the chip can
sleep. Good for card games, puzzles, menus. A button, the pad coming or
going, and a finger going down or up call the game at once. A stick, a
trigger or a moving finger call it at most 60 times a second: a pad
sends a report every few milliseconds, and a stick that shakes a
little would otherwise call the game hundreds of times a second. (A
shaking stick still calls it up to 60 times a second; the console has
no dead zone yet.)

**Four players.** Each `Input` carries four `PlayerInput` blocks,
`players[0]` to `players[3]`, one per board pad (index 0 to 3); on
screen they are players 1 to 4. Each has its own held, pressed and
released bits: a press on board pad 2 reaches `players[2]` only. A pad
that goes away lets go of its buttons (they arrive once as `released`).
Only `players[0]`'s pad, the board's own keys and the touch screen drive
the console's menus and the pause menu; `players[1..4]` never do. On the
CYD only `players[0]` has a Bluetooth pad today (the debug link can
press buttons on the other three); in the simulator the number pad
drives `players[1]` and scripts can drive all four.

**Input does not depend on the pad.** 12 buttons as bits (the same
bits as the debug link). A and B are named by their job: A confirms, B
goes back, the same buttons the console's own menus use. Which physical
button that is, is one table in the console. Sticks run from -32767 to
+32767, y down as on the screen. Triggers 0 to 32767 (a trigger that is
only a button reads 0 or 32767). Touch in the game's own pixels, with
flags for "down", "started" and "ended". Motion in fixed units (4096
per g, 16 per degree per second) and fixed axes (x right, y away from
the player, z up), with a flag that says whether the pad sends motion
now: some pads send it only in one of their modes. Home never reaches a
game: the console keeps it for the pause menu.

**Buttons held at a start or resume are hidden until let go.** The
button that started the game, or closed the pause menu, does not
arrive as a press in the game's first frame.

**Time.** `time_us` counts microseconds the game has run, pauses not
counted (64 bits). `delta_us` is the time since the last call. `frame`
counts calls.

**Ending.** `update` returns `status::RUN`, `DONE` (the game is over,
back home with a notice) or `ABORTED`. Any other value counts as
`ABORTED`. `exit` is called when the console ends the game (the player
chose "Back to home"), never after `DONE` or `ABORTED`.

**Abort and panics.** On the chip there is no unwinding. The kit's
panic handler calls `ConsoleApi::abort(abort_code::PANIC, line)`, which
never returns; the board decides what happens (today on the CYD: it
prints why on the serial port and stops until a reset; it does not
restart, as no watchdog runs). On the PC the kit catches a panic inside `update`,
writes it to the log, and returns `ABORTED`: the console goes home with
a warning and the simulator goes on. `abort` is `extern "C-unwind"` so
that on the PC it may panic and be caught the same way.
`UNWIND_CONSOLE_API` is that PC version. (A game written by hand that
calls `abort` on the PC outside the kit ends the program: the panic
cannot cross its `extern "C"` `update`.)

**Log.** A buffer of `log_cap` bytes per call. The game writes lines
ending in `\n` and puts the count in `log_len`. The console prints them
after the call, starting with `game: `. ASCII only; anything else shows
as `?`.

## What is in the crate

- `lib.rs`: the tables, `PLAYERS`, the constants (`button`,
  `player_flag`, `input_flag`,
  `motion`, `status`, `event`, `request`, `mode`, `rate`, `hint`,
  `colour`, `answer`, `exit_reason`, `abort_code`, `picture`, `card`),
  `field_end!` (where a field ends; usable in `const`), and the layout
  checks.
- `call.rs`: `UpdateFn`, `ExitFn` (`new` is `unsafe`, `call`, `as_fn`,
  `ExitFn::NONE`), `LiveFrame` (`new`, `get`), `PICTURE_BYTES`.
- `open.rs`: `unsafe fn open_game(entry, api)`, `LinkedGame` (`new`,
  `open`),
  `UNWIND_CONSOLE_API`, `no_exit`, `abort_by_panic`.
- `file.rs`: the game file header (`GameHeader`, `MAGIC`, `Crc32`) and
  the memory slots on the chip (`layout`), from the experiment, plus the
  spill (below), and the game flash parts (2026-11-22): `MAGIC_FLASH`
  ("GAMF"), `FlashPartHeader` (the 32-byte extension after the header:
  code address, size and CRC-32, data address, size and CRC-32),
  `read_game_header` (accepts GAME and GAMF; `GameHeader::validate` alone
  still refuses GAMF), and in `layout` the two flash windows
  (`FLASH_CODE_ADDR` 0x400D0020 + 128 KB, `FLASH_RO_ADDR` 0x3F400120 +
  64 KB, esp-hal 1.2.2 facts copied by hand) and each game's place in
  them (`FLASH_PLACES`, `flash_place`, `write_flash_place_symbols`,
  `write_flash_window_symbols`; `examples/slots_x.rs --flash NAME`).

## Code past the code slot (the spill, stage 1)

A game's code may be bigger than the 56 KB code slot. The rest runs on
past the slot's end (0x400A_0000, `layout::CODE_SPILL_ADDR`), which on
the ESP32 is the same RAM as the **top** of the 48 KB data slot, seen
from the instruction bus. Code eats the data slot from the top down;
the game's constants and variables fill it from the bottom up. The one
rule (`layout::fits`):

> code past the slot + constants + starting values + zeroed ≤ 48 KB,
> and code ≤ 56 KB + 48 KB (`layout::CODE_MAX`, 106,496 bytes)

`GameHeader::validate` refuses code over `CODE_MAX` (`BadCodeSize`),
data too big on its own (`BadDataSize`), and code and data that would
meet (`CodeMeetsData`, new). The file header, `API_VERSION` and
`MIN_GAME_VERSION` did not change: a file whose `code_size` is over
57,344 simply has a spill. An older console refuses it cleanly before
writing anything (GAME CODE TOO BIG), and every game that fits in the
slot is built byte for byte as before.

The mirror: instruction address `I` in 0x400A_0000..0x400C_0000 is
data address `0x4000_0000 - 4 - (I - 0x400A_0000)`
(`layout::sram1_iram_to_dram`): reversed word by word, bytes inside a
word in order. This comes from ESP-IDF's sources as quoted by an
earlier study, and was seen to hold on the CYD (2026-11-22). The
firmware checks it at every start-up, reading the instruction side
only, and refuses spill games when it does not hold.

`write_linker_symbols` writes the spill as two more lines,
`GAME_SPILL_ORIGIN` and `GAME_SPILL_LENGTH`, and the data words that
the spill's first and last word are, `GAME_SPILL_FIRST_DATA_WORD` and
`GAME_SPILL_LAST_DATA_WORD` (from `sram1_iram_to_dram`). The rule is
kept in step by hand in two more places, which read their numbers from
the generated slots.x: the `ASSERT` in games/chip/game.x and
games/chip/check_load.py (which stops when its copy of the mirror does
not give those two words). The loader's test fake has a third copy;
its tests compare it with `sram1_iram_to_dram` word by word.

## Testing

```sh
cargo test                        # in this folder
../../boards/desktop/check.sh     # every crate, plus chip builds
```

`tests/open.rs` opens current, older, newer, null, version 1, version 2
and too small tables.
