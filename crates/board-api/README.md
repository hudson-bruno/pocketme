# board-api

The board layer. It lists what the console may ask of the hardware.
Each kind of part has its own small trait. One `Board` trait gathers them.

| Trait | What it is | On the CYD | On the PC |
|---|---|---|---|
| `Screen` | the panel | ST7789 over SPI with DMA | a 320x240 picture in memory |
| `Touch` | the touch screen | resistive touch (chip not checked yet) | the mouse, or a script |
| `Keys` | the board's own buttons | BOOT | a key |
| `Pad` | gamepads, and pairing them | Bluetooth | the keyboard |
| `Clock` | time in microseconds | the chip's timer | moved by the loop or the test |
| `Files` | the SD card | FAT card on SPI3 | a folder |
| `Link` | the debug cable | UART0 | a TCP address (`--link`) |
| `LogOut` | text lines for a person | serial monitor | the terminal |
| `Store` | saved settings | a flash area | memory, or a file (`--store FILE`) |
| `Hci` | raw Bluetooth packets | feature `bt-classic` | |
| `Beacon` | pass-by messages | feature `passby` | |
| `Gps` | raw GPS bytes | feature `gps` | |
| `Audio` | sound out | feature `sound` | |

A feature that is off removes its trait, its item in `Board` and `Parts`,
and all code that uses it. A board that lacks a part uses `Absent`.
`Absent` takes no memory and does nothing.

## Rules

- No `std`, no heap, no `unsafe`. The same file builds for the ESP32
  and for the PC. Its one dependency is `game-api` (the game contract),
  which itself has none: starting a game hands its table across this
  line.
- It must never depend on a board crate, on esp-hal, or on the console.
- Only traits and small shared types live here. No logic.
- A new method on an existing trait must come with a default body.
  Then every board keeps compiling.
- The board crate turns on the same optional features as the console.
  If it forgets one, the build fails with a clear error. It does not
  fail quietly.

## Three choices, and why

### (a) The screen line

The console keeps a small picture: 160x120, one byte a pixel, 19 KB.
The panel needs 320x240 pixels of 16-bit colour. That full picture
would take 150 KB. The chip cannot spare that.

So the console never builds the full picture. It calls
`present(source, areas)`:

- `areas` are the changed rectangles, in panel pixels. The console found
  them. The board may widen, join or split them as its wire likes. It
  must send at least these pixels.
- `source` is a `PixelSource`. The board asks it for finished pixels, a
  short run at a time: "240 pixels, starting here, going down". It can
  go right, left, down or up. The board picks the order its wire needs.

Why this suits the CYD. Its driver sends small strips by DMA (the SPI
hardware reads RAM by itself). It fills one 15 KB strip while the other
is on the wire. For tear-free pictures it sends whole panel columns, in
the order the panel refreshes, with tight timing. With runs, the driver
asks for one column straight into its strip buffer. Nothing is copied
twice. The byte order can be "wire order" (high byte first), so the
buffer goes out as it is. The source also offers `indexed()`: the small
picture and its palette. The CYD can then keep its fill loops that were
measured on the board, including 12-bit colour. Using it is optional.

Why this suits the PC. It asks for whole rows, and writes them straight
into its picture.

Cost: one call per run, not per pixel. The board must not keep the
source after `present` returns. The borrow checker makes sure of that.

### (b) Borrowing several parts at once

The console often needs two parts at the same time. For example, it
reads touch while it holds the screen. With one `&mut board`, Rust
would refuse that.

So `Board::parts()` lends every part at once, each as its own `&mut`,
in a `Parts` struct. A board keeps each part in its own field, and
writes `Parts { screen: &mut self.screen, touch: &mut self.touch, .. }`.
Rust accepts this, because the fields are separate. There is no
`RefCell`, no `dyn` and no heap. The console is generic over `B: Board`,
so the compiler makes one copy for the board it is built for. Calls go
straight to the board's code, with no lookup at run time.

### (c) The per-frame contract

