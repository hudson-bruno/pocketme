# pocketme-cyd-lab

The console on the CYD board: an ESP32 (rev 3.1, 4 MB flash, 520 KB RAM),
a 320x240 ST7789 panel, a resistive touch screen (XPT2046), a microSD slot
and one button (BOOT, GPIO0). It runs `console_core::Console` on the
board's parts. It cannot start games yet; the two game slots in RAM are
kept free for later.

## Build and flash

```sh
cd boards/cyd
export PATH="$HOME/.cargo/bin:$PATH"
. ~/export-esp.sh
cargo build --release --offline                       # default features
cargo build --release --offline --features sd-card    # the real card
espflash flash --chip esp32 --port /dev/ttyUSB0 --non-interactive \
    target/xtensa-esp32-none-elf/release/pocketme-cyd
```

Never run `espflash erase-flash`: the nvs partition holds a saved
gamepad pairing. This firmware writes nothing to flash and nothing to the
card.

`./check.sh` runs `cargo fmt --check` (this crate only), clippy on three
feature sets and a build of all 64 feature combinations. It never touches
the board.

## Features

| feature | default | what it switches |
|---|---|---|
| `demo-card` | on | Four made-up games built into the firmware (the same as boards/desktop's sample card). Read only. |
| `sd-card` | off | The real microSD card, **read only** (every write is refused before it reaches the card). With `demo-card` also on, the real card wins. With neither, the console sees "no card". |
| `touch` | on | The touch chip (CLK 25, MOSI 32, MISO 39, CS 33, IRQ 36). One log line per touch, raw values and panel pixels. The calibration is a first guess. |
| `link` | on | The debug link on the USB serial port: `boardctl shot`, `key`, `tap`, `stats`, `text`. Without it the port carries only the log. |
| `tour` | on | The tour demo (below). Off at start; started by a letter or a long press. Also makes BOOT a "click" (see below). |
| `wide-strips` | off | Measuring only: 2 x 15,360-byte strip buffers instead of 2 x 5,120, and the letter `b` runs the strip bench. |
| `radio-fit` | off | Measuring only: starts the Bluetooth radio (classic + Low Energy, the patched driver of experiments/bt-classic-probe) and does nothing else with it. **Its game code slot is 4,352 bytes smaller** (see "Does the radio fit?"), so no game could run on it. With `cpu1-cache-ram` the slot is the full 64 KB again. |
| `cpu1-cache-ram` | off | Lab only: uses the second core's cache RAM (32 KB at 0x4007_8000) as code RAM. See "Lab: the second core's cache RAM". Adds the letters `F`, `B`, `W`. |
| `cpu1-wipe-test` | off | Lab only, **meant to crash**: wipes the PHY code in the area after the copy, so starting the radio must stop with "illegal instruction". Proves the code really runs from there. |
| `core2` | off | Lab only: starts the second core (esp-rtos way) with a one-job box. Letters `P`, `G`, `H`, `D` (below). Cannot be combined with `cpu1-cache-ram`. See "Lab: the second core at work". |
| `core2-swap` | off | Lab only (turns on `core2`): the second core sends straight from the console's own picture (console-video's swap rule), with no copy box, and sleeps while it has no work. Adds the letter `V`. See "Lab: the second core without the third picture". |
| `fs-bench` | off | Lab only: letter `X` runs the full-screen scroll bench (about 4 min, one `bench` log line per case). |
| `cpu1-start-test` | off | Lab only (turns on `cpu1-cache-ram`): letters `S` and `E` show what starting the second core does to its cache RAM. One of the two per start of the board. |

## Using it

- `boardctl key right` (and the other buttons) moves the home screen
  selection. The link's buttons are a "cable pad": pad 0 shows as
  connected from the first press. Link A is the bottom face button.
- `boardctl shot -o FILE` takes the panel's own memory (`--source fb`
  takes the console's 160x120 picture).
- `boardctl stats`: features, page, selection, screen counters, heap,
  stack, and the time per scene.
