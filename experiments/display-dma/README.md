# display-dma

Experiment firmware for the CYD board (ESP32-2432S028): the console's own
ST7789 driver, written straight on esp-hal's SPI with DMA (no mipidsi),
and the planned pipeline from a 160x120 one-byte screen to the 320x240
panel at 2x. It measures itself at start-up and prints every result as a
`bench=...` line of `key=value` pairs, then runs a demo (a bouncing
sprite sent as small changed rectangles, every 5 s switching to a
full-screen scroll, frame rate printed) and answers the debug link
(`tools/boardctl`).

| file | what |
|---|---|
| `src/lcd.rs` | the driver: init, window commands through the FIFO, pixels by DMA from two strip buffers taking turns, panel read-back |
| `src/pipeline.rs` | framebuffer, palette as ready-to-send 32-bit words, 16- and 12-bit conversion with doubling, rectangles |
| `src/dirty.rs` | comparing two screens into at most 8 rectangles |
| `src/bench.rs` | timing runs (full frames, rectangles, CPU-only work, colour checks by read-back) |
| `src/check.rs` | the clock check: known pictures sent, read back slowly, every pixel compared, with a planted fault that must be found |
| `src/scan.rs` | Get Scanline (45h) and the frame-rate register (FRCTRL2, C6h) |
| `src/beam.rs` | where the panel is refreshing: survey of the scanline counter, a model of the refresh, the tearing characterisation |
| `src/tear.rs` | sending without tearing (slices timed from the scanline), the old way kept, tear counters |
| `src/ble.rs` | Bluetooth advertising, only with `--features ble` |

## Build

```sh
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh
cd experiments/display-dma
cargo build --release                                            # target/
CARGO_TARGET_DIR=target-ble cargo build --release --features ble  # target-ble/
```

## Run on the shared board

`board-session.sh` does one locked session: flash, 26 s of start-up log
(the measurements, then the tearing characterisation with its read test:
`ready` comes at about 19 s), then optionally press a button (held) and
listen; `-` as the button listens without pressing one, and a button
ending in `*N` (N 1 to 9, quoted for the shell: `'up*3'`) is pressed and
let go N-1 times, then pressed a last time and held. Flashing, the 26 s
and the listen time must stay under the session's 154 s timeout.

```sh
./board-session.sh logs/boot.log                  # start-up measurements
./board-session.sh logs/soak-80-16.log b 100      # 90 s check, 80 MHz 16-bit
./board-session.sh logs/soak-80-12.log x 100      # 90 s check, 80 MHz 12-bit
./board-session.sh logs/soak-40-16.log y 100      # 90 s check, 40 MHz (control)
ELF=$PWD/target-ble/xtensa-esp32-none-elf/release/display-dma \
  ./board-session.sh logs/ble.log r 30            # Bluetooth build
```

Buttons (`boardctl key <name>`): select = demo / still test card,
start = 16 / 12-bit colour, a = run the measurements again, b / x / y =
the checks above, l = scanline test and tearing characterisation, r = demo
frame cap 60 fps -> 30 fps -> none -> 60 fps (logged as `demo: cap=..`,
in `boardctl stats` as `demo_cap`), up / down / left / right = tearing
modes (see "Tearing" below).

Screenshots work as in screen-mirror; in card mode the panel equals the
framebuffer at 2x:

```sh
B=../../tools/boardctl/target/release/boardctl; L=../../.local/board.lock
flock $L timeout 150 $B key select
flock $L timeout 150 $B shot --source fb -o fb.png
flock $L timeout 150 $B shot --source panel -o panel.png
$B compare fb.png panel.png --scale-expected 2
```

## Tearing

### Why the scroll tears

