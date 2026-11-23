# console-video

The shared renderer: the console's default video mode. The console (and
later each game) draws into a small picture. This crate finds what
changed and hands it to the board, which sends it to the panel.

## What is in it

- `Frame`: the picture. 160x120 pixels, one byte each (19,200 bytes).
  Each byte is a palette number.
- `Palette`: 256 colours in RGB565 (512 bytes), plus a version number
  that changes on every change. `rgb565(r, g, b)` makes a colour.
- Drawing on `Frame` (positions are `i16`; parts outside are cut off):
  `clear`, `pixel`, `hline`, `vline`, `rect` (filled), `outline`,
  `sprite` (one see-through palette number), `blit` (no see-through),
  `text` and `text_scaled`. `Sprite::new(w, h, &pixels)` only borrows
  its pixels, so sprites can stay in flash.
- Text: the design page's 5-pixel capital font. Letters are 1 to 5
  pixels wide, one empty column after each. `FONT_H` is 5, `LINE_H`
  (one line to the next) is 6, `text_width` measures a string. Small
  letters are drawn as capitals, signs the font lacks as `?`. For
  drawing the font another way (console-core's boot animation scales
  it), `glyph_for(byte)` gives a letter's bits, `glyph_w` its width and
  `glyph_ink(g, i, j)` whether pixel `(i, j)` is ink; all three are `const fn`.
- `Areas` and `diff`: the changed rectangles, at most `MAX_AREAS`.
- `View`: where the picture sits on the panel (2x on 320x240). It also
  turns a touch in panel pixels into picture pixels.
- `Source`: the pixels handed to the board. It makes finished RGB565
  pixels on request, along a row or a column, in either direction, in
  either byte order, with the 2x done inside. There is no full-panel
  buffer anywhere.
- `Video`: all of it together. Draw into `frame_mut()`, then call
  `present(screen)` once a frame. A palette change sends the whole
  screen again, except one made through `recolour_unseen(|pal| ..)`:
  when none of the changed colours is on the panel's last picture, the
  panel needs nothing new, and only the changed pixels go out. (Used by
  console-core's boot animation at the switch to home; tests in
  `tests/present.rs`.)
- For a running game: `Video::game_buffers()` lends the picture
  (`&mut [u8; 19200]`) and the 256 palette colours (`&mut [u16; 256]`),
  from one borrow. The console wraps them in a `game_api::LiveFrame`
  for one call of the game, which writes into them. After a game wrote
  colours that way, `palette_mut().touch()` marks the palette as
  changed (the version cannot see such writes).
- Feature `embedded-graphics` (off by default): `Frame` becomes an
  embedded-graphics `DrawTarget`. Its colour is `Gray8`, and the grey
  value is the palette number. The plain helpers above do not need it.
  It adds `embedded-graphics-core` 0.4.1 (MIT OR Apache-2.0).
- Feature `swap` (off by default): the two pictures swap roles at each
  present, so a second core can send one while the console draws the
  next. Adds `Sender`, `Handed` and `Inline`, and
  `Video::present_handing`. See "Swap: two cores, two pictures" below.
  Without it, the crate is compiled exactly as before (`src/video.rs`;
  with it, `src/video_swap.rs` takes its place).

## Rules

- No `std`, no heap, no `unsafe`, no floating point.
- It depends only on `board-api`. Never on a board, esp-hal or the
  console.
- Fixed sizes, written down where they are defined.

## Memory

Kept for as long as the console runs, inside `Video`:

| what                          | bytes  |
|-------------------------------|--------|
| picture being drawn (`Frame`) | 19,200 |
| picture the panel shows       | 19,200 |
| palette (256 x 2) + version   |    516 |
| changed rectangles (16 x 8)   |    132 |
| flags, border colour          |     12 |
| **total**                     | 39,060 |

On the PC the count fields are 8 bytes, so a test measures 39,064.
Only during a call, on the stack: `diff` uses 720 bytes, `present`
about 130. The font is 256 bytes of fixed data (in flash on the chip).

### Ways to shrink it (not built)

- Row checksums instead of the second picture. Keep one 32-bit number
  per row of the shown picture (120 x 4 = 480 bytes) instead of the
  picture itself (19,200 bytes). Saves about 18.7 KB. Costs: only
  whole changed rows are known (no left and right edge), so more
  pixels are sent; a checksum can, very rarely, miss a change. A
  middle way: one number per row per 16-pixel strip (120 x 10 x 2 =
  2,400 bytes) gives rough left and right edges back.