- Letters (`boardctl text X`): `t` tour on/off, `m` print the
  measurements now, `r` reset them, `b` strip bench (`wide-strips`),
  `n`/`c` act as if the card were out / back in (`sd-card`, measuring
  only: it keeps the card's chip-select high).
- Letters with `cpu1-cache-ram`: `F` switches the background flash check
  on or off (on at start), `B` reads one byte in the area and `W` writes
  one: **both stop the board on purpose** (it restarts).
- Letters with `core2`: `P` pictures sent by the second core on/off,
  `G` game stand-in per frame 0 → 5 → 10 → 20 ms, `H` stand-in on the
  first or the second core, `D` console clock 1x/2x (2x asks 60 frames a
  second of the home screen). With `fs-bench`: `X` the bench.
- Letters with `core2-swap`: `V` the picture check on/off (the second
  core sums the handed picture before and after each send and counts
  any change). With `link`: `K` reads the whole panel back (about half
  a second) and counts the panel pixels that differ from the console's
  picture (`panel check` log line; 0 is right).
- BOOT: with `tour`, a short press reaches the console as one press when
  you let go; held 1 s, it starts or stops the tour. Without `tour`, the
  console sees the button as it is.
- The tour: five steps right, a notification, five steps left, another
  notification; one step every 0.7 s, round and round.
- Every 30 s the log gets the measurements (`perf`, `card`, `screen`,
  `mem` lines).

## How it works

- **Screen** (screen.rs, lcd.rs, beam.rs): the panel is written in
  columns (MADCTL 40h). At start the panel's refresh is measured by
  reading its line counter (command 45h) for 200 ms; each present then
  waits for the right moment so that no window is drawn over while the
  panel shows it (tear-free). The picture goes out in strips of 5,120
  bytes (2 buffers, DMA), filled while the previous one is on the wire.
- The SPI driver runs from RAM (`.cargo/esp-config.toml`). From flash,
  a cache miss stretched every counter read past 25 us in one build, and
  the timing could not be measured at all.
- **Console**: one `static` (46,112 bytes), never on the stack.
- **The loop** (app.rs): one task. It calls `Console::step`, then sleeps
  until the returned time, or until a byte on the serial port, or a press
  of BOOT or the touch screen. While a button or the panel is held it
  wakes every frame. A present takes up to about 21 ms and does not give
  way to other tasks while it runs: a radio will need its own esp-rtos
  thread or a higher-priority executor.
- **Card** (sd.rs): SPI3 (SCK 18, MOSI 23, MISO 19, CS 5), woken at
  400 kHz, read at 20 MHz, FAT 8.3 names, every handle closed before a
  call returns. Before the card library wakes a card, one "go idle"
  command checks that a card answers at all (16 bytes); the library alone
  would retry for seconds with no card in, every 2 s.
- **Log** (logbuf.rs): a 4 KB ring, emptied by the loop when the port is
  free; a line that does not fit is dropped and counted, never waited for.

## Numbers (all from the board's own log)

The log files named below are kept outside the repository.

Strip size (step2-bench-strips.log, 20 sends each, time on the wire):

| area | 2 x 5,120 B | 2 x 15,360 B |
|---|---|---|
| full screen 320x240 | 15,629 us | 15,480 us |
| small 72x72 | 1,101 us | 1,095 us |
| row 320x88 | 5,750 us | 5,709 us |

The small strips cost at most 1% and save 20,480 bytes of RAM: chosen.

Step and present times, default build (step7-boot.log, step7-tour.log):

| scene | frames | step avg / max | present avg / max | frames a second |
|---|---|---|---|---|
| boot animation | 39 | 2,101 / 17,669 us | 1,447 / 16,896 us | 29.99 |
| row scroll | 55 | 17,695 / 22,977 us | 15,690 / 20,935 us | 29.91 |
| notification sliding | 132 | 6,199 / 12,378 us | 4,539 / 10,341 us | 29.93 |
| selection moves | 21 | 10,508 / 14,377 us | 8,427 / 12,370 us | (single frames) |
| idle home, nothing moving | 11 in 20 s | 1,676 / 1,708 us | none | one step per 2 s |

Tour, 45 s: 231 presents, 700 windows, 0 sent without the timing, 0
late, 0 drawn over the panel's refresh.

Second check, 2026-11-17 (soak-*.log):

- 5 minutes on the idle home screen, then 5 minutes of tour, no restart
  (uptime 617 s at the end), no warning or error line, heap and deepest
  stack use (8,944) the same from start to end.
- Idle: 226 loop wakes in about 300 s (0.75 a second; 150 of them the
  2-second card check), `early` 0. `loop wakes` and `early` are in
  `boardctl stats` and the log.
- Tour, 300 s: scroll 18,443 / 25,230 us step and 16,407 / 23,159 us
  present, 29.91 frames a second; 4,277 windows, 0 untimed, 0 crossed,
  9 late (started after their start window, the model still says not
  crossed), 165 presents that needed more than one refresh pass.
- The 2-second check on the real card (no games): 4,426 / 4,705 us.

Card (step7-boot.log, step4-sd.log):

| | time |
|---|---|
| home scan, demo card (4 games) | 555 us |
| home scan, real card (no games on it) | 12,030 us, of which 7,887 us waking the card |
| 2-second check, demo card | 66 us |
| 2-second check, real card, no games | 4,444 us |
| 2-second check, no card | 1,272 us |

Memory, default build (`xtensa-esp32-elf-size -A`, mem log line):

| | bytes |
|---|---|
| flash image | 316,080 of 4,128,768 (7.7%) |
| `.text` (flash) | 207,057 |
| `.rodata` (flash) | 35,148 |
| `.vectors` + `.rwtext` (IRAM) | 1,024 + 16,900 |
| `.data` (DRAM, includes the console, 46,112) | 55,592 |
| `.bss` (DRAM) | 21,352 |
| stack (DRAM, the rest up to 0x3FFE0000) | 119,664; deepest use seen 8,960 |
| heap (DRAM 0x3FFE7E30 up to the data slot) | 49,616, none used |
| game code slot (IRAM 0x4009_0000) | 65,536 |
| game data slot (DRAM 0x3FFF_4000) | 49,152 |

What is left for a game beside the two slots: 47,612 bytes of IRAM
between the console's RAM code and the code slot; the 49,616-byte heap
(meant for the radio); and most of the stack (119,664 bytes set aside,
8,960 used so far).

## Does the radio fit? (radio-fit, measured 2026-11-17)

Not quite. With the radio, the code that must sit in executable RAM
(IRAM) is 4,220 bytes too big for the 64 KB game code slot. Everything
else fits.

Executable RAM, 0x40080000 to 0x40090000 (64 KB before the code slot):

| | without radio | with radio |
|---|---|---|
| `.vectors` | 1,024 | 1,024 |
| `.rwtext` (the console's RAM code, and the radio driver's Rust part) | 16,900 | 20,880 |
| `.rwtext.wifi` (the radio's libraries: btdm 33,086, phy 14,468) | 0 | 47,852 |
| total | 17,924 | 69,756 |
| left before the 64 KB code slot | 47,612 | **-4,220** |

A normal build with the radio does not link: the linker stops with the
"GAME SLOT" message. A `radio-fit` build therefore starts the code slot
at 0x4009_1100 (61,184 bytes); then 132 bytes are left.

Data RAM with the radio: the radio takes the first 64 KB of the main
data RAM for itself (esp-hal moves `.data` from 0x3FFB0000 to
0x3FFC0000). `.data` 58,008 + `.data.wifi` 1,012 + `.bss` 25,100; the
stack shrinks from 119,664 to 46,948 bytes (deepest use seen 8,944).
The heap (49,616) is unchanged: the radio takes 19,580 of it when it
starts, 30,036 stay free. The game data slot is untouched.

On the board (radio-*.log): the controller
starts in dual mode in 387 ms; the panel timing is measured as usual;
in a 60 s tour the row scroll took 16,105 / 21,913 us a present
(average / longest) against 15,690 / 20,935 without the radio, still
29.9 frames a second, 0 windows drawn over the refresh, 0 untimed.

Ways to close the 4,220-byte gap (not done; each is the owner's call):

| way | bytes it frees in IRAM | cost |
|---|---|---|
| smaller code slot | every byte the slot gives up; it must give up 4,220 or more (61,316 bytes at most; radio-fit uses 61,184). 60 KB (61,440) is 124 bytes too big | games get less code room; games are built for the slot's address, so the game interface changes |
| the radio's RAM code to flash | up to 47,852 (phy alone 5,450 by section kind) | slower radio interrupts; with flash being written (a saved pairing) the radio must be stopped first |
| the SPI driver back to flash | 2,756 (not enough alone) | the panel timing failed in one build that way |
| smaller screen strips | 0 | the strips are in data RAM, not in IRAM |

## Lab: the second core's cache RAM (`cpu1-cache-ram`, 2026-11-17)

**Result: it works, with limits.** With the radio on, the console and the
radio now fit beside the full 64 KB game code slot at 0x4009_0000, and
10,208 bytes of code RAM are still free. The price: the second core can
never be started while this is in use.

### What the area is

The ESP32 keeps 64 KB of RAM at 0x4007_0000 for its two flash caches
(the cache is the small fast memory that holds the parts of flash in use
right now). The first half belongs to the first core and is always busy.
The second half, 0x4007_8000 to 0x4008_0000 (32 KB), belongs to the
second core. Its cache only switches on when the second core is started.
This firmware never starts the second core, so that half is plain RAM.
Espressif's own SDK does the same thing in its "single core" setting: it
hands this RAM to the heap as code RAM.

Measured on the board at every start and every 30 s for 10 minutes: the
first core's cache on, the second core's cache off, the second core held
in reset with its clock off, cache mode 0 (the log line `cpu1 regs now`).

### Who copies the code there

Not the bootloader. The bootloader itself runs from this very RAM while
it loads the firmware (its log says `load:0x40078000`), and it refuses
anything that lands on it. So:

1. `cpu1-cache.x` links the code to **run** at 0x4007_8000 but **stores**
   it at the start of the game code slot (0x4009_0000), which no game
   uses at start-up.
2. espflash writes it into the image at that stored address; the
   bootloader loads it there like any other RAM code.
3. The firmware's first lines (`cpu1_cache::start`, before anything else
   runs) test the whole 32 KB with five patterns, copy the code word by
   word, check every word, and wipe the stored copy in the game slot.

A first try stored the copy in the heap's RAM (0x3FFE_7E30). The
bootloader refused it ("bad load address range") and the board restarted
over and over until it was flashed again (`lab-try1-heap-lma-boot.log`).

What is in the area: the radio's PHY library's RAM code (from libphy.a)
and one 60-byte test function: 14,584 bytes, 18,184 bytes still free.

### Numbers

The log files named below are kept outside the repository.

| check | result | file |
|---|---|---|
| pattern test, 32,768 bytes x 5 patterns | 0 bad words, every start | lab-a-b-boot.log, final-restart-*.log |
| test function, 10,000 rounds: area / normal RAM / flash | 160,984 / 160,982 / 163,854 cycles (flash 160,982 the second time) | lab-a-b-boot.log |
| copy check after copy | 0 bad words, every start | final-restart-*.log |
| wipe test (PHY code wiped on purpose) | radio start stops with "Illegal" instruction at 0x40078169, inside the area | lab-wipe-test.log |
| one byte read / one byte write in the area | both stop with LoadStoreError (cause 3): only whole 32-bit words work | byte-read.log, byte-write.log |
| 5 restarts | all came up; radio up in 373 / 384 / 378 / 384 / 386 ms, dual mode | final-restart-1..5.log |
| 5 min tour + 5 min idle, area checked every 30 s, 128 KB of flash read and checked all the time | no restart, no warning, area 59 checks 0 bad, flash 38 passes 0 bad, heap free 30,036 the whole time | soak1-*.log |

Code RAM (0x4008_0000 up to the game code slot), with the radio:

| | last round (radio-fit) | this lab |
|---|---|---|
| `.vectors` | 1,024 | 1,024 |
| `.rwtext` | 20,880 | 20,940 |
| `.rwtext.wifi` | 47,852 (btdm + phy) | 33,364 (btdm only) |
| total | 69,756 | 55,328 |
| game code slot | 61,184 at 0x4009_1100 | **65,536 at 0x4009_0000** |
| left | 132 | 10,208, plus 18,184 in the area |

Tour against last round's radio build (same board, same day, 5 minutes
each, about 4,550 windows each; control = last round's firmware,
the radio-fit build):

| | control, run 1 | control, run 2 | lab, flash check on | lab, flash check off |
|---|---|---|---|---|
| scroll present avg / max | 15,809 / 22,510 us | 16,438 / 22,253 us | 13,828 / 24,245 us | 14,478 / 23,976 us |
| scroll frames a second | 29.92 | 29.91 | 29.91 | 29.90 |
| windows late | 10 | 9 | 30 | 40 |
| windows drawn over the refresh ("crossed") | **0** | **0** | **6** | **10** |
| radio up | 397 ms | 389 ms | 370 ms | 373-390 ms (6 starts) |

Files: control-tour-stats.txt, control2-tour-stats.txt,
soak1-tour-stats.txt, nocheck-tour-stats.txt.

**The lab build draws over the panel's refresh a few times in 5 minutes
(6 and 10 of about 4,570 windows); last round's build never did (0 and
0).** Switching the background flash check off (letter `F`) did not
help, so the check is not the reason. The panel timing measured at each
start is the same in both builds (line 49.53-49.65 us). Why it happens
is not known yet: the lab's scroll presents are also about 2 ms shorter
on average, so something in how the present lines up with the panel
changed. Nobody has looked at the panel during these runs, so whether a
tear can be seen is not known. This has to be found before option C goes
into the console.