The console never waits by itself. It does one frame of work in
`Console::step(&mut board)`, then returns a `Wake`. The board runs the
loop:

1. Before a step: bring the clock up to date. Collect input, so that
   `Touch`, `Keys` and `Pad` say what is true now.
2. Call `step`. The console reads the clock once, reads the input,
   draws, and calls `present` at most once.
3. After the step: sleep as `Wake` says.
   - `Wake::At(t)`: call again at clock time `t`. This is a fixed frame
     rate: 30 frames a second asks for "now + 33,333 us".
   - `Wake::InputOr(t)`: call again when input changes, or at `t` at
     the latest. This lets the chip sleep while nothing moves, to save
     battery. `t = u64::MAX` means "only on input".

Calling early or late is always safe. The console looks at the clock
and does only what is due. An early call does not read input, so no
button press is lost.

Time is a `Clock` that gives microseconds as a `u64`. A u32 would run
out after 71 minutes. On the CYD it is the chip's timer. On the PC with
no window, the loop moves the clock straight to the time the console
asked for. In tests, the test moves it by hand. So a minute of play
takes a few milliseconds, and gives the same result every run.

## Games and motion (added with the game interface)

Both are default methods, so a board that knows nothing of them
compiles unchanged.

- `Board::start_game(folder) -> Result<GameCallbacks, StartError>`: the
  console asks the board to start the game in `/CONSOLE/GAMES/<folder>`
  and gets back the game's two functions. On the chip that is the
  loader (read `GAME.BIN` into the game slots, call its entry). On the
  PC the games are linked in, and the board looks the folder up in a
  list of `game_api::LinkedGame` and calls its `open`. The chip calls
  `unsafe { game_api::open_game(entry, &console_api) }`. Either way
  that one function reads the game's table. Default: `Err(StartError::NotSupported)`; the console then
  shows "GAMES CANNOT START YET".
- `Board::end_game()`: the game has ended (done, error, or the player
  left). The board may free the game's memory. Default: nothing.
- `Pad::motion(index) -> Option<Motion>`: the pad's accelerometer and
  gyroscope, now. `None` when the pad sends no motion (some pads send it
  only in one of their modes). Units and axes are the console's, the
  same a game sees: 4096 per g, 16 per degree per second; x to the
  player's right, y away from the player, z up out of the buttons. The
  board turns its pad's own numbers into these. Default: `None`.

## Short presses (added in step 3)

A press can be shorter than a frame: the button goes down and up again
between two steps. Reading only "is it down now" would miss it. So a
board may also keep a **note**: "this was pressed since you last
asked". The console takes the note once per step, together with the
levels, and the note is then cleared.

- `Keys::take_pressed() -> u16`: the keys pressed since the last call.
- `Pad::take_pressed(index) -> u32`: the same for the pad's buttons
  (0 for a pad that is not there).
- `Touch::take_tap() -> Option<TouchPoint>`: where the finger first
  touched since the last call, even if it is up again now.

All three have defaults that note nothing (0 or `None`), so an old
board compiles and works as before. Release needs no note: a button
that was noted but is up now was pressed and released.

Rule for a board: collect the note and the levels at the same moment
(before the step), so they describe the same moment.

The fake board: `FakeKeys.pressed`, `FakePad.pressed` and
`FakeTouch.tap`, each handed out once.

## Four pads (added for four players)

The console reads pads 0 to 3 in every step (`read`, `take_pressed`
and, for a connected pad, `motion`), one per player of the game
interface. Pad 0 also drives the console's own menus; pads 1 to 3 reach
only games. A board with one pad answers `PadState::NONE` for 1 to 3,
as before. The CYD has one Bluetooth pad (pad 0); the debug link can
also fill pads 1 to 3 as cable pads (`boardctl key --player N`). The
simulator fills pad 0 and 1 from the keyboard (the number pad is pad 1)
and all four from scripts.

