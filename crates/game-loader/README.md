# game-loader

Loads a game file from the card into the two game slots. It reads the
file in small pieces, checks it, and hands the bytes to the board. It
never writes memory itself: no `unsafe`, no heap, no `std`. So the same
code runs on the chip and in the PC tests.

boards/cyd uses it (games.rs, since 2026-11-17): the demo card and the
real SD card both go through this same `load`. See "On the board"
below.

## What the board must implement (ten lines)

```text
1. Files (board-api): size(path) and read_at(path, offset, buf). The board has these already.
2. GameSlots (this crate), four methods (a fifth, flash_part, has a default body that
   refuses every "GAMF" game); offsets count from the start of a slot:
   write_code_word(offset, word)  one volatile 32-bit write at CODE_SLOT_ADDR + offset
   read_code_word(offset)         one volatile 32-bit read there (the loader checks it)
   write_data(offset, bytes)      a plain copy to DATA_SLOT_ADDR + offset
   zero_data(offset, len)         a plain fill with 0 there
3. Call order of load(): read_at(0, 36 bytes) -> ("GAMF" only) read_at(36, 32 bytes) ->
   size() -> ("GAMF" only) flash_part -> read_at per piece, each piece
   followed by its write_code_word + read_code_word pairs and/or one write_data ->
   read_at(end, 1 byte), which must give 0 -> checksum -> zero_data once ->
   (a game with a spill only) read_code_word over the spill once more -> Ok(Loaded).
4. On Ok, and only then: turn loaded.entry() into a GameEntry and call it. On Err: show e.notice().
```

A code offset is always a multiple of 4. The loader never asks for a
place outside the slot sizes it was given. Code offsets run from 0 to
`code_size + spill_size` (`Slots`): from `code_size` on they are the
spill, which on the chip is `CODE_SLOT_ADDR + offset` too (the spill
follows the code slot directly, see below).

## Why the code comes as words

The code slot is the chip's executable RAM. It can only be read and
written as whole, aligned 32-bit words. A byte access there stops the
program (measured on the board by the loader experiment). So the trait
has no byte access to code at all. The data slot is ordinary memory.

## The order of work, and why it is safe

1. Read the 36-byte header. Check it against the slots: the word
   "GAME" (or "GAMF": a game with a flash part, whose 32-byte
   extension is read next and asked of the board before any slot is
   written), the version, both addresses, the sizes (whole words, fitting
   their slots), and that the entry is inside the code.
2. Ask the card for the file's size. It must be exactly header + code +
   data. A file cut short is refused here.
   Up to here nothing has been written.
3. Read the rest in pieces. Add each piece to the checksum, then hand it
   to the board: code word by word (each read back at once), data as
   bytes.
4. Look one byte past the end: there must be nothing.
5. Compare the checksum. Only now is the file known to be whole.
6. Zero the game's variables that start at zero.
   6b. Only when the code runs on past the code slot: read the spill's
   words once more and compare their checksum with the one taken while
   they were written (`CodeOverwritten` if not). `check_file` skips it:
   it writes nothing.
7. Return `Loaded`, which holds the entry address.

A wrong checksum is only known at the end. By then the slots were
written, so a failed load leaves rubbish in them. That is safe:

- The only way to learn the entry address is the `Loaded` of step 7. A
  failed load returns an error instead. So nothing can jump into a game
  that failed.
- The loader keeps nothing between calls, and every load writes every
  byte its game uses. So the next load works whatever the last one left.
- `Loaded` cannot be copied. The board must start a game only from the
  `Loaded` of the last load: once another load begins, the slots hold
  other bytes, even if that load fails.

### One pass or two?

The other way: read the whole file once only to check it, then read it
again to copy it. The slots would stay clean on a bad file. Measured on
the real game files (`tests/real_files.rs`):

| file | size | one pass (chosen) | two passes |
|---|---|---|---|
| PADTEST.BIN | 11,484 bytes | 25 reads, 11,484 bytes | 50 reads, 22,968 bytes |
| STARDUST.BIN | 10,516 bytes | 23 reads, 10,516 bytes | 46 reads, 21,032 bytes |
| full slots (56 KB code + 48 KB data) | 106,532 bytes | about 209 reads | about 418 reads |

Time on the chip, from the loader experiment's measurements: the
checksum costs about 0.11 microseconds per byte (10.7 ms for a full
98 KB file), writing and reading back the code 3.4 ms for 64 KB (an
older, larger slot; the slot is 56 KB now). The
card is the slow part, and a second pass doubles it. For a full game
the second pass adds the card time again plus about 12 ms of checksum.