Could the game slot grow? By the linker's numbers only (not built): the
console and radio end at 0x4008_D820, so the slot could start at
0x4008_E000 and be 73,728 bytes (72 KB) instead of 64 KB. Moving part of
the radio's own link-layer code (btdm) into the 18,184 free bytes of the
area could free more, but that is the radio's most timing-critical code
and was not tried.

### How this can still break

- **Starting the second core.** esp-hal's `start_core1` (and esp-rtos's
  second-core start) switch the second core's cache on. The area then
  becomes cache again and the code in it is overwritten: a crash the
  next time the radio runs. With this feature, the second core must stay
  off, or run only from RAM with its cache kept off (that needs a change
  to esp-hal; not tried).
- **Code that runs before the copy.** Only code that first runs after
  `cpu1_cache::start` may go there: never interrupt handlers that could
  fire at start-up, never start-up code.
- **Whole words only.** Byte reads and writes stop the board. Code is
  fine; constants or data placed there are not, nor a byte-wise copy.
- **The stored copy's place.** It depends on the game slot being free
  at start-up and on espflash storing RAM-addressed sections as they
  are. A game loader must not expect the slot to hold anything at
  start-up (the firmware wipes it).
- **A debugger** (JTAG) may start or reset the second core and its
  cache.
- **Deep sleep** loses the area; a fresh start copies it again.
- **Tearing**: more windows drawn over the refresh than last round's
  build (see above); cause not found.

