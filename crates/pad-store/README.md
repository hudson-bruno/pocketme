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
- At most 4 pads. A 5th pad pushes the oldest out.

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
only at start-up, before the radio starts. While the radio runs it only
appends small records.

## Tests

```
cargo test
```

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
