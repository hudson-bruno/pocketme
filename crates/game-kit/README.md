# game-kit

The game side of the console. A game is a `no_std` library crate that
uses only this crate. It gets the console's drawing and font (from
`console-video`), and a safe face over the contract (`game-api`): no
`unsafe` in a game.

```rust
#![no_std]
use game_kit::{Ctx, Flow, Game, button};

struct Hello { x: i16 }

impl Game for Hello {
    fn start(_ctx: &mut Ctx) -> Self { Hello { x: 0 } }
    fn update(&mut self, ctx: &mut Ctx) -> Flow {
        if ctx.held(button::RIGHT) { self.x += 1; }
        let s = ctx.screen();
        s.clear(0);
        s.text(self.x, 50, "HELLO", 239);
        if ctx.pressed(button::START) { Flow::Done } else { Flow::Run }
    }
}

game_kit::game!(Hello);
```

## What a game gets

- `Game`: `start(ctx) -> Self` (first call of a run), `update(&mut self,
  ctx) -> Flow` (`Run` or `Done`), and `exit(&mut self, ctx, Exit)`
  (the player left through the pause menu; optional).
- `Ctx`, once per call:
  - Picture: `screen() -> &mut Frame` with all of console-video's
    drawing (`clear`, `pixel`, `rect`, `outline`, `sprite`, `blit`,
    `text`, `text_scaled`...). It keeps what was drawn last time.
  - Palette: `set_colour(i, rgb565)`, `set_colours(first, &[..])`,
    `colour(i)`, `game_colours()` (240). Numbers 240 and up are the
    console's: setting them returns false.
  - Input, four players: `player(n)` gives player `n`'s pad (`n` from
    0 to `PLAYERS - 1`; 0 is "player 1" on screen) as a `Player`:
    `held`, `pressed`, `released` (any of the given `button` bits),
    `held_bits`, `pressed_bits`, `released_bits` (all of them),
    `connected`, `left_stick`, `right_stick`, `triggers`, `motion()`
    (`Some(Motion { accel, gyro })` only while the pad sends it) and
    `raw()`. A player with no pad (or a number past the last) reads as
    nothing held. `has_pad(n)` and `pad_count()` say who has a pad.
  - One-player shortcuts, all player 1's: `held`, `pressed`,
    `released`, `left_stick`, `right_stick`, `triggers`, `motion()`,
    `pad_connected`. So a one-player game reads `ctx.pressed(button::A)`
    or `ctx.player(0).held_bits()`.
  - Touch (shared, not a player's): `touch()`
    (`Touch { x, y, down, started, ended }`). `input()` gives the raw
    table.
  - Time: `time_us` (pauses not counted), `delta_us`, `frame`,
    `started`, `resumed`, `must_redraw`.
  - Video: `video()` (in use), `offers()`, `ask_video(VideoSettings)`
    and `set_rate(rate)`. Both check the offer here and return
    `Err(Refused::Mode | Rate | Hint | Colour)` without asking. The
    answer comes at the next call (`last_answer`, `last_request_ok`).
    `wake_at(t)` / `wake_after(us)`: the timer for the on-demand rate.
  - Log: `log("text")`; `log_line().text("lap ").num(n).end()` for
    texts and whole numbers (every integer type up to 64 bits) without
    `core::fmt`, the same bytes as `{}` writes; and
    `log_fmt(format_args!(..))` for anything else (it brings in the
    formatting code: about 0.4 to 2 KB of chip code; every game uses
    `log` and `log_line` instead). One line each; cut when the call's
    buffer (256 bytes) is full, in the middle of a number too.
- Re-exports: `Frame`, `Sprite`, `rgb565`, `text_width`, `W`, `H`,
  `FONT_H`, `LINE_H`, `CLEAR`, `PLAYERS`, `button`, `rate`, `mode`, `hint`,
  `colour`, `answer`, `motion_units`, `STICK_MAX`, `TRIGGER_MAX`, and
  `api` (the whole contract).

## What `game!(Type)` makes

- `pub unsafe extern "C" fn game_entry(*const ConsoleApi) -> *const
  GameCallbacks` (`unsafe`: only the console calls it, with its real
  table). It returns null when the console's table is missing,
  older than version 3 (version 2 was the one-player contract), or
  too small.
  - Chip: `no_mangle`, so the game file can point at it (the link
    step needs `ENTRY(game_entry)` or `EXTERN(game_entry)`).
  - PC: a plain function. The simulator lists
    `unsafe { LinkedGame::new("NAME", the_game::game_entry) }`.
    Several games link side by side: nothing clashes.
- The game's table and its `update` and `exit`. The game's state is
  made at the first call (`event::START`) and dropped after `Done`, a
  panic, or `exit`. It lives in a `static` on the chip (one game at a
  time) and in a thread-local on the PC (tests run games side by side).
- Chip only: the `#[panic_handler]`. It calls the console's `abort`
  with the panic's line number. So a game crate must not have its own.
- PC: a panic inside `start`, `update` or `exit` is caught. The kit
  writes `panic: <message>` to the log and returns `ABORTED`. The
  console goes home with a warning; the program goes on.

The kit reads the console's tables by their `size`: a newer console's
bigger frame works (its new fields are left alone), and fields an older
console's input lacks read as 0 (a player the input does not reach
reads as no pad). A frame
smaller than version 2 aborts the call without calling the game.

## Rules

- `no_std`, no heap, no floating point in the kit. The kit's own
  `unsafe` is in `ctx.rs`, `rt.rs`, the `game!` macro's entry points
  in `lib.rs` and the PC test console (`testing.rs`), each block with
  its reason. Games need none.
- On the PC the kit uses `std` (catching panics, the thread-local). On
  the chip (`target_os = "none"`) it uses only `core`.

## Testing

```sh
cargo test                        # in this folder
../../boards/desktop/check.sh     # every crate, plus chip builds
```

`tests/kit.rs` drives games made with `game!` through their raw tables,
as a console would: drawing, palette, log, input, four players each
reading only its own pad, motion on and off,
done, exit, panic, video requests, and older and newer console tables.