## Lab: the second core at work (`core2`, 2026-11-17)

**Result: it works, with limits.** The second core can send the
pictures while the first core runs the console and the radio. In the
5-minute tours with the radio on, the windows the timing model calls
"late" went from 18 to 23 to **0**, and the first core's work per frame
dropped from about 16 ms to about 2 ms. The price: about 28 KB of data
RAM, and it cannot be combined with `cpu1-cache-ram` (the build refuses
it with an error).

### How the second core starts (sources read: esp-rtos 0.4.0, esp-hal 1.2.2)

- `esp_rtos::start_second_core(CPU_CTRL, FROM_CPU_INTR1, stack, f)`:
  a stack of our choice (esp-rtos's own example uses 8 KB; here 8 KB).
  esp-hal's `start_core1` **flushes and switches on the second core's
  flash cache** first, then releases the core; its first instructions
  come from flash. esp-hal puts a guard word 60 bytes above the bottom of
  the stack and watches it (a stack overflow stops the board instead of
  silently breaking things).
- On the second core, esp-rtos sets up its task-switch interrupt
  (FROM_CPU_INTR1; the first core uses FROM_CPU_INTR0, because the
  Bluetooth driver takes Software0), marks both cores active, and makes
  `f` the second core's main task, **pinned to the second core**. The
  first core waits at most 1 s for it.
- Task placement: a pinned task stays on its core; an unpinned one goes
  to the core running the lower priority. esp-radio pins its own tasks to
  the first core, and the radio's interrupts are switched on in the core
  that starts the radio (the first). So the radio stays on the first core.

### Proof: the second core and `cpu1-cache-ram` exclude each other

Feature `cpu1-start-test`, 32 KB area at 0x4007_8000 filled with a
pattern, changed words counted (8,192 words in all):

| step | words changed |
|---|---|
| second core started by hand, runs a RAM-only loop, its cache OFF | 0 |
| its cache switched ON (loop still RAM-only) | 8,190 |
| pattern written again while the cache is on | 8,192 |
| second core stopped, cache off, pattern again | 0 |
| esp-rtos `start_second_core` (the normal way), right after | 8,191 |

So the area is lost as soon as the second core's cache is on, whatever
the core runs. A second core that runs only from RAM with its cache kept
off does run (proved above), but esp-hal always switches the cache on,
and any flash code, constant or jump table it touches needs the cache: a
trap for real work.

### Costs

| | without `core2` | with `core2` |
|---|---|---|
| data RAM (.data + .bss), fs-bench build | 104,688 | 132,908 (+28,220: picture box 19.8 KB + stack 8 KB) |
| main stack, radio-fit build | 38,460 | **10,232** |
| flash code (.text) | 224,001 | 235,713 (+11.7 KB) |
| code RAM (.rwtext) | 16,900 | 16,900 (+0) |
| start of the second core | | 299 to 335 us |
| second core's stack, deepest use | | 1,760 of 8,192 bytes (sending) |
| bench, everything on the first core (mode a) | same fps | same fps (the idle second core costs nothing) |

With the radio, the main stack is only 10,232 bytes and the console has
used about 8,900: **too tight** for a real game. `core2 + fs-bench +
radio-fit` does not even link. The second core waits for work by
spinning (full power); a real version should sleep until it is woken.

### The full-screen scroll bench (letter `X`, 10 s per case)

Every frame the whole 320x240 picture moves one pixel. fps at a game
stand-in of 0 / 5 / 10 / 20 ms per frame; crossed 0 and untimed 0 in
every case.

| asked | a: one core | b: game on the second core | c: sending on the second core |
|---|---|---|---|
| 30 | 30 / 30 / 30 / 27.3 | 30 / 30 / 30 / 30 | 30 / 30 / 30 / 30 |
| 60 | 58.7 / 46.3 / 37.6 / 27.3 | 58.6 / 58.6 / 58.6 / 48.2 | 58.7 / 58.8 / 58.7 / 48.1 |

58.7 is the panel's own refresh rate (lines 343.6 x 49.59 us). A
full-screen scroll at 60 asked already reaches it on one core when the
game does nothing; with 5 or 10 ms of game work, only two cores keep it.

### 5-minute tours with the radio on (30 asked unless said)

| build, letters | late | crossed | scroll fps | first core per frame (scroll) |
|---|---|---|---|---|
| radio only (no `core2`) | 23 | 2 | 29.90 | 16.3 ms |
| radio + `core2`, first core sends | 18 | 0 | 29.89 | 16.3 ms |
| radio + `core2`, `P` (second core sends) | **0** | 0 | 29.90 | 2.2 ms |
| + game stand-in 10 ms on the first core (`GG`) | 18 | 0 | 30.10 | 24.8 ms (max 35.9) |
| + stand-in 10 ms on the second core (`GGH`) | 22 | 1 | 29.89 | 16.4 ms |
| + stand-in 10 ms on the first, `P` (`GGP`) | **0** | 0 | 30.15 | 10.2 ms |
| 60 asked (`D`) | 21 | 0 | 56.80 | 15.1 ms |
| 60 asked, `P` (`DP`) | **0** | 0 | 59.63 | 3.2 ms |

"late" and "crossed" are what the timing model predicts, not tearing
seen on the panel. Last round's radio build had 9 and 10 late on the
same tour. One run each: the 0s are a strong hint, not a proof.

Evidence: the board logs (`*-bench.log`, `d-*`, `e-*`,
`cores-starttest-*.log`), kept outside the repository.

## Lab: the second core without the third picture (`core2-swap`, 2026-11-17)

**Result: it works, with limits.** The second core sends straight from
the console's own picture: no 19.8 KB picture box, and the second core
sleeps while it has nothing to do. With the radio on, the main stack is
**29,792 bytes** (copy version 10,224, one core 38,436), and 33,888 with
a 4 KB stack for the second core. Tours, benches and pixel checks gave
the same results as the copy version; no wrong pixel was found.

### How it works (console-video's swap rule)

The console keeps two pictures anyway (one to draw in, one that says
what the panel shows, for finding the changes). With `swap`, each
present: 1 finds the changes, 2 waits for the second core (`wait_free`),
3 hands the drawn picture over (`hand_over`), 4 copies the changed
rectangles into the other picture ("catch-up"), 5 swaps the two roles.
The console then draws in the other picture while the second core reads
the handed one.

| memory | first core | second core | what makes it so |
|---|---|---|---|
| handed picture (19,200 B) | only reads it (catch-up, shots); writes it again only after the next `wait_free` | reads it while the job box says "present" | console-video's `present_handing` order; `JOB` store Release / load Acquire |
| the other picture, the palette | reads and writes | never | the second core gets no pointer to them |
| colours on the wire (`CydScreen::wire`) | rebuilt in `hand_over`, while the second core is idle | reads it during the send | `hand_over` is only called after `wait_free` |
| `SLOT` (16 areas, pointer) | writes it while the box is empty | reads it while the box holds the job | same `JOB` flag |
| the screen (`CydScreen`, SPI) | uses it only when the box is empty (`idle()` before letters, shots, backlight) | uses it during the send | `screen.idle()` / `wait_free` |
| `COUNTERS` (copy of the screen counters) | reads a copy | writes a copy after each send | critical section |

Nothing in the hardware stops a broken rule. If the console wrote the
handed picture early, the panel would show a mix of two pictures. Letter
`V` checks exactly that (a sum of the picture when handed, before and
after each send); `K` reads the whole panel back and compares it.

**Waiting without spinning** (esp-rtos 0.4.0 read): the second core runs
an esp-rtos thread-mode embassy `Executor` with one task. With no job,
the task waits on an `AtomicWaker`; the executor's thread then sleeps in
the esp-rtos scheduler and the core runs the idle hook (`waiti`). The
first core's `wake()` reaches it through esp-rtos (`__pender` →
`resume_task` → the second core's software interrupt FROM_CPU_INTR1).
Proof that it does not spin: in every run the task was polled exactly
twice per job and the executor went idle exactly once per job (tour:
1,466 jobs, 2,932 polls, 1,466 idles; the copy version's spin counter
was at 252 million in the same tour). The first core still waits for the
second by spinning (`join`), only when it has to.

### Costs (same day, same settings)

| | one core | `core2` (copy) | `core2-swap` |
|---|---|---|---|
| data RAM (.data + .bss), fs-bench build | 104,928 | 133,148 | 114,100 (+8,192 stack, +980 other) |
| main stack, fs-bench build | 91,680 | 63,452 | 82,500 |
| main stack, radio-fit build | 38,436 | **10,224** | **29,792** (33,888 with a 4 KB second-core stack) |
| main stack deepest use, radio-fit, boot | 8,944 | not measurable (pattern gone) | 8,944 |
| `+ fs-bench + radio-fit` links? | yes | no | yes (main stack 9,784) |
| flash code (.text), radio-fit build | 375,685 | 387,577 (+11.9 KB) | 394,881 (+19.2 KB) |
| code RAM (.rwtext) | 20,880 | 20,880 | 20,880 |
| second core's stack, deepest use | | 1,760 of 8,192 | 1,728 (bench) / 1,792 (radio tour) of 8,192 |
| first core: hand-over time | | copy 54 us (last), max 88 | max 7 us (bench), 78 (tour, with a colour change) |
| first core: catch-up copy | | | 79-84 us avg, max 111 (full screen); tour: hand-over to end of step 111 us avg, max 293 |
| second core's wake-up delay | (spins) | (spins) | 9-25 us avg, max 46 |

Second core's stack: deepest use measured 1,792 bytes (above the 256
bytes of guard and start function not counted). **Proposal: 4,096
bytes** (2.3 times the deepest use). A 4 KB build
(`CYD_CORE2_STACK=4096`, measuring only) ran a 5-minute radio tour:
deepest use 1,792 again, no stop (table below). Not proven: deeper paths that were not run (a panic
message printed from the second core, a future game update on it).

