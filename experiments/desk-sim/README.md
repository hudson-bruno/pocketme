# desk-sim: the desktop simulator (experiment)

## What it is

The console's own code, built for the PC, with a **fake board**
underneath. You can try a change in seconds, and several people can work
at once, because nobody needs the one real board.

## What it is not

It is **not an emulator of the ESP32 chip**. It does not pretend to be
the chip. So it cannot tell you:

- whether something fits in the ESP32's memory (520 KB of RAM);
- whether something is fast enough on the ESP32 (the PC is far faster);
- whether the screen tears (that depends on the real panel and its cable);
- anything about the radio (Bluetooth);
- how the real touch screen and gamepad feel (here the mouse and the
  keyboard stand in for them);
- whether code that is fine on the PC (a 64-bit machine) also works on
  the 32-bit chip. `./check-no-std.sh` catches part of this, by building
  for the chip.

The real board stays the judge of those.

## The three parts

| Folder   | What it is | Uses the PC's standard library? |
|----------|------------|------|
| `board/` | The **board layer**: what the console may ask of the hardware (screen, touch, gamepad, time, log). **Provisional**, see `board/README.md`. | No |
| `demo/`  | A small console program. It only talks to the board layer. | No |
| `sim/`   | The PC side: a fake board with a **window**, and one with **no window** that follows a script. | Yes |

`board/` and `demo/` build for the ESP32 too (`./check-no-std.sh` proves
it; it only compiles, it flashes nothing).

The demo shows a start screen, then a screen that proves every input:
a little character you move with the d-pad or the left stick, a yellow
mark where you touch, a lamp for each button, the two sticks and
triggers, the frame number and the time. SELECT shows the two test
pictures from `crates/test-card`, drawn by that crate's own code,
unchanged. HOME goes back to the start.

## How to run it

All commands run in this folder (`experiments/desk-sim`).

**First try** (the first build takes about 4 seconds):

```sh
cd experiments/desk-sim
cargo run --release -- window
```

A window opens, twice the size of the console's screen (640x480). You
see a dark blue start screen with "DESK SIM" and a little green
character. Press **Enter** (START) or click the window to go on. Then
move the character with the **arrow keys**, click and hold to touch, and
press **Backspace** (SELECT) to see the test pictures. **Esc** or the
window's close button stops it. The console's log lines appear in the
terminal.

More ways to run it:

```sh
cargo run --release -- window              # a window, 2x size, 30 frames a second
cargo run --release -- window --scale 3 --fps 60
cargo run --release -- window --close-after 5   # closes by itself after 5 s, saves shots/window-last.png
cargo run --release -- headless --frames 120 --script scripts/tour.txt
cargo run --release -- --help
```

The window needs the SDL3 library.

### Keys in the window

| Key | Does |
|-----|------|
| arrow keys | d-pad |
| X / Z | A / B |
| S / A | X / Y |
| Q / W | L / R (the shoulder buttons) |
| 1 / 2 | left / right trigger (fully pulled while held) |
| I J K L | left stick (up, left, down, right) |
| T F G H | right stick |
| Enter | START |
| Backspace | SELECT |
| Tab | HOME |
| mouse button | touch (where the pointer is) |
| P | pause, and go on |
| N | one frame forward (while paused) |
| F12 | save a screenshot (PNG) into `shots/` |
| Esc | quit |

Keys and clicks shorter than one frame still count for one frame. A
touch is where the pointer is while the mouse button is held; if you
drag outside the window, the touch stays at the nearest edge. The
window cannot be resized: choose the size with `--scale`.

`--close-after` reads back what the window itself showed last and saves
it as `window-last.png`. So a quick check needs nobody to look at the
screen. On the development PC that picture matched the no-window picture of the
same frame exactly (every pixel, at 2x).

**A real gamepad does not work yet.** The Linux in WSL2 cannot see a
pad that is connected to Windows. The code has a place for it
(`real_pad()` in `sim/src/window.rs`). A Windows build of the simulator
would fill it in. Until then the keyboard is the pad.

## Running with no window

`desk-sim headless` runs a fixed number of frames. A **script** says what
happens on which frame. The clock moves by exactly one frame each time
(at 30 frames a second, 33,333 microseconds). Nothing depends on how fast
the PC is. So **every run gives exactly the same pictures**.

```sh
cargo run --release -- headless --frames 120 --script scripts/tour.txt \
    --shot 0,64,last --out shots
```

It prints the console's log, saves the chosen frames as PNG files, and
prints a short **fingerprint** of each. Two runs with the same fingerprint
made the same picture.