- A "stream" mode with no second picture at all: the game says which
  rectangles it drew (or the whole screen every frame). Saves
  19,200 bytes; the game has to tell the truth.
- 4-bit pixels (16 colours): both pictures halve, saves 19,200 bytes.
  A different video mode, not a change to this one.
- `MAX_AREAS` 8 instead of 16 saves 64 bytes. Not worth it alone.

## How changes are found

`diff` follows the design page's simulator (its `present()`, "areas"
way) exactly. A test runs 400 frames from a fixed seed and compares
every rectangle with a file made by running the design page's own code
(`tests/data/sim_areas.mjs`, copied from the page, run in node). All
400 frames give the same rectangles. 162 of them hit the 16 limit, so
the joining is checked too.

1. Each row gives a span: first to last changed pixel.
2. Going down, a row joins the rectangle above it if the grown
   rectangle covers at most `WINDOW_COST_PX` (24) pixels more than the
   two kept apart. A row with no change closes the rectangle.
3. Over `MAX_AREAS`, the two neighbours whose joining wastes the fewest
   pixels are joined (the first such pair), again and again.

The older Rust code (`experiments/display-dma/src/dirty.rs`) differs in
four places:

- Its spans are rounded out to whole 4-pixel words. Here they are exact
  to the pixel (words are only used to compare fast).
- Changed rows next to each other always join. Here the cost rule of
  step 2 decides, so a diagonal line becomes a staircase of small
  rectangles instead of one big, mostly empty box.
- Over the limit it joins the two with the smallest gap between them.
  Here it joins the pair that wastes the fewest pixels.
- Its limit is 8.

The design page's way was chosen so the browser simulator and the
console send the same rectangles for the same frames.

### 16 or 8 rectangles

Each rectangle costs one window command on the wire: about 48-50 us,
measured on the CYD at both 40 and 80 MHz. So 16 rectangles cost at
most about 0.8 ms of a 33 ms frame, 8 cost 0.4 ms. More rectangles
mean fewer pixels sent for nothing, and one picture pixel (4 panel
pixels) takes about 0.8 us at 80 MHz, 1.6 us at 40 MHz. So 16 is the
default. It is the one constant `MAX_AREAS` in `src/areas.rs`.

`WINDOW_COST_PX` is 24, the design page's number. The measured cost of
a window (50 us) is worth about 31 picture pixels at 40 MHz and 62 at
80 MHz, so 24 leans toward more, smaller rectangles. Worth tuning on
the board; changing it means making the test file again.

## Speed

On the PC (release, `cargo run --release --example bench`):

| what                                         | time     |
|----------------------------------------------|----------|
| diff, nothing changed                        | 0.9 us   |
| diff, one 16x16 sprite moved                 | 0.9 us   |
| diff, 60 marks (many joins)                  | 2.5 us   |
| `Source::run`, whole panel, rows, native     | 16.4 us  |
| `Source::run`, whole panel, columns, wire    | 17.5 us  |
| 20 sprites 16x16 with see-through            | 1.5 us   |
| 4 lines of 30 letters                        | 1.6 us   |
| full present on the fake board (Indexed)     | 33 us    |

On the chip (read from the compiled code, not measured on the board):

- `diff` compares 4 pixels per 32-bit load (about 3-4 cycles per
  group). An unchanged frame should take about 80 us; the older code
  took 189 us, measured. Getting real 32-bit loads needed the
  `as_chunks::<4>()` view of a row; plain byte slices gave byte loads.
- `Source::run` with 2x: about 9 instructions per picture pixel along
  rows in native order, about 15 down columns in wire order (with the
  byte swap). That is about 18-30 cycles per picture pixel, against 17
  for the older fill loop. A full frame in the CYD's way (columns,
  wire order) is then about 2.4 ms of CPU. Boards that can use
  `indexed()` (the palette numbers) skip this.

## Swap: two cores, two pictures

Feature `swap`, off by default. A design for the ESP32's second core;
the CYD firmware turns it on with its `core2` feature (in its default
set).

### The idea

Without `swap`, `Video` keeps two pictures: one being drawn, and a copy
of what the panel shows. A present sends what changed and then copies
the whole drawn picture into the copy (19,200 bytes, every present).
Sending takes up to about 35 ms on the CYD, and the console waits for
it.

With `swap`, the same two pictures take turns. One is **drawn** (the
console and the game write into it). The other is **shown** (what the
panel shows, or what is being sent to it right now). At a present the
drawn one is handed to a sender, for example the second core. The
console then goes on drawing in the other picture. No third picture,
no extra memory.