### The full-screen scroll bench (letter `X`, 10 s per case)

fps at a game stand-in of 0 / 5 / 10 / 20 ms per frame. Swap: two runs
(`s` run 1 / run 2). untimed 0, crossed 0 and errors 0 in every case of
every build; "late" 0 to 2 per case in all builds.

| asked | one core: a | copy: c | swap: s |
|---|---|---|---|
| 30 | 29.99 / 29.99 / 29.99 / 27.37 | 30 / 30 / 30 / 30 | 30 / 30 / 30 / 30 (both runs) |
| 60 | 58.69 / 46.30 / 37.69 / 27.36 | 58.73 / 58.76 / 58.65 / 48.09 | 60.00 / 59.50 / 59.29 / 47.80 and 59.65 / 59.12 / 59.01 / 47.81 |

First core per frame (us) and worst gap between frames (us):

| asked, stand-in | one core a | copy c | swap s (run 2) |
|---|---|---|---|
| 30, 0 ms | 16,790 / 33,548 | 791 / 33,430 | 928 / 33,432 |
| 30, 10 ms | 26,800 / 33,487 | 10,793 / 33,400 | 10,929 / 33,451 |
| 60, 0 ms | 17,029 / 17,098 | 16,158 / 17,217 | 5,514 / 17,509 |
| 60, 10 ms | 26,527 / 28,007 | 16,943 / 17,214 | 15,338 / 17,509 |