The panel copies its memory to the glass one gate line at a time, 320
gate lines, then rests for about 1 ms (the porches), about 59 times a
second. In landscape one gate line is one **column** of our picture, so
the refresh sweeps **sideways** across the screen (about 50 us a column).
We send **rows**, top to bottom, and every row touches all 320 columns.
So a refresh that happens during a write shows the new picture on one
side of a slanted line and the old one on the other. A full frame takes
11.7 to 31 ms and the rest is only about 1 ms: sending rows tears on
every frame, whatever the timing. (Data sheet: 8.12, 9.1.28 MADCTL, 8.15.)

### What was built

* **Slices** (`tear.rs`). The frame is cut into 10 slices of 16 logical
  columns (32 panel columns), each sent as its own window, rows top to
  bottom inside it, in the order the sweep visits them. MADCTL stays 60h,
  so read-back, screenshots and the checks are unchanged.
* **Locked timing.** One Get Scanline read per frame (the "anchor") fixes
  a model of the sweep (`beam.rs`). A slice is started only after the
  sweep has left it, and must end before the next sweep comes back.
  Then one refresh shows all of the old frame and the next all of the new
  one. At 80 MHz a slice takes about 1.7 ms (1.56 ms on the wire plus
  118 us per piece, measured) and the sweep needs about 1.6 ms for 32
  columns, so the writer follows the sweep, a little slower. Changed
  rectangles (the sprite) are cut and timed the same way.
* **The anchor read** (`Beam::anchor`). One throwaway read comes first
  (`ANCHOR_WARM_UP`, about 22 us a frame), then up to 2 tries 300 us
  apart (6 when there is no recent anchor, enough to get past the 1.3 ms
  where the counter holds or sits at an edge). A try is rejected when the
  read fails, when its timed part is stretched past 25 us, or when the
  counter is at an untrusted value; each reason is counted. When no try
  works, an anchor at most 60 ms old is used again (`anchor_reused`) and
  the frame is still planned and timed; only without one is it sent
  untimed (`no_anchor`).
* **Columns** (`cols.rs`, `Way::Columns`; written, **not yet run on the
  board**). MADCTL 40h (MV=0) instead of 60h: the interface then fills
  one gate line (one picture column) before moving to the next (data
  sheet p124: with MV=0 X increments first; p215: D7 MY, D6 MX, D5 MV).
  The picture is turned in software (`pipeline::fill_cols`, in RAM, 16
  and 12 bit) and a window is sent as one write whose gate lines go in
  the sweep's order. The writer then moves with the sweep (80/16: 0.98
  line times a gate line, the sweep 1), so a whole frame has one start
  window of about 0.7 to 16.6 ms after the sweep passes column 0
  (`tear::cols_window` has the derivation; 8 guard lines). One window per
  changed rectangle, all in one pass (`plan_cols`), no slicing. Direction
  B flips MY and sends the columns right to left; there is no constant
  for a row mirror, since that would run the writer against the sweep.
  MADCTL goes back to 60h for every other draw and read; the `Lcd`
  remembers the value and sends 36h only when it changes, and forgets it
  after an error, a reset or a raw 36h.
* **The orientation check** (`cols::check`, `bench=tear_orient`), at
  start-up before the tearing checks: three coloured marks near three
  corners are written under 60h, read back under 60h (the control), then
  read back at the transposed addresses under 00h, 40h, 80h and C0h, each
  with and without each mirror. The columns way runs only if the mapping
  it would use (40h for A, C0h with rows mirrored for B) returned all
  three marks; else its frames go as `slices_locked`, counted in
  `cols_refused` and said once in the log. About 25 ms.
