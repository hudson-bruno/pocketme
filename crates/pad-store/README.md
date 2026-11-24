# pad-store

Keeps game pad pairings in flash, so a pad paired once still connects
after a restart or after flashing a new firmware.

A **pairing** is what the pad and the console agree on the first time:
the pad's address (6 bytes) and a secret **link key** (16 bytes). With
it, the pad can connect later without pairing mode.

This is an experiment. The keys sit in flash as plain bytes (not
encrypted). Anyone who reads the flash chip can read them.

## Where it lives

The store uses the first two 4 KB sectors of the `nvs` data partition
(0x9000 to 0xB000 on the default table). The firmware finds that
partition at run time with `find_nvs`: data type, `nvs` sub-type and the
exact name `nvs`. A read-only or encrypted partition is refused.
`check_region` refuses it if it is below 0x9000
(bootloader and partition table), overlaps another partition, or runs
past the end of the chip. Every write and erase goes through one guard
that checks the range again, so nothing outside those two sectors can
be touched.

If the partition holds data the store did not write, it is called
**foreign** and left alone: nothing is written or erased.

## How it is laid out

- Two areas of 4 KB. One is active, the other is spare.
- Each area: a 32-byte header, then 127 slots of 32 bytes.
- Each record holds the address, the key, the key type and a CRC-32
  check sum. A record with a bad check sum is ignored.
- At most 4 pads. With `save` a 5th pad pushes the oldest out; with
  `save_new` a 5th pad is refused.

## The API

Two ways to get a `Store`, for two kinds of firmware:

- `Store::open(flash, region)`: the test firmware's way (`experiments/bt-gamepad-test`).
  It writes at every start (the header again), and may set up a blank
  partition, repair a cut first header, erase a dirty spare area, clean
  up when fewer than 32 slots are free, and wipe pads dropped for room.
  Returns `Opened`, which says which of these it did.
- `Store::attach(flash, region)`: the console's way, for the rule "the
  pairing area is written only by pair and forget". It only reads: no
  write and no erase, whatever the area holds. It returns `Attached`
  (`fresh`, `cut_header`, `spare_dirty`, `damaged`, `generation`) or
  refuses with `AttachError`:
  - `Foreign`: not our data (nothing is ever written there);
  - `TooMany`: more than 4 pads saved (a power cut while `save` pushed one
    out); a pad left out would come back after the next forget. Only
    `open` clears this.
  It takes a blank area (a board flashed from zero) and one that holds
  only the start of our first header as `fresh`: the first save then
  writes the header, reads it back, and writes the record. A power cut
  there leaves a cut first header again, which `attach` takes again.
  It also takes an area with damaged records, a dirty spare area (a
  clean-up cut by a power loss) or no free slot, and changes none of
  them.

Then, on either:

- `save_new(addr, key, key_type)`: never pushes a pad out, never erases,
  never cleans up. `Saved::New { evicted: None }` (1 write; 2 on a fresh
  area), `Saved::Replaced` (a new key for a saved pad, also with 4 saved:
  the new record, read back, then the old one zeroed, 2 writes), or
  `Saved::Unchanged` (nothing written). Refuses with `SaveError::Full` (a
  5th address) or `SaveError::NoRoom` (no free slot) before writing
  anything.
- `save(&pairing)`: as `save_new`, but a 5th pad pushes the oldest out
  (`Saved::New { evicted: Some(..) }`). The test firmware's call.
- `forget(addr)`: a forget marker, then the old records zeroed (2
  writes). With no free slot, the wipe alone forgets (still `Ok(true)`).
- `forget_all()`, `load(f)`, `count()`, `free_slots()`, `into_flash()`.

And without a `Store`:

- `read_pairings(flash, region, f)`: reads through `ReadFlash`, a trait
  with no write at all; finds the same pads in the same order as `open`
  and `attach`, plus a CRC-32 of the 8 KB. The console uses it at start.
- `find_nvs(entries, capacity)` and `check_region(part, others,
  capacity)`: the only ways to make a `Region`.

No type here prints a link key or a full address (`Pairing`'s Debug says
"key hidden").

## Why a power cut cannot lose a pairing

Flash can only turn bits from 1 to 0. Only a whole-sector erase turns
them back to 1. So the store never changes a record in place:

- **Save**: write the new record, read it back, and only then wipe the
  old record of that address (wipe = write all zeros).
- **Forget**: write a "forget" marker, then wipe the records. The
  records are wiped even when the marker cannot be written (full area
  or a flash error).
- **Clean-up** (only at start-up, when fewer than 32 slots are free):
  copy the live records into the spare area, write the spare area's
  header last, then erase the old area.

Erasing takes about 45 ms and blocks the radio, so the store erases
only at start-up, before the radio starts, and only in `open`. While the
radio runs it only appends small records. A store taken with `attach`
never erases at all, so on the console the area is never cleaned up:
each new pad, new key and forget takes one of the 127 slots, and when
they are used up `save_new` says `NoRoom`.

## Tests

```
cargo fmt --check
cargo clippy --offline --all-targets --all-features -- -D warnings
cargo test --offline --all-features
```

51 tests (49 in `src/tests.rs`, 2 in `src/crc.rs`), about 14 s.

The tests run on the PC against a fake flash that behaves like the real
one (bits only go to 0, whole-sector erase) and can lose power after
any byte written or any 256-byte piece erased. For every step of every
operation, the tests cut the power there, start again, and check that
no saved pairing was lost and no wrong key came back. All addresses and
keys in the tests are made up.

Other tests: two power cuts in a row during the clean-up; torn writes
with random bits (and every single-bit flip) never give a wrong key;
data next to the store is never touched; partition picking; and the
number of flash writes per event (a new pad costs 1 write, the same
pairing again costs 0).

For `attach` and `save_new`: `attach` writes and erases nothing (counted
calls, compared bytes) on more than 100 kinds of area (blank, every cut
point of the first header, 0 to 4 pads, damaged records, no free slot,
a dirty spare, every cut point of a clean-up, foreign data, five live
pads), and refuses exactly the foreign ones and the hidden fifth pad;
the first pair on a blank area; a 5th pad refused with nothing written;
a replace with 4 saved; `NoRoom` with nothing written; forget after
attach; a power cut at every step of a first pair, a new pad, a replace
in a full list and a forget, after which `attach` still takes the area
and finds what `read_pairings` and `Store::open` find; and 30 random
lives with power cuts (attach, `save_new`, forget) checked against a
simple model: never more than 4 pads, none pushed out, no erase.