At 60 asked the first core waits for the second (it is still sending the
last picture): swap, 0 / 5 / 10 ms: 201 / 405 / 435 waits in about 595
frames, 13.6 / 10.7 / 6.0 ms each. "First core per frame" includes those
waits (the copy version's too). Without them, swap's first core works
about 130 us more per frame than the copy version's (928 against 791 at
30 asked): the change finding and the catch-up, against the copy.

Not understood: swap reaches 59.0 to 60.0 fps at 60 asked, above the
panel's own refresh (58.7), where the copy version stops at 58.7. The
timing model counts no crossed window, but whether every picture was
seen whole was not checked.

### 5-minute tours with the radio on (30 asked, letters `P`)

| build | late | crossed | presents | scroll fps | first core per frame (scroll) |
|---|---|---|---|---|---|
| one core (run 1 / run 2) | 17 / 13 | 1 / 2 | 1,502 / 1,503 | 29.90 / 29.90 | 18.5 / 18.7 ms |
| `core2`, copy, `P` | 1 / 1 | 0 / 0 | 1,512 / 1,512 | 30.54 / 30.33 | 2.25 / 2.25 ms |
| `core2-swap`, `P` | 3 / 7 | 0 / 0 | 1,516 / 1,509 | 29.92 / 29.92 | 1.93 / 1.93 ms |
| `core2-swap`, `P`, second-core stack 4 KB (one run) | 2 | 0 | 1,516 | 29.92 | 1.95 ms |