### The rule

A present does these five steps, in this order
(`Video::present_handing`):

1. **Find.** Compare shown with drawn and list the changed rectangles.
   Only reads. The sender may still be reading shown at this moment;
   two readers are fine.
2. **Wait.** `wait_free()`: returns once the sender reads nothing it
   was given before. From now on, nobody but the console touches shown.
3. **Hand over.** `hand_over(&Handed)`: the drawn picture goes to the
   sender. From now on, nobody writes it until the next `wait_free()`
   has returned.
4. **Catch up.** Copy the changed rectangles from the handed-over
   picture into shown (the whole picture on a full send). This writes
   only shown (free since step 2) and only reads the handed-over one.
5. **Swap.** The caught-up picture becomes the drawn one.

After every present, the drawn picture equals the one just handed over
(outside the console's own `put_back`, see (g)). When nothing changed,
there is no wait, no hand-over and no swap, and the report is all
zeros (as without `swap`).

In one line: **the console never writes the picture the sender holds,
and it only writes the other one after `wait_free` has returned.**

### What the board must give

```rust
// console-video, feature `swap`
pub struct Handed<'a> {
    pub frame: &'a Frame,     // read it until your next wait_free returns
    pub palette: &'a Palette, // read it only during hand_over: copy it
    pub view: View,           // plain value
    pub border: u16,          // plain value
    pub areas: &'a [Rect],    // panel pixels, at most 16: copy them
}
impl<'a> Handed<'a> { pub fn source(&self) -> Source<'a>; }

pub trait Sender {
    fn wait_free(&mut self);
    fn hand_over(&mut self, h: &Handed<'_>) -> SendReport;
}

pub struct Inline<'s, S: Screen>(pub &'s mut S); // sends at once

impl Video {
    pub fn present_handing<S: Screen, O: Sender + ?Sized>(&mut self, out: &mut O)
        -> SendReport;
}

// console-core, feature `swap` (it turns on console-video/swap)
impl Console {
    pub fn step_with_sender<B: Board, O: console_video::Sender + ?Sized>(
        &mut self, board: &mut B, sender: &mut O) -> Wake;
}
```

`Console::step` stays as it is. With `swap` on, it sends at once
through `Inline` (that is how the simulator and all the tests run).
`step_with_sender` still takes the board: the console reads its input
and clock there, uses `S::PANEL_W`/`PANEL_H` for the view, and sets
the backlight through `Screen::backlight`. It never calls
`Screen::present` on that path.

What `hand_over` returns: a board that sends at once gives the real
report. A board that sends on another core gives at least the areas and
pixels; its times may belong to the send before (say so in the board's
notes).

### The cases

**(a) The catch-up copy.** It copies only the changed rectangles: the
sum of their areas, in picture bytes (one byte per pixel). At most
19,200 bytes, on a full send. Without `swap` the copy is always 19,200
bytes. So `swap` never copies more than today, and usually much less.
In a 420-frame simulator tour (padtest, pause menu, notices, home), 105
presents sent something: 3 full (19,200 bytes each), the other 102 a
median of 212 bytes (mean 581, largest 4,489). In all, 5.8 % of what
the copy without `swap` moves. The rectangles are the ones
`last_areas()` gives after the present.

**(b) A present while the last send is still running.** The console
waits in step 2 (`wait_free`). It does not skip the frame. Skipping
was rejected: the console believes what it presented is on the panel
(its diff is built on that), and when it then sleeps until the next
input (`Wake::InputOr`), a skipped picture would stay missing for a
long time. Waiting costs at most the rest of one send (about 35 ms on
the CYD). The find step (1) runs before the wait, so it overlaps the
send.

**(c) `force_full` and the palette.** A palette change, a border
change or `force_full` sends everything, as before. The catch-up is
then a whole copy (19,200 bytes). The palette itself is not doubled:
`Handed.palette` may be read only during `hand_over`. The board copies
what it needs there (516 bytes in the model; the CYD could instead
rebuild its 512-byte wire palette only when the version changed,
inferred). The console may change its palette right after `hand_over`
returns (a game's next colour change), so a sender that reads it later
would show the wrong colours. A test breaks this on purpose and fails.

**(d) A game writing through `GameFrame.pixels`.** `game_buffers()`
always lends the drawn picture, never the held one. Its address
changes from one present to the next. The game interface allows that:
the pointers are valid for one call only. A game that keeps the
pointer past its call breaks that contract. With `swap` it would then
write the picture the sender reads (a torn frame). Without `swap` it
would write a stale picture (a lost change). The console cannot stop
this; the game kit gives the pointer only inside the call.

**(e) Screenshots through `link.rs`.** The CYD's shot reads
`console.video().frame()` and `palette()` between two steps, on the
console's core. `frame()` is always the drawn picture, never the held
one, and after a present it equals what was handed over. So the shot
needs no change and no wait. A shot read back from the panel itself
(through the LCD) is different: it uses the screen bus, so the board
must call `wait_free()` first.

**(f) The backlight right after a present.** The console waits for the
sender before it changes the backlight (`out.settle()`, which is
`wait_free()`, then `Screen::backlight`). So the screen is never lit
before the first picture is on it, and the backlight is never set while
the sender uses the screen. The two-core lab
(`experiments/cyd-lab`) hit this problem and fixed it the same way: it
joins the second core before it sets the backlight.

**(g) Leaving a game, notices, the pause menu.** Notices and the pause
menu are drawn over the game's picture before the present (`cover`),
and the game's own pixels are put back right after it (`put_back`).
With `swap`, `put_back` writes into the new drawn picture, after the
catch-up. That is the right one: the held picture keeps the menu (it is
what the panel must show), and the drawn one gets the game's pixels
back for the game's next call. Leaving a game and coming home draw a
new page into the drawn picture; nothing special. The order of
`present`, `settle`, `put_back` in `Console::step_via` is what makes
this hold, and the console-core test checks it at every tick.

### Who touches which memory, and when

Two cores: **core A** runs the console (`step`), **core B** sends.

| memory | who, and when | what enforces it | if broken |
|---|---|---|---|
| the held picture (19,200 B, the one last handed over) | A: reads it in step 1 of the next present (find), writes it only in step 4 (catch-up), after `wait_free` returned. B: reads it only, from `hand_over` until it says "done". | The order of steps in `present_handing` (tests); `Video` lends only the other picture (`frame_mut`, `game_buffers`, `frame`); on the board, the done flag (release store on B, acquire load in `wait_free` on A). | A data race (undefined behaviour in Rust). In practice (inferred): a torn frame, part old, part new. If A writes before B read it, the panel gets pixels of the next frame early; the next send covers them. |
| the drawn picture (19,200 B) | A only, at any time between presents (console, game, overlays, screenshots). B never. | Same as above: B only ever gets a picture through `hand_over`. | B would read a picture being drawn: half-drawn shapes on the panel. |
| the console's palette (516 B) | A only. B never reads it: the board copies it during `hand_over`, which runs on A. | The `Handed` rules (docs); a test that reads it late fails. | Wrong colours for one frame (a later palette with an older picture), and a data race. |
| areas, view, border (at most 16 x 8 B, plus 6 B) | A writes them; the board copies them during `hand_over`, on A. | `Handed.areas` borrows a stack array of `present_handing`: the compiler stops a board keeping the reference (only `unsafe` could). | Garbage areas: the send goes to the wrong places. |
| `Video`'s own fields (`second`, `in_step`, `shown_palette`, `areas`) | A only. | Private fields; B never sees `&Video`. | Not possible without `unsafe`. |
| the board's hand-over slot (picture address, palette copy, areas, flags; the board's code) | A writes it in `hand_over`, then sets "go" (release). B reads it after "go" (acquire), sets "done" (release) after its last read of the picture. A reads "done" (acquire) in `wait_free`. | The board's atomics. | B could see an old palette or areas with a new picture. |
| the screen bus, the LCD, the backlight | B, from `hand_over` until "done". A, only after `wait_free` (the console does that before the backlight; the board must do it before any panel read, a scanline calibration or anything else on the bus). | `out.settle()` in console-core for the backlight; the board for the rest. | Two cores driving the same bus: broken sends, a hang, or a lit white screen before the first picture. |

### Sizes

`size_of::<Video>()` does not change with `swap`: the two pictures
were there already, and the new one-byte flag fits in padding.
`Console` does not change either. Measured by compiling a small crate
for each target (kept outside the repository):

| target | `Video` without | `Video` with `swap` | `Console` without | `Console` with `swap` |
|---|---|---|---|---|
| ESP32 (`xtensa-esp32-none-elf`) | 39,056 | 39,056 | 46,344 | 46,344 |
| PC (64-bit) | 39,064 | 39,064 | 46,376 | 46,376 |

`Video::new()` stays all zero bytes (`second == false` means the first
picture is drawn). On the stack during a present: `Handed` (a few dozen
bytes) and a 16-rectangle panel list (128 bytes), as before. The board
needs its own hand-over slot: about 516 bytes for the palette copy (or
its 512-byte wire palette), 128 bytes of areas, a pointer and two flags.
And a stack for the second core: the two-core lab gave it 8 KB and
measured 1,760 bytes used at most (`experiments/cyd-lab`).

### What a board on two cores must watch

- **Release and acquire.** "Go" (A to B) and "done" (B to A) must be
  atomics with release on store and acquire on load. Otherwise B may
  read the picture before A's last writes, or A may write before B's
  last reads.
- **Never move the `Video`.** B reads the picture where it was. Keep the
  `Console` in a `static` (it already is on the CYD).
- **Aliasing (inferred).** The board keeps a raw pointer to the held
  picture while A holds `&mut Console`. That is fine under the "tree
  borrows" model of Rust and formally not under "stacked borrows"; it
  was not checked with Miri (not installed). B must only ever read
  through that pointer. Do not give B a `&mut` to anything A also uses
  (the lab used `&mut CydScreen` on both cores; do not copy that).
- **`wait_free` must really wait.** If it has a time limit, the only
  safe outcome is a stop or a reset, never "go on anyway".
- **Flash writes stop both cores' cache.** Saving settings turns the
  flash cache off. B must then not run from flash: call `wait_free`
  first and park B, or keep all of B's code in RAM (inferred from the
  ESP32 manual; not measured here).