* **Four ways of sending**, to compare (UP button cycles them):
  `slices_locked` (at start) -> `rows` (today's, free running) ->
  `slices_free` (slices, no timing: the order alone) -> `columns` -> ...
  The first three keep the order they had before the columns way, so one
  UP still gives rows; columns is three presses (`'up*3'`).
* **Tear counters**, the same for every way, printed every 5 s with the
  frame rate as a `bench=tear` line:
  * *predicted* (always on, free): each piece's start and end times put on
    the model say whether the sweep passed over the piece's columns while
    it was written (a crossing). A frame is torn if any piece crossed, or
    if its pieces land in different refreshes (split);
  * *measured* (RIGHT button): a real scanline read just before and after
    each piece. Only the guess of which counter value is which column is
    assumed. Costs 2 reads (24-83 us each) a piece. For rows and slices a
    frame is torn here only when a piece crossed; for columns a split
    (windows first shown by different refreshes) counts too.
  * For columns both counters place the writer from its first pixel byte
    to the end of its last transfer (two cycle stamps taken in
    `Lcd::stream`), moving evenly between them (the strip gaps are
    assumed away). The measured one allows for a counter value c meaning
    the sweep is anywhere in line c..c+1, on the side where that matters.
* **Characterisation** (`beam::characterise`), run at start-up before the
  demo and again with `l`: the refresh at three rates, the porches, a
  scanline read in the middle of a write, the model, predicted start
  windows, the real cost of slicing, 30 frames of each way with both
  counters, and the read test (`bench=tear_reads`, below). About 9 s;
  the orientation check, the columns windows, the columns cost and its
  30 selftest frames add about 1.3 s (estimate, not yet measured).
* **Read test** (`beam::test_reads`): is a 45h read right when taken just
  after pixel data, and does the straight-line model hold over one
  frame? Each round takes a fresh anchor, does one thing, then one read,
  and compares its counter value with the model's prediction at the
  read's time stamp. Conditions: `idle` (a pseudo-random 0.2 to 18 ms
  wait, nothing on the bus, 200 rounds), `after_slice` (one full-height
  16-wide slice at a pseudo-random place, read at once, 300 rounds),
  `after_rows` (one full frame the old way, read at once, 100 rounds),
  `after_slice_wait` (a slice, then 200 us, 300 rounds). About 5 s; the
  rounds are cut from 300 for the slow conditions to keep it so.
* **Demo scenes** (DOWN button): `Auto` (sprite and scroll, as before) ->
  `Bars` (black and white bars, 3 logical pixels a frame: vertical bars
  moving sideways at the top, lying bars moving up in the middle, slanted
  bars at the bottom; a tear is a step in the bars) -> `Flash` (the whole
  screen black, then grey, every frame; a tear is one edge between them).
  Two small squares at the top left show the mode: way (red rows, yellow
  slices free, green slices locked, magenta columns) and direction
  (white A, blue B).

### How to run

```sh
./board-session.sh logs/tear-boot.log                 # characterisation (selftest lines)
./board-session.sh logs/tear-demo.log - 30            # no button: Auto scene, sprite and scroll at the 60 cap
./board-session.sh logs/tear-cap30.log r 30           # R: the 30 fps cap, still the Auto scene
./board-session.sh logs/tear-rows.log up 30           # UP once: rows (today's way)
./board-session.sh logs/tear-free.log 'up*2' 30       # UP twice: slices_free
./board-session.sh logs/tear-cols.log 'up*3' 30       # UP three times: columns (not yet run on the board)
./board-session.sh logs/tear-measure.log right 30     # RIGHT: measured counter on (locked)
./board-session.sh logs/tear-dir.log left 30          # LEFT: direction B
```

By eye (a person at the board), with `boardctl key <name>`:

1. `down` until the scene is `Bars`. Green square = locked: the bars
   should move without steps.
2. `up`: red square, today's way: steps along a slanted line should
   show. `up` twice more: magenta square, columns: the bars should move
   without steps, as with green.
3. **Direction test, once**: back to green, then `left` (blue square, B)
   and `left` again (white, A). In the right direction the bars stay
   clean; in the wrong one every frame tears (the writer runs against
   the sweep). The firmware cannot tell which is right: if B is the
   clean one, set `beam::REVERSED_DEFAULT = true`.
4. `Flash` is the hardest test: any tear is a sharp edge across a flat
   colour.

### Log lines

