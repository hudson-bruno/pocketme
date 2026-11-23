# Optimisations

This page lists the optimisations in PocketMe that were measured. For
each one it gives the problem, what was done, the result with its date
and where it was measured (on the board, or on the PC as a built size),
and what it cost. It ends with the things that were tried and not kept,
and with how to measure.

- [Screen](#screen)
- [Memory](#memory)
- [Games](#games)
- [Tried and not kept](#tried-and-not-kept)
- [How to measure](#how-to-measure)
- [Where this comes from](#where-this-comes-from)

How the screen path works as a whole is on [the display page](display.md).
Why each choice was made is on [the decisions page](decisions.md).

## Screen

### Send only what changed

**Problem.** A full 320x240 picture at 16 bits is 153,600 bytes, and
sending it takes about 15.6 ms at 80 MHz (15,629 us, measured on the
board).

**Done.** The console compares each new 160x120 picture with the one
the panel shows and sends only the changed parts, as at most 16
rectangles. Neighbouring rows are joined when that wastes at most 24
pixels; over 16, the pair that wastes least is joined. The compare loop
reads four pixels at a time as one 32-bit word.

**Result.** A still picture sends nothing. One rectangle costs about 48
to 50 us of set-up on the wire (board, at 40 and 80 MHz), so 16 cost at
most about 0.8 ms of a 33 ms frame. On the PC the compare takes 0.9 us
for an unchanged picture and 2.5 us for 60 marks. On the chip it is
estimated at about 80 us for an unchanged picture, against 189 us
measured for the older code; the new figure is read from the code, not
measured.

**Cost.** Two pictures in RAM instead of one: 39,060 bytes for the
picture code in all.

### Windows or one box

**Problem.** A turning 3D scene changes about ten low, wide bands a
frame. Each band, sent as its own window, may only start in a narrow
part of the panel's refresh, and ten of them seldom fit one refresh.
On 2026-11-17 LANTERN at NORMAL detail and the 60 setting ran 33.35
frames a second, and 582 of 911 presents found no plan that fitted one
refresh (board).

**Done.** Since 2026-11-18 each present also plans one box around all
the changes, and the timing model takes whichever ends sooner
(`send-plan`, module `choose`). All planning happens before the first
window is sent.

**Result** (board, 2026-11-18, one core, LANTERN level 4, a gamepad
connected):

| setting | frames a second | present average | presents without a one-pass plan |
|---|---|---|---|
| NORMAL/30 | 26.31 to 30.00 | 29.8 to 14.0 ms | 281/452 to 7/528 |
| HEAVY/30 | 26.06 to 30.00 | 32.5 to 14.8 ms | 236/370 to 0/535 |
| NORMAL/60 | 39.75 to 56.16 | 28.9 to 13.8 ms | 296/458 to 13/964 |
| HEAVY/60 | 36.56 to 52.46 | 27.2 to 14.3 ms | 265/546 to 3/883 |

Crossed windows (possible tears) in NORMAL/60 went from 78 to 0.

**Cost.** A box sends pixels that did not change: in the model about
56,500 a present against 43,900. About 850 bytes of `.bss`; no code
RAM.

### DMA with small strips

**Problem and what was done.** Pixels go out by DMA (a part of the chip
that moves bytes to the bus by itself) from two strips of 5,120 bytes:
the processor fills one while the other is sent. The question was the
strip size.

**Result** (board; the README gives no day):

| area | 2 x 5,120 B | 2 x 15,360 B |
|---|---|---|
| full screen | 15,629 us | 15,480 us |
| 72x72 | 1,101 us | 1,095 us |
| 320x88 | 5,750 us | 5,709 us |

**Cost.** At most 1% slower than large strips, and 20,480 bytes of RAM
saved.

### The SPI driver and the scanline read in RAM

**Problem.** Code in flash runs through a cache. A cache miss in the
timed reads stretched the time-stamped scanline read to 41 us or more
(board).

**Done.** esp-hal's low-level SPI driver, the scanline read
(`Lcd::scanline_stamped_raw`) and `fill_fast` run from RAM.

**Result.** Together with the hot block below, the fastest scanline
read was 17 us in every recorded board run on 2026-11-19 (the boot
log's `fastest_read_us=17`).

**Cost.** Code RAM below the game slot, which is shared with the
console's other RAM code; about 8.2 KB of it was left in the build with
the gamepad (2026-11-19).

### The hot block and the flash cache

**Problem.** The flash cache is 32 KB per core, two ways, 32-byte
lines, 512 sets. Addresses 16 KB apart share a set, and a set holds two
lines, so three lines a loop needs in one set throw each other out on
every pass. Before 2026-11-18 the linker placed the send path wherever
it liked. The lead (send call to first pixel byte) was 30 us on average
in one build and 100 to 122 us in six builds that differed only in
layout; the scanline read took 17 us in some layouts and 25 us in
others, with no change to the code (board).

**Done.** The functions of the timed paths are linked as one block
(`.text_hot`) right after all other flash code. `hot-block.list` names
them; `build.rs` makes the linker script. The part used for every
window is under 16 KB and the whole block under 32 KB, so the block
alone can never put three lines into one set. The link stops when a
listed function is missing or a part is too small or too big, and the
firmware check runs `hot-check.py` on seven builds, the owner's sets
with the gamepad and one measuring build.

**Result** (board, 2026-11-19, two cores, KESTREL, five layouts made
by moving the block on purpose, two runs each): game lead 26 to 28 us
in every layout, 0 crossed in every run, `fastest_read_us=17` and
`calibrated=true` in all 20 runs of both core settings.

**Cost.** No RAM. The block is 20,924 bytes with two cores (window part
13,652 bytes in 427 cache lines, 85 lines left) and 19,536 bytes with
one core (window part 14,732 bytes, 461 lines, 51 left), measured on
the PC. Every refactor or esp-hal update on the path means updating the
list.

### Flash at 80 MHz

**Problem.** Code that misses the cache waits for a flash read, and the
flash ran at the default 40 MHz.

**Done.** Flash the firmware with `--flash-freq 80mhz`. Only the clock
in the image header changes; the file is the same. Adopted 2026-11-22.

**Result** (board). On 2026-11-19, the same firmware file: steps about
0.5 ms shorter (CAMBER 17.8 ms against 18.3 ms), 4 starts fine. On
2026-11-22: 10 starts in a row fine, settings load 3,750 us against
5,012 us at 40 MHz.

**Cost.** None measured. Timings from before 2026-11-22 were taken at
40 MHz and are not comparable with later ones.

### The second core sends the pictures

**Problem.** On one core the console waited for the panel: a present
does not give way to anything else, for up to about 35 ms.

**Done.** With `core2` (default since 2026-11-18) the first core hands
each finished picture to the second core and goes on. The console keeps
two pictures that swap roles, and copies only the changed parts across
after each hand-over.

**Result** (board):

| | one core | two cores |
|---|---|---|
| STARDUST step (2026-11-17) | 17.1 ms | 0.6 ms |
| row scroll step (2026-11-17) | 15.5 to 17.4 ms | 2.0 to 2.4 ms |
| longest wait for a gamepad report (2026-11-17) | about 33 ms | about 3.3 ms |
| HAIRPIN HEAVY at the 60 setting | 45.7 frames a second | 58.7 |
| crossed windows, seven games | up to 5 a run | 0 in every run |

The catch-up copy cost 5.8% of a full copy in a 420-frame simulator
tour.

**Cost** (PC, 2026-11-18): 8,732 bytes less main stack (the second
core's 8 KB stack and its statics), 56 bytes of code RAM, about 8.5 KB
of flash. With `core2` off the firmware is the one before, byte for byte
except source line numbers.

## Memory

### The console as an all-zero static

**Problem.** The `Console` struct is tens of kilobytes. On the stack it
would not fit; as a static with a start value it would cost the same in
flash.

**Done.** `Console::new()` is all zero bytes, so the value lives in
`.bss` (memory the start-up code fills with zeros) and costs no flash.
A test (`tests/zeros.rs`) keeps it that way.

**Result.** 45,032 bytes in the boot log on 2026-11-18; 45,176 bytes
(about 45 KB) in the firmware built on 2026-11-23. In the 2026-11-18
change the home screen's data moved into the game data slot while no
game is loaded (5,396 bytes): the `Console` went from 50,456 to 45,032
bytes and the stack gained 5,320 bytes (linker numbers on the PC).

**Cost.** Home reloads its list and pictures from the card after a
game, in 3 steps.

### Big values never on the stack

**Problem.** The stack is small, and a frame that is too big overwrites
something else without warning.

**Done.** The gamepad host (about 19.7 KB) is copied onto the heap once
and never moved. In games, the game's state is built in the kit's
`restart`, a frame of its own, not inside the update call. Every chip
build reads every function's stack frame and fails when `restart` has a
frame over 16,384 bytes or any other function one over 4,096.

**Result.** The guard came from a fault found on the board on
2026-11-22: a 10.8 KB game state built inside the update call made a
25.8 KB frame. The biggest `restart` frames today (PC, 2026-11-23):
PROTO3D 13,792 bytes, DRIVE3D 10,576, MERCATOR 9,104.

**Cost.** The limits are estimates. The check reads each frame alone,
so it does not prove that a chain of calls fits.

### A feature for every big part

**Problem.** With so little RAM, a part's cost is only known by
building without it. **Done.** The radio, the second core, the saved
settings, the debug link, the boot animation and the game flash parts
are each a build feature. **Result.** Each cost on this page is the
difference between two builds. **Cost.** Build time: the firmware check
builds 154 sets in about 22 minutes (2026-11-23).

### Fixed slots

**Problem.** Loading a game should not need relocation. **Done.** A
game has a 56 KB code slot and a 48 KB data slot at fixed addresses,
written in one place (`game_api::layout`); the linker stops when the
console's RAM code or heap would reach a slot. **Result.** Loading is a
check and a copy: CAMBER in 22,604 us, MERCATOR in 29,813 us (board,
2026-11-22). **Cost.** Moving a slot means rebuilding every game.

## Games

### Fixed-point maths, no floating point

**Problem.** By the project's rules, games and the console core use no
floating point, yet the 3D games need fractions.

**Done.** The shared 3D library `game-3d` works in whole numbers: 1/256
metre for positions, 1/16 pixel on screen, 1/16384 for sine, cosine
and directions, 1/65536 of a turn for angles. Its
steps are culling, transform, back faces, clipping, perspective,
sorting and a triangle filler with a top-left rule (so neighbouring
triangles never draw a pixel twice or leave a gap).

**Result.** No measurement against a floating-point version exists.
KESTREL, CAMBER, PROTO3D and DRIVE3D use the library; MERCATOR
has its own copy. Choices such as the lens and checked or wrapping sums
are made at compile time, in the game's `Setup`.

**Cost.** The library is compiled into each game, so each game carries
its own copy.

### Detail levels sized by measurement

**Problem.** A 3D scene can take any amount of time; the frame does not.

**Done.** The 3D games offer detail levels. The top one of PROTO3D and
DRIVE3D (MAX) is sized for an average of 24 frames a second
(41.67 ms), decided 2026-11-22.
PROTO3D's levels come from a cost model,
`0.48 + 0.01473 T + 0.000291 P` ms for T triangles and P pixels
(estimated, not checked on the board). KESTREL's budgets (160, 240 and
320 triangles) were guessed; its LOW level drops a gate early, because
a ring that fills the view makes the console send almost the whole
picture.

**Result** (board, 2026-11-22): PROTO3D at about 30 frames a second at
all four levels, MAX at 23.74 ms on average; DRIVE3D MAX 26.36 ms
standing; CAMBER 10.92 ms a step standing and 10.30 ms under thrust, at
30 frames a second.

**Cost.** Each level is a menu choice the player keeps.

### Small builds

**Problem.** The code slot is 56 KB.

**Done.** Games for the chip are built for size (`opt-level s`), in one
piece (fat LTO, link-time optimisation across all crates), with a panic
that stops instead of unwinding, and with unwinding tables thrown away.

**Result** (PC, BENCH, built sizes after the 60-a-second ramp change):

| build | overflow checks | code bytes |
|---|---|---|
| opt-level s (used) | on | 46,480 |
| opt-level s | off | 42,224 |
| opt-level 2 | on | 50,768 |
| opt-level 3 | on | 55,008 |
| opt-level 3 | off | 49,080 |

Turning overflow checks off saved CAMBER 9,488 bytes of code and 5,776
of data (counted 2026-11-19). Only CAMBER has them off, by the author's
choice for speed.

**Cost.** Speed from higher optimisation levels was not measured.

### Fitting the data slot

**Problem.** BENCH's first build with its island scene had data 11,996
bytes over the slot.

**Done.** The work memory got an all-zero start value, so starting it
is a memory fill and not an 8 KB constant. The 2 KB sort lists became
locals. Floor strips are drawn, not stored, and island origins became
16-bit.

**Result** (PC): it fits, with 316 bytes of data slot to spare
(2026-11-22). The pictures did not change.

**Cost.** A 2 KB zero fill a call, about 2 to 3 us (estimated).

### Code past the code slot

**Problem.** Some games outgrew the 56 KB code slot while the data slot
had room.

**Done.** Code may run on past the code slot into the top of the data
slot. The rule is: code past the slot plus all data at most 48 KB.
Built 2026-11-21, on the board 2026-11-22.

**Result** (board, 2026-11-22, two probe games, average step time over
10 s):

| | code in the slot | code past the slot |
|---|---|---|
| small probe, no data reads | 12,578 us | 12,569 us |
| small probe, with data reads | 16,067 us | 16,172 us |
| large probe, no data reads | 11,430 us | 11,439 us |
| large probe, with data reads | 14,935 us | 15,440 us |

Between -0.07% and +3.4%. Three games use it: MERCATOR (18,628 bytes
past), PROTO3D (15,824) and DRIVE3D (4,252).

**Cost.** A memory check at start-up (1,919 us). The wipe after each
game now also reads the spill back; it took about 1,100 us (board,
2026-11-22), as the wipe did before.

### Game flash parts

**Problem.** MERCATOR with its planet landing outgrew RAM altogether.

**Done.** Code and constants named in the game's `flash.list` are placed
in two reserved windows of the firmware's own flash (code at
0x400D0020, constants at 0x3F400120) and run through the cache. The
game file carries their checksums; the firmware starts the game only
when it holds the same bytes. Built 2026-11-22, on the board since
2026-11-23.

**Result** (PC, 2026-11-22, the first split):

| | before | with the flash part |
|---|---|---|
| code in RAM | 69,764 | 51,236 |
| constants in RAM | 23,744 | 13,224 |
| room left in the slots | 5,912 | 34,960 |
| game file | about 100 KB | 71,600 |

On the board (2026-11-23), at its starting settings (HIGH detail, chase
view, 60), MERCATOR flew in space at 4.5 ms a step and 59.99 frames a
second. The check of the flash part at game start reads
106,808 bytes in 17.0 ms.

**Flash against RAM.** The planet's inner loops took twice the time
from flash (board). Nineteen of them, about 11,400 bytes, went back to
RAM, and screens that run only once a picture of their own (the docked
menus, the market, the title text) moved to flash to pay for them.

**Cost.** The firmware image grew from 1,675,632 to 1,845,952 bytes
(PC, 2026-11-22, with MERCATOR's first flash part). A
new build of a game with a flash part needs a new firmware.

## Tried and not kept

| what | measured | why not |
|---|---|---|
| writing the panel row by row | tore on every frame: a full frame takes 11.7 to 31 ms, the free time between refreshes is about 1 ms | replaced by column by column |
| writing at 40 MHz | the writer is half the speed of the refresh | cannot be tear-free; 80 MHz is used |
| reading the panel at 8 MHz | wrong red bits | 4 MHz is used |
| large DMA strips, 2 x 15,360 B | at most 1% faster | cost 20,480 B of RAM; kept only as the measuring feature `wide-strips` |
| groups of windows sharing columns | always equal to one box over LANTERN's lists (model) | left out |
| the timed esp-hal functions in RAM instead of the flash block | lead 26 to 40 us, too few runs to rank | cost 6,096 B of RAM, against about 8.2 KB then left |
| a second core with a copied picture instead of swapped ones | main stack 10,224 B, too tight | needed a 19.8 KB picture box |
| rectangle limit of 8, joined by gap | | replaced by 16, joined by waste |
| opt-level 2 or 3 for games | 4,288 to 8,528 B more code (BENCH) | size is the limit |
| the planet's inner loops in flash | twice the time | back in RAM |

Ideas that were costed but not built: a checksum per row instead of a
second picture (would save about 18.7 KB), a streaming mode, and 4-bit
pixels.

## How to measure

The board decides speed, memory and tearing; the simulator cannot.
Every command that opens the serial port must hold the board lock.

- **The boot log's `panel:` line.** After any change to the memory
  layout it must show `fastest_read_us=17` and `calibrated=true`.
- **The `send` line** in the firmware log: presents, windows, how many
  went as windows or as a box, `no_plan`, `late`, `crossed`, and
  average and longest `wait_us`, `send_us` and `lead_us`, plus pixels
  and refreshes a present.
- **The `perf` report** (letter `m` over the link, `r` to reset first):
  `fps`, `step_avg_us`, `present_avg_us`. The work without the panel
  wait is the step minus the present.
- **The picture check** (letter `V` with two cores): checks sums of the
  pictures, to catch a panel that shows a mix of two.
- **`boardctl stats`**: a short form of the same counters, plus heap
  and stack.
- **`hot-check.py`** on a built firmware: the hot block's parts, sizes
  and cache lines left.
- **`games/build-chip.sh`**: each game's code, data and room left, the
  biggest stack frames, and for MERCATOR the flash part's use.
- **Compare like with like:** the same flash speed, and builds from the
  same tree, since the layout alone moves timings.

## Where this comes from

- [boards/cyd/README.md](../boards/cyd/README.md)
- [boards/cyd/hot-block.list](../boards/cyd/hot-block.list),
  [boards/cyd/hot-check.py](../boards/cyd/hot-check.py),
  [boards/cyd/build.rs](../boards/cyd/build.rs),
  [boards/cyd/Cargo.toml](../boards/cyd/Cargo.toml)
- [boards/cyd/src/screen.rs](../boards/cyd/src/screen.rs),
  [boards/cyd/src/lcd.rs](../boards/cyd/src/lcd.rs),
  [boards/cyd/src/core2.rs](../boards/cyd/src/core2.rs),
  [boards/cyd/src/main.rs](../boards/cyd/src/main.rs)
- [boards/cyd/send-plan/src/lib.rs](../boards/cyd/send-plan/src/lib.rs),
  [boards/cyd/send-plan/src/choose.rs](../boards/cyd/send-plan/src/choose.rs),
  [boards/cyd/send-plan/src/beam.rs](../boards/cyd/send-plan/src/beam.rs)
- [boards/cyd/handover/src/lib.rs](../boards/cyd/handover/src/lib.rs)
- [crates/console-video/README.md](../crates/console-video/README.md)
- [crates/game-3d/README.md](../crates/game-3d/README.md)
- [crates/game-api/src/file.rs](../crates/game-api/src/file.rs)
- [games/README.md](../games/README.md),
  [games/build-chip.sh](../games/build-chip.sh)
- [games/bench/README.md](../games/bench/README.md),
  [games/proto3d/README.md](../games/proto3d/README.md),
  [games/kestrel/README.md](../games/kestrel/README.md),
  [games/mercator/README.md](../games/mercator/README.md),
  [games/mercator/flash.list](../games/mercator/flash.list)
- [experiments/display-dma/README.md](../experiments/display-dma/README.md),
  [experiments/cyd-lab/README.md](../experiments/cyd-lab/README.md)
- [decisions](decisions.md), [the display](display.md)