- **B cannot run code from RTC fast memory** (the radio's PHY code is
  placed there on the CYD).
- **The lab's `cpu1-cache-ram` option** cannot be combined with this.
- **The report.** Times in `SendReport` from `hand_over` may belong to
  the send before; areas and pixels are this one's.
- **The CYD's beam timing.** Sends are timed to the panel's refresh
  (`beam.rs`). On B that code must stay in RAM; check the boot log's
  `panel:` line after any change.

### Tests

- `tests/swap.rs` (this crate). In both builds: a plain model of the old
  rule against `Video`, 1,500 random frames per panel walk (pixels,
  rectangles, clears, a game's pixels and colours, border, full sends,
  cover and put back), and the size check. With `swap` only: a slow
  sender that keeps each picture until the next present, reads it in
  pieces, and checks every byte it reads against a copy taken at
  hand-over; plus the order of wait and hand-over, and "drawn equals
  handed over" after each present.
- `crates/console-core/tests/swap.rs`: the whole console with a game,
  the pause menu, notices, the backlight and restarts, stepped once
  with `step` and once with `step_with_sender` and a slow sender, side
  by side, for 3 x 1,000 ticks (about 2,000 hand-overs).
- Each test was shown to fail when the rule is broken on purpose (wait
  removed, catch-up removed or partial, no swap, catch-up before the
  wait, the game given the held picture, backlight without the wait,
  palette read late). The script and logs are kept outside the repo.

