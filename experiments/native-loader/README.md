# native-loader

An experiment: the console firmware loads a game that was compiled on its
own, as a separate program, into RAM on a classic ESP32 and runs it. The
game calls the console through a table of functions.

Board: ESP32-2432S028 ("CYD"), ESP32 rev 3.1, no PSRAM, serial port
`/dev/ttyUSB0` at 115200 baud.

## The parts

| folder | what it is |
|---|---|
| `game-api/` | The contract, used by all the others: where the two slots are, what a game file looks like, the two tables of functions. Start reading here (`src/lib.rs`). |
| `game/` | The test game. A checklist in the shape of a game: it uses every kind of memory and call, and reports numbers. Its own linker script is `game.x`. |
| `console/` | The firmware. `game-slots.x` reserves the slots, `src/loader.rs` copies a game in, `src/api.rs` is what a game may call, `src/crash.rs` is what happens when a game crashes. |
| `tools/game-pack/` | PC program: turns the game's ELF file into the one file the console loads (36-byte header, code, data, CRC-32). |
| `tools/` | PC scripts, listed below. |

The slots (numbers live in `game-api/src/lib.rs`, nowhere else):

- code slot `0x40090000`, 64 KB: the upper half of the chip's executable RAM;
- data slot `0x3FFF4000`, 48 KB: the top of the second block of data RAM.
  The console's heap is the 49,616 bytes below it.

There is no relocation: every game is linked for exactly these addresses.

## Build

```sh
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh

./build-game.sh                 # game/target/images/HELLO.BIN, CRASH.BIN, FAULT.BIN + section lists
(cd console && cargo build --release)                    # plain
(cd console && cargo build --release --features ble)     # with Bluetooth advertising
(cd game-api && cargo test)                              # the header and CRC code, on the PC
```

The console builds the game files in with `include_bytes!`, so
`build-game.sh` comes first. Do not change the test game once
`/CONSOLE/HELLO.BIN` is on the SD card: the card's copy is written once
and never replaced (delete it on a PC if the game has to change).

## Flash and run

One command builds, flashes, listens for N seconds and prints a summary.
The cleaned log is kept in `logs/<name>.log`.

```sh
export BOARD_LOCK=/path/to/the/shared/board.lock   # only when the board is shared
tools/board-run.sh plain 70                    # built-in game, no radio
tools/board-run.sh ble   100 ble               # the same with Bluetooth advertising
tools/board-run.sh sd    110 ble,sd            # and from /CONSOLE/HELLO.BIN on the SD card
tools/board-run.sh probe 60  probe-byte-read   # reads single bytes from the code slot: 6 faults, 6 restarts
tools/board-run.sh crash 60  ble,crash-test    # a game that panics in frame 150
tools/board-run.sh fault 60  ble,fault-test    # a game that reads through a bad pointer in frame 150
tools/board-run.sh listen 30                   # no build, no flash: reset the board and listen
```

What `board-run.sh` runs on the port is this, and nothing else:

```sh
flock "$BOARD_LOCK" timeout N espflash flash --chip esp32 --port /dev/ttyUSB0 \
    --monitor --non-interactive console/target/builds/<name>.elf 2>&1 \
  | python3 tools/serial-filter.py | tee logs/<name>.log
```

**The `sd` build writes to the SD card**, once: if `/CONSOLE/HELLO.BIN`
is missing it creates the folder and the file (1,544 bytes). It never
changes or deletes anything, and never touches anything else on the card.

A good run ends its first part with

```
INFO - SUMMARY: every load and every check passed
```

and then prints one `SOAK round N ... ok` line every 4 seconds for as
long as it is left running.

## Tools for seeing what the board does

The firmware draws nothing; everything it has to say goes out of the
serial port, with a word in capitals in front of every line that carries
a result (`MEM`, `LOAD`, `PLAY`, `CHECKS`, `REFUSED`, `FULLSIZE`, `SD`,
`PROBE`, `LAST START`, `SUMMARY`, `SOAK`).

| tool | use |
|---|---|
| `tools/board-run.sh` | Build, flash, listen, summarise (above). |
| `tools/serial-filter.py` | Sits in the pipe after espflash: adds a time stamp to every line, removes colour codes, **drops GPS sentences** (a GPS module may share the serial pins) and blanks hardware addresses before anything reaches a file. |
| `tools/log-summary.py logs/x.log` | The result lines of a log, the SOAK lines boiled down to min/median/max, every line that speaks of trouble, and PASS or FAIL. |
| `tools/console-sizes.py console/target/builds/*.elf` | Who has how much of each block of RAM in a console build. |
| `tools/code-slot-audit.py game/target/images/*.elf` | Checks that a game keeps nothing in its code slot that code could read bytewise. |
| `tools/game-pack/target/release/game-pack check FILE.BIN` | Header and checksum of a game file. |
| `tools/expected.py` | The numbers the test game must report, worked out a second way. They are copied into `console/src/checks.rs`. |

## Checking the guards

The console must not grow into the slots. Two builds try to, and must
fail at the link step with a line that starts `GAME SLOT:`:

```sh
(cd console && cargo build --release --features test-grow-code)   # 64 KB more RAM code
(cd console && cargo build --release --features test-grow-heap)   # 4 KB more heap
```

## Writing another game

Copy `game/`. Keep `game.x`, `build.rs`, `.cargo/config.toml` and the
`panic_handler`; export one function,

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn game_entry(api: *const ConsoleApi) -> *const GameCallbacks
```

which checks `api.version` and `api.size`, keeps the pointer, and returns
a table with `init`, `update(frame)` and `exit`. Pack it with
`game-pack pack <elf> <NAME.BIN>`; the packer refuses a game that does
not fit the slots or has a section it does not know.

A game may not contain anything that needs an address fixed up at load
time (there is no relocation), and gets `memcpy`, `memset` and 64-bit
division from the chip's ROM (the addresses are in `game.x`).