In the tours the first core never had to wait for the second (0 waits in
1,466 hand-overs). Swap has a few more "late" windows than the copy
version (3 and 7 against 1 and 1); the cause is not known. "late" and
"crossed" are the timing model's guesses, not tearing seen on the panel.

### Pixels

- Five still positions (home, two steps right, `start` on a tile, again),
  for one core, copy and swap (`P` on): the console's picture (fb) at 2x
  against the panel read back: **0 differing pixels** in all 15. The
  panel shots of copy and swap against one core: **0 differing pixels**
  in all 10 (the status bar clock read 0:00 in all builds).
- Letter `K` (whole panel against the console's picture, between two
  steps): 0 differing pixels in all 15 checks above.
- What was compared: still pictures, not the moving tour (the tour is
  timed, so two builds are never at the same frame). See "Trying to
  break it" for checks during the tour.

### Trying to break it (radio build, `D` `P` `V`: 60 asked, stand-in 0 ms)

About 5 minutes of tour at 60 asked with the picture check on, and in
every round (20 rounds): a `K` panel check, a panel shot and an fb shot
(each shot and check stops the console for about half a second, after
the second core is done). Then 30 times the menu opened and closed
(`start`, `start`), 30 times `a` on a tile and `b`, and two more `K`.

| check | result |
|---|---|
| `K` panel checks | 22, all 0 differing pixels |
| picture check (`V`) | 1,582 sends checked, 0 changed during a send |
| shots | 40 taken, none failed |
| screen | 1,582 presents, late 1, crossed 0, untimed 0, errors 0 |
| first core waited for the second | 222 times of 1,582, 5.9 ms average, 17.0 ms longest |
| hang, guard stop, panic | none; the board answered `ping` at the end |

