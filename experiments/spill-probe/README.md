# Spill probe

Stage 1 lets a game's code run on past the 56 KB code slot into the top of
the data slot, seen from the instruction bus (the **spill**, from
0x400A0000; `game_api::layout::CODE_SPILL_ADDR`). This probe is the test
game for it. It is **not** in `games/`, never on a normal card, and the
six-places rule for new games does not apply.

**Status: run on the CYD on 2026-11-22; every step passed (Results
below).** The mirror order (instruction word I in the spill is data
word `0x40000000 - 4 - (I - 0x400A0000)`, reversed word by word) comes
from ESP-IDF and held on the board; the firmware checks it
at every start-up and refuses spill games when it is wrong.

## What it proves (on the board)

- Code in the spill runs: the self-check calls functions on both sides of
  0x400A0000 and one that crosses it, and the result must equal a sum the
  compiler worked out (`EXPECTED`).
- Data right below the spill and code in the spill do not disturb each
  other: a buffer that ends 28 bytes below the spill is written and
  checked every frame.
- Speed: the same loop run from the code slot and from the spill, with and
  without data reads (phases A to D).
- The console refuses the two bad files before it writes anything.

## Files

```
common/probe.rs     the game (shared source)
common/tests.rs     the PC tests (shared)
spill/              SPILL: spill of 9,344 bytes, data ending 28 bytes below it
spillmax/           SPILLMAX: spill of 40,560 bytes
tools/make_bad.py   makes SPILLBAD.BIN and SPILLBIG.BIN from SPILL.BIN
```

Each crate is a normal game (`no_std`, only `game-kit`, no unsafe, no heap,
no floating point). Its `lib.rs` sets `PAD`, `BUF_WORDS` and `GAP_BYTES`,
then includes `common/probe.rs`.

## What the game does

- **Bulk code.** `PAD` distinct functions `pad::<K>` (221 bytes each on
  the chip), never inlined, called through a table of function pointers
  (`TABLE`, a constant in the data slot).
- **Self-check**, at the start and once a second: every table entry is
  called with `SEED` and the results are folded into a sum, compared with
  `EXPECTED`. The table and `SEED` pass through `core::hint::black_box`,
  so the compiler cannot work the sum out ahead: the calls really go to
  the functions where they lie (the disassembly shows the `callx8` per
  entry). Log:
  `spill: fns=228 below=219 above=9 check=PASS sum=0x… expected=0x… data=PASS bad_words=0 fails=0`.
  Screen: a big PASS or FAIL and the same numbers.
- **Data.** `BUF`, a static of `BUF_WORDS` words in the zeroed data. Every
  frame the game checks the pattern it wrote the frame before and writes
  a new one (in every phase).
- **Speed phases**, 10 s each, in a loop, logged as
  `spill phase=A fn=0x… data=no rounds=120000`. The same loop body
  (16 copies) is run 120,000 rounds a frame:
  - A: the copy with the lowest address (code slot), no data reads;
  - B: the copy with the highest address (spill), no data reads;
  - C: B, reading `BUF` one word per round;
  - D: A, reading `BUF` one word per round.
  The console's `perf … step_avg_us` line gives the time per frame
  (serial letter `m`; `r` resets it at the start of a phase). Note: the
  game's own frame code (`update`, the drawing) also lies in the spill in
  both probes; only the loop's copy changes between phases.

Two differences from the stage 1 design:
- The buffer is a static, not part of the game's state: the game kit
  builds the state on the stack once at the start, and 36 KB would not fit
  there.
- The 16 copies of the loop have two names, `low::<1..8>` and
  `spin::<9..16>`. The linker happens to place a game's functions in the
  order of their mangled names, so `low` lands before the `pad` functions
  (in the code slot) and `spin` after them (in the spill). With one name
  all 16 landed in the spill. `K` starts at 1 so that every copy is the
  same size (with 0 the compiler drops one step).

## Build (PC)