The fake board: `FakePad` keeps pad 0 in its own fields (`state`,
`motion`, `pressed`) and pads 1 to 3 in `others` (`OnePad`, or
`FakePad::other(index)`).

`StartError`: `NotSupported`, `NotFound`, `BadFile` (damaged, or built
for other slots), `BadTable(OpenError)` (the game's table was refused:
`NoTable` when the entry gave null, `TooSmall`, `TooOld`), and
`Refused(&'static str)`: the board's loader refused the file and says
why in a short notice (capitals, at most 24 letters, such as GAME FOR
OTHER CODE SLOT); the console shows that text. `FakeGames.refuse`
makes the fake board answer with it.

The fake board: `FakePad.motion` (only reported while the pad is
connected), and `FakeBoard.games` (`FakeGames { list, started, ended,
refuse, lent }`; an empty list means "cannot start games").

## Lent RAM (added so home can live in the empty game data slot)

`Board::parts_and_lent() -> (Parts, Option<&mut [u8]>)`: the parts, as
`parts()`, and a piece of RAM the board lends the console while no
game is loaded. On the chip it is the game data slot (48 KB, from
`game_api::layout`): while the player is on the home screen no game is
in it, so the home screen keeps its game list and tile pictures there
(5,396 bytes today) instead of in the console's own RAM. Default: the
parts and `None` (a board that lends nothing still compiles and runs;
its home screen then shows no games).

The rules (full text in `src/lent.rs`):

1. Lent only while no game is loaded: before the first `start_game`,
   after `end_game`, and after a `start_game` that failed.
2. Every `start_game` call ends the loan, whatever it returns: the
   bytes may be overwritten (the loader writes the game there, and
   wipes it after a failed load). The console trusts nothing it wrote
   there before; it reads the card again.
3. Between two loans with no `start_game` in between, the same bytes
   come back unchanged.
4. The first loan holds anything (zeros on the chip, rubbish on purpose
   in the simulator and the fake board).
5. Always the same length.

Why plain bytes and not a typed value: the console has no `unsafe`, and
any byte is a valid `u8`, so rubbish there can at worst show wrong
letters, never crash anything. (The console also checks a stamp of its
own before it reads.) Only the board needs `unsafe` to hand out the
slot; on the CYD that is `GameSlots::lend` in `boards/cyd/src/games.rs`.

The fake board: `FakeGames.lent` (`FakeLent`, 48 KB). It starts as
rubbish and is filled with other rubbish at every `start_game` and
`end_game` (`scrambles` counts them), so a console that reads stale
data shows wrong names in a test instead of the right ones by luck.
`lent.loaded` is true while a game runs (nothing is lent then); a test
may set it to act like a board that lends nothing.

## Pairing a pad (added for the settings window)

The console may pair a new Bluetooth pad and forget a saved one. Its
step may not touch the radio, the BOOT pin or the flash, and may not
wait, so the `Pad` methods for this are **requests, not answers**: the
console leaves a request, the board serves it after the step, and the
outcome comes back through `pair_state` in a later step. The types are
in `src/pair.rs`.

| Method | What it says or does | Default |
|---|---|---|
| `pairing(&self) -> bool` | this board can pair from the console | `false` |
| `pair_slots(&self) -> Option<u8>` | free records left in the pairing area | `None` |
| `saved_pad(&self, place) -> Option<SavedPad>` | the saved pad at a place (`None` when empty or past the last) | `None` |
| `pair_request(&mut self, PairReq)` | leaves one request | dropped |
| `pair_state(&mut self) -> PairState` | how far it is now | `Off` |
| `pair_found(&self, i) -> Option<FoundPad>` | pad `i` the search found | `None` |

- `PairReq`: `Begin` (start search; triggered directly from UI on CYD, or
  after `PAIR_HOLD_MS` on boards requiring a physical hold),
  `Choose { i, tail }` (call found pad `i` after YES), `Cancel` (the
  page was left: stop all, back to `Off`), `Forget { place, tail }`
  (after confirming in UI, plus BOOT hold only on boards that check
  it), `Ack` (an ending was shown). One box: a newer request replaces
  one not served yet, but a waiting `Cancel` beats every other.