Options: `--frames N`, `--fps 30|60`, `--script FILE`, `--shot LIST`
(frame numbers, or `last`), `--out DIR`, `--quiet` (no log lines).

### Writing a script

One line per event: the frame number (the word `at` before it is
allowed), then what happens. `#` starts a comment.

```text
at 10 press start          # hold a button down
at 12 release start        # let it go
at 20 touch 100 50         # finger down at x=100, y=50 (screen pixels, 0..319, 0..239)
at 25 lift                 # finger up
at 30 stick left 127 0     # left stick fully right (x and y: -128..127; 0 is the middle)
at 40 trigger right 255    # right trigger fully pulled (0..255)
at 45 shot                 # save a picture of this frame
```

Button names: `up down left right a b x y l r start select home`.
Everything stays as the script left it until a later line changes it.
`scripts/tour.txt` is a full example.

Some rules:

- Several lines may use the same frame. They happen in the order written.
- A `press` and a `release` of the same button on the same frame still
  count: the console sees the button held for that one frame. The same
  for `touch` then `lift`. (A key tapped quickly in the window behaves
  the same way.)
- A touch outside the screen (x 320 or more, y 240 or more) is a mistake.
- A mistake stops the run before it starts, with the line number, for
  example `script line 3: unknown word 'pres'`.
- Lines for frames after the end of the run are not used. The program
  says so in a warning, and tells you which `--frames` would reach them.
- `--shot` with a frame that never runs is a mistake too.

## Testing without a window

```sh
cargo test                          # all tests, about 2 s after the first build
cargo test --no-default-features    # the same with no SDL3 at all
./check-no-std.sh                   # board/ and demo/ build for the ESP32
```

The tests need no desktop:

- `sim/tests/demo.rs` runs the demo with scripts and checks pixels: every
  button lights its lamp, the stick and d-pad move the character, the
  touch mark shows, the triggers fill their bars, the clock moves one
  frame at a time, and the same script gives the same pictures twice.
  The two test pictures must match `crates/test-card` pixel for pixel.
- `sim/tests/window.rs` opens the window with SDL's **offscreen** driver
  (the window exists only in memory, nothing appears) and puts key
  presses and clicks into its queue. It checks the keys, the mouse,
  pause, single step, screenshot and Esc. It also reads back what the
  window shows and checks it pixel by pixel: sharp 2x2 squares, and red,
  green and blue in the right places.
- `sim/tests/cli.rs` runs the `desk-sim` program itself and checks that
  mistakes give a clear message and a failing exit code, never a crash.
- When a picture test fails, it writes the wrong picture (and a picture of
  the difference, wrong pixels in magenta) under `target/tmp/` and says
  where.

**When you change the demo's drawing on purpose**, the fingerprint test
fails. Run the tour, look at the PNG files, and if they are right, copy
the new fingerprints into `TOUR_FINGERPRINTS` in `sim/tests/demo.rs`.

Use `cargo fmt`, not `cargo fmt --all`: `--all` also reformats
`crates/test-card`, a path dependency outside this folder.

## What is provisional

- **The board layer's shape.** The owner has not decided it. In
  particular the screen is a full 320x240 RGB565 copy (150 KB), while the
  plan is a 160x120 screen with a palette.
- **The button list** (debug link's 12 + HOME), and **touch in screen
  pixels** (320x240).
- **The keys** chosen for the keyboard.
- Not here yet: loading games from a folder that stands in for the SD
  card, the debug link over a fake serial port, video modes, sound,
  saved settings, Bluetooth.

## Measured on the development PC (24 cores, WSL2)

| What | Result |
|------|--------|
| Build from clean (debug, with window) | 4.3 to 4.6 s (18 crates) |
| Build from clean (no window) | 2.4 s |
| Build from clean (release) | 3.6 s |
| Rebuild after a one-line change in `demo/` | 0.4 s |
| `cargo test` (26 tests) from clean, then again | 6.1 s, then about 2 s |
| Console work per frame, release | 0.055 ms |
| Console work per frame, debug | 2.5 ms |
| Frames per second in the window | 30.2 (asked 30), 60.4 (asked 60) |
| Program size, release | 742 KB (574 KB stripped), plus the system's libSDL3 |
| Program size, debug | 8.6 MB |

"Console work" is the time from the start of a frame to `present()`. It
says nothing about the ESP32's speed.

## Licences of what it uses

All free to use. embedded-graphics, embedded-graphics-core, bitflags,
libc, num-traits, az, micromath: MIT or Apache-2.0. float-cmp: MIT.
byteorder: Unlicense or MIT. sdl3 (Rust crate): MIT. sdl3-sys and the
SDL3 library itself: Zlib.