## Testing

```sh
cargo test --all-features         # in this folder; no window opens
cargo test --features swap --test swap   # the swap tests alone
cargo run --release --example bench
../../boards/desktop/check.sh     # every new crate, plus chip builds
node tests/data/sim_areas.mjs > tests/data/sim_areas.txt
                                  # only after changing the rule on
                                  # purpose (makes the test file again)
```

- `tests/draw.rs`: exact pixels for each drawing helper, cut edges too.
- `tests/areas.rs`: hand-made cases; the 400 frames against the design
  page; random frames on every way the fake board walks the panel,
  checking that every changed pixel is sent, nothing overlaps, and the
  panel ends up equal to the picture.
- `tests/source.rs`: `Source::run` against a simple one-pixel-at-a-time
  version, in all four directions and both byte orders, on several
  panel sizes, with whole lines and random short runs.
- `tests/present.rs`: the test card from `crates/test-card` through the
  whole path on every walk, a palette change, the memory sizes, and
  (with the feature) the embedded-graphics drawing.
- `tests/swap.rs`: the feature `swap` (see "Swap: two cores, two
  pictures"); part of it runs without the feature too.
- `tests/attack.rs`: long seeded random runs (pixels, rectangles,
  palette, border, full sends) on every walk, in both byte orders, and
  on a panel with a border; after each present the whole panel must
  equal the picture. Also a text too long for its width.

Chip build (compile only):

```sh
cargo +esp build --release -Zbuild-std=core --target xtensa-esp32-none-elf
```