- `PairState`: `Off`, `Starting`, `Searching`, `Calling`, and the
  endings `Saved(place)`, `Forgotten(place)`, `Failed(PairFail)`. An
  ending stays until `Ack` or the next `Begin`. `Off` is the zero
  value (`repr(u8)`).
- `PairFail`: `NotHeld`, `Linked`, `Full`, `NoRoom`, `Damaged`,
  `Timeout`, `OldPin`, `Refused`, `Lost`, `Store`, `Busy`; each doc
  comment says what the page tells the player.
- A pad is named only by its **tail**, the last two bytes of its
  address in written order (`..:AB:CD` is `[0xAB, 0xCD]`). `Choose` and
  `Forget` carry the tail the player saw; the board refuses one that
  does not match (`Failed(Busy)`), so a list that changed under the
  press never hits another pad. The found list only grows during one
  search and is emptied at the next `Begin`.
- `SavedPad { tail, connected }`; `FoundPad { tail, name, name_len }`
  (at most `PAD_NAME_MAX`, 24, bytes of the name the pad sent;
  `FoundPad::new`, `FoundPad::name`). `FoundPad`'s `Debug` shows the
  name's length, never the name.
- How many pads a board keeps is not written here: `saved_pad` gives
  `None` past the board's last place.

The fake board: `FakePad.radio` (`FakeRadio`) plays a script the test
chooses (`PairScript`), with no clock of its own: every `pair_state`
call is one turn (or only `FakeRadio::turn`, with `manual`). A turn
serves the waiting request, or else moves the script on by one.
`FakeRadio::default()` cannot pair (the default bodies' answers), so
older tests see the same console. `FakeRadio::new(script)` can;
`FakeRadio::full(script)` has four saved pads; `with_saved`,
`with_slots` set the rest. Ready scripts: `found_and_saved(n, m)`,
`two_found(a, b)`, `nothing_found(turns)`, `old_pin`, `link_lost`,
`store_fails`, `not_held()`; or build one with `PairScript::new()` and
`find`, `begin_fails`, `starting`, `searching`, `calling`,
`call_fails`, `forget_fails`. It hangs up a linked pad at `Begin`,
refuses `Full` at `Choose` (a saved pad may be paired again), refuses
`NoRoom` at `Begin` when `slots` is `Some(0)`, and refuses `Choose` and
`Forget` with a wrong tail. Every tail and name in it is made up
(`FAKE_TAIL_A`, `FAKE_NAME_A`, `FAKE_SAVED_TAILS`, ...). Its tests are
`tests/fake_pairing.rs`.

## The fake board for tests

Feature `testing` adds `fake::FakeBoard`. Every part is a struct with
public fields. A test sets the input, moves `clock.now_us`, calls `step`,
then looks at `screen.image`, `log.last()` or `link.sent()`. The fake
screen can walk the panel in rows, columns, either way, or through
`indexed()`. All must give the same picture, so it checks a source is
right in every direction. It holds a 150 KB picture: PC only.

Use it from another crate:

```toml
[dev-dependencies]
board-api = { path = "../board-api", features = ["testing"] }
```

## Testing

```sh
cargo test --features testing     # in this folder
../../boards/desktop/check.sh     # every new crate, plus chip builds
```

The folder pins Rust 1.99.0 (rust-toolchain.toml). It does not pin the
esp toolchain: the chip build names it on the command line.

## Open points

- `Store` has its own shape: offsets count from the start of the store.
  `crates/pad-store`'s `Flash` counts from the start of the chip. A
  15-line adapter joins them. This keeps this crate free of other
  crates.
- Where the Bluetooth gamepad host runs (in the console over `Hci`, or
  as a board task that fills `Pad`) is for the Bluetooth round.
- Which pad buttons a game sees (owner decision D4) is not decided.
  The board layer carries all 18, with the pad decoder's numbering.
