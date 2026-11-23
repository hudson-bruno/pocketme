# settings-store

The console's settings in flash. Today that is the brightness step
(0 to 4) and the boot animation choice (FULL, SHORT, OFF). There is
room for five more one-byte values.

No `std`, no heap, no `unsafe`. It only calls board-api's `Store`
trait, so the same code runs on the chip and in the PC tests.

The simulator uses it (`boards/desktop/src/settings.rs`, with
`--store FILE`) and so does boards/cyd (feature `store`); see "For the
board" below.

## Why a crate and not a module of console-core

- **The board saves, not the console.** console-core never writes
  flash; it only says that a setting changed (`take_settings_changed`).
  The code that writes belongs to the boards. A crate lets the
  simulator and the CYD both use it without console-core depending on
  flash.
- **It can be left out.** A board that keeps no settings does not link
  it (see "modular code, switchable parts").
- **It is tested alone.** Its tests use their own pretend flash that
  loses power at any byte. They do not need a console.

## The format

Two sectors of 4 KB each (8 KB). Each sector has 256 slots of 16 bytes.

```text
0..4    sequence number, 1, 2, 3, ... (counts over both sectors)
4       kind: 1 = settings. Other kinds (from a newer firmware) are
        skipped, but their numbers and slots count.
5..12   seven one-byte values; 0xFF = not set (the default).
        5: brightness step. 6: boot animation (0 FULL, 1 SHORT,
        2 OFF). 7..12: not used yet, kept as they are.
12..16  CRC-32 of bytes 0..12, xor 0x5E771A65
```

The newest record (the highest number with a good check) is the
value. A brightness above 4 reads as 4.

## The rules

- A change adds one record in the next blank slot. A record is never
  written over another one.
- A sector is erased only when the one in use is full. Then the
  **other** sector is erased and used. The sector that holds the newest
  record is never erased, so the only good copy is never lost.
- After a save, the record is read back. If it does not match (a worn
  cell), the next slot is tried, up to 3 times.
- The next save writes after the last slot that is not blank. A
  damaged slot is never used again, so the store never gets stuck.
- A blank area, a damaged area, or a store that cannot be read gives
  the defaults. Saving still works after that.
- Saving the same settings again writes nothing.

## Power cuts

A cut at any moment gives the old value or the new one after a
restart, never anything else. The next save works.

The tests prove it with a pretend flash (`tests/common/mod.rs`). It
loses power after any byte of a write or any 4-byte word of an erase.
It can also leave the byte or word it was changing half done (some
bits changed, others not). `tests/cuts.rs` tries **every** cut point
of every save, not a sample:

| Test | Cut points |
|---|---|
| 808 saves from a blank area (2 erases) | 14,976, and again with half-done bits |
| 276 saves from random bytes | 6,464, with half-done bits |
| erase of `prepare`, every word | 1,024 |
| 6 random runs of 4,000 steps (cuts, cuts during the save after a cut, `prepare`) | about 3,150 saves and 20 erases each |

With `CUT_SAVES=3000 cargo test --release --test cuts`: 3,000 saves
from blank, 58,240 cut points twice, 10 erases, all pass (4.4 s).

## Wear

From blank, the first erase comes at change 513 (both sectors full).
After that, one erase every 256 changes, taking turns. So each sector
is erased once per 512 changes (`tests/format.rs`: 10 x 512 changes
give 9 erases per sector).

At 10 changes a day, that is about 7 erases per sector a year. NOR
flash is usually rated for about 100,000 erases.

## Using it

```rust
use settings_store::{SaveTimer, Saved, SettingsStore};

let mut kept = SettingsStore::new(0);         // the area's start in the store
let settings = kept.load(&mut store);         // never fails; defaults if nothing stored
console.set_brightness(settings.brightness);  // before the first step
let _ = kept.prepare(&mut store);             // optional: erase the spare now, at start

let mut timer = SaveTimer::new();
// after every console.step:
if console.take_settings_changed() { timer.changed(now_us); }
if timer.due(now_us) {
    let new = kept.current().with_brightness(console.brightness());
    match kept.save(&mut store, &new) {
        Ok(Saved::Unchanged) | Ok(Saved::Written { .. }) => {}
        Err(_) => timer.changed(now_us),      // try again later
    }
}
// wake the board by timer.deadline() if one is set
```

- `load`: reads both sectors (8 KB, 64 reads of 128 bytes).
- `save`: one 16-byte write and one 16-byte read back. When the sector
  in use is full, it first checks the other one (up to 32 reads of 128
  bytes) and erases it if it is not blank.
- `prepare`: if the sector in use is at least half full and the other
  one is not blank, erases the other one now. Then no save in the next
  128 changes needs an erase. Call it at start, before the radio runs.
- `SaveTimer`: the save comes 2 s (`QUIET_US`) after the last change.
  So a finger sliding on the brightness bars makes one record, not ten.

All buffers handed to the store are 4-byte aligned. Writes are 16
bytes at offsets that are multiples of 16.

## On the chip

Measured with a probe (kept outside the repository, built like
boards/cyd: release, opt-level 3, fat LTO, one codegen unit):

| What | Code (bytes) |
|---|---|
| load, prepare and save together | 3,091 |
| load only | 1,959 |
| `SaveTimer` | 155 |

Stack: at most 512 bytes (the save's frame of 256 bytes calls the scan,
224 bytes, which calls the check sum, 32 bytes), plus whatever the
board's flash functions use.

RAM: `SettingsStore` is at most 24 bytes, `SaveTimer` 16 (a test
checks both).

On the CYD (round 5): the start (pairing CRC of 8 KB, partition table,
`load`, `prepare`) takes 4,976 us; a record write 36 us on average, 118
at most; an erase 29.0 ms, once every 256 saves.

## For the board

Owner decision D2: the brightness is kept in the chip's flash, in its
own small area, away from the gamepad pairing area.

The CYD uses it (boards/cyd, feature `store`). Its area is its own
partition, "settings" (data / undefined, 0x3FE000, 8 KB, the last 8 KB
of the chip; boards/cyd/partitions.csv), not part of `nvs`. Two
modules here make that safe and are tested on the PC:

- `area`: `Entry::parse` reads one 32-byte line of an ESP-IDF partition
  table (the board reads the table line by line with it), and
  `find_area` takes the settings partition only when every check passes
  (name, type, size, address, sector, read-only and encrypted flags,
  above the table, inside the chip, clear of every other partition).
  Only its `Area` makes an `AreaStore`, which refuses every read, write
  or erase outside the area before the flash is called
  (`tests/area.rs`, 14 tests, among them the CYD's real table as
  espflash writes it).
- `policy`: `SavePolicy`, when to save: `QUIET_US` after the last
  change, at once when a menu closes, never while a game runs unpaused,
  and at most `TRIES_AFTER_FAIL` tries again after a failure
  (`tests/policy.rs`).

On the ESP32 with esp-storage 0.10.0: use the `NorFlash` write
(`write_nor`), not `FlashStorage::write`. The second one reads, erases
and rewrites the whole sector, which breaks the rules above. Hand it
word-aligned buffers: for any other buffer it copies through a 4 KB
buffer on the stack. An erase stops the loop (and would stop a radio)
for about 29 ms on the CYD.

## Tests

```sh
cargo test                                    # all (about 1.3 s)
CUT_SAVES=3000 cargo test --release --test cuts
```

`boards/desktop/check.sh` checks this crate too: format, clippy, tests,
and the build for the chip and for a bare PC target.
