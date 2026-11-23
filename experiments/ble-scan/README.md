# ble-scan

An experiment for the CYD board (ESP32-2432S028, classic ESP32): the
scanning side of Bluetooth Low Energy, memory figures once a second, and a
first try of the pass-by beacon. Nothing here connects or pairs.

It does not use the screen, the touch panel or any pin. Everything it has
to say goes out on the serial port (115200 baud). Nothing is read from the
serial port, so a GPS module on connector P1 cannot confuse it.

## Builds

One firmware, chosen by a cargo feature:

| Feature    | What it does                                                             |
|------------|--------------------------------------------------------------------------|
| (none)     | Listens all the time. **The one to use with a gamepad in pairing mode.** |
| `duty15`   | Listens 150 ms in every second (15%).                                    |
| `beacon`   | Pass-by beacon every 200 ms and listening 15%, for long runs.            |
| `sequence` | 25 s each of: 15%, 15% + beacon, 100%, 100% + beacon, then again.        |
| `active`   | Like (none), but also asks each advertiser for its scan response.        |
| `security` | Like (none), with the Bluetooth library's pairing code compiled in.      |

A second small program, `rf-check`, scans for WiFi access points and
prints how many and how strong (no names). It checks the board's receiver
without Bluetooth.

## Build

```sh
export PATH="$HOME/.cargo/bin:$PATH"; . ~/export-esp.sh
cd experiments/ble-scan
cargo build --release                      # or: --features sequence, ...
cargo build --release --features rfcheck --bin rf-check
```

The result is `target/xtensa-esp32-none-elf/release/ble-scan` (or
`rf-check`).

`BLE_SCAN_NO_ROM_FIX=1 cargo build --release` leaves out the linker
workaround in `esp32-rom-bluetooth.x`, to show that the link then fails.

## Flash and watch

The board is shared: every command that opens the port
runs under the lock and a timeout.

```sh
LOCK=<path to board.lock>
ELF=target/xtensa-esp32-none-elf/release/ble-scan

# Flash, then read for a minute and print a report:
flock $LOCK timeout 150 sh -c "
  espflash flash --chip esp32 --port /dev/ttyUSB0 --non-interactive $ELF &&
  tools/scanlog.py --port /dev/ttyUSB0 --reset --secs 60 --save run.log"

# Or with espflash's own monitor, into a file, and the report afterwards:
flock $LOCK timeout 120 espflash flash --chip esp32 --port /dev/ttyUSB0 \
  --monitor --non-interactive $ELF > run.log 2>&1
tools/scanlog.py run.log
```

At your own desk, with nobody else on the board, the lock is not needed:
`espflash flash --chip esp32 --port /dev/ttyUSB0 --monitor $ELF`.

## Looking for a gamepad

Flash the build with no feature, put the gamepad in pairing mode near the
board, and look for this line (it comes back every ten seconds while the
gamepad is still heard):

```sh
grep CONTROLLER-LIKE run.log
```

`why=` says what gave it away: `hid-service-0x1812` (it advertises the
service keyboards, mice and game controllers share),
`appearance-gamepad-0x03c4`, `appearance-joystick-0x03c3` or `name-hint`
(only its name sounds like one). `rssi=` is the signal
strength; a gamepad on the desk should be far stronger than -90.

Some gamepads put their name or the HID service only in the scan
response, which a passive scan never asks for. If nothing shows, try
the `active` build as well (it transmits scan requests to every
advertiser around, so it is not the default).

Caution on `rssi=`: on this board no advertiser has yet been seen
stronger than about -92 dBm, while the `rf-check` WiFi scan hears access
points at -20 dBm. Nothing close by was ever measured, so "far stronger
than -90" is an expectation, not a checked fact.

No such line does not prove the gamepad is broken. Many gamepads speak
only the older "classic" Bluetooth in some of their modes, and this chip
is run as a Low Energy radio only, so it cannot hear those at all.

## Reading the beacon builds

- Every `ROTATE ... while_scanning=true result=Err(... Command Disallowed)`
  is expected: this radio refuses a new address while it scans, so the
  firmware stops the scan, changes the address and starts again
  (`while_scanning=false result=Ok(())`). In the `beacon` build this is
  why a `PHASE end` / `PHASE start` pair appears every minute; it is not a
  fault.
- The board cannot hear its own beacon. "beacon=on" means the radio
  accepted the commands, not that the advertisement was seen on air.
- Advertisement counts change by a factor of about three between runs a
  few minutes apart, and a 25 s phase hears only a handful. Compare
  phases over several cycles, never from one.

## tools/scanlog.py

Reads a log (a file, or the port itself) and prints a short report:
phases and advertisements a minute, devices by category, game controller
candidates, the lowest memory figures, and every fault. Python 3, standard
library only.

```sh
tools/scanlog.py run.log               # report
tools/scanlog.py --json run.log        # the same as JSON, for scripts
tools/scanlog.py --devices run.log     # adds every device line (private)
tools/scanlog.py --help
```

Exit code 0: no fault. 1: an `ERROR` line, a panic, a restart in the
middle, or a refused memory request. 2: no line from this firmware.

The report holds counts and categories only. `--devices` and
`--watch all` show addresses and names of other people's devices: fine on
your own screen, not for pasting elsewhere.

## The lines

Every line starts with a tag; the rest is `key=value` words.

| Tag               | When                | What                                                       |
|-------------------|---------------------|------------------------------------------------------------|
| `BOOT`            | start               | version and the plan of phases                             |
| `PROBE`           | start               | the radio's answers to single commands                     |
| `DEV`             | on news             | a new advertiser, or something new about a known one       |
| `CONTROLLER-LIKE` | on news, every 10 s | an advertiser that looks like a game controller            |
| `MEM`             | every second        | heap free / least ever / largest block, main stack used    |
| `STAT`            | every second        | advertisements this second, in this phase, in all; devices |
| `PHASE`           | start, end          | one stretch with one radio set-up, and its rate            |
| `MINUTE`          | every minute        | advertisements in that minute (builds without an end)      |
| `SUMMARY`         | with PHASE, MINUTE  | counts by category; no address, no name                    |
| `ROTATE`          | beacon builds       | a change of the beacon's address, and whether it was taken |
| `PASSBY`          | when heard          | a beacon of our own format from another console            |
| `ERROR`           | when it happens     | anything that went wrong                                   |
| `RF`              | `rf-check` only     | access points heard and how strong                         |

## Files

- `src/main.rs`: the phases, the scanning, the beacon, the printing.
- `src/seen.rs`: the table of devices and the reading of advertisements.
- `src/beacon.rs`: the 27 bytes of the pass-by beacon.
- `src/mem.rs`: heap and stack figures.
- `src/bin/rf_check.rs`: the WiFi receiver check.
- `esp32-rom-bluetooth.x`, `build.rs`: three addresses in the chip's ROM
  that the radio's library needs and the linker does not otherwise find.