| line | what it says | what to look for |
|---|---|---|
| `bench=tear_scan frctrl2=..` | 45h read back to back for 200 ms at 00h, 1Eh, 0Fh: `period_us`, `jitter_us` (longest minus shortest refresh, +-1 read), `line_us` (fitted time of one counter step), `lines_per_refresh` (period / line), `values_per_refresh` (counter values seen in one refresh), `held_line`/`held_us`/`held_lines` (the value held longest: a porch the counter does not count), `read_us_*` (one whole read), `inner_us_*` (command to last answer byte) | `lines_per_refresh` about 344 (data sheet) or 334 (counter range). If 344 while `values_per_refresh` is about 333, `held_lines` should be about 10. |
| `bench=tear_porch bpa=.. fpa=..` | the same with 32 back or front porch lines instead of 12 | `lines_per_refresh` should grow by 20; whether `line_max` or `held_lines` grows tells which porch the counter counts |
| `bench=tear_ramwrc` | one frame written as one memory write, paused after every strip for a 45h read and resumed with 3Ch, then read back | `wrong=0`: a read between pieces of a write is safe. `lines_read` shows the sweep moving during a row-order write |
| `bench=tear_orient` | the orientation check (not yet run on the board): `control_ok` (the marks read back under 60h), `errors` (failed reads), `matches` (each MADCTL value with the mirror pairs that returned all marks, `none` if no pair did), `use` (the value that needs no mirror), `configured` (`cols::MADCTL_COLS`), `confirmed_a`/`confirmed_b` (the mapping each direction uses), `madctl_now` (must be `0x60`) | `use=0x40 confirmed_a=true confirmed_b=true madctl_now=0x60`. If `use` is another value, set `MADCTL_COLS` to it; if no value matches, the way stays off |
| `bench=tear_model` | the model used: line time, lines, period, porch lines, first visible line (a guess), direction (a guess) | `calibrated=true` |
| `bench=tear_window mhz=.. depth=..` | the plan run for every start position of the sweep, for a full sliced frame: `fits_pct` (frames that can be placed at all), `no_wait_pct` (start at once), `wait_us_*`, `frame_us_avg` | with `JOB_OVERHEAD_US` 60 the board printed 80/16 fits 100%, no wait 67%. With 118, the same arithmetic on the PC (board's model: 49.53 us, 343.45 lines) gives: 80/16 `piece_us=1677.04 busy_us=16770.40 fits_pct=100 no_wait_pct=64`, wait 1088 us average, 6065 us at most, frame 17858 us average; 80/12 100% and 55%; 40/16 0% (cannot work: the writer is half the sweep's speed); 40/12 100% and 18%. Not yet seen on the board |
| `bench=tear_window_cols mhz=.. depth=..` | the same for the columns way, one full-frame window: `per_gate` (line times the wire takes a gate line), `gates`, `lo_us`/`hi_us`/`window_us` (start window after the sweep passes the first gate line), `lead_us`, `busy_us`, the plan figures as above, `sprite_per_gate`, `sprite_lo_us`, `sprite_hi_us` (a 16x16 logical sprite), `guard_lines`, `refresh_us` | not yet run on the board. Host arithmetic (49.53 us a line, 344 lines): 80/16 0.70..16.59 ms, 80/12 4.59..16.61 ms, 40/16 0.45..1.26 ms, 40/12 0.45..9.06 ms, sprite 1.78..16.64 ms |
| `bench=tear_cols` | a full frame sent the columns way, free (no anchor, no wait), against `rows_us`, `slices_us` from the line above: `cols_us`, `cols_us_max`, `wire_floor_us`, `cols_extra_us` (columns minus rows), `rows_fill_us` and `cols_fill_us` (CPU filling strips), `cols_window_us` (window commands with MADCTL), `madctl_sends` (should be 1: only the first frame changes it), `errors`. `refused=1` when the orientation check did not confirm | not yet run on the board. Expected `cols_fill_us` near `rows_fill_us` (about 1.4 to 1.6 ms against 1.37 ms) |
| `bench=tear_slices` | today's full frame against the sliced one, both free running | `per_job_overhead_us`: what `tear::JOB_OVERHEAD_US` should be (118 measured, now set). It leaves out the wire time and the anchor as timed in the run (`anchor_us_avg`, last field: the throwaway read, the tries and their waits); the 118 left out one read (`anchor_read_us`) instead |
| `bench=tear_reads cond=..` | the read test, one line per condition (at the end: `bad_bits`, checked reads with any of the top 6 bits set, left out; `anchor_bad_bits` and `anchor_rej_bits`, the same in the anchor calls before each read, and the anchor tries rejected for it, so a `no_anchor` caused by misreads shows): `rounds`, `n` (reads compared), `skipped` (untrusted counter value), `failed` (read failed or stretched), `no_anchor` (no fresh anchor in 4 anchor calls), `err_mean_lines` (signed, read minus model), `err_abs_mean_lines`, `err_abs_max_lines`, counts of the error in bins `lt1 lt2 lt3 lt6 lt12 lt24 ge24` (lines), `us_since_anchor_avg`/`_max`, `ms` | if `idle` is good (most under 1 to 2 lines) but `after_slice`/`after_rows` are not, a read right after pixel data is wrong, and `after_slice_wait` tells whether a pause helps. If `idle` is bad too, the model drifts (or its anchor is bad) |
| `bench=tear_read_sample cond=..` | up to 8 raw reads a condition: `line` (counter read), `model` (predicted counter value, 0 to lines), `us_since_anchor`, `err_lines`, `kind` (`first`: the first 4 compared, `worst`: the 4 largest errors after them) | whether the bad reads share a value (for example near the held value or an edge) |
| `bench=tear ... scene=selftest` | 30 back-to-back full frames of each way, both counters on (`columns` too, not yet run) | `columns`: both 0, `late_pieces=0`, `cols_refused=0`. `rows`: `torn_pct=100` and `meas_torn_pct=100`. `slices_locked`: both 0, `late_pieces=0`. `slices_free`: measured 0 and 0 in the first run (back-to-back frames happened to follow the sweep); free running, so not sure to stay so |
| `bench=tear ... scene=sprite/scroll/bars/flash` | the demo, every 5 s, after the `bench=demo` line | see below |
| `bench=demo` | the demo's frame rate and costs every 5 s; `capped` is true at either cap, `cap_fps` (last field: `none`, `60`, `30`) says which | |

