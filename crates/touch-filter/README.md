# touch-filter

The CYD's touch filter: it turns the resistive panel's readings into
the point the console sees. It was `boards/cyd/src/touchfilt.rs` until
round 5. It moved here so that the PC tests can run it
(`tests/filter.rs`).

No `std`, no heap, no `unsafe`. It has no pins. The chip's readings and
the debug link's scripted finger both go through it.

## What it does

The full rules are in `src/lib.rs`. In short:

| rule | constant | value |
|---|---|---|
| A touch starts at this pressure... | `Z_ON` | 300 |
| ...and goes on down to this one | `Z_OFF` | 180 |
| Light readings in a row that keep the touch (last point held) | `HOLD_READS` | 2 |
| Dead band at rest, panel px | `DEAD_PX` | 3 |
| Readings near the point that make a moving finger rest again | `REST_READS` | 3 |
| Firm readings in a row outside the band that make a resting finger move | `CONFIRM_READS` | 2 (new in round 5; it was 1) |

When the panel's interrupt line goes high, the touch ends at once.

## Tuning (round 5, from the owner's finger test)

The source is the owner's finger test log: 278 real touches in two
sessions. The log, the scripts and their output are kept outside the
repository.

### What a real finger did

- **Lost readings (drop-outs).** 93 light readings out of about 4,841
  (1.9%), spread over 74 touches. A touch had at most 3, and never more
  than 2 in a row: `dropout_ends` is 0. Every touch ended because the
  line went high, never because of pressure.
- **Resting jitter.** About 1.0 px on average. The largest was 3 px,
  which is also where the dead band starts. There were 303 dead band
  break-outs. Only 13 of them (4%) came back to rest at once (jitter
  that got through).
- **Jumps.** Jumps of 20 px or more happened only in drags and flicks.
  The largest was 166 px, in a flick that also had 3 drop-outs.
  **Short "drags" were mostly one jump.** 34 drags or flicks moved less
  than 24 px. In 28 of them, one jump between two readings was at least
  half of the whole travel, and 24 lasted under 150 ms. For example, 13
  "drags" were read only 2 or 3 times, moved 7 to 15 px, and lasted 9
  to 82 ms. Their single jump was their whole travel. These look like
  taps that one bad reading turned into a drag.
- **Taps.** 150 taps. The median tap lasted 40 ms (2 readings). 33
  taps had only 1 reading, and 55 were under 30 ms. The first reading's
  pressure was 527 at p10 and 1,072 at the median. The lowest was 323.
- **"Rebounds".** 93 touches started within 100 ms and 16 px of the
  last lift. These do not look like lost contact. The touch that
  starts near the last tap is a full touch: only 6% of them had one
  reading, against 16% of touches that start elsewhere. They are taps,
  drags and flicks of their own. A 1-reading tap is followed by a touch
  at the same spot as often as a long tap is (33% against 35%). So the
  owner touched the same spot again, as the menu bars and tiles
  invite. Without times between touches, the log cannot tell more. The
  firmware now logs that time (below).
- **Lost touches.** 33 times (22 + 11) the line fell but the finger was
  gone before the board read it (`lost`).
- **How many taps were meant?** About 150 taps were seen. Most of the
  13 to 24 short single-jump "drags" were probably meant as taps too.
  Some of the 33 lost touches may also have been taps. So the owner
  meant roughly 165 to 200 taps.

### What changed, and what it costs

- **`CONFIRM_READS` = 2 (new).** A resting finger moves only after two
  firm readings in a row land outside the band. If one wild reading is
  followed by one back inside the band, or by the lift, that reading
  is left out and counted as `spikes`.
  - Gain: a tap with one bad reading stays a tap. That covers the 13
    short drags of 2 to 3 readings, and most of the 34 under 24 px.
  - Cost: a drag from rest starts one reading later, about 16 to 24 ms
    on the CYD. After that the point follows every reading, as before,
    and no distance is lost: it jumps to where the finger is.
  - Flick speed is unchanged, because the console measures it over
    later frames.
  - A swipe so short that the panel saw only 2 readings is now a tap.
    In the log, 1 touch of 278 looked like that kind of real swipe
    (47 px in 33 ms).
- **Kept: `Z_ON` 300, `Z_OFF` 180.**
  - Raising `Z_ON` would lose light taps: 22 starts were below 500 and
    the lowest was 323.
  - Raising `Z_OFF` would turn more readings into drop-outs inside
    light drags. The pressure of readings inside a touch is not logged,
    so there is nothing to tune it on yet.
- **Kept: `HOLD_READS` 2.** It was never used up: no run of 3 light
  readings. Lowering it to 1 might end touches if a pair ever came. It
  costs nothing on a lift, because the line ends the touch first.
- **Kept: `DEAD_PX` 3, `REST_READS` 3.**
  - The jitter is 1 px on average, and 4% of break-outs were jitter.
  - A band of 4 px would cut a few of those false moves. It would also
    make a drag start half a picture pixel later, for a gain the
    console never sees, because a drag needs 8 px anyway.
  - `REST_READS` 3 is about 50 to 70 ms to come to rest.
- **Not changed: the lift.** A short hold after the line goes high
  would merge lost contacts. The data does not show such losses (see
  "Rebounds"). The hold would also delay every tap and every flick
  release, and a held point stops the flick's speed.

### New in the log, for the next finger test

- The start line has `after=N`: milliseconds since the last lift, or
  -1 for the first touch. This tells a lost contact (a few ms) from a
  second tap.
- The end line has `jump_at=K`: which reading made the largest jump
  (2 means at the landing, K = reads means at the lift). It also has
  `spikes=S`: wild readings that were left out.
- The counters have `spk`, and the long form has `spikes`.

### Only the owner can confirm

- Whether the single-jump taps now stay taps on the real panel. The
  debug link cannot make one wild reading. Its frames come in batches,
  so a 6 to 9 ms detour was never read, and a 12 ms one was read twice.
- Whether a drag that starts one reading later feels laggy.
- What `after=` shows for "rebounds".

### Suggestions for console-core (not changed; for the owner)

- `DRAG_START` (4 picture px = 8 panel px): 25 drags or flicks moved
  under 16 px, 18 of them in under 150 ms. With `CONFIRM_READS` most
  should now be taps. If the next test still shows short drags made by
  two readings, try 5 or 6.
- `TAP_MAX_US` (500 ms): only 2 of 150 taps were over 300 ms, so it
  has room.
- `TOUCH_GRACE` (1 frame): no lost contact was seen, so no change is
  suggested.
- `FLICK_MIN` (150 px/s): 2 "flicks" of 2 readings that moved 11 to
  13 px look like taps with a bad reading. `CONFIRM_READS` covers them.
  No change is suggested.