Not possible in this lab: starting STARDUST (the lab cannot start games;
`a` on a tile shows "GAMES CANNOT START YET"). The menu and the tile
presses stand in for it. A run before this one lost its `K` results (the
log was not read right after the letter; the script was fixed); its
other numbers were the same kind (1,542 presents, 0 changed, late 0).

Evidence: the board logs (`*-bench.log`, `*-tour.log`,
`*-tour-stats.txt`, `*-boot.log`, `shots/`, `break/`, `queue*.out`),
kept outside the repository.

## Second reader's check (2026-11-17)

Evidence: kept outside the repository. New letters in this
lab (only with `cpu1-cache-ram`): `R` reads 2 x 64 KB of flash through
the flash driver and then checks the area; `Z` wipes the radio's PHY
code in the area while everything runs (a test meant to break things).
The stand-in now also logs how much work it really did
(`standin rounds_per_ms`).

### Option C (`cpu1-cache-ram`)

| check | result | file |
|---|---|---|
| what sits at 0x4007_8000 | 14,584 bytes: the 60-byte test function and the radio's PHY RAM code (92 functions). Stored at 0x4009_0000 in the image, copied by the firmware | `readelf` / `nm` of lab-final.elf |
| copy the same as the build? | the area's checksum at start, 0x3909b103, is the one worked out on the PC from the ELF file, at all 10 starts | restarts10.out |
| 10 minutes of tour, radio on | 85 area checks, 0 changed; 65 passes over 128 KB of flash, 0 wrong; the cache registers the same in all 20 reports | soak-labfinal-tour.log |
| 10 restarts (reset button) | all came up; radio up in 372-392 ms | labfinal-restart-*.log |
| flash read through the driver (`R`, 3 times) | data right (same sum as the image file), area unchanged | r2-R.log |
| PHY code wiped while running (`Z`) | **the board went on running for 200 s**; the area check said "AREA CHANGED" within seconds | zr-3-after-Z.log |

What the `Z` test means: with the radio only switched on (no gamepad),
**no code in the area runs**. It runs once, while the radio starts. So a
long run without a crash proves nothing about the area: only the
checksum does, and the checksum does notice a change. With a gamepad
connected, the PHY's power and gain code may run often; that could not
be tested (no pairing in this lab).

Not tested: a restart after a crash or a watchdog (all restarts here
were the reset line), waking from sleep, writing flash.

Note for the code-RAM sums: the flash driver (needed later to keep a
gamepad pairing) puts 1,156 bytes of its own code into the code RAM
(`rwtext_end` 0x4008_D820 to 0x4008_DCA4).

"Crossed" windows (the model's guess at tearing), radio on:

| build | crossed | late | time |
|---|---|---|---|
| last round's firmware | 0, 0 | 10, 9 | 5 min each |
| this lab, radio only (no option C) | 2, 2 | 23, 17 | 5 min each |
| this lab, option C | 6, 10, 13 | 30, 40, 58 | 5, 5, 10 min |
| this lab, second core sends | 0, 0 | 0, 0 | 5 min each |

The option C build crosses more, but the `Z` test shows its area code
does not run during the tour. So the cause must be something else in
that build. One guess, not proven: the code that sends to the panel runs
from flash in every build (0x400F_xxxx), so its speed depends on where
the linker puts things.

### The second core

- Repeated: radio only, late 17 / crossed 2, first core 16.2 ms per
  scroll frame. Second core sends (`P`): late 0 / crossed 0, first core
  2.2 ms. Same as the first runs.
- Is the gain real? The stand-in runs for a set time, so a slower memory
  would not show in its time. Its work rate was measured instead: 218
  rounds per ms on the first core both when the first core sends and when
  the second one sends. No slowdown seen. With `GGP` "late" was 2, not
  0: the 0 is usual, not certain.
- A bug found and fixed: the console turns the backlight on right after
  sending a picture. With `P`, the second core could still be using the
  screen then. `backlight()` now waits for it first (`screen.rs`).