Tests on the PC (the run-time sum equals the compiler's, the game passes
in the kit's test console), from each crate's folder, into an existing
build folder:

```bash
cargo +1.99.0 test --offline --locked --target-dir ../../../boards/desktop/target/check
```

Chip files, into their own folder so they never land in the firmware's
card tree:

```bash
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh
GAMES_TARGET=$PWD/boards/cyd/target/spill-probe games/build-chip.sh \
  experiments/spill-probe/spill experiments/spill-probe/spillmax
F=boards/cyd/target/spill-probe/chip-games/files
python3 experiments/spill-probe/tools/make_bad.py $F/slots.x $F/SPILL.BIN $F
```

The loader's tests load these files too (`crates/game-loader/tests/real_files.rs`,
`SPILL_FILES_DIR` or the folder above).

## Tuning (done on the PC, 2026-11-21)

- `PAD` sets the code size. The linker places, in order: the literals of
  all functions, `low`, the `pad` functions, `spin`, then the rest of the
  game (about 7.3 KB). One `pad` must cross 0x400A0000 (check with
  `xtensa-esp32-elf-nm -S -n -C SPILL.elf`); if the line falls between two,
  change `PAD` by one. Each `pad` costs 256 bytes (224 of code, 32 of
  literals).
- `BUF_WORDS` sets how far SPILL's data ends below the spill (build-chip.sh
  prints it as "room left"); `GAP_BYTES` records it.

| | PAD | BUF_WORDS | code | past the slot | data + zeroed | room left |
|---|---|---|---|---|---|---|
| SPILL | 228 | 9,080 | 66,688 | 9,344 | 39,780 | 28 |
| SPILLMAX | 350 | 256 | 97,904 | 40,560 | 4,972 | 3,620 |

(MEASURED ON THE PC, bytes. SPILL: `pad::<220>` at 0x4009FFD0, 221 bytes,
crosses the line; 9 of its 228 table entries lie in the spill, and 8 of
the 16 loop copies. SPILLMAX: `pad::<115>` crosses; 149 of 350 entries in
the spill.)

## SPILLOVER (one-off, the linker must refuse it)

`SPILL_PROBE_OVER=1` at build time makes `BUF` `GAP_BYTES / 4 + 1` words
bigger: SPILL's data then meets its spill by one word. The link must fail:

```bash
SPILL_PROBE_OVER=1 GAMES_TARGET=$PWD/boards/cyd/target/spill-probe \
  games/build-chip.sh experiments/spill-probe/spill
```

Result (2026-11-21): `GAME TOO BIG: its code past the code slot and its
data meet in the data slot.` from `games/chip/game.x`, bss ending at
0x3FFFDB84 against the spill's bottom at 0x3FFFDB80. Build the good files
again afterwards (the failed link leaves no ELF). Never use this switch
for a file that is put on a card.

## The bad files

`make_bad.py` writes two files with correct CRCs, so that only the sizes
are wrong:

- `SPILLBAD.BIN`: SPILL with its zeroed size raised so that spill + data +
  zeroed is 4 bytes more than the data slot. Expected notice: GAME TOO BIG
  FOR SLOTS.
- `SPILLBIG.BIN`: SPILL with its code 106,500 bytes (the code slot and
  the whole spill, plus 4). Expected notice: GAME CODE TOO BIG.

Both are refused before anything is written (the loader's tests check
this on the PC; the firmware logs `slots written: 0 code words, 0 data
bytes`).

## On the board

Put the four files on the demo card with the measuring switch
`CYD_DEMO_EXTRA="SPILL=…/SPILL.BIN;SPILLMAX=…/SPILLMAX.BIN;SPILLBAD=…;SPILLBIG=…"`
(never in a build handed over for use). The steps and the log lines to
expect are in `boards/cyd/README.md` ("Code past the code slot").

## Results

On the PC: see the table above. On the board (2026-11-22): SPILL and
SPILLMAX start and pass every check, SPILLBAD and SPILLBIG are refused
with nothing written, and code in the spill runs at the speed of code
in the slot (-0.07 % to +3.4 % over the four comparisons). The numbers are in
`boards/cyd/README.md` ("The probe on the board").