Fields of `bench=tear`:

* `way`, `scene`, `dir` (A left to right, B right to left), `measure`,
  `fps`, `frames`;
* `pieces` (windows sent), `crossings` and `crossings_per_frame`
  (predicted), `torn_frames` and `torn_pct` (predicted: crossing or
  split), `split_frames`;
* `meas_frames`, `meas_crossings`, `meas_torn_frames`, `meas_torn_pct`:
  the same from real reads (only with `measure=true`); `bracket_fails`
  are reads that an interrupt stretched;
* `no_window`: locked frames sent untimed (free): `no_anchor` (no fresh
  anchor and no recent one) plus `no_fit` (the plan found no pass);
  `late_pieces`: pieces that started too late to finish before the sweep
  came back (should be 0);
* `wait_us_avg`/`wait_us_max`: time spent waiting for the sweep a frame;
* `min_gap_lines`: the closest the sweep came to a piece that it did not
  cross (1 line = 1 panel column = about 50 us);
* `anchor_err_max_lines`: how far the last frame's model was from this
  frame's anchor read (drift over one frame; over up to 60 ms when the
  last anchor was a reused one or the frame rate is low; not counted
  after a longer pause); `bracket_err_max_lines`
  and `bracket_err_over3`: model against the measured reads (should be
  under 3 lines);