Two passes buy nothing here. No game lives in the slots while another
is being loaded, so clean slots protect nothing. And the file could
change between the two passes, so the copy pass would need the
checksum again anyway. So the loader does one pass. `check_file` (the
first pass alone, writing nothing) is there if the console ever wants
to know a game is whole without loading it.

## Refusals

Each error has a short text for the console's notice panel
(`LoadError::notice`, capitals only, at most 24 letters):

| error | notice |
|---|---|
| `Card(NoCard)` / `Card(NotFound)` / `Card(BadName)` / `Card(Io)` / `Card(Unsupported)` | NO CARD / GAME NOT FOUND / BAD GAME FILE NAME / CARD READ ERROR / CARD CANNOT DO THIS |
| `NotAGame` (no "GAME" at the start) | NOT A GAME FILE |
| `TooOld` (a version below `game_api::MIN_GAME_VERSION`, today 3) | GAME IS TOO OLD |
| `OtherCodeSlot` / `OtherDataSlot` (built for other addresses) | GAME FOR OTHER CODE SLOT / GAME FOR OTHER DATA SLOT |
| `OddSize` (a size not a whole number of words) | BAD SIZES IN GAME FILE |
| `CodeTooBig` / `DataTooBig` | GAME CODE TOO BIG / GAME DATA TOO BIG |
| `CodeMeetsData` (code past the slot and the data do not fit in the data slot together) | GAME TOO BIG FOR SLOTS |
| `BadEntry` (start address not inside the code) | BAD GAME START ADDRESS |
| `CutShort` / `TooLong` (size not what the header says) | GAME FILE CUT SHORT / GAME FILE TOO LONG |
| `Changed` (the card gave fewer or more bytes than its size said) | FILE CHANGED WHILE READ |
| `Damaged` (wrong checksum: header, code or data) | GAME FILE IS DAMAGED |
| `CodeSlotFailed` (a word read back wrong: the board's fault) | CODE SLOT WRITE FAILED |
| `CodeOverwritten` (the spill changed before the load ended: the board's mirror, or a bug) | GAME CODE OVERWRITTEN |
| `BadFlashPart` (a "GAMF" file whose extension breaks its rules) | BAD FLASH PART IN FILE |
| `FlashPartMissing` (the board holds no such flash part, or knows none) | FLASH PART MISSING |
| `FlashPartDiffers { got_code, got_ro }` (a part there, other bytes) | FLASH PART DIFFERS |

A game built for a later contract version is let through, as the
contract says (crates/game-api README: a newer game may run on an older
console, and checks the console's tables itself).

## Games with a flash part ("GAMF", 2026-11-22)

A "GAMF" file has a 32-byte `FlashPartHeader` after the 36-byte header
(crates/game-api, `read_game_header`); its payload starts at byte 68 and
the CRC covers bytes 0..32 and 36..end, the extension too. The code and
constants it names are not in the file but in the console's firmware.
The order: header, extension (cut short: `CutShort`; broken:
`BadFlashPart`), sizes, then **`GameSlots::flash_part(&part)`, before any
slot is written**: `FlashHeld::Held` lets the load go on, `Missing` and
`Differs` refuse it. The method has a default that says `Missing`, so a
board that knows nothing of flash parts (the simulator, a firmware
without `game-flash`) refuses every such game and needs no change.
`check_file` checks the file only and treats the part as held. The
board's side is boards/cyd/src/games.rs (`ChipSlots::flash_part`: the
CRC-32s of the bytes as they lie in flash).

## The slot layout comes from one place

`Slots::CONSOLE` is made from `game_api::layout`. The loader checks
every game against the `Slots` it is given and knows no other number.
When the owner moved the code slot (2026-11-17: 56 KB at 0x4009_2000,
instead of 64 KB at 0x4009_0000, to make room for the Bluetooth radio's
RAM code), only `game_api::layout` changed; the linker scripts of the
console and of the games are made from it. A game built for the old
layout is refused with GAME FOR OTHER CODE SLOT (moved start) or GAME
CODE TOO BIG (less room); `tests/rules.rs` shows both with the old
layout, and the board showed the first (below).

## Code past the code slot (the spill, stage 1)

A game's code may run on past the 56 KB code slot. On the ESP32 the
addresses right after the slot (0x400A_0000 on) are the same RAM as the
top of the data slot, seen from the instruction bus in reverse word
order (crates/game-api README; the order was seen to hold on the CYD,
2026-11-22). So the code past the slot (the spill) takes the
data slot from the top down, while the game's data takes it from the
bottom up. `Slots::spill_size` says how much spill the console has
(`Slots::CONSOLE`: 48 KB, the whole data slot; 0 on an older console or
when the firmware's start-up mirror check failed).

`check_header`, before anything is written:

1. code over `code_size + min(spill_size, data_size)`: `CodeTooBig`
   (with `spill_size` 0 exactly the answer of an older console);
2. data + zeroed over the data slot: `DataTooBig`, as before;
3. spill + data + zeroed over the data slot: `CodeMeetsData`
   (GAME TOO BIG FOR SLOTS).

So a good load never writes data or zeroes over its own code, and
`GameHeader::validate` (game-api) says yes to exactly the same headers.
After the zeroing, the spill is read back once more (step 6b); about
2 to 4 ms for a 25 KB spill on the chip (ESTIMATED from the checksum
speed above). A game without a spill pays nothing, and its loads are
exactly as before.

## Memory and size on the chip

Measured by compiling a small probe for `xtensa-esp32-none-elf` with
the board's release settings (opt-level 3, fat LTO), with a card and
slots written the way a board would, and reading the frame size of each
function (`entry a1, N`) in its assembly. The probe is not kept in the
repository.

- Stack for one load: 624 bytes for `load` (512 of them are the piece
  buffer, `PIECE`) plus 64 for its read helper: about 690 bytes, plus
  whatever the board's own `read_at` needs. The same at opt-level "s".
- Code: about 1,070 bytes of machine code and 470 bytes of constants
  (the notice texts, the 64-byte checksum table, jump tables): about
  1.5 KB of flash in all.
- Nothing is kept between loads.

`PIECE` (512, one card block) is the one setting. Reads are lined up
with the card's blocks: after the header, every read ends at a multiple
of 512 in the file. A smaller `PIECE` saves stack and costs more reads.

## On the board

boards/cyd/src/games.rs does what this crate asks:

1. At start-up it builds `Slots` from the linker's symbols, compares
   them with `Slots::CONSOLE` (one log line) and refuses every game if
   they differ.
2. `ChipSlots`: code as whole words with `write_volatile` /
   `read_volatile`, data with `copy_nonoverlapping` / `write_bytes`.
3. `start_game(folder)`: `load` on `/CONSOLE/GAMES/<folder>/GAME.BIN`;
   on `Ok`, ISYNC, the entry address to a `GameEntry` (inside the same
   match arm: the `Loaded` and the address are never kept), then
   `game_api::open_game`. On `Err(e)`: `Card(NotFound)` gives
   `StartError::NotFound`, anything else `StartError::Refused(e.notice())`
   (board-api), which the console shows.
4. `end_game` and every failed load that wrote something set both slots
   to zero (1.09 ms on the board).

Measured on the board, 2026-11-17 (the logs are kept outside the
repository): PADTEST (11,484 bytes) loads in about 4.7 ms and STARDUST
(10,516 bytes) in about 4.3 ms from the demo card (in flash); a made-up
file is refused in 0.36 ms with NOT A GAME FILE and nothing written; a
PADTEST built for the old slot is refused in 0.46 ms with GAME FOR OTHER
CODE SLOT and nothing written. Loading from the real SD card was not
tried (no games on it).

## Testing

```sh
GAMES_TARGET=$PWD/target/chip ../../games/build-chip.sh   # once: the real game files
cargo test                                                # in this folder
../../boards/desktop/check.sh                             # every crate; sets GAME_FILES_DIR itself
```

- `tests/rules.rs`: small made-up files. A good load, block-aligned
  reads, short reads, every refusal (each header field, every cut-off
  length, growing and shrinking files, a read error at every call, a
  broken code slot), another slot layout, and 200,000 random headers on
  which the loader and `GameHeader::validate` agree (also on which ones
  meet in the data slot). For the spill: where its words land, code and
  data that meet by one word (refused, nothing touched), that fill the
  room exactly (loaded), code over slot and spill, an older console
  with no spill, an old game that leaves the data slot's top alone, an
  entry in the spill, huge sizes, and a spill changed after the copy
  (and a board whose mirror is straight instead of reversed), found by
  step 6b.
- `tests/shared.rs`: 20,000 seeded random sizes near the edges of the
  shared room; the loader says yes exactly when the rule (written out
  again in the test) does, and 300 of the good ones load into the
  strict pretend slots. `SHARED_LOADS=all` loads every good one (9,322,
  about 36 s in a debug build, measured on the PC).
- `tests/real_files.rs`: PADTEST.BIN and STARDUST.BIN loaded and
  compared byte for byte with their linked ELF files, the same way
  games/chip/check_load.py does; cut at every length; a read error at
  every call; a change before every read; and 6,000 seeded random
  damages, none accepted, no panic.
- The pretend slots (`tests/common`) panic on a byte access to code, a
  word that is not aligned, or anything outside a slot. They keep the
  spill inside their data slot, mirrored as on the chip, and (strict,
  the default) panic when a data or zero byte lands on code the same
  load wrote there.