* `anchor_retries`: tries beyond the first; `anchor_fails`: frames where
  no try gave a trusted read (reused or not);
* `total_*`: since start, for frames really sent this way (a columns
  frame refused and sent as slices_locked is in slices_locked's totals).

Fields added at the end of `bench=tear` (the ones above keep their
order):

* `anchor_rej_read`, `anchor_rej_inner`, `anchor_rej_value`: anchor
  tries rejected because the read failed, because its timed part took
  longer than 25 us, or because the counter was at an untrusted value
  (held or at an edge); a try both long and untrusted counts as long;
* `anchor_first_inner_long`: frames whose first try was too long;
  `anchor_inner_us_max`: the longest timed part of any try;
* `anchor_warm` (whether the throwaway read is on), `anchor_warm_long`
  (throwaway reads that were too long themselves) and
  `anchor_warm_inner_us_max`;
* `anchor_err_n`, `anchor_err_mean_abs_lines`, `anchor_err_over1`,
  `anchor_err_over3`: the anchor error (see `anchor_err_max_lines`):
  how many, mean of its size, how many above 1 and 3 lines;
* `anchor_reused`: frames that used a recent anchor again; `no_anchor`,
  `no_fit`: the two parts of `no_window` (see above);
* `bracket_before_over3`, `bracket_before_max_lines`,
  `bracket_after_over3`, `bracket_after_max_lines`: the bracket errors
  split by the read before a piece and the read after it;
* `unscored_frames`: frames with no usable anchor (no fresh one, no
  recent one). The model is too old to say whether they tore, so they
  are left out of `torn_frames`, `crossings`, `min_gap_lines` and the
  bracket errors (but counted in `frames`, so `torn_pct` counts them as
  clean); the measured counter still scores them;
* `anchor_us_avg`: time spent in the anchor a frame (throwaway read,
  tries and waits).

Added after those, for the columns way and the read hygiene (not yet
run on the board):

* `cols_refused`: frames asked for as `columns` but sent as
  `slices_locked`, because the orientation check did not confirm the
  mapping (in this period's counters under `way=columns`, but in
  slices_locked's `total_*`);
* `cross_writer`, `cross_sweep`: predicted crossings split by kind (any
  way's crossings stay in `crossings`; these two only for columns): the
  writer ran into a sweep ahead of it (started too soon), or a sweep ran
  into the writer (started too late or the writer is slower);
* `meas_cross_writer`, `meas_cross_sweep`: the same from the real reads,
  with the writer's first and last pixel byte placed on the sweep's
  measured path;
* `meas_untrusted`: measured pieces where a bracket read was at an
  untrusted counter value (any way);
* `madctl_sends`: 36h commands sent in the period (columns alternates
  40h and 60h only when something else draws in between);
* `anchor_bad_bits`, `anchor_rej_bits`: anchor reads (throwaway and
  tries) with any of the top 6 bits of the 45h answer set, and the tries
  rejected for it. The data sheet (p231) has the counter in the low 10
  bits;
* `meas_split_frames` (columns only): measured frames whose windows were
  first shown by different refreshes, every window's pass put on one
  numbering from the time between the reads; these frames are in
  `meas_torn_frames` too;
* `cols_lead_us_max`, `cols_tail_us_max` (columns only): the longest
  time from the call to the first pixel byte and from the last byte to
  the return; what `COLS_LEAD_US` (35) and `COLS_TAIL_US` (10) guess;
* `total_cols_refused`: `cols_refused` since start (only under
  `way=columns`).

Open questions these fields are for (from the first run; not answered
yet):

* Hypothesis, not proven: the scroll scene's torn frames (all
  `no_window`) are failed anchors, not frames that do not fit (the plan
  fits 100% at 80/16). The frame loop follows the sweep, so a frame
  start keeps landing in the blanking, where the counter holds 1 for
  about 23.5 lines. If so, `no_fit` stays 0, `anchor_reused` takes
  their place and `no_window` drops.
* Hypothesis, not proven: the anchor's first try in the demo loop is
  rejected for a long timed part (the read's code fetched from flash
  after other code pushed it out of the cache). If so, without the
  throwaway read `anchor_first_inner_long` is close to the frame count,
  and with it close to 0 while `anchor_warm_long` takes the count.
* Half of the bracket reads are more than 3 lines from the model: the
  `before`/`after` split and `bench=tear_reads` say whether reads after
  pixel data are wrong or the model is.

Success: in the scroll, bars and flash scenes at 80 MHz, `rows` shows
`torn_pct=100`, `slices_locked` shows `torn_pct=0` (and `meas_torn_pct=0`
with `measure=true`), `late_pieces=0`, at about 55-58 fps; and the
person at the board sees no steps in the bars only in the direction the
counters were run with.

### Numbers to tune

| constant | file | what |
|---|---|---|
| `FIRST_VISIBLE_LINE` = 13 | `beam.rs` | counter value of the first gate line (data sheet guess: 1 + back porch 12) |
| `REVERSED_DEFAULT` = false | `beam.rs` | sweep direction; set from the eye test |
| `SLACK_LINES` = 16 | `tear.rs` | extra margin for the guess above; 0 once confirmed |
| `GUARD_LINES` = 4 | `tear.rs` | margin for model errors |
| `JOB_OVERHEAD_US` = 118 | `tear.rs` | per-piece cost beyond the wire; measured by `bench=tear_slices` |
| `WIRE_SLOWDOWN` = 1.015 | `tear.rs` | measured wire time over bytes x 8 / MHz |
| `SLICE_LW` = 16 | `tear.rs` | slice width (logical pixels): narrower = wider window, more overhead |
| `ANCHOR_WARM_UP` = true | `beam.rs` | throwaway read before the anchor (about 22 us a frame); false to compare |
| `ANCHOR_TRIES` = 2, `ANCHOR_TRIES_COLD` = 6, `ANCHOR_RETRY_US` = 300 | `beam.rs` | anchor tries with / without a recent anchor, and the wait between them |
| `ANCHOR_REUSE_MS` = 60 | `beam.rs` | how old an anchor may be to stand in for a failed one |
| `SURVEY_MS`, `HELD_LINES`, `MAX_INNER_US` | `beam.rs` | survey and anchor details |
| `MADCTL_COLS` = 40h, `MIRROR_C` = false | `cols.rs` | the columns way's MADCTL; set from `bench=tear_orient` (`use=`) |
| `COLS_GUARD_LINES` = 8 | `tear.rs` | margin each side of the columns start window |
| `COLS_LEAD_US` = 35, `COLS_TAIL_US` = 10 | `tear.rs` | columns way: call to first pixel byte (window commands, MADCTL, first strip of one column), and after the last byte |
| `FIRST_STRIP_BYTES` = 1024, `SECOND_STRIP_BYTES` = 4096 | `cols.rs` | the columns way's first two strips, small so the wire starts early |

### Left out on purpose

* Column order is now built (the `columns` way above, MADCTL 40h, not
  yet run on the board). Left out: read-back and screenshots under 40h
  (every read still goes under 60h), and making it the way at start
  (`WAY_AT_START` stays `slices_locked` until the board has run it).
* Locking the game's frame clock to the panel (every refresh or every
  second one): the sender waits for its window, so it is already tear
  free; the demo still uses its own timer (16.7 ms, or 33.3 ms at
  the 30 fps cap), so some refreshes repeat a frame (judder, not tearing).
* RTNA/porch tuning for exactly 60 Hz or 30 Hz, the long-porch fallback,
  hardware scrolling, an alpha-beta filter on the model (the model is
  simply re-anchored every frame), the tear-placement eye test for the
  first visible line.
* 40 MHz 16-bit cannot be tear-free with slices at 59 Hz (the writer is
  half the sweep's speed); the working clock is 80 MHz.
